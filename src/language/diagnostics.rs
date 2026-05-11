use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticStatus {
    Passed,
    Failed,
    Unavailable,
}

impl DiagnosticStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticReport {
    pub language: String,
    pub engine: String,
    pub lsp_server: String,
    pub lsp_available: bool,
    pub command: String,
    pub cwd: String,
    pub checked_files: Vec<String>,
    pub status: DiagnosticStatus,
    pub stdout: String,
    pub stderr: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticCommand {
    language: String,
    engine: String,
    lsp_server: String,
    bin: String,
    args: Vec<String>,
    checked_files: Vec<String>,
    note: Option<String>,
}

const OUTPUT_LIMIT: usize = 16 * 1024;
const LSP_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(2);
const LSP_DIAGNOSTIC_TIMEOUT: Duration = Duration::from_secs(3);

pub fn run_diagnostics(cwd: &Path, files: &[String]) -> DiagnosticReport {
    let cwd = cwd.to_path_buf();
    let files = normalize_files(files);
    let language = detect_language(&cwd, &files);
    let lsp_server = lsp_server_for(&language);
    let lsp_available = command_available(lsp_server);
    let lsp_error_note = if lsp_available && !files.is_empty() {
        match run_lsp_diagnostics_with_command(
            &cwd,
            &files,
            &language,
            LspCommand::for_server(lsp_server),
        ) {
            Ok(report) => return report,
            Err(error) => Some(format!(
                "LSP protocol attempt failed: {error}; using fallback diagnostics"
            )),
        }
    } else {
        None
    };
    run_fallback_diagnostics(
        &cwd,
        &files,
        language,
        lsp_server,
        lsp_available,
        lsp_error_note,
    )
}

fn run_fallback_diagnostics(
    cwd: &Path,
    files: &[String],
    language: String,
    lsp_server: &str,
    lsp_available: bool,
    lsp_error_note: Option<String>,
) -> DiagnosticReport {
    let Some(command) = select_diagnostic_command(cwd, files) else {
        return DiagnosticReport {
            language,
            engine: "none".to_string(),
            lsp_available,
            lsp_server: lsp_server.to_string(),
            command: String::new(),
            cwd: cwd.display().to_string(),
            checked_files: files.to_vec(),
            status: DiagnosticStatus::Unavailable,
            stdout: String::new(),
            stderr: String::new(),
            note: Some(lsp_error_note.unwrap_or_else(|| {
                "no local diagnostic command was available for this workspace".to_string()
            })),
        };
    };

    let command_text = render_command(&command.bin, &command.args);
    let output = Command::new(&command.bin)
        .args(&command.args)
        .current_dir(cwd)
        .output();
    let fallback_note = merge_notes(lsp_error_note, command.note.clone());

    match output {
        Ok(output) => DiagnosticReport {
            language: command.language,
            engine: command.engine,
            lsp_server: command.lsp_server,
            lsp_available,
            command: command_text,
            cwd: cwd.display().to_string(),
            checked_files: command.checked_files,
            status: if output.status.success() {
                DiagnosticStatus::Passed
            } else {
                DiagnosticStatus::Failed
            },
            stdout: tail_bytes(&String::from_utf8_lossy(&output.stdout), OUTPUT_LIMIT),
            stderr: tail_bytes(&String::from_utf8_lossy(&output.stderr), OUTPUT_LIMIT),
            note: fallback_note,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DiagnosticReport {
            language: command.language,
            engine: command.engine,
            lsp_server: command.lsp_server,
            lsp_available,
            command: command_text,
            cwd: cwd.display().to_string(),
            checked_files: command.checked_files,
            status: DiagnosticStatus::Unavailable,
            stdout: String::new(),
            stderr: String::new(),
            note: merge_notes(
                fallback_note,
                Some(format!(
                    "diagnostic command `{}` was not found in PATH",
                    command.bin
                )),
            ),
        },
        Err(error) => DiagnosticReport {
            language: command.language,
            engine: command.engine,
            lsp_server: command.lsp_server,
            lsp_available,
            command: command_text,
            cwd: cwd.display().to_string(),
            checked_files: command.checked_files,
            status: DiagnosticStatus::Unavailable,
            stdout: String::new(),
            stderr: String::new(),
            note: merge_notes(
                fallback_note,
                Some(format!("could not invoke diagnostics: {error}")),
            ),
        },
    }
}

pub struct WarmDiagnosticSession {
    cwd: PathBuf,
    language: String,
    lsp_server: String,
    lsp_available: bool,
    lsp: Option<LspSession>,
    command_override: Option<LspCommand>,
}

impl WarmDiagnosticSession {
    pub fn new(cwd: PathBuf, files: &[String]) -> Self {
        let files = normalize_files(files);
        let language = detect_language(&cwd, &files);
        let lsp_server = lsp_server_for(&language).to_string();
        let lsp_available = command_available(&lsp_server);
        Self {
            cwd,
            language,
            lsp_server,
            lsp_available,
            lsp: None,
            command_override: None,
        }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn run(&mut self, files: &[String]) -> DiagnosticReport {
        let files = normalize_files(files);
        let language = detect_language(&self.cwd, &files);
        if language != self.language {
            self.reset_lsp();
            self.language = language;
            self.lsp_server = lsp_server_for(&self.language).to_string();
            self.lsp_available = command_available(&self.lsp_server);
        }

        let mut lsp_error_note = None;
        if self.lsp_available && !files.is_empty() {
            match self.run_lsp(&files) {
                Ok(report) => return report,
                Err(error) => {
                    self.reset_lsp();
                    lsp_error_note = Some(format!(
                        "warmed LSP session failed: {error}; using fallback diagnostics"
                    ));
                }
            }
        }

        run_fallback_diagnostics(
            &self.cwd,
            &files,
            self.language.clone(),
            &self.lsp_server,
            self.lsp_available,
            lsp_error_note,
        )
    }

    fn run_lsp(&mut self, files: &[String]) -> crate::error::AppResult<DiagnosticReport> {
        let documents = lsp_documents(&self.cwd, files, &self.language)?;
        if documents.is_empty() {
            return Err(crate::error::app_error(
                "no readable files were available for LSP diagnostics",
            ));
        }
        if self.lsp.is_none() {
            let command = self
                .command_override
                .clone()
                .unwrap_or_else(|| LspCommand::for_server(&self.lsp_server));
            self.lsp = Some(LspSession::start(&self.cwd, command)?);
        }
        let lsp = self
            .lsp
            .as_mut()
            .ok_or_else(|| crate::error::app_error("warmed LSP session was not initialized"))?;
        let diagnostics = lsp.diagnose(&documents)?;
        Ok(lsp_report(
            &self.cwd,
            &self.language,
            &lsp.command,
            &documents,
            &diagnostics,
            "warmed LSP session consumed textDocument/publishDiagnostics for opened files",
        ))
    }

    fn reset_lsp(&mut self) {
        if let Some(mut lsp) = self.lsp.take() {
            lsp.shutdown();
        }
    }

    #[cfg(test)]
    fn new_with_command(cwd: PathBuf, language: String, command: LspCommand) -> Self {
        Self {
            cwd,
            language,
            lsp_server: command.bin.clone(),
            lsp_available: true,
            lsp: None,
            command_override: Some(command),
        }
    }
}

impl Drop for WarmDiagnosticSession {
    fn drop(&mut self) {
        self.reset_lsp();
    }
}

#[derive(Debug, Clone)]
struct LspCommand {
    bin: String,
    args: Vec<String>,
}

impl LspCommand {
    fn for_server(server: &str) -> Self {
        let args = match server {
            "typescript-language-server" | "pyright-langserver" => vec!["--stdio".to_string()],
            "gopls" => vec!["serve".to_string()],
            _ => Vec::new(),
        };
        Self {
            bin: server.to_string(),
            args,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LspDiagnostic {
    uri: String,
    line: u64,
    character: u64,
    severity: String,
    message: String,
}

fn run_lsp_diagnostics_with_command(
    cwd: &Path,
    files: &[String],
    language: &str,
    command: LspCommand,
) -> crate::error::AppResult<DiagnosticReport> {
    let documents = lsp_documents(cwd, files, language)?;
    if documents.is_empty() {
        return Err(crate::error::app_error(
            "no readable files were available for LSP diagnostics",
        ));
    }
    let mut child = Command::new(&command.bin)
        .args(&command.args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| crate::error::app_error(format!("could not start LSP server: {error}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| crate::error::app_error("LSP server did not expose stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| crate::error::app_error("LSP server did not expose stdout"))?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || read_lsp_stdout(stdout, tx));

    let root_uri = file_uri(cwd)?;
    write_lsp_message(
        &mut stdin,
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"processId":null,"rootUri":"{}","capabilities":{{}},"workspaceFolders":[{{"uri":"{}","name":"workspace"}}]}}}}"#,
            crate::util::json::json_escape(&root_uri),
            crate::util::json::json_escape(&root_uri)
        ),
    )?;
    wait_for_lsp_response(&rx, 1, LSP_INITIALIZE_TIMEOUT)?;
    write_lsp_message(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
    )?;

    for document in &documents {
        write_lsp_message(
            &mut stdin,
            &format!(
                r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"{}","version":1,"text":"{}"}}}}}}"#,
                crate::util::json::json_escape(&document.uri),
                crate::util::json::json_escape(&document.language_id),
                crate::util::json::json_escape(&document.text)
            ),
        )?;
    }

    let expected_uris = documents
        .iter()
        .map(|document| document.uri.clone())
        .collect::<Vec<_>>();
    let diagnostics = collect_lsp_diagnostics(&rx, LSP_DIAGNOSTIC_TIMEOUT, &expected_uris);
    let _ = write_lsp_message(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":null}"#,
    );
    let _ = write_lsp_message(&mut stdin, r#"{"jsonrpc":"2.0","method":"exit"}"#);
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();

    Ok(lsp_report(
        cwd,
        language,
        &command,
        &documents,
        &diagnostics,
        "LSP protocol path consumed textDocument/publishDiagnostics for opened files",
    ))
}

struct LspSession {
    command: LspCommand,
    child: Child,
    stdin: Option<ChildStdin>,
    rx: mpsc::Receiver<String>,
    reader: Option<JoinHandle<()>>,
    opened_uris: BTreeSet<String>,
    version: u64,
}

impl LspSession {
    fn start(cwd: &Path, command: LspCommand) -> crate::error::AppResult<Self> {
        let mut child = Command::new(&command.bin)
            .args(&command.args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                crate::error::app_error(format!("could not start LSP server: {error}"))
            })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| crate::error::app_error("LSP server did not expose stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| crate::error::app_error("LSP server did not expose stdout"))?;
        let (tx, rx) = mpsc::channel();
        let reader = thread::spawn(move || read_lsp_stdout(stdout, tx));
        let root_uri = file_uri(cwd)?;
        write_lsp_message(
            &mut stdin,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"processId":null,"rootUri":"{}","capabilities":{{}},"workspaceFolders":[{{"uri":"{}","name":"workspace"}}]}}}}"#,
                crate::util::json::json_escape(&root_uri),
                crate::util::json::json_escape(&root_uri)
            ),
        )?;
        wait_for_lsp_response(&rx, 1, LSP_INITIALIZE_TIMEOUT)?;
        write_lsp_message(
            &mut stdin,
            r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#,
        )?;
        Ok(Self {
            command,
            child,
            stdin: Some(stdin),
            rx,
            reader: Some(reader),
            opened_uris: BTreeSet::new(),
            version: 1,
        })
    }

    fn diagnose(
        &mut self,
        documents: &[LspDocument],
    ) -> crate::error::AppResult<Vec<LspDiagnostic>> {
        for document in documents {
            self.version = self.version.saturating_add(1);
            let version = self.version;
            let stdin = self
                .stdin
                .as_mut()
                .ok_or_else(|| crate::error::app_error("LSP server stdin is closed"))?;
            if self.opened_uris.insert(document.uri.clone()) {
                write_lsp_message(
                    stdin,
                    &format!(
                        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"{}","version":{},"text":"{}"}}}}}}"#,
                        crate::util::json::json_escape(&document.uri),
                        crate::util::json::json_escape(&document.language_id),
                        version,
                        crate::util::json::json_escape(&document.text)
                    ),
                )?;
            } else {
                write_lsp_message(
                    stdin,
                    &format!(
                        r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{}","version":{}}},"contentChanges":[{{"text":"{}"}}]}}}}"#,
                        crate::util::json::json_escape(&document.uri),
                        version,
                        crate::util::json::json_escape(&document.text)
                    ),
                )?;
            }
        }

        let expected_uris = documents
            .iter()
            .map(|document| document.uri.clone())
            .collect::<Vec<_>>();
        Ok(collect_lsp_diagnostics(
            &self.rx,
            LSP_DIAGNOSTIC_TIMEOUT,
            &expected_uris,
        ))
    }

    fn shutdown(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = write_lsp_message(
                &mut stdin,
                r#"{"jsonrpc":"2.0","id":99,"method":"shutdown","params":null}"#,
            );
            let _ = write_lsp_message(&mut stdin, r#"{"jsonrpc":"2.0","method":"exit"}"#);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn lsp_report(
    cwd: &Path,
    language: &str,
    command: &LspCommand,
    documents: &[LspDocument],
    diagnostics: &[LspDiagnostic],
    note: &str,
) -> DiagnosticReport {
    DiagnosticReport {
        language: language.to_string(),
        engine: "lsp publishDiagnostics".to_string(),
        lsp_server: command.bin.clone(),
        lsp_available: true,
        command: render_command(&command.bin, &command.args),
        cwd: cwd.display().to_string(),
        checked_files: documents
            .iter()
            .map(|document| document.path.clone())
            .collect::<Vec<_>>(),
        status: if diagnostics.is_empty() {
            DiagnosticStatus::Passed
        } else {
            DiagnosticStatus::Failed
        },
        stdout: render_lsp_diagnostics(diagnostics),
        stderr: String::new(),
        note: Some(note.to_string()),
    }
}

struct LspDocument {
    path: String,
    uri: String,
    language_id: String,
    text: String,
}

fn lsp_documents(
    cwd: &Path,
    files: &[String],
    language: &str,
) -> crate::error::AppResult<Vec<LspDocument>> {
    let mut documents = Vec::new();
    for file in files {
        let path = cwd.join(file);
        if !path.is_file() {
            continue;
        }
        documents.push(LspDocument {
            path: file.clone(),
            uri: file_uri(&path)?,
            language_id: lsp_language_id(language, file).to_string(),
            text: std::fs::read_to_string(&path).map_err(|error| {
                crate::error::app_error(format!("failed to read `{file}` for LSP: {error}"))
            })?,
        });
    }
    Ok(documents)
}

fn lsp_language_id(language: &str, file: &str) -> &'static str {
    match language {
        "rust" => "rust",
        "typescript" if file.ends_with(".tsx") => "typescriptreact",
        "typescript" => "typescript",
        "javascript" if file.ends_with(".jsx") => "javascriptreact",
        "javascript" => "javascript",
        "python" => "python",
        "go" => "go",
        _ => "plaintext",
    }
}

fn file_uri(path: &Path) -> crate::error::AppResult<String> {
    let path = path.canonicalize().map_err(|error| {
        crate::error::app_error(format!(
            "failed to canonicalize path `{}` for LSP: {error}",
            path.display()
        ))
    })?;
    let mut value = path.to_string_lossy().replace('\\', "/");
    if !value.starts_with('/') {
        value.insert(0, '/');
    }
    Ok(format!("file://{}", percent_encode_uri_path(&value)))
}

fn percent_encode_uri_path(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn write_lsp_message(stdin: &mut ChildStdin, body: &str) -> crate::error::AppResult<()> {
    write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).map_err(|error| {
        crate::error::app_error(format!("failed to write LSP message: {error}"))
    })?;
    stdin
        .flush()
        .map_err(|error| crate::error::app_error(format!("failed to flush LSP message: {error}")))
}

fn read_lsp_stdout(stdout: std::process::ChildStdout, tx: mpsc::Sender<String>) {
    let mut reader = BufReader::new(stdout);
    while let Ok(Some(body)) = read_lsp_message(&mut reader) {
        if tx.send(body).is_err() {
            break;
        }
    }
}

fn read_lsp_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = content_length else {
        return Ok(None);
    };
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

fn wait_for_lsp_response(
    rx: &mpsc::Receiver<String>,
    id: u64,
    timeout: Duration,
) -> crate::error::AppResult<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(crate::error::app_error(
                "timed out waiting for LSP initialize",
            ));
        }
        let body = rx.recv_timeout(remaining).map_err(|_| {
            crate::error::app_error("timed out waiting for LSP initialize response")
        })?;
        if json_number_field(&body, "id") == Some(id) {
            return Ok(());
        }
    }
}

fn collect_lsp_diagnostics(
    rx: &mpsc::Receiver<String>,
    timeout: Duration,
    expected_uris: &[String],
) -> Vec<LspDiagnostic> {
    let deadline = Instant::now() + timeout;
    let mut diagnostics = Vec::new();
    let mut seen_uris = BTreeSet::new();
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(body) = rx.recv_timeout(remaining.min(Duration::from_millis(250))) else {
            continue;
        };
        if let Some((uri, mut next)) = parse_publish_diagnostics_event(&body) {
            seen_uris.insert(uri);
            diagnostics.append(&mut next);
            if expected_uris.iter().all(|uri| seen_uris.contains(uri)) {
                break;
            }
        }
    }
    diagnostics
}

#[cfg(test)]
fn parse_publish_diagnostics(body: &str) -> Vec<LspDiagnostic> {
    parse_publish_diagnostics_event(body)
        .map(|(_, diagnostics)| diagnostics)
        .unwrap_or_default()
}

fn parse_publish_diagnostics_event(body: &str) -> Option<(String, Vec<LspDiagnostic>)> {
    let Ok(root) = crate::util::json::parse_root_object(body) else {
        return None;
    };
    if root
        .get("method")
        .and_then(crate::util::json::json_as_string)
        != Some("textDocument/publishDiagnostics")
    {
        return None;
    }
    let Some(params) = root
        .get("params")
        .and_then(crate::util::json::json_as_object)
    else {
        return None;
    };
    let uri = params
        .get("uri")
        .and_then(crate::util::json::json_as_string)
        .unwrap_or("")
        .to_string();
    let Some(items) = params
        .get("diagnostics")
        .and_then(crate::util::json::json_as_array)
    else {
        return None;
    };
    let diagnostics = items
        .iter()
        .filter_map(|item| parse_lsp_diagnostic(&uri, item))
        .collect();
    Some((uri, diagnostics))
}

fn parse_lsp_diagnostic(uri: &str, item: &crate::util::json::JsonValue) -> Option<LspDiagnostic> {
    let root = crate::util::json::json_as_object(item)?;
    let message = root
        .get("message")
        .and_then(crate::util::json::json_as_string)?
        .to_string();
    let severity = root
        .get("severity")
        .and_then(crate::util::json::json_as_u64)
        .map(lsp_severity_label)
        .unwrap_or("diagnostic")
        .to_string();
    let start = root
        .get("range")
        .and_then(crate::util::json::json_as_object)
        .and_then(|range| range.get("start"))
        .and_then(crate::util::json::json_as_object);
    let line = start
        .and_then(|start| start.get("line"))
        .and_then(crate::util::json::json_as_u64)
        .unwrap_or(0);
    let character = start
        .and_then(|start| start.get("character"))
        .and_then(crate::util::json::json_as_u64)
        .unwrap_or(0);
    Some(LspDiagnostic {
        uri: uri.to_string(),
        line: line + 1,
        character: character + 1,
        severity,
        message,
    })
}

fn lsp_severity_label(value: u64) -> &'static str {
    match value {
        1 => "error",
        2 => "warning",
        3 => "information",
        4 => "hint",
        _ => "diagnostic",
    }
}

fn render_lsp_diagnostics(diagnostics: &[LspDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{}:{}:{}: {}: {}",
                diagnostic.uri,
                diagnostic.line,
                diagnostic.character,
                diagnostic.severity,
                diagnostic.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn json_number_field(body: &str, field: &str) -> Option<u64> {
    crate::util::json::parse_root_object(body)
        .ok()?
        .get(field)
        .and_then(crate::util::json::json_as_u64)
}

impl DiagnosticReport {
    pub fn render_text(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("diagnostics: {}\n", self.status.as_str()));
        output.push_str(&format!("  language: {}\n", self.language));
        output.push_str(&format!("  engine: {}\n", self.engine));
        output.push_str(&format!(
            "  lsp_server: {} ({})\n",
            self.lsp_server,
            if self.lsp_available {
                "available"
            } else {
                "missing"
            }
        ));
        if !self.command.is_empty() {
            output.push_str(&format!("  command: {}\n", self.command));
        }
        output.push_str(&format!("  cwd: {}\n", self.cwd));
        if !self.checked_files.is_empty() {
            output.push_str("  files:\n");
            for file in &self.checked_files {
                output.push_str(&format!("    - {file}\n"));
            }
        }
        if let Some(note) = &self.note {
            output.push_str(&format!("  note: {note}\n"));
        }
        if !self.stdout.trim().is_empty() {
            output.push_str("stdout:\n");
            output.push_str(self.stdout.trim_end());
            output.push('\n');
        }
        if !self.stderr.trim().is_empty() {
            output.push_str("stderr:\n");
            output.push_str(self.stderr.trim_end());
            output.push('\n');
        }
        output.trim_end().to_string()
    }
}

fn select_diagnostic_command(cwd: &Path, files: &[String]) -> Option<DiagnosticCommand> {
    let language = detect_language(cwd, files);
    match language.as_str() {
        "rust" => rust_command(files),
        "typescript" => typescript_command(cwd, files),
        "javascript" => javascript_command(cwd, files),
        "python" => python_command(files),
        "go" => go_command(),
        _ => None,
    }
}

fn rust_command(files: &[String]) -> Option<DiagnosticCommand> {
    let bin = cargo_bin()?;
    Some(DiagnosticCommand {
        language: "rust".to_string(),
        engine: "cargo check".to_string(),
        lsp_server: "rust-analyzer".to_string(),
        bin,
        args: vec!["check".to_string(), "--message-format=short".to_string()],
        checked_files: files.to_vec(),
        note: Some(
            "LSP protocol integration is not active yet; using cargo check fallback".to_string(),
        ),
    })
}

fn typescript_command(cwd: &Path, files: &[String]) -> Option<DiagnosticCommand> {
    let bin = tsc_bin(cwd)?;
    Some(DiagnosticCommand {
        language: "typescript".to_string(),
        engine: "tsc --noEmit".to_string(),
        lsp_server: "typescript-language-server".to_string(),
        bin,
        args: vec![
            "--noEmit".to_string(),
            "--pretty".to_string(),
            "false".to_string(),
        ],
        checked_files: files.to_vec(),
        note: Some("LSP protocol integration is not active yet; using tsc fallback".to_string()),
    })
}

fn javascript_command(cwd: &Path, files: &[String]) -> Option<DiagnosticCommand> {
    if cwd.join("tsconfig.json").is_file() {
        return typescript_command(cwd, files).map(|mut command| {
            command.language = "javascript".to_string();
            command
        });
    }
    if package_json_has_typecheck(cwd) {
        return Some(DiagnosticCommand {
            language: "javascript".to_string(),
            engine: "npm run type-check".to_string(),
            lsp_server: "typescript-language-server".to_string(),
            bin: "npm".to_string(),
            args: vec!["run".to_string(), "type-check".to_string()],
            checked_files: files.to_vec(),
            note: Some(
                "LSP protocol integration is not active yet; using package type-check script"
                    .to_string(),
            ),
        });
    }
    None
}

fn python_command(files: &[String]) -> Option<DiagnosticCommand> {
    let bin = first_available(&["python3", "python"])?;
    let py_files = files
        .iter()
        .filter(|file| file.ends_with(".py"))
        .cloned()
        .collect::<Vec<_>>();
    if py_files.is_empty() {
        return Some(DiagnosticCommand {
            language: "python".to_string(),
            engine: "py_compile".to_string(),
            lsp_server: "pyright-langserver".to_string(),
            bin,
            args: vec![
                "-m".to_string(),
                "compileall".to_string(),
                "-q".to_string(),
                ".".to_string(),
            ],
            checked_files: Vec::new(),
            note: Some(
                "LSP protocol integration is not active yet; using compileall fallback".to_string(),
            ),
        });
    }
    let mut args = vec!["-m".to_string(), "py_compile".to_string()];
    args.extend(py_files.iter().cloned());
    Some(DiagnosticCommand {
        language: "python".to_string(),
        engine: "py_compile".to_string(),
        lsp_server: "pyright-langserver".to_string(),
        bin,
        args,
        checked_files: py_files,
        note: Some(
            "LSP protocol integration is not active yet; using py_compile fallback".to_string(),
        ),
    })
}

fn go_command() -> Option<DiagnosticCommand> {
    let bin = first_available(&["go"])?;
    Some(DiagnosticCommand {
        language: "go".to_string(),
        engine: "go test ./...".to_string(),
        lsp_server: "gopls".to_string(),
        bin,
        args: vec!["test".to_string(), "./...".to_string()],
        checked_files: Vec::new(),
        note: Some(
            "LSP protocol integration is not active yet; using go test fallback".to_string(),
        ),
    })
}

fn detect_language(cwd: &Path, files: &[String]) -> String {
    if files.iter().any(|file| file.ends_with(".rs")) {
        return "rust".to_string();
    }
    if files
        .iter()
        .any(|file| matches_extension(file, &["ts", "tsx"]))
    {
        return "typescript".to_string();
    }
    if files
        .iter()
        .any(|file| matches_extension(file, &["js", "jsx", "mjs", "cjs"]))
    {
        return "javascript".to_string();
    }
    if files.iter().any(|file| file.ends_with(".py")) {
        return "python".to_string();
    }
    if files.iter().any(|file| file.ends_with(".go")) {
        return "go".to_string();
    }
    if !files.is_empty() {
        return "generic".to_string();
    }
    if cwd.join("Cargo.toml").is_file() {
        return "rust".to_string();
    }
    if cwd.join("tsconfig.json").is_file() {
        return "typescript".to_string();
    }
    if cwd.join("package.json").is_file() {
        return "javascript".to_string();
    }
    if cwd.join("pyproject.toml").is_file() || cwd.join("requirements.txt").is_file() {
        return "python".to_string();
    }
    if cwd.join("go.mod").is_file() {
        return "go".to_string();
    }
    "generic".to_string()
}

fn lsp_server_for(language: &str) -> &'static str {
    match language {
        "rust" => "rust-analyzer",
        "typescript" | "javascript" => "typescript-language-server",
        "python" => "pyright-langserver",
        "go" => "gopls",
        _ => "unknown",
    }
}

fn matches_extension(path: &str, extensions: &[&str]) -> bool {
    let Some(extension) = Path::new(path).extension().and_then(|value| value.to_str()) else {
        return false;
    };
    extensions.iter().any(|candidate| extension == *candidate)
}

fn cargo_bin() -> Option<String> {
    if let Ok(value) = std::env::var("CARGO") {
        if !value.trim().is_empty() {
            return Some(value);
        }
    }
    home_bin("cargo").or_else(|| first_available(&["cargo"]))
}

fn tsc_bin(cwd: &Path) -> Option<String> {
    let local = cwd
        .join("node_modules")
        .join(".bin")
        .join(if cfg!(windows) { "tsc.cmd" } else { "tsc" });
    if local.is_file() {
        return Some(local.display().to_string());
    }
    first_available(&["tsc"])
}

fn package_json_has_typecheck(cwd: &Path) -> bool {
    std::fs::read_to_string(cwd.join("package.json"))
        .map(|content| content.contains("\"type-check\"") || content.contains("\"typecheck\""))
        .unwrap_or(false)
}

fn first_available(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| command_available(candidate))
        .map(|candidate| (*candidate).to_string())
}

fn home_bin(name: &str) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let path = PathBuf::from(home).join(".cargo").join("bin").join(name);
    path.is_file().then(|| path.display().to_string())
}

fn command_available(command: &str) -> bool {
    if command == "unknown" {
        return false;
    }
    Command::new(command).arg("--version").output().is_ok()
}

fn normalize_files(files: &[String]) -> Vec<String> {
    let mut normalized = files
        .iter()
        .map(|file| file.trim())
        .filter(|file| !file.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

fn render_command(bin: &str, args: &[String]) -> String {
    std::iter::once(bin.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn tail_bytes(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let start = value
        .char_indices()
        .map(|(index, _)| index)
        .find(|index| value.len() - index <= limit)
        .unwrap_or(value.len());
    format!("... truncated earlier output ...\n{}", &value[start..])
}

fn merge_notes(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!("{first}; {second}")),
        (Some(first), None) => Some(first),
        (None, Some(second)) => Some(second),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-diagnostics-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn detects_language_from_files_before_workspace_markers() {
        let root = temp_root("detect");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(detect_language(&root, &["src/main.rs".to_string()]), "rust");
        assert_eq!(
            detect_language(&root, &["src/app.ts".to_string()]),
            "typescript"
        );
        assert_eq!(
            detect_language(&root, &["src/app.py".to_string()]),
            "python"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn python_diagnostics_reports_syntax_error_when_python_is_available() {
        if first_available(&["python3", "python"]).is_none() {
            return;
        }
        let root = temp_root("python");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("bad.py"), "def nope(:\n").unwrap();

        let report = run_diagnostics(&root, &["bad.py".to_string()]);

        assert_eq!(report.language, "python");
        assert_eq!(report.status, DiagnosticStatus::Failed);
        assert!(report.render_text().contains("diagnostics: failed"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parse_publish_diagnostics_reads_lsp_notification() {
        let body = r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///tmp/app.py","diagnostics":[{"severity":1,"message":"bad syntax","range":{"start":{"line":2,"character":4}}}]}}"#;

        let diagnostics = parse_publish_diagnostics(body);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].uri, "file:///tmp/app.py");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].character, 5);
        assert_eq!(diagnostics[0].severity, "error");
        assert_eq!(diagnostics[0].message, "bad syntax");
    }

    #[test]
    fn lsp_protocol_path_consumes_publish_diagnostics() {
        let Some(python) = first_available(&["python3", "python"]) else {
            return;
        };
        let root = temp_root("lsp");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("app.py"), "print('hello')\n").unwrap();
        let server = root.join("fake_lsp.py");
        fs::write(
            &server,
            r#"
import json
import sys

if "--version" in sys.argv:
    print("fake-lsp 1.0")
    sys.exit(0)

def read_msg():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def send(obj):
    body = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

while True:
    msg = read_msg()
    if msg is None:
        break
    if msg.get("method") == "initialize":
        send({"jsonrpc":"2.0","id":msg["id"],"result":{"capabilities":{"textDocumentSync":1}}})
    elif msg.get("method") == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[{"severity":1,"message":"fake syntax error","range":{"start":{"line":1,"character":2}}}]}})
    elif msg.get("method") == "shutdown":
        send({"jsonrpc":"2.0","id":msg["id"],"result":None})
"#,
        )
        .unwrap();

        let report = run_lsp_diagnostics_with_command(
            &root,
            &["app.py".to_string()],
            "python",
            LspCommand {
                bin: python,
                args: vec![server.display().to_string()],
            },
        )
        .unwrap();

        assert_eq!(report.engine, "lsp publishDiagnostics");
        assert_eq!(report.status, DiagnosticStatus::Failed);
        assert!(report.stdout.contains("fake syntax error"));
        assert!(report
            .note
            .as_deref()
            .unwrap()
            .contains("publishDiagnostics"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn warmed_lsp_session_reuses_process_for_did_change() {
        let Some(python) = first_available(&["python3", "python"]) else {
            return;
        };
        let root = temp_root("warm-lsp");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("app.py"), "print('hello')\n").unwrap();
        let server = root.join("fake_warm_lsp.py");
        fs::write(
            &server,
            r#"
import json
import sys

def read_msg():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def send(obj):
    body = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

while True:
    msg = read_msg()
    if msg is None:
        break
    if msg.get("method") == "initialize":
        send({"jsonrpc":"2.0","id":msg["id"],"result":{"capabilities":{"textDocumentSync":1}}})
    elif msg.get("method") == "textDocument/didOpen":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[{"severity":2,"message":"opened document","range":{"start":{"line":0,"character":0}}}]}})
    elif msg.get("method") == "textDocument/didChange":
        uri = msg["params"]["textDocument"]["uri"]
        send({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":uri,"diagnostics":[{"severity":1,"message":"changed document","range":{"start":{"line":0,"character":1}}}]}})
    elif msg.get("method") == "shutdown":
        send({"jsonrpc":"2.0","id":msg["id"],"result":None})
"#,
        )
        .unwrap();
        let mut session = WarmDiagnosticSession::new_with_command(
            root.clone(),
            "python".to_string(),
            LspCommand {
                bin: python,
                args: vec![server.display().to_string()],
            },
        );

        let first = session.run(&["app.py".to_string()]);
        fs::write(root.join("app.py"), "print('changed')\n").unwrap();
        let second = session.run(&["app.py".to_string()]);

        assert_eq!(first.engine, "lsp publishDiagnostics");
        assert!(first.stdout.contains("opened document"));
        assert!(second.stdout.contains("changed document"));
        assert!(second
            .note
            .as_deref()
            .unwrap()
            .contains("warmed LSP session"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn render_text_includes_lsp_server_status() {
        let report = DiagnosticReport {
            language: "rust".to_string(),
            engine: "cargo check".to_string(),
            lsp_server: "rust-analyzer".to_string(),
            lsp_available: false,
            command: "cargo check".to_string(),
            cwd: ".".to_string(),
            checked_files: vec!["src/lib.rs".to_string()],
            status: DiagnosticStatus::Passed,
            stdout: String::new(),
            stderr: String::new(),
            note: Some("fallback".to_string()),
        };

        let text = report.render_text();

        assert!(text.contains("diagnostics: passed"));
        assert!(text.contains("lsp_server: rust-analyzer (missing)"));
        assert!(text.contains("src/lib.rs"));
    }
}

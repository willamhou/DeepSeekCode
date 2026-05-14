#![cfg(all(unix, target_os = "linux"))]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use deepseek_code::tools::exec_shell::{
    ExecShellCancelTool, ExecShellInteractTool, ExecShellResizeTool,
};
use deepseek_code::tools::types::{Tool, ToolInput};

#[test]
fn shell_supervisor_native_pty_survives_start_connection_exit() {
    let root = temp_root("shell-supervisor-owner-exit");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = request(
        &socket,
        r#"{"method":"start","arguments":{"command":"echo owner-exit-ready; cat","tty":true,"tty_rows":24,"tty_cols":80}}"#,
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = request(
            &socket,
            &format!(
                r#"{{"method":"replay","arguments":{{"task_id":"{task_id}","stream":"terminal","cursor":0,"limit_bytes":4000}}}}"#
            ),
        );
        response.contains("owner-exit-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("terminal replay never observed PTY output for task {task_id}"));
    assert_contains(&replay, "stream: terminal");
    assert_contains(&replay, "owner-exit-ready");

    let stdin = ExecShellInteractTool {
        tool_name: "exec_shell_interact",
    }
    .execute(
        ToolInput::new()
            .with_arg("cwd", root.display().to_string())
            .with_arg("task_id", task_id.clone())
            .with_arg("input", "from-fresh-client\n")
            .with_arg("timeout_ms", "100"),
    )
    .unwrap();
    assert_contains(&stdin.summary, "meta.supervisor_forwarded=true");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = request(
            &socket,
            &format!(
                r#"{{"method":"replay","arguments":{{"task_id":"{task_id}","stream":"terminal","cursor":0,"limit_bytes":4000}}}}"#
            ),
        );
        response.contains("from-fresh-client").then_some(response)
    })
    .unwrap_or_else(|| panic!("terminal replay never observed forwarded stdin for task {task_id}"));
    assert_contains(&replay, "from-fresh-client");

    let resize = ExecShellResizeTool
        .execute(
            ToolInput::new()
                .with_arg("cwd", root.display().to_string())
                .with_arg("task_id", task_id.clone())
                .with_arg("tty_rows", "33")
                .with_arg("tty_cols", "101"),
        )
        .unwrap();
    assert_contains(&resize.summary, "meta.supervisor_forwarded=true");
    assert_contains(&resize.summary, "meta.live_resize=native_tiocswinsz");

    let attach = request(
        &socket,
        &format!(
            r#"{{"method":"attach","arguments":{{"task_id":"{task_id}","cursor":0,"limit_bytes":4000}}}}"#
        ),
    );
    assert_contains(&attach, "mode: terminal_event_attach");
    assert_contains(&attach, "rows=33 cols=101");

    let cancel = ExecShellCancelTool
        .execute(
            ToolInput::new()
                .with_arg("cwd", root.display().to_string())
                .with_arg("task_id", task_id),
        )
        .unwrap();
    assert_contains(&cancel.summary, "meta.supervisor_forwarded=true");
    assert_contains(&cancel.summary, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

struct SupervisorGuard {
    child: Option<Child>,
}

impl SupervisorGuard {
    fn wait(mut self) -> std::io::Result<ExitStatus> {
        self.child.take().unwrap().wait()
    }
}

impl Drop for SupervisorGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_shell_supervisor(root: &Path) -> SupervisorGuard {
    let child = Command::new(deepseek_bin())
        .args(["agents", "shell-supervisor", "--json"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn deepseek shell supervisor");
    SupervisorGuard { child: Some(child) }
}

fn deepseek_bin() -> PathBuf {
    option_env!("CARGO_BIN_EXE_deepseek")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/deepseek"))
}

fn request(socket: &Path, body: &str) -> String {
    let mut stream = UnixStream::connect(socket)
        .unwrap_or_else(|error| panic!("connect {}: {error}", socket.display()));
    stream.write_all(body.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    line
}

fn wait_for_socket(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            let _ = stream.write_all(br#"{"method":"health"}"#);
            let _ = stream.write_all(b"\n");
            let _ = stream.flush();
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_ok()
                && line.contains(r#""status":"ok""#)
            {
                return;
            }
        }
        thread::sleep(Duration::from_millis(25));
    }
    panic!(
        "shell supervisor socket was not ready: {}",
        socket.display()
    );
}

fn poll_until<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(value) = f() {
            return Some(value);
        }
        thread::sleep(Duration::from_millis(25));
    }
    None
}

fn json_string_field(json: &str, field: &str) -> Option<String> {
    let needle = format!(r#""{field}":""#);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "expected `{needle}` in response:\n{haystack}"
    );
}

fn temp_root(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        % 100_000;
    let short_name = name
        .split('-')
        .filter_map(|part| part.chars().next())
        .collect::<String>();
    std::env::temp_dir().join(format!("ds-{short_name}-{}-{suffix}", std::process::id()))
}

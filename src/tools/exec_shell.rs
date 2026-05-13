use crate::error::{app_error, AppResult};
use crate::tools::run_shell::{is_safe_shell_command, RunShellTool};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_WAIT_MS: u64 = 5_000;
const MAX_TIMEOUT_MS: u64 = 600_000;

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);
static SHELL_JOBS: OnceLock<Mutex<BackgroundShellManager>> = OnceLock::new();

pub struct ExecShellTool;

pub struct ExecShellWaitTool {
    pub tool_name: &'static str,
}

pub struct ExecShellListTool;

pub struct ExecShellShowTool;

pub struct ExecShellInteractTool {
    pub tool_name: &'static str,
}

pub struct ExecShellCancelTool;

pub struct TaskShellStartTool;

pub struct TaskShellWaitTool;

pub fn run_trusted_background_shell(command: &str, cwd: &str) -> AppResult<ToolOutput> {
    let command = command.trim();
    if command.is_empty() {
        return Err(app_error("trusted background shell requires a command"));
    }
    let task_id = shell_manager().lock().unwrap().spawn(command, cwd, None)?;
    Ok(ToolOutput {
        summary: format!(
            "task_id: {task_id}\nstatus: running\ncommand: {command}\ncwd: {cwd}\ntrusted_foreground_approval: true\nPoll with exec_shell_wait task_id={task_id} or cancel with exec_shell_cancel task_id={task_id}."
        ),
    })
}

impl Tool for ExecShellTool {
    fn name(&self) -> &str {
        "exec_shell"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let command = input
            .get("command")
            .ok_or_else(|| app_error("exec_shell requires a command"))?;
        if !is_safe_shell_command(command) {
            return Err(app_error(format!("command not allowed: {command}")));
        }
        let background = truthy(input.get("background"));
        if !background {
            let mut shell_input = ToolInput::new().with_arg("command", command.to_string());
            if let Some(cwd) = input.get("cwd") {
                shell_input = shell_input.with_arg("cwd", cwd.to_string());
            }
            return RunShellTool.execute(shell_input);
        }

        let cwd = input.get("cwd").unwrap_or(".");
        let stdin = input
            .get("stdin")
            .or_else(|| input.get("input"))
            .or_else(|| input.get("data"));
        let task_id = shell_manager().lock().unwrap().spawn(command, cwd, stdin)?;
        Ok(ToolOutput {
            summary: format!(
                "task_id: {task_id}\nstatus: running\ncommand: {command}\ncwd: {cwd}\nPoll with exec_shell_wait task_id={task_id} or cancel with exec_shell_cancel task_id={task_id}."
            ),
        })
    }
}

impl Tool for TaskShellStartTool {
    fn name(&self) -> &str {
        "task_shell_start"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let command = input
            .get("command")
            .ok_or_else(|| app_error("task_shell_start requires a command"))?;
        let mut shell_input = ToolInput::new()
            .with_arg("command", command.to_string())
            .with_arg("background", "true");
        if let Some(cwd) = input.get("cwd") {
            shell_input = shell_input.with_arg("cwd", cwd.to_string());
        }
        if let Some(stdin) = input.get("stdin").or_else(|| input.get("input")) {
            shell_input = shell_input.with_arg("stdin", stdin.to_string());
        }
        if let Some(timeout_ms) = input.get("timeout_ms") {
            shell_input = shell_input.with_arg("timeout_ms", timeout_ms.to_string());
        }
        let mut output = ExecShellTool.execute(shell_input)?;
        output.summary = output
            .summary
            .replace("Poll with exec_shell_wait", "Poll with task_shell_wait");
        output.summary.push_str("\nmeta.task_shell=true");
        if input.get("tty").is_some() {
            output.summary.push_str("\nmeta.tty_compat=accepted");
        }
        Ok(output)
    }
}

impl Tool for TaskShellWaitTool {
    fn name(&self) -> &str {
        "task_shell_wait"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let mut output = ExecShellWaitTool {
            tool_name: "task_shell_wait",
        }
        .execute(input.clone())?;
        if let Some(gate) = input
            .get("gate")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            output.summary = format!("meta.gate={gate}\n{}", output.summary);
        }
        if let Some(command) = input
            .get("command")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            output.summary = format!("meta.command={command}\n{}", output.summary);
        }
        Ok(output)
    }
}

impl Tool for ExecShellWaitTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let task_id = required_task_id(&input)?;
        let wait = input.get("wait").map_or(true, |value| truthy(Some(value)));
        let timeout_ms = input_u64(&input, "timeout_ms", DEFAULT_WAIT_MS).min(MAX_TIMEOUT_MS);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let mut manager = shell_manager().lock().unwrap();
            manager.refresh(task_id)?;
            if !wait || manager.is_finished(task_id)? || Instant::now() >= deadline {
                return Ok(ToolOutput {
                    summary: manager.render_delta(task_id)?,
                });
            }
            drop(manager);
            thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Tool for ExecShellListTool {
    fn name(&self) -> &str {
        "exec_shell_list"
    }

    fn execute(&self, _input: ToolInput) -> AppResult<ToolOutput> {
        let mut manager = shell_manager().lock().unwrap();
        manager.refresh_all()?;
        Ok(ToolOutput {
            summary: manager.render_list(),
        })
    }
}

impl Tool for ExecShellShowTool {
    fn name(&self) -> &str {
        "exec_shell_show"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let task_id = required_task_id(&input)?;
        let mut manager = shell_manager().lock().unwrap();
        manager.refresh(task_id)?;
        Ok(ToolOutput {
            summary: manager.render_snapshot(task_id)?,
        })
    }
}

impl Tool for ExecShellInteractTool {
    fn name(&self) -> &str {
        self.tool_name
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let task_id = required_task_id(&input)?;
        let data = input
            .get("input")
            .or_else(|| input.get("stdin"))
            .or_else(|| input.get("data"))
            .unwrap_or("");
        let close_stdin = truthy(input.get("close_stdin"));
        let timeout_ms = input_u64(&input, "timeout_ms", 1_000).min(MAX_TIMEOUT_MS);
        {
            let mut manager = shell_manager().lock().unwrap();
            manager.write_stdin(task_id, data, close_stdin)?;
        }
        ExecShellWaitTool {
            tool_name: self.tool_name,
        }
        .execute(
            ToolInput::new()
                .with_arg("task_id", task_id.to_string())
                .with_arg("timeout_ms", timeout_ms.to_string()),
        )
    }
}

impl Tool for ExecShellCancelTool {
    fn name(&self) -> &str {
        "exec_shell_cancel"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        if truthy(input.get("all")) {
            let cancelled = shell_manager().lock().unwrap().cancel_all()?;
            return Ok(ToolOutput {
                summary: if cancelled.is_empty() {
                    "No running background shell jobs.".to_string()
                } else {
                    format!(
                        "Canceled {} background shell job{}: {}",
                        cancelled.len(),
                        if cancelled.len() == 1 { "" } else { "s" },
                        cancelled.join(", ")
                    )
                },
            });
        }
        let task_id = required_task_id(&input)?;
        shell_manager().lock().unwrap().cancel(task_id)?;
        Ok(ToolOutput {
            summary: format!("Canceled background shell job: {task_id}"),
        })
    }
}

struct BackgroundShellManager {
    jobs: BTreeMap<String, BackgroundShellJob>,
}

struct BackgroundShellJob {
    id: String,
    command: String,
    cwd: String,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    stdout_cursor: usize,
    stderr_cursor: usize,
    stdout_reader: Option<thread::JoinHandle<std::io::Result<()>>>,
    stderr_reader: Option<thread::JoinHandle<std::io::Result<()>>>,
    status: ShellJobStatus,
    exit_code: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellJobStatus {
    Running,
    Completed,
    Failed,
    Killed,
}

impl BackgroundShellManager {
    fn spawn(&mut self, command: &str, cwd: &str, stdin_data: Option<&str>) -> AppResult<String> {
        let mut process = Command::new("sh");
        process
            .args(["-lc", command])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut process);
        let mut child = process.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| app_error("exec_shell child produced no stdout pipe"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| app_error("exec_shell child produced no stderr pipe"))?;
        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::new(Mutex::new(Vec::new()));
        let stdout_reader = spawn_reader(stdout, stdout_buffer.clone());
        let stderr_reader = spawn_reader(stderr, stderr_buffer.clone());
        let mut stdin = child.stdin.take();
        if let Some(data) = stdin_data {
            if let Some(handle) = stdin.as_mut() {
                handle.write_all(data.as_bytes())?;
                handle.flush()?;
            }
        }

        let id = generated_job_id();
        self.jobs.insert(
            id.clone(),
            BackgroundShellJob {
                id: id.clone(),
                command: command.to_string(),
                cwd: cwd.to_string(),
                child: Some(child),
                stdin,
                stdout: stdout_buffer,
                stderr: stderr_buffer,
                stdout_cursor: 0,
                stderr_cursor: 0,
                stdout_reader: Some(stdout_reader),
                stderr_reader: Some(stderr_reader),
                status: ShellJobStatus::Running,
                exit_code: None,
            },
        );
        Ok(id)
    }

    fn refresh(&mut self, task_id: &str) -> AppResult<()> {
        let job = self
            .jobs
            .get_mut(task_id)
            .ok_or_else(|| app_error(format!("unknown background shell task: {task_id}")))?;
        if job.status != ShellJobStatus::Running {
            return Ok(());
        }
        let Some(child) = job.child.as_mut() else {
            return Ok(());
        };
        if let Some(status) = child.try_wait()? {
            job.exit_code = status.code();
            job.status = if status.success() {
                ShellJobStatus::Completed
            } else {
                ShellJobStatus::Failed
            };
            job.child = None;
            job.stdin = None;
            join_reader(job.stdout_reader.take(), "stdout")?;
            join_reader(job.stderr_reader.take(), "stderr")?;
        }
        Ok(())
    }

    fn is_finished(&self, task_id: &str) -> AppResult<bool> {
        let job = self
            .jobs
            .get(task_id)
            .ok_or_else(|| app_error(format!("unknown background shell task: {task_id}")))?;
        Ok(job.status != ShellJobStatus::Running)
    }

    fn refresh_all(&mut self) -> AppResult<()> {
        let ids = self.jobs.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            self.refresh(&id)?;
        }
        Ok(())
    }

    fn render_list(&self) -> String {
        if self.jobs.is_empty() {
            return "No background shell jobs.".to_string();
        }

        let mut lines = vec![format!("Background shell jobs: {}", self.jobs.len())];
        for job in self.jobs.values() {
            let stdout_total = job.stdout.lock().unwrap().len();
            let stderr_total = job.stderr.lock().unwrap().len();
            lines.push(format!(
                "- {} [{}] exit={} stdout={} stderr={} cwd={}",
                job.id,
                job.status.as_str(),
                job.exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                stdout_total,
                stderr_total,
                job.cwd
            ));
            lines.push(format!("  command: {}", job.command));
        }
        lines.push(
            "Controls: shell show <id>, shell poll <id>, shell wait <id>, shell stdin <id> <input>, shell cancel <id>."
                .to_string(),
        );
        lines.join("\n")
    }

    fn render_delta(&mut self, task_id: &str) -> AppResult<String> {
        let job = self
            .jobs
            .get_mut(task_id)
            .ok_or_else(|| app_error(format!("unknown background shell task: {task_id}")))?;
        let stdout_delta = read_delta(&job.stdout, &mut job.stdout_cursor)?;
        let stderr_delta = read_delta(&job.stderr, &mut job.stderr_cursor)?;
        let stdout_total = job.stdout.lock().unwrap().len();
        let stderr_total = job.stderr.lock().unwrap().len();
        let mut out = format!(
            "task_id: {}\nstatus: {}\nexit_code: {}\ncommand: {}\ncwd: {}\nstdout_total_bytes: {stdout_total}\nstderr_total_bytes: {stderr_total}\n",
            job.id,
            job.status.as_str(),
            job.exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "null".to_string()),
            job.command,
            job.cwd
        );
        if !stdout_delta.trim().is_empty() {
            out.push_str("stdout_delta:\n");
            out.push_str(stdout_delta.trim_end());
            out.push('\n');
        }
        if !stderr_delta.trim().is_empty() {
            out.push_str("stderr_delta:\n");
            out.push_str(stderr_delta.trim_end());
            out.push('\n');
        }
        Ok(out.trim_end().to_string())
    }

    fn render_snapshot(&self, task_id: &str) -> AppResult<String> {
        let job = self
            .jobs
            .get(task_id)
            .ok_or_else(|| app_error(format!("unknown background shell task: {task_id}")))?;
        let stdout = String::from_utf8_lossy(&job.stdout.lock().unwrap()).to_string();
        let stderr = String::from_utf8_lossy(&job.stderr.lock().unwrap()).to_string();
        let stdout_total = stdout.len();
        let stderr_total = stderr.len();
        let mut out = format!(
            "task_id: {}\nstatus: {}\nexit_code: {}\ncommand: {}\ncwd: {}\nstdout_total_bytes: {stdout_total}\nstderr_total_bytes: {stderr_total}\n",
            job.id,
            job.status.as_str(),
            job.exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "null".to_string()),
            job.command,
            job.cwd
        );
        if !stdout.trim().is_empty() {
            out.push_str("stdout:\n");
            out.push_str(&clip_shell_snapshot(&stdout));
            out.push('\n');
        }
        if !stderr.trim().is_empty() {
            out.push_str("stderr:\n");
            out.push_str(&clip_shell_snapshot(&stderr));
            out.push('\n');
        }
        Ok(out.trim_end().to_string())
    }

    fn write_stdin(&mut self, task_id: &str, data: &str, close_stdin: bool) -> AppResult<()> {
        self.refresh(task_id)?;
        let job = self
            .jobs
            .get_mut(task_id)
            .ok_or_else(|| app_error(format!("unknown background shell task: {task_id}")))?;
        if job.status != ShellJobStatus::Running {
            return Err(app_error(format!(
                "background shell task {task_id} is {}",
                job.status.as_str()
            )));
        }
        let Some(stdin) = job.stdin.as_mut() else {
            return Err(app_error(format!(
                "stdin is not available for background shell task {task_id}"
            )));
        };
        if !data.is_empty() {
            stdin.write_all(data.as_bytes())?;
            stdin.flush()?;
        }
        if close_stdin {
            job.stdin = None;
        }
        Ok(())
    }

    fn cancel(&mut self, task_id: &str) -> AppResult<()> {
        let job = self
            .jobs
            .get_mut(task_id)
            .ok_or_else(|| app_error(format!("unknown background shell task: {task_id}")))?;
        if let Some(child) = job.child.as_mut() {
            kill_child_process_group(child);
            let _ = child.wait();
        }
        job.child = None;
        job.stdin = None;
        job.status = ShellJobStatus::Killed;
        join_reader(job.stdout_reader.take(), "stdout")?;
        join_reader(job.stderr_reader.take(), "stderr")?;
        Ok(())
    }

    fn cancel_all(&mut self) -> AppResult<Vec<String>> {
        let ids = self
            .jobs
            .iter()
            .filter_map(|(id, job)| {
                if job.status == ShellJobStatus::Running {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for id in &ids {
            self.cancel(id)?;
        }
        Ok(ids)
    }
}

impl Default for BackgroundShellManager {
    fn default() -> Self {
        Self {
            jobs: BTreeMap::new(),
        }
    }
}

impl ShellJobStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Killed => "killed",
        }
    }
}

fn shell_manager() -> &'static Mutex<BackgroundShellManager> {
    SHELL_JOBS.get_or_init(|| Mutex::new(BackgroundShellManager::default()))
}

fn required_task_id(input: &ToolInput) -> AppResult<&str> {
    input
        .get("task_id")
        .or_else(|| input.get("id"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| app_error("background shell task_id is required"))
}

fn input_u64(input: &ToolInput, key: &str, default: u64) -> u64 {
    input
        .get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn clip_shell_snapshot(value: &str) -> String {
    const MAX_CHARS: usize = 20_000;
    let trimmed = value.trim_end();
    if trimmed.chars().count() <= MAX_CHARS {
        return trimmed.to_string();
    }
    let mut clipped = trimmed.chars().rev().take(MAX_CHARS).collect::<Vec<_>>();
    clipped.reverse();
    format!(
        "[truncated to last {MAX_CHARS} chars]\n{}",
        clipped.into_iter().collect::<String>()
    )
}

fn truthy(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
    )
}

fn generated_job_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("shell-{}-{nanos}-{counter}", std::process::id())
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    buffer: Arc<Mutex<Vec<u8>>>,
) -> thread::JoinHandle<std::io::Result<()>> {
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                return Ok(());
            }
            buffer.lock().unwrap().extend_from_slice(&chunk[..read]);
        }
    })
}

fn join_reader(
    handle: Option<thread::JoinHandle<std::io::Result<()>>>,
    stream_name: &str,
) -> AppResult<()> {
    let Some(handle) = handle else {
        return Ok(());
    };
    match handle.join() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(app_error(format!(
            "failed to read exec_shell {stream_name}: {error}"
        ))),
        Err(_) => Err(app_error(format!(
            "exec_shell {stream_name} reader panicked"
        ))),
    }
}

fn read_delta(buffer: &Arc<Mutex<Vec<u8>>>, cursor: &mut usize) -> AppResult<String> {
    let guard = buffer.lock().unwrap();
    let start = (*cursor).min(guard.len());
    let delta = String::from_utf8_lossy(&guard[start..]).to_string();
    *cursor = guard.len();
    Ok(delta)
}

#[cfg(unix)]
fn configure_process_group(process: &mut Command) {
    use std::os::unix::process::CommandExt;
    process.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_process: &mut Command) {}

fn kill_child_process_group(child: &mut Child) {
    #[cfg(unix)]
    {
        const SIGKILL: i32 = 9;
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        let process_group = -(child.id() as i32);
        unsafe {
            let _ = kill(process_group, SIGKILL);
        }
    }
    let _ = child.kill();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_id_from(summary: &str) -> String {
        summary
            .lines()
            .find_map(|line| line.strip_prefix("task_id: "))
            .expect("task_id line")
            .to_string()
    }

    #[test]
    fn exec_shell_foreground_delegates_to_run_shell() {
        let output = ExecShellTool
            .execute(ToolInput::new().with_arg("command", "echo hello"))
            .unwrap();
        assert!(output.summary.contains("meta.result=ok"));
        assert!(output.summary.contains("hello"));
    }

    #[test]
    fn exec_shell_background_wait_reports_completion() {
        let started = ExecShellTool
            .execute(
                ToolInput::new()
                    .with_arg("command", "echo ready")
                    .with_arg("background", "true"),
            )
            .unwrap();
        let task_id = task_id_from(&started.summary);
        let waited = ExecShellWaitTool {
            tool_name: "exec_shell_wait",
        }
        .execute(
            ToolInput::new()
                .with_arg("task_id", task_id)
                .with_arg("timeout_ms", "1000"),
        )
        .unwrap();
        assert!(waited.summary.contains("status: completed"));
        assert!(waited.summary.contains("stdout_delta:"));
        assert!(waited.summary.contains("ready"));
    }

    #[test]
    fn exec_shell_list_and_show_report_jobs() {
        let started = ExecShellTool
            .execute(
                ToolInput::new()
                    .with_arg("command", "echo listed")
                    .with_arg("background", "true"),
            )
            .unwrap();
        let task_id = task_id_from(&started.summary).to_string();
        let _ = ExecShellWaitTool {
            tool_name: "exec_shell_wait",
        }
        .execute(
            ToolInput::new()
                .with_arg("task_id", task_id.clone())
                .with_arg("timeout_ms", "1000"),
        )
        .unwrap();

        let listed = ExecShellListTool.execute(ToolInput::new()).unwrap();
        assert!(listed.summary.contains(&task_id), "{}", listed.summary);
        assert!(listed.summary.contains("echo listed"), "{}", listed.summary);

        let shown = ExecShellShowTool
            .execute(ToolInput::new().with_arg("task_id", task_id))
            .unwrap();
        assert!(shown.summary.contains("stdout:"), "{}", shown.summary);
        assert!(shown.summary.contains("listed"), "{}", shown.summary);
    }

    #[test]
    fn task_shell_start_and_wait_alias_background_shell() {
        let started = TaskShellStartTool
            .execute(ToolInput::new().with_arg("command", "echo task-ready"))
            .unwrap();
        assert!(started.summary.contains("Poll with task_shell_wait"));
        assert!(started.summary.contains("meta.task_shell=true"));
        let task_id = task_id_from(&started.summary);
        let waited = TaskShellWaitTool
            .execute(
                ToolInput::new()
                    .with_arg("task_id", task_id)
                    .with_arg("timeout_ms", "1000")
                    .with_arg("gate", "test")
                    .with_arg("command", "echo task-ready"),
            )
            .unwrap();
        assert!(waited.summary.contains("meta.gate=test"));
        assert!(waited.summary.contains("meta.command=echo task-ready"));
        assert!(waited.summary.contains("status: completed"));
        assert!(waited.summary.contains("task-ready"));
    }

    #[test]
    fn exec_shell_interact_sends_stdin_and_closes_it() {
        let started = ExecShellTool
            .execute(
                ToolInput::new()
                    .with_arg("command", "cat -")
                    .with_arg("background", "true"),
            )
            .unwrap();
        let task_id = task_id_from(&started.summary);
        let interacted = ExecShellInteractTool {
            tool_name: "exec_shell_interact",
        }
        .execute(
            ToolInput::new()
                .with_arg("task_id", task_id)
                .with_arg("input", "hello stdin\n")
                .with_arg("close_stdin", "true")
                .with_arg("timeout_ms", "1000"),
        )
        .unwrap();
        assert!(interacted.summary.contains("status: completed"));
        assert!(interacted.summary.contains("hello stdin"));
    }

    #[test]
    fn exec_shell_cancel_kills_running_job() {
        let started = ExecShellTool
            .execute(
                ToolInput::new()
                    .with_arg("command", "tail -f /dev/null")
                    .with_arg("background", "true"),
            )
            .unwrap();
        let task_id = task_id_from(&started.summary);
        let cancelled = ExecShellCancelTool
            .execute(ToolInput::new().with_arg("task_id", task_id))
            .unwrap();
        assert!(cancelled.summary.contains("Canceled background shell job"));
    }
}

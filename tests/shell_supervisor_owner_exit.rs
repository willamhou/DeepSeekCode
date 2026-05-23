#![cfg(all(unix, target_os = "linux"))]

use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
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
        r#"{"method":"start","arguments":{"command":"echo owner-exit-ready; while read line; do stty size; echo \"$line\"; done","tty":true,"tty_rows":24,"tty_cols":80}}"#,
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

    let stdin = ExecShellInteractTool {
        tool_name: "exec_shell_interact",
    }
    .execute(
        ToolInput::new()
            .with_arg("cwd", root.display().to_string())
            .with_arg("task_id", task_id.clone())
            .with_arg("input", "probe-size\n")
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
        (response.contains("33 101") && response.contains("probe-size")).then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("terminal replay never observed child PTY resize output for task {task_id}")
    });
    assert_contains(&replay, "33 101");
    assert_contains(&replay, "probe-size");

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

#[test]
fn agents_shell_cli_controls_supervised_native_pty() {
    let root = temp_root("shell-supervisor-cli-control");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo cli-ready; while read line; do stty size; echo "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("cli-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("agents shell replay never observed PTY output for {task_id}"));
    assert_contains(&replay, "stream: terminal");
    assert_contains(&replay, "cli-ready");

    let resize = deepseek_cli(&root, &["agents", "shell", "resize", &task_id, "31", "99"]);
    assert_contains(&resize, "meta.live_resize=native_tiocswinsz");

    let stdin = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "stdin",
            &task_id,
            "--input",
            "cli-probe\n",
            "--timeout-ms",
            "100",
        ],
    );
    assert_contains(&stdin, "status: running");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("31 99") && response.contains("cli-probe")).then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("agents shell replay never observed CLI child resize output for {task_id}")
    });
    assert_contains(&replay, "31 99");
    assert_contains(&replay, "cli-probe");

    let byte_stream = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "byte-stream",
            &task_id,
            "--rows",
            "34",
            "--cols",
            "102",
            "--input",
            "byte-probe\n",
            "--max-ms",
            "1000",
            "--poll-ms",
            "25",
            "--max-events",
            "8",
            "--limit-bytes",
            "4096",
            "--json",
        ],
    );
    assert_contains(&byte_stream, r#""method":"byte_stream""#);
    assert_contains(&byte_stream, r#""stream_method":"pty_byte_stream""#);
    assert_contains(&byte_stream, r#""control""#);
    assert_contains(&byte_stream, "resize_summary");
    assert_contains(&byte_stream, "stdin_summary");
    assert_contains(&byte_stream, r#""byte_outputs""#);
    assert_contains(&byte_stream, r#""bytes_base64""#);
    assert_contains(&byte_stream, "34 102");
    assert_contains(&byte_stream, "byte-probe");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn shell_supervisor_byte_stream_accepts_duplex_control_frames() {
    let root = temp_root("shell-supervisor-byte-stream-duplex");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo frame-ready; while IFS= read -r line; do stty size; printf 'duplex:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("frame-ready").then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("byte stream duplex smoke never observed PTY readiness for {task_id}")
    });

    let mut stream = UnixStream::connect(&socket)
        .unwrap_or_else(|error| panic!("connect {}: {error}", socket.display()));
    stream
        .write_all(
            format!(
                r#"{{"method":"byte_stream","arguments":{{"task_id":"{task_id}","cursor":0,"limit_bytes":4096,"max_ms":1200,"max_events":12,"poll_ms":25}}}}"#
            )
            .as_bytes(),
        )
        .unwrap();
    stream.write_all(b"\n").unwrap();
    stream
        .write_all(br#"{"type":"resize","rows":36,"cols":104}"#)
        .unwrap();
    stream.write_all(b"\n").unwrap();
    stream
        .write_all(br#"{"type":"stdin","input":"frame-probe\n"}"#)
        .unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();

    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    assert_contains(&body, r#""method":"byte_stream""#);
    assert_contains(&body, r#""stream_method":"pty_byte_stream""#);
    assert_contains(&body, r#""control_frames""#);
    assert_contains(&body, "resize_summary");
    assert_contains(&body, "stdin_summary");
    assert_contains(&body, r#""byte_outputs""#);
    assert_contains(&body, r#""bytes_base64""#);
    assert_contains(&body, "36 104");
    assert_contains(&body, "duplex:frame-probe");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn shell_supervisor_byte_stream_raw_proxy_forwards_socket_bytes() {
    let root = temp_root("shell-supervisor-byte-stream-raw");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo raw-ready; while IFS= read -r line; do printf 'raw:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("raw-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("raw proxy smoke never observed PTY readiness for {task_id}"));

    let mut stream = UnixStream::connect(&socket)
        .unwrap_or_else(|error| panic!("connect {}: {error}", socket.display()));
    stream
        .write_all(
            format!(
                r#"{{"method":"byte_stream","arguments":{{"task_id":"{task_id}","cursor":0,"limit_bytes":4096,"max_ms":1200,"poll_ms":10,"raw_proxy":true}}}}"#
            )
            .as_bytes(),
        )
        .unwrap();
    stream.write_all(b"\nraw-probe\n").unwrap();
    stream.flush().unwrap();
    stream.shutdown(std::net::Shutdown::Write).unwrap();

    let mut body = String::new();
    stream.read_to_string(&mut body).unwrap();
    assert_contains(&body, "raw-ready");
    assert_contains(&body, "raw:raw-probe");
    assert!(
        !body.contains("byte_outputs"),
        "raw proxy should return PTY bytes, not JSON frames:\n{body}"
    );

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn shell_supervisor_pty_fd_handoff_passes_master_fd() {
    let root = temp_root("shell-supervisor-pty-fd");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo fd-ready; while IFS= read -r line; do stty size; printf 'fd:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    let mut stream = UnixStream::connect(&socket)
        .unwrap_or_else(|error| panic!("connect {}: {error}", socket.display()));
    stream
        .write_all(
            format!(
                r#"{{"method":"pty_fd","arguments":{{"task_id":"{task_id}","tty_rows":37,"tty_cols":105,"max_ms":1500}}}}"#
            )
            .as_bytes(),
        )
        .unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();
    let response = read_unix_line(&mut stream);
    assert_contains(&response, r#""status":"ok""#);
    assert_contains(&response, r#""stream_method":"pty_fd_handoff""#);
    assert_contains(&response, r#""handoff":"scm_rights""#);

    let mut pty = recv_unix_fd(&stream);
    set_nonblocking(pty.as_raw_fd());
    pty.write_all(b"fd-probe\n").unwrap();
    pty.flush().unwrap();
    let mut observed = String::new();
    let output = poll_until(Duration::from_secs(3), || {
        let bytes = read_pty_output(&mut pty);
        if !bytes.is_empty() {
            observed.push_str(&String::from_utf8_lossy(&bytes));
        }
        if observed.contains("fd:fd-probe") && observed.contains("37 105") {
            Some(observed.clone())
        } else {
            None
        }
    })
    .unwrap_or_else(|| panic!("pty_fd handoff never observed direct PTY output for {task_id}"));
    assert_contains(&output, "fd:fd-probe");
    assert_contains(&output, "37 105");
    drop(pty);
    drop(stream);

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd_handoff")
            && response.contains("] started")
            && response.contains("] ended"))
        .then_some(response)
    })
    .unwrap_or_else(|| panic!("terminal replay never observed fd_handoff events for {task_id}"));
    assert_contains(&replay, "fd_handoff");
    assert_contains(&replay, "] started");
    assert_contains(&replay, "] ended");

    let resize = deepseek_cli(&root, &["agents", "shell", "resize", &task_id, "38", "106"]);
    assert_contains(&resize, "meta.live_resize=native_tiocswinsz");

    let stdin = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "stdin",
            &task_id,
            "--input",
            "after-handoff\n",
            "--timeout-ms",
            "100",
        ],
    );
    assert_contains(&stdin, "status: running");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd:after-handoff") && response.contains("38 106")).then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("terminal replay never resumed after pty_fd handoff ended for {task_id}")
    });
    assert_contains(&replay, "fd:after-handoff");
    assert_contains(&replay, "38 106");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn agents_shell_attach_interactive_smoke_forwards_input_and_detaches() {
    let root = temp_root("shell-supervisor-interactive-attach");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo interactive-ready; while IFS= read -r line; do stty size; printf 'interactive:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("interactive-ready").then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("interactive attach smoke never observed PTY readiness for {task_id}")
    });

    let attach = run_interactive_attach_under_pty(
        &root,
        &task_id,
        "deepseek-interactive-probe\n",
        Duration::from_secs(6),
    );
    assert!(
        !attach.timed_out,
        "interactive attach smoke timed out\nstdout:\n{}\nstderr:\n{}",
        attach.stdout, attach.stderr
    );
    assert!(
        attach.status.success(),
        "interactive attach exited with {}\nstdout:\n{}\nstderr:\n{}",
        attach.status,
        attach.stdout,
        attach.stderr
    );
    let transcript = format!("{}\n{}", attach.stdout, attach.stderr);
    assert_contains(&transcript, "attached to");
    assert_contains(&transcript, "deepseek-interactive-probe");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("interactive:deepseek-interactive-probe") && response.contains("33 101"))
            .then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("interactive attach smoke never observed forwarded stdin/resize for {task_id}")
    });
    assert_contains(&replay, "interactive:deepseek-interactive-probe");
    assert_contains(&replay, "33 101");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn agents_shell_proxy_smoke_uses_raw_proxy_and_terminal_size() {
    let root = temp_root("shell-supervisor-terminal-proxy");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo proxy-ready; while IFS= read -r line; do stty size; printf 'proxy:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("proxy-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("terminal proxy smoke never observed PTY readiness for {task_id}"));

    let proxy = run_shell_proxy_under_pty(
        &root,
        &task_id,
        "deepseek-proxy-probe\n",
        Duration::from_secs(6),
    );
    assert!(
        !proxy.timed_out,
        "terminal proxy smoke timed out\nstdout:\n{}\nstderr:\n{}",
        proxy.stdout, proxy.stderr
    );
    assert!(
        proxy.status.success(),
        "terminal proxy exited with {}\nstdout:\n{}\nstderr:\n{}",
        proxy.status,
        proxy.stdout,
        proxy.stderr
    );
    let transcript = format!("{}\n{}", proxy.stdout, proxy.stderr);
    assert_contains(&transcript, "proxied to");
    assert_contains(&transcript, "deepseek-proxy-probe");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("proxy:deepseek-proxy-probe") && response.contains("35 103"))
            .then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("terminal proxy smoke never observed forwarded raw stdin/size for {task_id}")
    });
    assert_contains(&replay, "proxy:deepseek-proxy-probe");
    assert_contains(&replay, "35 103");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn agents_shell_fd_proxy_smoke_receives_native_pty_fd() {
    let root = temp_root("shell-supervisor-fd-proxy");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo fd-cli-ready; while IFS= read -r line; do stty size; printf 'fd:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("fd-cli-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("fd proxy smoke never observed PTY readiness for {task_id}"));

    let proxy = run_shell_fd_proxy_under_pty(
        &root,
        &task_id,
        "deepseek-fd-proxy-probe\n",
        Duration::from_secs(6),
    );
    assert!(
        !proxy.timed_out,
        "fd proxy smoke timed out\nstdout:\n{}\nstderr:\n{}",
        proxy.stdout, proxy.stderr
    );
    assert!(
        proxy.status.success(),
        "fd proxy exited with {}\nstdout:\n{}\nstderr:\n{}",
        proxy.status,
        proxy.stdout,
        proxy.stderr
    );
    let transcript = format!("{}\n{}", proxy.stdout, proxy.stderr);
    assert_contains(&transcript, "fd-proxied to");
    assert_contains(&transcript, "fd:deepseek-fd-proxy-probe");
    assert_contains(&transcript, "36 104");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd_handoff")
            && response.contains("] started")
            && response.contains("] ended"))
        .then_some(response)
    })
    .unwrap_or_else(|| panic!("fd proxy smoke never observed fd_handoff replay for {task_id}"));
    assert_contains(&replay, "fd_handoff");
    assert_contains(&replay, "] started");
    assert_contains(&replay, "] ended");

    let resize = deepseek_cli(&root, &["agents", "shell", "resize", &task_id, "39", "107"]);
    assert_contains(&resize, "meta.live_resize=native_tiocswinsz");

    let stdin = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "stdin",
            &task_id,
            "--input",
            "after-fd-proxy\n",
            "--timeout-ms",
            "100",
        ],
    );
    assert_contains(&stdin, "status: running");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd:after-fd-proxy") && response.contains("39 107")).then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("terminal replay never resumed after fd-proxy detached for {task_id}")
    });
    assert_contains(&replay, "fd:after-fd-proxy");
    assert_contains(&replay, "39 107");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn agents_shell_fd_proxy_forwards_sigwinch_resize() {
    let root = temp_root("shell-supervisor-fd-proxy-resize");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo fd-resize-ready; while IFS= read -r line; do stty size; printf 'fd:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("fd-resize-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("fd resize smoke never observed PTY readiness for {task_id}"));

    let proxy = run_shell_fd_proxy_resize_under_pty(&root, &task_id, Duration::from_secs(6));
    assert!(
        !proxy.timed_out,
        "fd proxy resize smoke timed out\nstdout:\n{}\nstderr:\n{}",
        proxy.stdout, proxy.stderr
    );
    assert!(
        proxy.status.success(),
        "fd proxy resize exited with {}\nstdout:\n{}\nstderr:\n{}",
        proxy.status,
        proxy.stdout,
        proxy.stderr
    );
    let transcript = format!("{}\n{}", proxy.stdout, proxy.stderr);
    assert_contains(&transcript, "fd-proxied to");
    assert_contains(&transcript, "fd:resize-probe");
    assert_contains(&transcript, "41 109");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn agents_shell_fd_proxy_ctrl_c_interrupts_target_pty() {
    let root = temp_root("shell-supervisor-fd-proxy-ctrl-c");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo fd-int-ready; trap 'printf "fd:int\n"; exit 130' INT; while :; do sleep 30; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("fd-int-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("fd Ctrl-C smoke never observed PTY readiness for {task_id}"));

    let proxy = run_shell_fd_proxy_ctrl_c_under_pty(&root, &task_id, Duration::from_secs(6));
    assert!(
        !proxy.timed_out,
        "fd proxy Ctrl-C smoke timed out\nstdout:\n{}\nstderr:\n{}",
        proxy.stdout, proxy.stderr
    );
    assert!(
        proxy.status.success(),
        "fd proxy Ctrl-C exited with {}\nstdout:\n{}\nstderr:\n{}",
        proxy.status,
        proxy.stdout,
        proxy.stderr
    );
    let transcript = format!("{}\n{}", proxy.stdout, proxy.stderr);
    assert_contains(&transcript, "fd-proxied to");
    assert_contains(&transcript, "fd:int");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd_handoff")
            && response.contains("] started")
            && response.contains("] ended"))
        .then_some(response)
    })
    .unwrap_or_else(|| panic!("fd Ctrl-C smoke never observed handoff end for {task_id}"));
    assert_contains(&replay, "fd_handoff");
    assert_contains(&replay, "] ended");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn agents_shell_fd_proxy_abnormal_exit_releases_handoff() {
    let root = temp_root("shell-supervisor-fd-proxy-abnormal");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo fd-abnormal-ready; while IFS= read -r line; do stty size; printf 'fd:%s\n' "$line"; done"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("fd-abnormal-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("fd abnormal smoke never observed PTY readiness for {task_id}"));

    let proxy = run_shell_fd_proxy_kill_under_pty(&root, &task_id, Duration::from_secs(6));
    assert!(
        !proxy.timed_out,
        "fd proxy abnormal-exit smoke timed out\nstdout:\n{}\nstderr:\n{}",
        proxy.stdout, proxy.stderr
    );
    assert!(
        !proxy.status.success(),
        "fd proxy abnormal-exit smoke expected killed child, got {}\nstdout:\n{}\nstderr:\n{}",
        proxy.status,
        proxy.stdout,
        proxy.stderr
    );
    let transcript = format!("{}\n{}", proxy.stdout, proxy.stderr);
    assert_contains(&transcript, "fd-proxied to");
    assert_contains(&transcript, "fd:before-abnormal");
    assert_contains(&transcript, "36 104");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd_handoff")
            && response.contains("] started")
            && response.contains("] ended"))
        .then_some(response)
    })
    .unwrap_or_else(|| panic!("fd abnormal smoke never observed handoff end for {task_id}"));
    assert_contains(&replay, "fd_handoff");
    assert_contains(&replay, "] ended");

    let resize = deepseek_cli(&root, &["agents", "shell", "resize", &task_id, "42", "110"]);
    assert_contains(&resize, "meta.live_resize=native_tiocswinsz");

    let stdin = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "stdin",
            &task_id,
            "--input",
            "after-abnormal\n",
            "--timeout-ms",
            "100",
        ],
    );
    assert_contains(&stdin, "status: running");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd:after-abnormal") && response.contains("42 110")).then_some(response)
    })
    .unwrap_or_else(|| {
        panic!("terminal replay never resumed after fd-proxy abnormal exit for {task_id}")
    });
    assert_contains(&replay, "fd:after-abnormal");
    assert_contains(&replay, "42 110");

    let cancel = deepseek_cli(&root, &["agents", "shell", "cancel", &task_id]);
    assert_contains(&cancel, "Canceled background shell job");

    let shutdown = request(&socket, r#"{"method":"shutdown"}"#);
    assert_contains(&shutdown, r#""status":"ok""#);
    let status = supervisor.wait().unwrap();
    assert!(status.success(), "supervisor exited with {status}");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn agents_shell_fd_proxy_ctrl_d_eof_exits_cleanly() {
    let root = temp_root("shell-supervisor-fd-proxy-eof");
    fs::create_dir_all(&root).unwrap();
    let socket = root.join(".dscode/shell-supervisor/supervisor.sock");
    let supervisor = spawn_shell_supervisor(&root);
    wait_for_socket(&socket);

    let start = deepseek_cli(
        &root,
        &[
            "agents",
            "shell",
            "start",
            "--tty",
            "--rows",
            "24",
            "--cols",
            "80",
            "--json",
            "--",
            r#"echo fd-eof-ready; while IFS= read -r line; do printf 'fd:%s\n' "$line"; done; printf 'fd:eof\n'"#,
        ],
    );
    assert_contains(&start, r#""status":"ok""#);
    assert_contains(&start, r#""job_pty_backend":"native-supervisor""#);
    let task_id =
        json_string_field(&start, "task_id").expect("start response should contain task_id");

    poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        response.contains("fd-eof-ready").then_some(response)
    })
    .unwrap_or_else(|| panic!("fd EOF smoke never observed PTY readiness for {task_id}"));

    let proxy = run_shell_fd_proxy_ctrl_d_under_pty(&root, &task_id, Duration::from_secs(6));
    assert!(
        !proxy.timed_out,
        "fd proxy Ctrl-D smoke timed out\nstdout:\n{}\nstderr:\n{}",
        proxy.stdout, proxy.stderr
    );
    assert!(
        proxy.status.success(),
        "fd proxy Ctrl-D exited with {}\nstdout:\n{}\nstderr:\n{}",
        proxy.status,
        proxy.stdout,
        proxy.stderr
    );
    let transcript = format!("{}\n{}", proxy.stdout, proxy.stderr);
    assert_contains(&transcript, "fd-proxied to");
    assert_contains(&transcript, "fd:ctrl-d-probe");
    assert_contains(&transcript, "fd:eof");

    let replay = poll_until(Duration::from_secs(3), || {
        let response = deepseek_cli(
            &root,
            &[
                "agents", "shell", "replay", &task_id, "--stream", "terminal", "--cursor", "0",
            ],
        );
        (response.contains("fd_handoff")
            && response.contains("] started")
            && response.contains("] ended"))
        .then_some(response)
    })
    .unwrap_or_else(|| panic!("fd proxy Ctrl-D smoke never observed handoff end for {task_id}"));
    assert_contains(&replay, "fd_handoff");
    assert_contains(&replay, "] ended");

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

fn read_unix_line(stream: &mut UnixStream) -> String {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let read = stream.read(&mut byte).expect("read unix line byte");
        if read == 0 || byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
    }
    String::from_utf8(line).expect("unix line should be utf-8")
}

fn recv_unix_fd(stream: &UnixStream) -> File {
    let mut byte = [0u8; 1];
    let mut iov = TestIovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: byte.len(),
    };
    let mut control = vec![0u8; cmsg_space(std::mem::size_of::<i32>())];
    let mut msg = TestMsghdr {
        msg_name: std::ptr::null_mut(),
        msg_namelen: 0,
        msg_iov: &mut iov,
        msg_iovlen: 1,
        msg_control: control.as_mut_ptr().cast(),
        msg_controllen: control.len(),
        msg_flags: 0,
    };
    let received = unsafe { recvmsg(stream.as_raw_fd(), &mut msg, 0) };
    assert!(received > 0, "recvmsg did not receive fd");
    let header = control.as_ptr().cast::<TestCmsghdr>();
    unsafe {
        assert_eq!((*header).cmsg_level, SOL_SOCKET);
        assert_eq!((*header).cmsg_type, SCM_RIGHTS);
        assert!((*header).cmsg_len >= cmsg_len(std::mem::size_of::<i32>()));
    }
    let mut fd_bytes = [0u8; std::mem::size_of::<i32>()];
    unsafe {
        std::ptr::copy_nonoverlapping(
            control
                .as_ptr()
                .add(cmsg_align(std::mem::size_of::<TestCmsghdr>())),
            fd_bytes.as_mut_ptr(),
            fd_bytes.len(),
        );
    }
    let fd = i32::from_ne_bytes(fd_bytes);
    assert!(fd >= 0, "received invalid fd");
    unsafe { File::from_raw_fd(fd) }
}

fn deepseek_cli(root: &Path, args: &[&str]) -> String {
    let output = Command::new(deepseek_bin())
        .args(args)
        .current_dir(root)
        .output()
        .unwrap_or_else(|error| panic!("run deepseek {}: {error}", args.join(" ")));
    assert!(
        output.status.success(),
        "deepseek {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

struct PtyRun {
    status: ExitStatus,
    timed_out: bool,
    stdout: String,
    stderr: String,
}

fn run_interactive_attach_under_pty(
    root: &Path,
    task_id: &str,
    input: &str,
    timeout: Duration,
) -> PtyRun {
    let (mut master, slave) = open_test_pty(33, 101);
    let stdin = slave.try_clone().expect("clone pty slave for stdin");
    let stdout = slave.try_clone().expect("clone pty slave for stdout");
    let stderr = slave;
    let mut command = Command::new(deepseek_bin());
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if tcsetpgrp(0, getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .args([
            "agents",
            "shell",
            "attach",
            task_id,
            "--interactive",
            "--max-ms",
            "2500",
            "--poll-ms",
            "25",
            "--limit-bytes",
            "4096",
        ])
        .current_dir(root)
        .env("TERM", "xterm-256color")
        .env("DSCODE_TEST_AGENTS_SHELL_ATTACH_INTERACTIVE_INPUT", input)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn interactive attach smoke: {error}"));

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll interactive attach child") {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child
                .wait()
                .expect("reap timed-out interactive attach child");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = read_pty_output(&mut master);

    PtyRun {
        status,
        timed_out,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::new(),
    }
}

fn run_shell_proxy_under_pty(root: &Path, task_id: &str, input: &str, timeout: Duration) -> PtyRun {
    let (mut master, slave) = open_test_pty(35, 103);
    let stdin = slave.try_clone().expect("clone pty slave for stdin");
    let stdout = slave.try_clone().expect("clone pty slave for stdout");
    let stderr = slave;
    let mut command = Command::new(deepseek_bin());
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if tcsetpgrp(0, getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .args([
            "agents",
            "shell",
            "proxy",
            task_id,
            "--max-ms",
            "1500",
            "--poll-ms",
            "10",
            "--limit-bytes",
            "4096",
        ])
        .current_dir(root)
        .env("TERM", "xterm-256color")
        .env("DSCODE_TEST_AGENTS_SHELL_PROXY_INPUT", input)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn terminal proxy smoke: {error}"));

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll terminal proxy child") {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().expect("reap timed-out terminal proxy child");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = read_pty_output(&mut master);

    PtyRun {
        status,
        timed_out,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::new(),
    }
}

fn run_shell_fd_proxy_under_pty(
    root: &Path,
    task_id: &str,
    input: &str,
    timeout: Duration,
) -> PtyRun {
    let (mut master, slave) = open_test_pty(36, 104);
    let stdin = slave.try_clone().expect("clone pty slave for stdin");
    let stdout = slave.try_clone().expect("clone pty slave for stdout");
    let stderr = slave;
    let mut command = Command::new(deepseek_bin());
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if tcsetpgrp(0, getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .args([
            "agents", "shell", "fd-proxy", task_id, "--rows", "36", "--cols", "104", "--max-ms",
            "1500",
        ])
        .current_dir(root)
        .env("TERM", "xterm-256color")
        .env("DSCODE_TEST_AGENTS_SHELL_FD_PROXY_INPUT", input)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn fd proxy smoke: {error}"));

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll fd proxy child") {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().expect("reap timed-out fd proxy child");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let stdout = read_pty_output(&mut master);

    PtyRun {
        status,
        timed_out,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::new(),
    }
}

fn run_shell_fd_proxy_ctrl_d_under_pty(root: &Path, task_id: &str, timeout: Duration) -> PtyRun {
    let (mut master, slave) = open_test_pty(36, 104);
    let stdin = slave.try_clone().expect("clone pty slave for stdin");
    let stdout = slave.try_clone().expect("clone pty slave for stdout");
    let stderr = slave;
    let mut command = Command::new(deepseek_bin());
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if tcsetpgrp(0, getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .args([
            "agents", "shell", "fd-proxy", task_id, "--rows", "36", "--cols", "104", "--max-ms",
            "4000",
        ])
        .current_dir(root)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn fd proxy Ctrl-D smoke: {error}"));

    let started = Instant::now();
    let mut timed_out = false;
    let mut transcript = String::new();
    let mut sent_probe = false;
    let mut sent_eof = false;
    let status = loop {
        let output = read_pty_output(&mut master);
        if !output.is_empty() {
            transcript.push_str(&String::from_utf8_lossy(&output));
        }
        if !sent_probe && transcript.contains("fd-proxied to") {
            master
                .write_all(b"ctrl-d-probe\n")
                .expect("write probe to fd proxy pty");
            master.flush().expect("flush probe to fd proxy pty");
            sent_probe = true;
        }
        if sent_probe && !sent_eof && transcript.contains("fd:ctrl-d-probe") {
            master
                .write_all(b"\x04")
                .expect("write Ctrl-D to fd proxy pty");
            master.flush().expect("flush Ctrl-D to fd proxy pty");
            sent_eof = true;
        }
        if let Some(status) = child.try_wait().expect("poll fd proxy Ctrl-D child") {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().expect("reap timed-out fd proxy Ctrl-D child");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = read_pty_output(&mut master);
    if !output.is_empty() {
        transcript.push_str(&String::from_utf8_lossy(&output));
    }

    PtyRun {
        status,
        timed_out,
        stdout: transcript,
        stderr: String::new(),
    }
}

fn run_shell_fd_proxy_ctrl_c_under_pty(root: &Path, task_id: &str, timeout: Duration) -> PtyRun {
    let (mut master, slave) = open_test_pty(36, 104);
    let stdin = slave.try_clone().expect("clone pty slave for stdin");
    let stdout = slave.try_clone().expect("clone pty slave for stdout");
    let stderr = slave;
    let mut command = Command::new(deepseek_bin());
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if tcsetpgrp(0, getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .args([
            "agents", "shell", "fd-proxy", task_id, "--rows", "36", "--cols", "104", "--max-ms",
            "4000",
        ])
        .current_dir(root)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn fd proxy Ctrl-C smoke: {error}"));

    let started = Instant::now();
    let mut timed_out = false;
    let mut transcript = String::new();
    let mut banner_seen_at: Option<Instant> = None;
    let mut sent_interrupt = false;
    let status = loop {
        let output = read_pty_output(&mut master);
        if !output.is_empty() {
            transcript.push_str(&String::from_utf8_lossy(&output));
        }
        if banner_seen_at.is_none() && transcript.contains("fd-proxied to") {
            banner_seen_at = Some(Instant::now());
        }
        if !sent_interrupt
            && banner_seen_at.is_some_and(|seen_at| seen_at.elapsed() >= Duration::from_millis(150))
        {
            master
                .write_all(b"\x03")
                .expect("write Ctrl-C to fd proxy pty");
            master.flush().expect("flush Ctrl-C to fd proxy pty");
            sent_interrupt = true;
        }
        if let Some(status) = child.try_wait().expect("poll fd proxy Ctrl-C child") {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().expect("reap timed-out fd proxy Ctrl-C child");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = read_pty_output(&mut master);
    if !output.is_empty() {
        transcript.push_str(&String::from_utf8_lossy(&output));
    }

    PtyRun {
        status,
        timed_out,
        stdout: transcript,
        stderr: String::new(),
    }
}

fn run_shell_fd_proxy_kill_under_pty(root: &Path, task_id: &str, timeout: Duration) -> PtyRun {
    let (mut master, slave) = open_test_pty(36, 104);
    let stdin = slave.try_clone().expect("clone pty slave for stdin");
    let stdout = slave.try_clone().expect("clone pty slave for stdout");
    let stderr = slave;
    let mut command = Command::new(deepseek_bin());
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if tcsetpgrp(0, getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .args([
            "agents", "shell", "fd-proxy", task_id, "--rows", "36", "--cols", "104", "--max-ms",
            "4000",
        ])
        .current_dir(root)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn fd proxy abnormal-exit smoke: {error}"));

    let started = Instant::now();
    let mut timed_out = false;
    let mut transcript = String::new();
    let mut sent_probe = false;
    let mut killed = false;
    let status = loop {
        let output = read_pty_output(&mut master);
        if !output.is_empty() {
            transcript.push_str(&String::from_utf8_lossy(&output));
        }
        if !sent_probe && transcript.contains("fd-proxied to") {
            master
                .write_all(b"before-abnormal\n")
                .expect("write abnormal-exit probe to fd proxy pty");
            master
                .flush()
                .expect("flush abnormal-exit probe to fd proxy pty");
            sent_probe = true;
        }
        if sent_probe && !killed && transcript.contains("fd:before-abnormal") {
            unsafe {
                let _ = kill(child.id() as i32, SIGKILL);
            }
            killed = true;
        }
        if let Some(status) = child.try_wait().expect("poll fd proxy abnormal-exit child") {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child
                .wait()
                .expect("reap timed-out fd proxy abnormal-exit child");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = read_pty_output(&mut master);
    if !output.is_empty() {
        transcript.push_str(&String::from_utf8_lossy(&output));
    }

    PtyRun {
        status,
        timed_out,
        stdout: transcript,
        stderr: String::new(),
    }
}

fn run_shell_fd_proxy_resize_under_pty(root: &Path, task_id: &str, timeout: Duration) -> PtyRun {
    let (mut master, slave) = open_test_pty(36, 104);
    let stdin = slave.try_clone().expect("clone pty slave for stdin");
    let stdout = slave.try_clone().expect("clone pty slave for stdout");
    let stderr = slave;
    let mut command = Command::new(deepseek_bin());
    unsafe {
        command.pre_exec(|| {
            if setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if tcsetpgrp(0, getpid()) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .args([
            "agents", "shell", "fd-proxy", task_id, "--rows", "36", "--cols", "104", "--max-ms",
            "4000",
        ])
        .current_dir(root)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap_or_else(|error| panic!("spawn fd proxy resize smoke: {error}"));

    let started = Instant::now();
    let mut timed_out = false;
    let mut transcript = String::new();
    let mut resize_sent_at: Option<Instant> = None;
    let mut sent_probe = false;
    let mut sent_eof = false;
    let status = loop {
        let output = read_pty_output(&mut master);
        if !output.is_empty() {
            transcript.push_str(&String::from_utf8_lossy(&output));
        }
        if resize_sent_at.is_none() && transcript.contains("fd-proxied to") {
            set_test_pty_winsize(&master, 41, 109);
            unsafe {
                let _ = kill(child.id() as i32, SIGWINCH);
            }
            resize_sent_at = Some(Instant::now());
        }
        if !sent_probe
            && resize_sent_at.is_some_and(|sent_at| sent_at.elapsed() >= Duration::from_millis(150))
        {
            master
                .write_all(b"resize-probe\n")
                .expect("write resize probe to fd proxy pty");
            master.flush().expect("flush resize probe to fd proxy pty");
            sent_probe = true;
        }
        if sent_probe
            && !sent_eof
            && transcript.contains("fd:resize-probe")
            && transcript.contains("41 109")
        {
            master
                .write_all(b"\x04")
                .expect("write Ctrl-D to fd proxy pty");
            master.flush().expect("flush Ctrl-D to fd proxy pty");
            sent_eof = true;
        }
        if let Some(status) = child.try_wait().expect("poll fd proxy resize child") {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().expect("reap timed-out fd proxy resize child");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = read_pty_output(&mut master);
    if !output.is_empty() {
        transcript.push_str(&String::from_utf8_lossy(&output));
    }

    PtyRun {
        status,
        timed_out,
        stdout: transcript,
        stderr: String::new(),
    }
}

fn read_pty_output(master: &mut File) -> Vec<u8> {
    set_nonblocking(master.as_raw_fd());
    let mut output = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
    output
}

fn set_nonblocking(fd: i32) {
    const F_GETFL: i32 = 3;
    const F_SETFL: i32 = 4;
    const O_NONBLOCK: i32 = 0x0800;

    let flags = unsafe { fcntl(fd, F_GETFL, 0) };
    assert!(flags >= 0, "fcntl(F_GETFL) failed");
    let result = unsafe { fcntl(fd, F_SETFL, flags | O_NONBLOCK) };
    assert!(result >= 0, "fcntl(F_SETFL) failed");
}

fn cmsg_align(len: usize) -> usize {
    let align = std::mem::size_of::<usize>();
    (len + align - 1) & !(align - 1)
}

fn cmsg_len(data_len: usize) -> usize {
    cmsg_align(std::mem::size_of::<TestCmsghdr>()) + data_len
}

fn cmsg_space(data_len: usize) -> usize {
    cmsg_align(std::mem::size_of::<TestCmsghdr>()) + cmsg_align(data_len)
}

#[repr(C)]
struct TestWinsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[repr(C)]
struct TestIovec {
    iov_base: *mut std::ffi::c_void,
    iov_len: usize,
}

#[repr(C)]
struct TestMsghdr {
    msg_name: *mut std::ffi::c_void,
    msg_namelen: u32,
    msg_iov: *mut TestIovec,
    msg_iovlen: usize,
    msg_control: *mut std::ffi::c_void,
    msg_controllen: usize,
    msg_flags: i32,
}

#[repr(C)]
struct TestCmsghdr {
    cmsg_len: usize,
    cmsg_level: i32,
    cmsg_type: i32,
}

unsafe extern "C" {
    fn posix_openpt(flags: i32) -> i32;
    fn grantpt(fd: i32) -> i32;
    fn unlockpt(fd: i32) -> i32;
    fn ptsname(fd: i32) -> *mut std::os::raw::c_char;
    fn setsid() -> i32;
    fn getpid() -> i32;
    fn tcsetpgrp(fd: i32, pgrp: i32) -> i32;
    fn ioctl(fd: i32, request: u64, ...) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn recvmsg(fd: i32, msg: *mut TestMsghdr, flags: i32) -> isize;
}

const TIOCSCTTY: u64 = 0x540E;
const TIOCSWINSZ: u64 = 0x5414;
const SIGKILL: i32 = 9;
const SIGWINCH: i32 = 28;
const SOL_SOCKET: i32 = 1;
const SCM_RIGHTS: i32 = 1;

fn open_test_pty(rows: u16, cols: u16) -> (File, File) {
    const O_RDWR: i32 = 0x0002;
    const O_NOCTTY: i32 = 0x0100;

    let master_fd = unsafe { posix_openpt(O_RDWR | O_NOCTTY) };
    assert!(master_fd >= 0, "posix_openpt failed");
    if unsafe { grantpt(master_fd) } < 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            let _ = File::from_raw_fd(master_fd);
        }
        panic!("grantpt failed: {error}");
    }
    if unsafe { unlockpt(master_fd) } < 0 {
        let error = std::io::Error::last_os_error();
        unsafe {
            let _ = File::from_raw_fd(master_fd);
        }
        panic!("unlockpt failed: {error}");
    }
    let slave_name = unsafe { ptsname(master_fd) };
    assert!(!slave_name.is_null(), "ptsname failed");
    let slave_path = unsafe { CStr::from_ptr(slave_name) }
        .to_string_lossy()
        .to_string();

    let master = unsafe { File::from_raw_fd(master_fd) };
    let slave = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&slave_path)
        .unwrap_or_else(|error| panic!("open pty slave {slave_path}: {error}"));
    set_test_pty_winsize(&slave, rows, cols);
    (master, slave)
}

fn set_test_pty_winsize(pty: &File, rows: u16, cols: u16) {
    let size = TestWinsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { ioctl(pty.as_raw_fd(), TIOCSWINSZ, &size) };
    assert!(result >= 0, "set pty window size failed");
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

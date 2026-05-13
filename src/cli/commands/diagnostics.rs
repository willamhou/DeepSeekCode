use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::cli::app::DiagnosticsArgs;
use crate::error::{app_error, AppResult};
use crate::language::diagnostics::DiagnosticReport;
use crate::util::json::{json_value_to_string, JsonValue};

pub fn run(args: DiagnosticsArgs) -> AppResult<()> {
    let cwd = std::env::current_dir()?;
    if args.watch {
        return run_watch(args, cwd);
    }
    let files = if args.changed {
        changed_files(&cwd)?
    } else {
        args.paths
    };
    if args.changed && files.is_empty() {
        if args.json {
            print_json_line(diagnostics_protocol_json(
                &cwd,
                DiagnosticsProtocolOptions {
                    watch: false,
                    tick: 1,
                    changed: true,
                    skipped: true,
                },
                &files,
                None,
            ))?;
        } else {
            println!("no changed files to diagnose");
        }
        return Ok(());
    }
    let report = crate::language::diagnostics::run_diagnostics(&cwd, &files);
    if args.json {
        print_json_line(diagnostics_protocol_json(
            &cwd,
            DiagnosticsProtocolOptions {
                watch: false,
                tick: 1,
                changed: args.changed,
                skipped: false,
            },
            &files,
            Some(&report),
        ))?;
    } else {
        println!("{}", report.render_text());
    }
    Ok(())
}

fn print_json_line(value: JsonValue) -> AppResult<()> {
    println!("{}", json_value_to_string(&value));
    std::io::stdout().flush()?;
    Ok(())
}

fn run_watch(args: DiagnosticsArgs, cwd: std::path::PathBuf) -> AppResult<()> {
    let interval = Duration::from_millis(args.interval_ms.max(100));
    let mut session = crate::language::diagnostics::WarmDiagnosticSession::new(
        cwd.clone(),
        &diagnostic_files(&cwd, &args)?,
    );
    let mut tick = 0_u64;
    loop {
        tick += 1;
        let files = diagnostic_files(&cwd, &args)?;
        if args.changed && files.is_empty() {
            if args.json {
                print_json_line(diagnostics_protocol_json(
                    &cwd,
                    DiagnosticsProtocolOptions {
                        watch: true,
                        tick,
                        changed: true,
                        skipped: true,
                    },
                    &files,
                    None,
                ))?;
            } else {
                println!("diagnostics watch: no changed files");
            }
        } else {
            let report = session.run(&files);
            if args.json {
                print_json_line(diagnostics_protocol_json(
                    &cwd,
                    DiagnosticsProtocolOptions {
                        watch: true,
                        tick,
                        changed: args.changed,
                        skipped: false,
                    },
                    &files,
                    Some(&report),
                ))?;
            } else {
                println!("diagnostics watch tick:");
                println!("{}", report.render_text());
            }
        }
        if args.once {
            return Ok(());
        }
        std::thread::sleep(interval);
    }
}

fn diagnostic_files(cwd: &Path, args: &DiagnosticsArgs) -> AppResult<Vec<String>> {
    if args.changed {
        changed_files(cwd)
    } else {
        Ok(args.paths.clone())
    }
}

pub(crate) fn changed_files(cwd: &Path) -> AppResult<Vec<String>> {
    let output = Command::new("git")
        .args([
            "diff",
            "--name-only",
            "--diff-filter=ACMRTUXB",
            "HEAD",
            "--",
        ])
        .current_dir(cwd)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                app_error("git not found in PATH; cannot enumerate changed files")
            } else {
                app_error(format!("could not invoke git: {error}"))
            }
        })?;
    if !output.status.success() {
        return Err(app_error(format!(
            "git diff --name-only failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

struct DiagnosticsProtocolOptions {
    watch: bool,
    tick: u64,
    changed: bool,
    skipped: bool,
}

fn diagnostics_protocol_json(
    cwd: &Path,
    options: DiagnosticsProtocolOptions,
    files: &[String],
    report: Option<&DiagnosticReport>,
) -> JsonValue {
    json_object([
        (
            "schema",
            JsonValue::String(
                if options.watch {
                    "deepseek.diagnostics.daemon_tick.v1"
                } else {
                    "deepseek.diagnostics.report.v1"
                }
                .to_string(),
            ),
        ),
        ("cwd", JsonValue::String(cwd.display().to_string())),
        ("watch", JsonValue::Bool(options.watch)),
        ("tick", JsonValue::Number(options.tick.to_string())),
        ("changed", JsonValue::Bool(options.changed)),
        ("skipped", JsonValue::Bool(options.skipped)),
        (
            "files",
            JsonValue::Array(files.iter().cloned().map(JsonValue::String).collect()),
        ),
        (
            "report",
            report
                .map(DiagnosticReport::to_json_value)
                .unwrap_or(JsonValue::Null),
        ),
    ])
}

fn json_object<const N: usize>(items: [(&str, JsonValue); N]) -> JsonValue {
    let mut map = std::collections::BTreeMap::new();
    for (key, value) in items {
        map.insert(key.to_string(), value);
    }
    JsonValue::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::diagnostics::DiagnosticStatus;
    use crate::util::json::{json_as_array, json_as_object, json_as_string, json_as_u64};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-cli-diagnostics-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn changed_files_reads_tracked_git_diff() {
        let root = temp_root("changed");
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        fs::write(root.join("app.py"), "print('ok')\n").unwrap();
        run_git(&root, &["add", "app.py"]);
        run_git(
            &root,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "commit",
                "-m",
                "initial",
            ],
        );
        fs::write(root.join("app.py"), "print('changed')\n").unwrap();

        let files = changed_files(&root).unwrap();

        assert_eq!(files, vec!["app.py".to_string()]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn diagnostics_protocol_json_serializes_report() {
        let root = temp_root("json-report");
        let report = DiagnosticReport {
            language: "rust".to_string(),
            engine: "cargo".to_string(),
            lsp_server: "rust-analyzer".to_string(),
            lsp_available: false,
            command: "cargo check".to_string(),
            cwd: root.display().to_string(),
            checked_files: vec!["src/lib.rs".to_string()],
            status: DiagnosticStatus::Failed,
            stdout: "stdout".to_string(),
            stderr: "stderr".to_string(),
            note: Some("note".to_string()),
        };

        let value = diagnostics_protocol_json(
            &root,
            DiagnosticsProtocolOptions {
                watch: true,
                tick: 7,
                changed: false,
                skipped: false,
            },
            &["src/lib.rs".to_string()],
            Some(&report),
        );
        let root = json_as_object(&value).unwrap();

        assert_eq!(
            root.get("schema").and_then(json_as_string),
            Some("deepseek.diagnostics.daemon_tick.v1")
        );
        assert_eq!(root.get("tick").and_then(json_as_u64), Some(7));
        let report = root
            .get("report")
            .and_then(json_as_object)
            .expect("report object");
        assert_eq!(
            report.get("status").and_then(json_as_string),
            Some("failed")
        );
        assert_eq!(report.get("note").and_then(json_as_string), Some("note"));
    }

    #[test]
    fn diagnostics_protocol_json_serializes_skipped_tick() {
        let root = temp_root("json-skipped");
        let value = diagnostics_protocol_json(
            &root,
            DiagnosticsProtocolOptions {
                watch: false,
                tick: 1,
                changed: true,
                skipped: true,
            },
            &[],
            None,
        );
        let root = json_as_object(&value).unwrap();

        assert_eq!(
            root.get("schema").and_then(json_as_string),
            Some("deepseek.diagnostics.report.v1")
        );
        assert!(matches!(root.get("skipped"), Some(JsonValue::Bool(true))));
        assert!(matches!(root.get("report"), Some(JsonValue::Null)));
        assert_eq!(
            root.get("files")
                .and_then(json_as_array)
                .map(std::vec::Vec::len),
            Some(0)
        );
    }
}

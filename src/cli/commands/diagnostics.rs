use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::cli::app::DiagnosticsArgs;
use crate::error::{app_error, AppResult};

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
        println!("no changed files to diagnose");
        return Ok(());
    }
    let report = crate::language::diagnostics::run_diagnostics(&cwd, &files);
    println!("{}", report.render_text());
    Ok(())
}

fn run_watch(args: DiagnosticsArgs, cwd: std::path::PathBuf) -> AppResult<()> {
    let interval = Duration::from_millis(args.interval_ms.max(100));
    let mut session = crate::language::diagnostics::WarmDiagnosticSession::new(
        cwd.clone(),
        &diagnostic_files(&cwd, &args)?,
    );
    loop {
        let files = diagnostic_files(&cwd, &args)?;
        if args.changed && files.is_empty() {
            println!("diagnostics watch: no changed files");
        } else {
            let report = session.run(&files);
            println!("diagnostics watch tick:");
            println!("{}", report.render_text());
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

#[cfg(test)]
mod tests {
    use super::*;
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
}

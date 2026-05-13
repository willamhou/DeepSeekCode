use std::path::PathBuf;

use crate::core::rollback::{RollbackStore, SnapshotRecord};
use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};

const DEFAULT_OFFSET: usize = 1;
const MAX_OFFSET: usize = 50;

#[derive(Debug, Clone)]
pub struct RevertTurnTool {
    store: RollbackStore,
}

impl RevertTurnTool {
    pub fn new(store_root: PathBuf) -> Self {
        Self {
            store: RollbackStore::new(store_root),
        }
    }

    pub fn from_store(store: RollbackStore) -> Self {
        Self { store }
    }
}

impl Tool for RevertTurnTool {
    fn name(&self) -> &str {
        "revert_turn"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let target = self.resolve_target(&input)?;
        let dry_run = bool_arg(&input, "dry_run", false) || !bool_arg(&input, "apply", true);
        let plan = self.store.restore_snapshot(&target.id, !dry_run)?;

        let mut summary = String::new();
        summary.push_str(&format!("meta.snapshot_id={}\n", target.id));
        if let Some(turn_id) = target.runtime_turn_id.as_deref() {
            summary.push_str(&format!("meta.turn_id={turn_id}\n"));
        }
        if let Some(thread_id) = target.runtime_thread_id.as_deref() {
            summary.push_str(&format!("meta.thread_id={thread_id}\n"));
        }
        summary.push_str(&format!("meta.applied={}\n", plan.applied));
        summary.push_str(&format!("meta.git_head={}\n", plan.git_head));
        summary.push_str(&format!(
            "meta.current_patch_bytes={}\n",
            plan.current_patch_bytes
        ));
        summary.push_str(&format!("meta.snapshot_patch_bytes={}\n", plan.patch_bytes));
        summary.push_str(&format!(
            "meta.changed_files={}\n",
            plan.changed_files.join(",")
        ));
        if plan.applied {
            summary.push_str(&format!(
                "Restored rollback snapshot {} ({}) to workspace files. Conversation history is unchanged.\n",
                target.id, target.label
            ));
        } else {
            summary.push_str(&format!(
                "Dry run: rollback snapshot {} ({}) can be restored with apply=true. Conversation history would be unchanged.\n",
                target.id, target.label
            ));
        }
        Ok(ToolOutput { summary })
    }
}

impl RevertTurnTool {
    fn resolve_target(&self, input: &ToolInput) -> AppResult<SnapshotRecord> {
        if let Some(id) = input
            .get("snapshot_id")
            .or_else(|| input.get("checkpoint_id"))
            .or_else(|| input.get("id"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return self.store.load_snapshot_or_turn(id);
        }
        if let Some(turn_id) = input
            .get("turn_id")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return self.store.load_snapshot_for_turn(turn_id);
        }

        let offset = input
            .get("turn_offset")
            .or_else(|| input.get("offset"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_OFFSET);
        if offset == 0 || offset > MAX_OFFSET {
            return Err(app_error(format!(
                "turn_offset must be between 1 and {MAX_OFFSET}; got {offset}"
            )));
        }

        let thread_id = input
            .get("thread_id")
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let snapshots = self.store.list_snapshots(MAX_OFFSET.saturating_mul(4))?;
        let mut candidates = snapshots
            .iter()
            .filter(|snapshot| {
                thread_id
                    .map(|thread_id| snapshot.runtime_thread_id.as_deref() == Some(thread_id))
                    .unwrap_or(true)
            })
            .filter(|snapshot| snapshot.runtime_turn_id.is_some())
            .cloned()
            .collect::<Vec<_>>();
        if candidates.is_empty() && thread_id.is_none() {
            candidates = snapshots;
        }
        candidates.get(offset - 1).cloned().ok_or_else(|| {
            app_error(format!(
                "only {} rollback snapshot(s) match the request; turn_offset={offset} is out of range",
                candidates.len()
            ))
        })
    }
}

fn bool_arg(input: &ToolInput, key: &str, default: bool) -> bool {
    input
        .get(key)
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deepseek-revert-turn-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
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

    fn temp_git_repo(label: &str) -> PathBuf {
        let repo = temp_root(label);
        run_git(&repo, &["init"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "DeepSeekCode Test"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(&repo, &["commit", "-m", "initial commit"]);
        repo
    }

    #[test]
    fn revert_turn_restores_snapshot_by_id() {
        let repo = temp_git_repo("by-id");
        let store_root = temp_root("by-id-store");
        let store = RollbackStore::new(store_root.clone());
        fs::write(repo.join("src.txt"), "snapshot\n").unwrap();
        let snapshot = store
            .create_snapshot(&repo, "before model edit".to_string())
            .unwrap();
        fs::write(repo.join("src.txt"), "later\n").unwrap();

        let output = RevertTurnTool::new(store_root)
            .execute(ToolInput::new().with_arg("snapshot_id", snapshot.id))
            .unwrap();

        assert!(output.summary.contains("meta.applied=true"));
        assert_eq!(
            fs::read_to_string(repo.join("src.txt")).unwrap(),
            "snapshot\n"
        );
    }

    #[test]
    fn revert_turn_dry_run_does_not_mutate() {
        let repo = temp_git_repo("dry-run");
        let store_root = temp_root("dry-run-store");
        let store = RollbackStore::new(store_root.clone());
        fs::write(repo.join("src.txt"), "snapshot\n").unwrap();
        let snapshot = store
            .create_snapshot(&repo, "before model edit".to_string())
            .unwrap();
        fs::write(repo.join("src.txt"), "later\n").unwrap();

        let output = RevertTurnTool::new(store_root)
            .execute(
                ToolInput::new()
                    .with_arg("snapshot_id", snapshot.id)
                    .with_arg("dry_run", "true"),
            )
            .unwrap();

        assert!(output.summary.contains("meta.applied=false"));
        assert_eq!(fs::read_to_string(repo.join("src.txt")).unwrap(), "later\n");
    }

    #[test]
    fn revert_turn_offset_prefers_runtime_turn_snapshots() {
        let repo = temp_git_repo("offset");
        let store_root = temp_root("offset-store");
        let store = RollbackStore::new(store_root.clone());
        fs::write(repo.join("src.txt"), "first\n").unwrap();
        let first = store.create_snapshot(&repo, "first".to_string()).unwrap();
        store
            .bind_snapshot_runtime(&first.id, Some("thread-one"), Some("turn-one"))
            .unwrap();
        fs::write(repo.join("src.txt"), "second\n").unwrap();
        let second = store.create_snapshot(&repo, "second".to_string()).unwrap();
        store
            .bind_snapshot_runtime(&second.id, Some("thread-one"), Some("turn-two"))
            .unwrap();
        fs::write(repo.join("src.txt"), "later\n").unwrap();

        let output = RevertTurnTool::new(store_root)
            .execute(
                ToolInput::new()
                    .with_arg("thread_id", "thread-one")
                    .with_arg("turn_offset", "2"),
            )
            .unwrap();

        assert!(output.summary.contains("meta.turn_id=turn-one"));
        assert_eq!(fs::read_to_string(repo.join("src.txt")).unwrap(), "first\n");
    }
}

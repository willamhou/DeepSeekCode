use std::path::{Path, PathBuf};

use crate::cli::app::RestoreAction;
use crate::config::load::load_or_default;
use crate::core::rollback::RollbackStore;
use crate::error::AppResult;

pub fn run(action: RestoreAction) -> AppResult<()> {
    let config = load_or_default()?;
    let store = RollbackStore::new(PathBuf::from(config.workspace.config_dir).join("rollback"));
    let workspace = std::env::current_dir()?;
    match action {
        RestoreAction::Snapshot { label } => {
            let snapshot = store.create_snapshot(
                &workspace,
                label.unwrap_or_else(|| "manual snapshot".to_string()),
            )?;
            println!("created rollback snapshot {}", snapshot.id);
            println!("  label: {}", snapshot.label);
            println!("  git_head: {}", snapshot.git_head);
            println!("  patch_bytes: {}", snapshot.patch_bytes);
            println!("  staged_patch_bytes: {}", snapshot.staged_patch_bytes);
            println!("  unstaged_patch_bytes: {}", snapshot.unstaged_patch_bytes);
            println!("  untracked_files: {}", snapshot.untracked_files.len());
            println!(
                "  untracked_directories: {}",
                snapshot.untracked_directories.len()
            );
            println!(
                "  untracked_symlinks: {}",
                snapshot.untracked_symlinks.len()
            );
            println!("  untracked_bytes: {}", snapshot.untracked_bytes);
            println!("  tracked_only: {}", snapshot.tracked_only);
        }
        RestoreAction::List { limit } => {
            let snapshots = store.list_snapshots(limit)?;
            if snapshots.is_empty() {
                println!("no rollback snapshots");
            } else {
                for snapshot in snapshots {
                    println!(
                        "{}  {}  patch={}  untracked={}  turn={}  {}",
                        snapshot.id,
                        snapshot.created_at,
                        snapshot.patch_bytes,
                        snapshot.untracked_files.len()
                            + snapshot.untracked_directories.len()
                            + snapshot.untracked_symlinks.len(),
                        snapshot.runtime_turn_id.as_deref().unwrap_or("-"),
                        snapshot.label
                    );
                }
            }
        }
        RestoreAction::Show { id, patch } => {
            let snapshot = store.load_snapshot_or_turn(&id)?;
            println!("snapshot: {}", snapshot.id);
            println!("  label: {}", snapshot.label);
            println!("  created_at: {}", snapshot.created_at);
            println!("  git_root: {}", snapshot.git_root);
            println!("  git_head: {}", snapshot.git_head);
            println!(
                "  runtime_thread_id: {}",
                snapshot.runtime_thread_id.as_deref().unwrap_or("-")
            );
            println!(
                "  runtime_turn_id: {}",
                snapshot.runtime_turn_id.as_deref().unwrap_or("-")
            );
            println!("  status_bytes: {}", snapshot.status_bytes);
            println!("  patch_bytes: {}", snapshot.patch_bytes);
            println!("  staged_patch_bytes: {}", snapshot.staged_patch_bytes);
            println!("  unstaged_patch_bytes: {}", snapshot.unstaged_patch_bytes);
            println!("  untracked_files: {}", snapshot.untracked_files.len());
            println!(
                "  untracked_directories: {}",
                snapshot.untracked_directories.len()
            );
            println!(
                "  untracked_symlinks: {}",
                snapshot.untracked_symlinks.len()
            );
            println!("  untracked_bytes: {}", snapshot.untracked_bytes);
            println!("  tracked_only: {}", snapshot.tracked_only);
            if !snapshot.untracked_files.is_empty() {
                println!("  untracked file paths:");
                for file in &snapshot.untracked_files {
                    println!("    - {file}");
                }
            }
            if !snapshot.untracked_directories.is_empty() {
                println!("  untracked directory paths:");
                for directory in &snapshot.untracked_directories {
                    println!("    - {directory}");
                }
            }
            if !snapshot.untracked_symlinks.is_empty() {
                println!("  untracked symlink paths:");
                for symlink in &snapshot.untracked_symlinks {
                    println!("    - {} -> {}", symlink.path, symlink.target);
                }
            }
            if patch {
                println!("{}", store.snapshot_patch(&snapshot.id)?);
            }
        }
        RestoreAction::RevertTurn { id, apply } => {
            let plan = store.restore_snapshot(&id, apply)?;
            if plan.applied {
                println!(
                    "restored tracked changes from snapshot {}",
                    plan.snapshot_id
                );
                print_changed_files(&plan.changed_files);
                print_post_restore_diagnostics(Path::new(&plan.git_root), &plan.changed_files);
            } else {
                println!("dry-run restore for snapshot {}", plan.snapshot_id);
                println!("  pass --apply to restore tracked changes to this snapshot");
            }
            println!("  git_head: {}", plan.git_head);
            println!("  snapshot_patch_bytes: {}", plan.patch_bytes);
            println!("  snapshot_staged_patch_bytes: {}", plan.staged_patch_bytes);
            println!(
                "  snapshot_unstaged_patch_bytes: {}",
                plan.unstaged_patch_bytes
            );
            println!("  current_patch_bytes: {}", plan.current_patch_bytes);
        }
    }
    Ok(())
}

fn print_changed_files(files: &[String]) {
    if files.is_empty() {
        println!("  changed_files: none");
        return;
    }
    println!("  changed_files:");
    for file in files {
        println!("    - {file}");
    }
}

fn print_post_restore_diagnostics(workspace: &Path, files: &[String]) {
    if files.is_empty() {
        return;
    }
    let report = crate::language::diagnostics::run_diagnostics(workspace, files);
    println!("post-restore diagnostics:");
    println!("{}", report.render_text());
}

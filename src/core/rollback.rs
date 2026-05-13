use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{app_error, AppResult};
use crate::util::json::{
    json_as_array, json_as_object, json_as_string, json_as_u64, json_value_to_string,
    parse_root_object, JsonValue,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub workspace: String,
    pub git_root: String,
    pub git_head: String,
    pub status_bytes: u64,
    pub patch_bytes: u64,
    pub staged_patch_bytes: u64,
    pub unstaged_patch_bytes: u64,
    pub untracked_bytes: u64,
    pub untracked_files: Vec<String>,
    pub untracked_directories: Vec<String>,
    pub untracked_fifos: Vec<String>,
    pub untracked_sockets: Vec<String>,
    pub untracked_symlinks: Vec<UntrackedSymlinkRecord>,
    pub tracked_only: bool,
    pub runtime_thread_id: Option<String>,
    pub runtime_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedSymlinkRecord {
    pub path: String,
    pub target: String,
}

impl SnapshotRecord {
    pub fn untracked_entry_count(&self) -> usize {
        self.untracked_files.len()
            + self.untracked_directories.len()
            + self.untracked_fifos.len()
            + self.untracked_sockets.len()
            + self.untracked_symlinks.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorePlan {
    pub snapshot_id: String,
    pub applied: bool,
    pub git_root: String,
    pub git_head: String,
    pub patch_bytes: u64,
    pub staged_patch_bytes: u64,
    pub unstaged_patch_bytes: u64,
    pub current_patch_bytes: u64,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RollbackStore {
    root: PathBuf,
}

impl RollbackStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn create_snapshot(&self, workspace: &Path, label: String) -> AppResult<SnapshotRecord> {
        fs::create_dir_all(self.snapshots_dir())?;
        let git_root = git_stdout(workspace, &["rev-parse", "--show-toplevel"])?;
        let git_root = git_root.trim().to_string();
        let git_head = git_stdout(Path::new(&git_root), &["rev-parse", "HEAD"])?;
        let git_head = git_head.trim().to_string();
        let status = git_stdout(Path::new(&git_root), &["status", "--porcelain=v1"])?;
        let patch = git_stdout(
            Path::new(&git_root),
            &["diff", "--binary", "--no-ext-diff", "HEAD", "--"],
        )?;
        let staged_patch = git_stdout(
            Path::new(&git_root),
            &[
                "diff",
                "--binary",
                "--no-ext-diff",
                "--cached",
                "HEAD",
                "--",
            ],
        )?;
        let unstaged_patch = git_stdout(
            Path::new(&git_root),
            &["diff", "--binary", "--no-ext-diff", "--"],
        )?;
        let untracked_candidates = filter_snapshot_storage_files(
            Path::new(&git_root),
            &self.root,
            git_untracked_files(Path::new(&git_root))?,
        );
        let id = new_id("snapshot");
        let dir = self.snapshot_dir(&id);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("status.txt"), &status)?;
        fs::write(dir.join("diff.patch"), &patch)?;
        fs::write(dir.join("staged.patch"), &staged_patch)?;
        fs::write(dir.join("unstaged.patch"), &unstaged_patch)?;
        let (untracked_files, untracked_symlinks, untracked_bytes) =
            capture_untracked_entries_to_snapshot(
                Path::new(&git_root),
                &dir,
                &untracked_candidates,
            )?;
        let untracked_directories =
            capture_empty_untracked_directories(Path::new(&git_root), &self.root)?;
        let untracked_fifos = capture_untracked_fifos(Path::new(&git_root), &self.root)?;
        let untracked_sockets = capture_untracked_sockets(Path::new(&git_root), &self.root)?;
        let tracked_only = untracked_files.is_empty()
            && untracked_directories.is_empty()
            && untracked_fifos.is_empty()
            && untracked_sockets.is_empty()
            && untracked_symlinks.is_empty();
        let record = SnapshotRecord {
            id,
            label: if label.trim().is_empty() {
                "manual snapshot".to_string()
            } else {
                label
            },
            created_at: epoch_label(),
            workspace: workspace.display().to_string(),
            git_root,
            git_head,
            status_bytes: status.len() as u64,
            patch_bytes: patch.len() as u64,
            staged_patch_bytes: staged_patch.len() as u64,
            unstaged_patch_bytes: unstaged_patch.len() as u64,
            untracked_bytes,
            untracked_files,
            untracked_directories,
            untracked_fifos,
            untracked_sockets,
            untracked_symlinks,
            tracked_only,
            runtime_thread_id: None,
            runtime_turn_id: None,
        };
        self.write_manifest(&record)?;
        Ok(record)
    }

    pub fn list_snapshots(&self, limit: usize) -> AppResult<Vec<SnapshotRecord>> {
        let dir = self.snapshots_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path().join("manifest.json");
            if !path.is_file() {
                continue;
            }
            let content = fs::read_to_string(path)?;
            records.push(parse_snapshot_record(&parse_root_object(&content)?)?);
        }
        records.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        records.truncate(limit);
        Ok(records)
    }

    pub fn load_snapshot(&self, id: &str) -> AppResult<SnapshotRecord> {
        validate_snapshot_id(id)?;
        let path = self.snapshot_dir(id).join("manifest.json");
        if !path.is_file() {
            return Err(app_error(format!("rollback snapshot not found: {id}")));
        }
        let content = fs::read_to_string(path)?;
        parse_snapshot_record(&parse_root_object(&content)?)
    }

    pub fn load_snapshot_or_turn(&self, id: &str) -> AppResult<SnapshotRecord> {
        validate_snapshot_id(id)?;
        match self.load_snapshot(id) {
            Ok(record) => Ok(record),
            Err(_) => self.load_snapshot_for_turn(id),
        }
    }

    pub fn load_snapshot_for_turn(&self, turn_id: &str) -> AppResult<SnapshotRecord> {
        validate_snapshot_id(turn_id)?;
        let dir = self.snapshots_dir();
        if !dir.exists() {
            return Err(app_error(format!(
                "rollback snapshot not found for runtime turn: {turn_id}"
            )));
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path().join("manifest.json");
            if !path.is_file() {
                continue;
            }
            let content = fs::read_to_string(path)?;
            let record = parse_snapshot_record(&parse_root_object(&content)?)?;
            if record.runtime_turn_id.as_deref() == Some(turn_id) {
                records.push(record);
            }
        }
        records.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        records.into_iter().next().ok_or_else(|| {
            app_error(format!(
                "rollback snapshot not found for runtime turn: {turn_id}"
            ))
        })
    }

    pub fn bind_snapshot_runtime(
        &self,
        id: &str,
        thread_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> AppResult<SnapshotRecord> {
        let mut record = self.load_snapshot(id)?;
        if let Some(thread_id) = thread_id {
            validate_snapshot_id(thread_id)?;
        }
        if let Some(turn_id) = turn_id {
            validate_snapshot_id(turn_id)?;
        }
        record.runtime_thread_id = thread_id.map(str::to_string);
        record.runtime_turn_id = turn_id.map(str::to_string);
        self.write_manifest(&record)?;
        Ok(record)
    }

    pub fn restore_snapshot(&self, id: &str, apply: bool) -> AppResult<RestorePlan> {
        let record = self.load_snapshot_or_turn(id)?;
        let root = Path::new(&record.git_root);
        let current_head = git_stdout(root, &["rev-parse", "HEAD"])?;
        let current_head = current_head.trim();
        if current_head != record.git_head {
            return Err(app_error(format!(
                "snapshot {} was captured at {}, current HEAD is {}; checkout the original commit before restoring",
                record.id, record.git_head, current_head
            )));
        }
        let current_patch = git_stdout(root, &["diff", "--binary", "--no-ext-diff", "HEAD", "--"])?;
        let current_staged_patch = git_stdout(
            root,
            &[
                "diff",
                "--binary",
                "--no-ext-diff",
                "--cached",
                "HEAD",
                "--",
            ],
        )?;
        let current_unstaged_patch =
            git_stdout(root, &["diff", "--binary", "--no-ext-diff", "--"])?;
        let snapshot_dir = self.snapshot_dir(&record.id);
        let patches = load_snapshot_patches(&snapshot_dir)?;
        if apply {
            restore_current_tracked_to_head(root, &current_staged_patch, &current_unstaged_patch)?;
            match &patches {
                SnapshotPatches::Split { staged, unstaged } => {
                    if !staged.trim().is_empty() {
                        git_apply(root, staged, false, ApplyTarget::IndexAndWorktree)?;
                    }
                    if !unstaged.trim().is_empty() {
                        git_apply(root, unstaged, false, ApplyTarget::Worktree)?;
                    }
                }
                SnapshotPatches::Legacy { combined } => {
                    if !combined.trim().is_empty() {
                        git_apply(root, combined, false, ApplyTarget::Worktree)?;
                    }
                }
            }
            restore_untracked_entries(
                root,
                &snapshot_dir,
                &record.untracked_files,
                &record.untracked_directories,
                &record.untracked_fifos,
                &record.untracked_sockets,
                &record.untracked_symlinks,
            )?;
        }
        let mut changed_files = if apply {
            git_changed_files(root)?
        } else {
            Vec::new()
        };
        if apply {
            changed_files.extend(record.untracked_files.iter().cloned());
            changed_files.extend(record.untracked_directories.iter().cloned());
            changed_files.extend(record.untracked_fifos.iter().cloned());
            changed_files.extend(record.untracked_sockets.iter().cloned());
            changed_files.extend(
                record
                    .untracked_symlinks
                    .iter()
                    .map(|entry| entry.path.clone()),
            );
            normalize_file_list(&mut changed_files);
        }
        Ok(RestorePlan {
            snapshot_id: record.id,
            applied: apply,
            git_root: record.git_root,
            git_head: record.git_head,
            patch_bytes: patches.combined_len() as u64,
            staged_patch_bytes: patches.staged_len() as u64,
            unstaged_patch_bytes: patches.unstaged_len() as u64,
            current_patch_bytes: current_patch.len() as u64,
            changed_files,
        })
    }

    pub fn snapshot_patch(&self, id: &str) -> AppResult<String> {
        validate_snapshot_id(id)?;
        fs::read_to_string(self.snapshot_dir(id).join("diff.patch"))
            .map_err(|error| app_error(format!("failed to read rollback patch for {id}: {error}")))
    }

    fn write_manifest(&self, record: &SnapshotRecord) -> AppResult<()> {
        fs::write(
            self.snapshot_dir(&record.id).join("manifest.json"),
            json_value_to_string(&snapshot_to_json(record)),
        )?;
        Ok(())
    }

    fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    fn snapshot_dir(&self, id: &str) -> PathBuf {
        self.snapshots_dir().join(id)
    }
}

pub fn snapshot_to_json(record: &SnapshotRecord) -> JsonValue {
    let mut value = object([
        ("id", JsonValue::String(record.id.clone())),
        ("label", JsonValue::String(record.label.clone())),
        ("created_at", JsonValue::String(record.created_at.clone())),
        ("workspace", JsonValue::String(record.workspace.clone())),
        ("git_root", JsonValue::String(record.git_root.clone())),
        ("git_head", JsonValue::String(record.git_head.clone())),
        (
            "status_bytes",
            JsonValue::Number(record.status_bytes.to_string()),
        ),
        (
            "patch_bytes",
            JsonValue::Number(record.patch_bytes.to_string()),
        ),
        (
            "staged_patch_bytes",
            JsonValue::Number(record.staged_patch_bytes.to_string()),
        ),
        (
            "unstaged_patch_bytes",
            JsonValue::Number(record.unstaged_patch_bytes.to_string()),
        ),
        (
            "untracked_bytes",
            JsonValue::Number(record.untracked_bytes.to_string()),
        ),
        ("tracked_only", JsonValue::Bool(record.tracked_only)),
    ]);
    value.insert(
        "runtime_thread_id".to_string(),
        record
            .runtime_thread_id
            .as_ref()
            .map(|id| JsonValue::String(id.clone()))
            .unwrap_or(JsonValue::Null),
    );
    value.insert(
        "runtime_turn_id".to_string(),
        record
            .runtime_turn_id
            .as_ref()
            .map(|id| JsonValue::String(id.clone()))
            .unwrap_or(JsonValue::Null),
    );
    value.insert(
        "untracked_files".to_string(),
        JsonValue::Array(
            record
                .untracked_files
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    value.insert(
        "untracked_directories".to_string(),
        JsonValue::Array(
            record
                .untracked_directories
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    value.insert(
        "untracked_symlinks".to_string(),
        JsonValue::Array(
            record
                .untracked_symlinks
                .iter()
                .map(untracked_symlink_to_json)
                .collect(),
        ),
    );
    value.insert(
        "untracked_fifos".to_string(),
        JsonValue::Array(
            record
                .untracked_fifos
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    value.insert(
        "untracked_sockets".to_string(),
        JsonValue::Array(
            record
                .untracked_sockets
                .iter()
                .cloned()
                .map(JsonValue::String)
                .collect(),
        ),
    );
    JsonValue::Object(value)
}

fn untracked_symlink_to_json(record: &UntrackedSymlinkRecord) -> JsonValue {
    JsonValue::Object(object([
        ("path", JsonValue::String(record.path.clone())),
        ("target", JsonValue::String(record.target.clone())),
    ]))
}

fn parse_snapshot_record(root: &BTreeMap<String, JsonValue>) -> AppResult<SnapshotRecord> {
    Ok(SnapshotRecord {
        id: required_string(root, "id")?,
        label: required_string(root, "label")?,
        created_at: required_string(root, "created_at")?,
        workspace: required_string(root, "workspace")?,
        git_root: required_string(root, "git_root")?,
        git_head: required_string(root, "git_head")?,
        status_bytes: required_u64(root, "status_bytes")?,
        patch_bytes: required_u64(root, "patch_bytes")?,
        staged_patch_bytes: optional_u64(root, "staged_patch_bytes")?,
        unstaged_patch_bytes: optional_u64(root, "unstaged_patch_bytes")?,
        untracked_bytes: optional_u64(root, "untracked_bytes")?,
        untracked_files: optional_string_array(root, "untracked_files")?,
        untracked_directories: optional_string_array(root, "untracked_directories")?,
        untracked_fifos: optional_string_array(root, "untracked_fifos")?,
        untracked_sockets: optional_string_array(root, "untracked_sockets")?,
        untracked_symlinks: optional_untracked_symlinks(root)?,
        tracked_only: matches!(root.get("tracked_only"), Some(JsonValue::Bool(true))),
        runtime_thread_id: optional_safe_string(root, "runtime_thread_id")?,
        runtime_turn_id: optional_safe_string(root, "runtime_turn_id")?,
    })
}

fn git_stdout(cwd: &Path, args: &[&str]) -> AppResult<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| app_error(format!("could not invoke git: {error}")))?;
    if !output.status.success() {
        return Err(app_error(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApplyTarget {
    Worktree,
    IndexAndWorktree,
}

enum SnapshotPatches {
    Split { staged: String, unstaged: String },
    Legacy { combined: String },
}

impl SnapshotPatches {
    fn combined_len(&self) -> usize {
        match self {
            Self::Split { staged, unstaged } => staged.len() + unstaged.len(),
            Self::Legacy { combined } => combined.len(),
        }
    }

    fn staged_len(&self) -> usize {
        match self {
            Self::Split { staged, .. } => staged.len(),
            Self::Legacy { .. } => 0,
        }
    }

    fn unstaged_len(&self) -> usize {
        match self {
            Self::Split { unstaged, .. } => unstaged.len(),
            Self::Legacy { combined } => combined.len(),
        }
    }
}

fn load_snapshot_patches(snapshot_dir: &Path) -> AppResult<SnapshotPatches> {
    let staged_path = snapshot_dir.join("staged.patch");
    let unstaged_path = snapshot_dir.join("unstaged.patch");
    if staged_path.is_file() && unstaged_path.is_file() {
        return Ok(SnapshotPatches::Split {
            staged: fs::read_to_string(staged_path)?,
            unstaged: fs::read_to_string(unstaged_path)?,
        });
    }
    Ok(SnapshotPatches::Legacy {
        combined: fs::read_to_string(snapshot_dir.join("diff.patch"))?,
    })
}

fn restore_current_tracked_to_head(
    cwd: &Path,
    _current_staged_patch: &str,
    _current_unstaged_patch: &str,
) -> AppResult<()> {
    git_stdout(cwd, &["reset", "--hard", "HEAD"]).map(|_| ())
}

fn git_apply(cwd: &Path, patch: &str, reverse: bool, target: ApplyTarget) -> AppResult<()> {
    let mut command = Command::new("git");
    command.arg("apply").arg("--binary");
    if target == ApplyTarget::IndexAndWorktree {
        command.arg("--index");
    }
    if reverse {
        command.arg("--reverse");
    }
    let mut child = command
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| app_error(format!("could not invoke git apply: {error}")))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| app_error("git apply produced no stdin pipe"))?;
        stdin
            .write_all(patch.as_bytes())
            .map_err(|error| app_error(format!("failed to write patch to git apply: {error}")))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| app_error(format!("failed to await git apply: {error}")))?;
    if !output.status.success() {
        return Err(app_error(format!(
            "git apply failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn git_changed_files(cwd: &Path) -> AppResult<Vec<String>> {
    let output = git_stdout(
        cwd,
        &[
            "diff",
            "--name-only",
            "--diff-filter=ACMRTUXB",
            "HEAD",
            "--",
        ],
    )?;
    let mut files = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    Ok(files)
}

fn git_untracked_files(cwd: &Path) -> AppResult<Vec<String>> {
    let output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .current_dir(cwd)
        .output()
        .map_err(|error| app_error(format!("could not invoke git: {error}")))?;
    if !output.status.success() {
        return Err(app_error(format!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect::<Vec<_>>();
    for file in &files {
        safe_relative_path(file)?;
    }
    normalize_file_list(&mut files);
    Ok(files)
}

fn filter_snapshot_storage_files(
    git_root: &Path,
    store_root: &Path,
    files: Vec<String>,
) -> Vec<String> {
    let Some(store_prefix) = store_root_relative_prefix(git_root, store_root) else {
        return files;
    };
    files
        .into_iter()
        .filter(|file| file != &store_prefix && !file.starts_with(&format!("{store_prefix}/")))
        .collect()
}

fn store_root_relative_prefix(git_root: &Path, store_root: &Path) -> Option<String> {
    let store_root = if store_root.is_absolute() {
        store_root.to_path_buf()
    } else {
        git_root.join(store_root)
    };
    let relative = store_root.strip_prefix(git_root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn capture_untracked_entries_to_snapshot(
    git_root: &Path,
    snapshot_dir: &Path,
    files: &[String],
) -> AppResult<(Vec<String>, Vec<UntrackedSymlinkRecord>, u64)> {
    let mut captured_files = Vec::new();
    let mut captured_symlinks = Vec::new();
    let mut total_bytes = 0;
    for file in files {
        let relative = safe_relative_path(file)?;
        let source = git_root.join(&relative);
        let metadata = fs::symlink_metadata(&source).map_err(|error| {
            app_error(format!(
                "failed to inspect untracked file `{file}`: {error}"
            ))
        })?;
        let file_type = metadata.file_type();
        if file_type.is_file() {
            let target = snapshot_dir.join("untracked").join(&relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            total_bytes += fs::copy(&source, &target).map_err(|error| {
                app_error(format!(
                    "failed to snapshot untracked file `{file}`: {error}"
                ))
            })?;
            captured_files.push(file.clone());
        } else if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let target = fs::read_link(&source).map_err(|error| {
                    app_error(format!(
                        "failed to read untracked symlink `{file}`: {error}"
                    ))
                })?;
                let target = target.to_string_lossy().into_owned();
                total_bytes += target.len() as u64;
                captured_symlinks.push(UntrackedSymlinkRecord {
                    path: file.clone(),
                    target,
                });
            }
        }
    }
    normalize_file_list(&mut captured_files);
    captured_symlinks.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.target.cmp(&b.target)));
    captured_symlinks.dedup_by(|a, b| a.path == b.path && a.target == b.target);
    Ok((captured_files, captured_symlinks, total_bytes))
}

fn capture_empty_untracked_directories(
    git_root: &Path,
    store_root: &Path,
) -> AppResult<Vec<String>> {
    let store_prefix = store_root_relative_prefix(git_root, store_root);
    let mut directories = Vec::new();
    collect_empty_untracked_directories(
        git_root,
        git_root,
        store_prefix.as_deref(),
        &mut directories,
    )?;
    normalize_file_list(&mut directories);
    Ok(directories)
}

#[cfg(unix)]
fn capture_untracked_fifos(git_root: &Path, store_root: &Path) -> AppResult<Vec<String>> {
    let store_prefix = store_root_relative_prefix(git_root, store_root);
    let mut fifos = Vec::new();
    collect_untracked_fifos(git_root, git_root, store_prefix.as_deref(), &mut fifos)?;
    normalize_file_list(&mut fifos);
    Ok(fifos)
}

#[cfg(not(unix))]
fn capture_untracked_fifos(_git_root: &Path, _store_root: &Path) -> AppResult<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(unix)]
fn capture_untracked_sockets(git_root: &Path, store_root: &Path) -> AppResult<Vec<String>> {
    let store_prefix = store_root_relative_prefix(git_root, store_root);
    let mut sockets = Vec::new();
    collect_untracked_sockets(git_root, git_root, store_prefix.as_deref(), &mut sockets)?;
    normalize_file_list(&mut sockets);
    Ok(sockets)
}

#[cfg(not(unix))]
fn capture_untracked_sockets(_git_root: &Path, _store_root: &Path) -> AppResult<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(unix)]
fn collect_untracked_fifos(
    git_root: &Path,
    current: &Path,
    store_prefix: Option<&str>,
    fifos: &mut Vec<String>,
) -> AppResult<()> {
    use std::os::unix::fs::FileTypeExt;

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let Some(relative) = relative_git_path(git_root, &path)? else {
            continue;
        };
        if is_rollback_internal_path(&relative, store_prefix)
            || is_git_internal_path(&relative)
            || is_git_ignored_path(git_root, &relative)?
        {
            continue;
        }
        let file_type = metadata.file_type();
        if file_type.is_dir() && !file_type.is_symlink() {
            collect_untracked_fifos(git_root, &path, store_prefix, fifos)?;
        } else if file_type.is_fifo() && !is_git_tracked_path(git_root, &relative)? {
            safe_relative_path(&relative)?;
            fifos.push(relative);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn collect_untracked_sockets(
    git_root: &Path,
    current: &Path,
    store_prefix: Option<&str>,
    sockets: &mut Vec<String>,
) -> AppResult<()> {
    use std::os::unix::fs::FileTypeExt;

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let Some(relative) = relative_git_path(git_root, &path)? else {
            continue;
        };
        if is_rollback_internal_path(&relative, store_prefix)
            || is_git_internal_path(&relative)
            || is_git_ignored_path(git_root, &relative)?
        {
            continue;
        }
        let file_type = metadata.file_type();
        if file_type.is_dir() && !file_type.is_symlink() {
            collect_untracked_sockets(git_root, &path, store_prefix, sockets)?;
        } else if file_type.is_socket() && !is_git_tracked_path(git_root, &relative)? {
            safe_relative_path(&relative)?;
            sockets.push(relative);
        }
    }
    Ok(())
}

fn collect_empty_untracked_directories(
    git_root: &Path,
    current: &Path,
    store_prefix: Option<&str>,
    directories: &mut Vec<String>,
) -> AppResult<bool> {
    let Some(relative) = relative_git_path(git_root, current)? else {
        let mut has_entries = false;
        for entry in fs::read_dir(current)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            has_entries = true;
            if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
                collect_empty_untracked_directories(git_root, &path, store_prefix, directories)?;
            }
        }
        return Ok(has_entries);
    };

    if is_rollback_internal_path(&relative, store_prefix) || is_git_internal_path(&relative) {
        return Ok(true);
    }
    if is_git_ignored_path(git_root, &relative)? {
        return Ok(true);
    }

    let mut has_entries = false;
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        has_entries = true;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            collect_empty_untracked_directories(git_root, &path, store_prefix, directories)?;
        }
    }
    if !has_entries {
        safe_relative_path(&relative)?;
        directories.push(relative);
    }
    Ok(has_entries)
}

fn relative_git_path(git_root: &Path, path: &Path) -> AppResult<Option<String>> {
    let relative = path.strip_prefix(git_root).map_err(|error| {
        app_error(format!(
            "failed to compute rollback relative path for `{}`: {error}",
            path.display()
        ))
    })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            _ => {
                return Err(app_error(format!(
                    "unsafe rollback directory path `{}`",
                    path.display()
                )))
            }
        }
    }
    Ok((!parts.is_empty()).then(|| parts.join("/")))
}

fn is_rollback_internal_path(path: &str, store_prefix: Option<&str>) -> bool {
    let Some(store_prefix) = store_prefix else {
        return false;
    };
    path == store_prefix || path.starts_with(&format!("{store_prefix}/"))
}

fn is_git_internal_path(path: &str) -> bool {
    path == ".git" || path.starts_with(".git/")
}

fn is_git_ignored_path(git_root: &Path, path: &str) -> AppResult<bool> {
    let output = Command::new("git")
        .args(["check-ignore", "-q", "--", path])
        .current_dir(git_root)
        .output()
        .map_err(|error| app_error(format!("could not invoke git check-ignore: {error}")))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(app_error(format!(
            "git check-ignore failed for `{path}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

fn is_git_tracked_path(git_root: &Path, path: &str) -> AppResult<bool> {
    let output = Command::new("git")
        .args(["ls-files", "--error-unmatch", "--", path])
        .current_dir(git_root)
        .output()
        .map_err(|error| app_error(format!("could not invoke git ls-files: {error}")))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(app_error(format!(
            "git ls-files failed for `{path}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

fn restore_untracked_entries(
    git_root: &Path,
    snapshot_dir: &Path,
    files: &[String],
    directories: &[String],
    fifos: &[String],
    sockets: &[String],
    symlinks: &[UntrackedSymlinkRecord],
) -> AppResult<()> {
    for directory in directories {
        let relative = safe_relative_path(directory)?;
        let target = git_root.join(&relative);
        restore_untracked_directory(&target)?;
    }
    for file in files {
        let relative = safe_relative_path(file)?;
        let source = snapshot_dir.join("untracked").join(&relative);
        if !source.is_file() {
            return Err(app_error(format!(
                "snapshot is missing captured untracked file `{file}`"
            )));
        }
        let target = git_root.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        remove_existing_restore_target(&target)?;
        fs::copy(&source, &target).map_err(|error| {
            app_error(format!(
                "failed to restore untracked file `{file}`: {error}"
            ))
        })?;
    }
    for fifo in fifos {
        let relative = safe_relative_path(fifo)?;
        let target = git_root.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        remove_existing_restore_target(&target)?;
        create_fifo(&target).map_err(|error| {
            app_error(format!(
                "failed to restore untracked FIFO `{fifo}`: {error}"
            ))
        })?;
    }
    for socket in sockets {
        let relative = safe_relative_path(socket)?;
        let target = git_root.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        remove_existing_restore_target(&target)?;
        create_socket(&target).map_err(|error| {
            app_error(format!(
                "failed to restore untracked socket `{socket}`: {error}"
            ))
        })?;
    }
    for symlink in symlinks {
        let relative = safe_relative_path(&symlink.path)?;
        let target = git_root.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        remove_existing_restore_target(&target)?;
        create_symlink(&symlink.target, &target).map_err(|error| {
            app_error(format!(
                "failed to restore untracked symlink `{}`: {error}",
                symlink.path
            ))
        })?;
    }
    Ok(())
}

fn restore_untracked_directory(target: &Path) -> AppResult<()> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Ok(_) => {
            fs::remove_file(target).map_err(|error| {
                app_error(format!(
                    "failed to remove existing restore target `{}`: {error}",
                    target.display()
                ))
            })?;
            fs::create_dir_all(target).map_err(|error| {
                app_error(format!(
                    "failed to restore untracked directory `{}`: {error}",
                    target.display()
                ))
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(target)
            .map_err(|error| {
                app_error(format!(
                    "failed to restore untracked directory `{}`: {error}",
                    target.display()
                ))
            }),
        Err(error) => Err(app_error(format!(
            "failed to inspect restore target `{}`: {error}",
            target.display()
        ))),
    }
}

fn remove_existing_restore_target(target: &Path) -> AppResult<()> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            Err(app_error(format!(
                "cannot restore untracked entry over directory `{}`",
                target.display()
            )))
        }
        Ok(_) => fs::remove_file(target).map_err(|error| {
            app_error(format!(
                "failed to remove existing restore target `{}`: {error}",
                target.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(app_error(format!(
            "failed to inspect restore target `{}`: {error}",
            target.display()
        ))),
    }
}

#[cfg(unix)]
fn create_symlink(target: &str, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn create_symlink(_target: &str, _link: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink restore is only supported on Unix",
    ))
}

#[cfg(unix)]
fn create_fifo(target: &Path) -> AppResult<()> {
    let output = Command::new("mkfifo")
        .arg(target)
        .output()
        .map_err(|error| app_error(format!("could not invoke mkfifo: {error}")))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(app_error(format!(
            "mkfifo failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(not(unix))]
fn create_fifo(_target: &Path) -> AppResult<()> {
    Err(app_error("FIFO restore is only supported on Unix"))
}

#[cfg(unix)]
fn create_socket(target: &Path) -> AppResult<()> {
    let listener = std::os::unix::net::UnixListener::bind(target)
        .map_err(|error| app_error(format!("could not bind Unix socket: {error}")))?;
    drop(listener);
    Ok(())
}

#[cfg(not(unix))]
fn create_socket(_target: &Path) -> AppResult<()> {
    Err(app_error("Unix socket restore is only supported on Unix"))
}

fn safe_relative_path(path: &str) -> AppResult<PathBuf> {
    let value = Path::new(path);
    if value.as_os_str().is_empty() || value.is_absolute() {
        return Err(app_error(format!("unsafe rollback path `{path}`")));
    }
    let mut output = PathBuf::new();
    for component in value.components() {
        match component {
            Component::Normal(part) => output.push(part),
            _ => return Err(app_error(format!("unsafe rollback path `{path}`"))),
        }
    }
    Ok(output)
}

fn normalize_file_list(files: &mut Vec<String>) {
    files.sort();
    files.dedup();
}

fn validate_snapshot_id(id: &str) -> AppResult<()> {
    let valid = !id.is_empty()
        && !id.starts_with('.')
        && !id.contains("..")
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'));
    if valid {
        Ok(())
    } else {
        Err(app_error(format!("invalid rollback snapshot id `{id}`")))
    }
}

fn required_string(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<String> {
    root.get(key)
        .and_then(json_as_string)
        .map(str::to_string)
        .ok_or_else(|| app_error(format!("rollback manifest missing string `{key}`")))
}

fn required_u64(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<u64> {
    root.get(key)
        .and_then(json_as_u64)
        .ok_or_else(|| app_error(format!("rollback manifest missing number `{key}`")))
}

fn optional_u64(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<u64> {
    match root.get(key) {
        Some(value) => json_as_u64(value)
            .ok_or_else(|| app_error(format!("rollback manifest `{key}` must be a number"))),
        None => Ok(0),
    }
}

fn optional_string_array(root: &BTreeMap<String, JsonValue>, key: &str) -> AppResult<Vec<String>> {
    let Some(value) = root.get(key) else {
        return Ok(Vec::new());
    };
    let array = json_as_array(value)
        .ok_or_else(|| app_error(format!("rollback manifest `{key}` must be an array")))?;
    let mut items = Vec::with_capacity(array.len());
    for item in array {
        let value = json_as_string(item)
            .ok_or_else(|| app_error(format!("rollback manifest `{key}` must contain strings")))?;
        safe_relative_path(value)?;
        items.push(value.to_string());
    }
    normalize_file_list(&mut items);
    Ok(items)
}

fn optional_untracked_symlinks(
    root: &BTreeMap<String, JsonValue>,
) -> AppResult<Vec<UntrackedSymlinkRecord>> {
    let Some(value) = root.get("untracked_symlinks") else {
        return Ok(Vec::new());
    };
    let array = json_as_array(value)
        .ok_or_else(|| app_error("rollback manifest `untracked_symlinks` must be an array"))?;
    let mut items = Vec::with_capacity(array.len());
    for item in array {
        let object = json_as_object(item).ok_or_else(|| {
            app_error("rollback manifest `untracked_symlinks` must contain objects")
        })?;
        let path = required_string(object, "path")?;
        let target = required_string(object, "target")?;
        safe_relative_path(&path)?;
        items.push(UntrackedSymlinkRecord { path, target });
    }
    items.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.target.cmp(&b.target)));
    items.dedup_by(|a, b| a.path == b.path && a.target == b.target);
    Ok(items)
}

fn optional_safe_string(
    root: &BTreeMap<String, JsonValue>,
    key: &str,
) -> AppResult<Option<String>> {
    match root.get(key) {
        Some(JsonValue::Null) | None => Ok(None),
        Some(value) => {
            let value = json_as_string(value)
                .ok_or_else(|| app_error(format!("rollback manifest `{key}` must be a string")))?;
            validate_snapshot_id(value)?;
            Ok(Some(value.to_string()))
        }
    }
}

fn object<const N: usize>(items: [(&str, JsonValue); N]) -> BTreeMap<String, JsonValue> {
    let mut map = BTreeMap::new();
    for (key, value) in items {
        map.insert(key.to_string(), value);
    }
    map
}

fn epoch_label() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("epoch+{secs}")
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-rollback-{label}-{}-{nanos}",
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
    fn snapshot_restore_round_trip_tracked_changes() {
        let repo = temp_root("repo");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        fs::write(repo.join("src.txt"), "snapshot\n").unwrap();
        let store = RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store
            .create_snapshot(&repo, "before risky turn".to_string())
            .unwrap();
        assert!(snapshot.patch_bytes > 0);

        fs::write(repo.join("src.txt"), "later\n").unwrap();
        let plan = store.restore_snapshot(&snapshot.id, true).unwrap();

        assert!(plan.applied);
        assert_eq!(
            fs::read_to_string(repo.join("src.txt")).unwrap(),
            "snapshot\n"
        );
        assert_eq!(plan.git_root, repo.display().to_string());
        assert_eq!(plan.changed_files, vec!["src.txt".to_string()]);
    }

    #[test]
    fn snapshot_restore_preserves_staged_and_unstaged_split() {
        let repo = temp_root("staged");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        fs::write(repo.join("src.txt"), "snapshot staged\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        fs::write(repo.join("src.txt"), "snapshot unstaged\n").unwrap();
        let store = RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store
            .create_snapshot(&repo, "staged split".to_string())
            .unwrap();

        assert!(snapshot.patch_bytes > 0);
        assert!(snapshot.staged_patch_bytes > 0);
        assert!(snapshot.unstaged_patch_bytes > 0);

        fs::write(repo.join("src.txt"), "later staged\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        fs::write(repo.join("src.txt"), "later unstaged\n").unwrap();
        let plan = store.restore_snapshot(&snapshot.id, true).unwrap();

        assert_eq!(
            git_stdout(&repo, &["show", ":src.txt"]).unwrap(),
            "snapshot staged\n"
        );
        assert_eq!(
            fs::read_to_string(repo.join("src.txt")).unwrap(),
            "snapshot unstaged\n"
        );
        assert_eq!(
            git_stdout(&repo, &["diff", "--name-only", "--cached"]).unwrap(),
            "src.txt\n"
        );
        assert_eq!(
            git_stdout(&repo, &["diff", "--name-only"]).unwrap(),
            "src.txt\n"
        );
        assert!(plan.staged_patch_bytes > 0);
        assert!(plan.unstaged_patch_bytes > 0);
    }

    #[test]
    fn snapshot_restore_round_trip_untracked_files() {
        let repo = temp_root("untracked");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        let store_root = repo.join(".dscode/rollback");
        fs::create_dir_all(store_root.join("snapshots/old")).unwrap();
        fs::write(store_root.join("snapshots/old/junk.txt"), "ignore\n").unwrap();
        fs::create_dir_all(repo.join("notes")).unwrap();
        fs::write(repo.join("src.txt"), "snapshot\n").unwrap();
        fs::write(repo.join("notes/todo.txt"), "snapshot note\n").unwrap();
        let store = RollbackStore::new(store_root);

        let snapshot = store
            .create_snapshot(&repo, "capture untracked".to_string())
            .unwrap();

        assert!(!snapshot.tracked_only);
        assert_eq!(snapshot.untracked_files, vec!["notes/todo.txt".to_string()]);
        assert!(snapshot.untracked_bytes > 0);

        fs::write(repo.join("src.txt"), "later\n").unwrap();
        fs::write(repo.join("notes/todo.txt"), "later note\n").unwrap();
        let plan = store.restore_snapshot(&snapshot.id, true).unwrap();

        assert_eq!(
            fs::read_to_string(repo.join("src.txt")).unwrap(),
            "snapshot\n"
        );
        assert_eq!(
            fs::read_to_string(repo.join("notes/todo.txt")).unwrap(),
            "snapshot note\n"
        );
        assert_eq!(
            plan.changed_files,
            vec!["notes/todo.txt".to_string(), "src.txt".to_string()]
        );

        let loaded = store.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.untracked_files, snapshot.untracked_files);
        assert_eq!(loaded.untracked_bytes, snapshot.untracked_bytes);
    }

    #[test]
    fn snapshot_restore_round_trip_empty_untracked_directories() {
        let repo = temp_root("empty-dir");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        let store_root = repo.join(".dscode/rollback");
        fs::create_dir_all(store_root.join("snapshots/old")).unwrap();
        fs::create_dir_all(repo.join("notes/empty")).unwrap();
        let store = RollbackStore::new(store_root);
        let snapshot = store
            .create_snapshot(&repo, "capture empty directory".to_string())
            .unwrap();

        assert!(!snapshot.tracked_only);
        assert!(snapshot.untracked_files.is_empty());
        assert_eq!(
            snapshot.untracked_directories,
            vec!["notes/empty".to_string()]
        );

        fs::remove_dir_all(repo.join("notes")).unwrap();
        fs::create_dir_all(repo.join("notes")).unwrap();
        fs::write(repo.join("notes/empty"), "later file\n").unwrap();
        let plan = store.restore_snapshot(&snapshot.id, true).unwrap();

        assert!(repo.join("notes/empty").is_dir());
        assert_eq!(plan.changed_files, vec!["notes/empty".to_string()]);

        let loaded = store.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.untracked_directories, snapshot.untracked_directories);
    }

    #[test]
    fn snapshot_ignores_ignored_empty_directories() {
        let repo = temp_root("ignored-empty-dir");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join(".gitignore"), "ignored/\n").unwrap();
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", ".gitignore", "src.txt"]);
        run_git(
            &repo,
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

        fs::create_dir_all(repo.join("ignored/empty")).unwrap();
        let store = RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store
            .create_snapshot(&repo, "ignore empty directory".to_string())
            .unwrap();

        assert!(snapshot.untracked_directories.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_restore_round_trip_untracked_symlinks() {
        let repo = temp_root("symlink");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        std::os::unix::fs::symlink("missing-target.txt", repo.join("link.txt")).unwrap();
        let store = RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store
            .create_snapshot(&repo, "capture symlink".to_string())
            .unwrap();

        assert!(!snapshot.tracked_only);
        assert!(snapshot.untracked_files.is_empty());
        assert_eq!(
            snapshot.untracked_symlinks,
            vec![UntrackedSymlinkRecord {
                path: "link.txt".to_string(),
                target: "missing-target.txt".to_string(),
            }]
        );
        assert!(snapshot.untracked_bytes > 0);

        fs::remove_file(repo.join("link.txt")).unwrap();
        fs::write(repo.join("link.txt"), "later file\n").unwrap();
        let plan = store.restore_snapshot(&snapshot.id, true).unwrap();

        assert_eq!(
            fs::read_link(repo.join("link.txt")).unwrap(),
            PathBuf::from("missing-target.txt")
        );
        assert_eq!(plan.changed_files, vec!["link.txt".to_string()]);

        let loaded = store.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.untracked_symlinks, snapshot.untracked_symlinks);
        assert_eq!(loaded.untracked_bytes, snapshot.untracked_bytes);
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_restore_round_trip_untracked_fifos() {
        use std::os::unix::fs::FileTypeExt;

        let repo = temp_root("fifo");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        fs::create_dir_all(repo.join("pipes")).unwrap();
        let fifo_path = repo.join("pipes/input.fifo");
        let mkfifo_status = Command::new("mkfifo").arg(&fifo_path).status().unwrap();
        assert!(mkfifo_status.success());
        let store = RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store
            .create_snapshot(&repo, "capture fifo".to_string())
            .unwrap();

        assert!(!snapshot.tracked_only);
        assert!(snapshot.untracked_files.is_empty());
        assert_eq!(
            snapshot.untracked_fifos,
            vec!["pipes/input.fifo".to_string()]
        );

        fs::remove_file(&fifo_path).unwrap();
        fs::write(&fifo_path, "later file\n").unwrap();
        let plan = store.restore_snapshot(&snapshot.id, true).unwrap();

        assert!(fs::symlink_metadata(&fifo_path)
            .unwrap()
            .file_type()
            .is_fifo());
        assert_eq!(plan.changed_files, vec!["pipes/input.fifo".to_string()]);

        let loaded = store.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.untracked_fifos, snapshot.untracked_fifos);
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_restore_round_trip_untracked_sockets() {
        use std::os::unix::fs::FileTypeExt;
        use std::os::unix::net::UnixListener;

        let repo = temp_root("socket");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        fs::create_dir_all(repo.join("sockets")).unwrap();
        let socket_path = repo.join("sockets/agent.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let store = RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store
            .create_snapshot(&repo, "capture socket".to_string())
            .unwrap();

        assert!(!snapshot.tracked_only);
        assert!(snapshot.untracked_files.is_empty());
        assert_eq!(
            snapshot.untracked_sockets,
            vec!["sockets/agent.sock".to_string()]
        );

        drop(listener);
        fs::remove_file(&socket_path).unwrap();
        fs::write(&socket_path, "later file\n").unwrap();
        let plan = store.restore_snapshot(&snapshot.id, true).unwrap();

        assert!(fs::symlink_metadata(&socket_path)
            .unwrap()
            .file_type()
            .is_socket());
        assert_eq!(plan.changed_files, vec!["sockets/agent.sock".to_string()]);

        let loaded = store.load_snapshot(&snapshot.id).unwrap();
        assert_eq!(loaded.untracked_sockets, snapshot.untracked_sockets);
    }

    #[test]
    fn legacy_snapshot_manifest_without_symlinks_still_parses() {
        let manifest = r#"{
            "id":"snapshot-legacy",
            "label":"legacy",
            "created_at":"epoch+1",
            "workspace":".",
            "git_root":".",
            "git_head":"abc123",
            "status_bytes":0,
            "patch_bytes":0,
            "tracked_only":true,
            "untracked_files":[]
        }"#;
        let record = parse_snapshot_record(&parse_root_object(manifest).unwrap()).unwrap();

        assert_eq!(record.id, "snapshot-legacy");
        assert!(record.untracked_files.is_empty());
        assert!(record.untracked_directories.is_empty());
        assert!(record.untracked_fifos.is_empty());
        assert!(record.untracked_sockets.is_empty());
        assert!(record.untracked_symlinks.is_empty());
    }

    #[test]
    fn snapshot_can_bind_and_restore_by_runtime_turn_id() {
        let repo = temp_root("turn");
        fs::create_dir_all(&repo).unwrap();
        run_git(&repo, &["init"]);
        fs::write(repo.join("src.txt"), "base\n").unwrap();
        run_git(&repo, &["add", "src.txt"]);
        run_git(
            &repo,
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

        fs::write(repo.join("src.txt"), "snapshot\n").unwrap();
        let store = RollbackStore::new(repo.join(".dscode/rollback"));
        let snapshot = store
            .create_snapshot(&repo, "runtime turn".to_string())
            .unwrap();
        let bound = store
            .bind_snapshot_runtime(&snapshot.id, Some("thread-abc"), Some("turn-abc"))
            .unwrap();

        assert_eq!(bound.runtime_thread_id.as_deref(), Some("thread-abc"));
        assert_eq!(bound.runtime_turn_id.as_deref(), Some("turn-abc"));
        assert_eq!(
            store.load_snapshot_for_turn("turn-abc").unwrap().id,
            snapshot.id
        );

        fs::write(repo.join("src.txt"), "later\n").unwrap();
        let plan = store.restore_snapshot("turn-abc", true).unwrap();

        assert_eq!(plan.snapshot_id, snapshot.id);
        assert_eq!(
            fs::read_to_string(repo.join("src.txt")).unwrap(),
            "snapshot\n"
        );
    }
}

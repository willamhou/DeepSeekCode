use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{app_error, AppResult};

const MAX_MEMORY_BYTES: usize = 32 * 1024;

pub struct MemoryState {
    active_profile: String,
}

impl MemoryState {
    pub fn new(active_profile: String) -> Self {
        Self { active_profile }
    }

    pub fn summary(&self) -> String {
        format!("active profile = {}", self.active_profile)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentMemory {
    pub path: PathBuf,
    pub content: String,
    pub truncated: bool,
}

pub fn load_user_memory(enabled: bool, path: &Path) -> AppResult<Option<PersistentMemory>> {
    if !enabled || !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let (content, truncated) = truncate_utf8(&raw, MAX_MEMORY_BYTES);
    Ok(Some(PersistentMemory {
        path: path.to_path_buf(),
        content,
        truncated,
    }))
}

pub fn render_user_memory(memory: &PersistentMemory) -> String {
    let mut rendered = format!(
        "User memory (loaded from {}; durable user preferences and conventions):\n",
        memory.path.display()
    );
    rendered.push_str(memory.content.trim());
    rendered.push('\n');
    if memory.truncated {
        rendered.push_str("[truncated to 32768 bytes]\n");
    }
    rendered
}

pub fn append_user_memory(path: &Path, note: &str) -> AppResult<String> {
    let trimmed = note.trim_start_matches('#').trim();
    if trimmed.is_empty() {
        return Err(app_error("remember note must not be empty"));
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "- ({timestamp} unix) {trimmed}")?;
    Ok(trimmed.to_string())
}

fn truncate_utf8(content: &str, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content.to_string(), false);
    }
    let mut end = max_bytes;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-memory-{name}-{}-{suffix}",
            std::process::id()
        ))
    }

    #[test]
    fn load_user_memory_omits_disabled_missing_and_empty_memory() {
        let root = temp_root("empty");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("memory.md");

        assert!(load_user_memory(false, &path).unwrap().is_none());
        assert!(load_user_memory(true, &path).unwrap().is_none());
        fs::write(&path, " \n").unwrap();
        assert!(load_user_memory(true, &path).unwrap().is_none());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn load_user_memory_reads_and_renders_content() {
        let root = temp_root("read");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("memory.md");
        fs::write(&path, "- prefer cargo test\n").unwrap();

        let memory = load_user_memory(true, &path).unwrap().unwrap();
        let rendered = render_user_memory(&memory);

        assert!(rendered.contains("prefer cargo test"));
        assert!(rendered.contains(&path.display().to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn append_user_memory_strips_hash_and_writes_bullet() {
        let root = temp_root("append");
        let path = root.join("memory.md");

        let remembered = append_user_memory(&path, "# prefer rustfmt").unwrap();
        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(remembered, "prefer rustfmt");
        assert!(content.contains("- ("));
        assert!(content.contains(") prefer rustfmt"));
        assert!(!content.contains("# prefer rustfmt"));

        let _ = fs::remove_dir_all(root);
    }
}

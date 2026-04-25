use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{app_error, AppResult};

#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub id: String,
    pub task: String,
    pub profile: String,
}

impl SessionSnapshot {
    pub fn new(task: String, profile: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id: format!("session-{}", now),
            task,
            profile,
        }
    }
}

pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn save(&self, snapshot: &SessionSnapshot) -> AppResult<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{}.toml", snapshot.id));
        let body = format!(
            "id = \"{}\"\ntask = \"{}\"\nprofile = \"{}\"\n",
            snapshot.id,
            escape(&snapshot.task),
            escape(&snapshot.profile)
        );
        fs::write(path, body)?;
        Ok(())
    }

    pub fn load_latest(&self, requested: Option<&str>) -> AppResult<SessionSnapshot> {
        let path = if let Some(id) = requested {
            self.dir.join(format!("{}.toml", id))
        } else {
            latest_session_file(&self.dir)?
        };

        let content = fs::read_to_string(path)?;
        parse_snapshot(&content)
    }
}

fn latest_session_file(dir: &Path) -> AppResult<PathBuf> {
    let mut entries = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .collect::<Vec<_>>();
    entries.sort();
    entries
        .pop()
        .ok_or_else(|| app_error("no saved sessions found"))
}

fn parse_snapshot(content: &str) -> AppResult<SessionSnapshot> {
    let mut id = String::new();
    let mut task = String::new();
    let mut profile = String::new();

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());
        match key {
            "id" => id = value,
            "task" => task = value,
            "profile" => profile = value,
            _ => {}
        }
    }

    if id.is_empty() {
        return Err(app_error("invalid session snapshot"));
    }

    Ok(SessionSnapshot { id, task, profile })
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn unquote(value: &str) -> String {
    value
        .trim_matches('"')
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

use std::path::PathBuf;

use super::types::WorkspaceConfig;

impl WorkspaceConfig {
    pub fn config_path(&self) -> PathBuf {
        PathBuf::from(&self.config_dir).join("config.toml")
    }

    pub fn session_dir(&self) -> PathBuf {
        PathBuf::from(&self.session_dir)
    }
}


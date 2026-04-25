use std::fs;
use std::path::Path;

use crate::error::AppResult;
use super::types::AppConfig;

pub fn load_or_default() -> AppResult<AppConfig> {
    let default = AppConfig::default();
    let path = default.workspace.config_path();

    if !Path::new(&path).exists() {
        return Ok(default);
    }

    let _content = fs::read_to_string(path)?;
    Ok(default)
}

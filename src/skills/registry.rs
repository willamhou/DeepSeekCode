use std::fs;
use std::path::Path;

use crate::error::AppResult;
use crate::skills::loader::load_skill;
use crate::skills::schema::SkillSpec;

#[derive(Debug, Default)]
pub struct SkillRegistry {
    skills: Vec<SkillSpec>,
}

impl SkillRegistry {
    pub fn load_dir(path: &str) -> AppResult<Self> {
        let dir = Path::new(path);
        if !dir.exists() {
            return Ok(Self::default());
        }

        let mut skills = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
                skills.push(load_skill(&path)?);
            }
        }

        Ok(Self { skills })
    }

    pub fn all(&self) -> &[SkillSpec] {
        &self.skills
    }
}

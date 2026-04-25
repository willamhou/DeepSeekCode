#[derive(Debug, Clone)]
pub struct SkillSpec {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub system_append: String,
    pub suggested_steps: Vec<String>,
    pub policy: SkillPolicy,
}

#[derive(Debug, Clone)]
pub struct SkillPolicy {
    pub require_write_confirmation: bool,
    pub require_shell_confirmation: bool,
    pub shell_allowlist: Vec<String>,
}

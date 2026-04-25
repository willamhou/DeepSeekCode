use crate::config::types::AppConfig;
use crate::core::context::TaskContext;
use crate::core::memory::MemoryState;
use crate::core::session::{SessionSnapshot, SessionStore};
use crate::error::AppResult;
use crate::language::detect::detect_profile;
use crate::skills::registry::SkillRegistry;
use crate::skills::resolver::resolve_skill;
use crate::tools::registry::default_registry;
use crate::ui::render::print_banner;

pub struct AgentLoop {
    config: AppConfig,
}

impl AgentLoop {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }

    pub fn run(&self, context: TaskContext) -> AppResult<()> {
        print_banner("DeepseekCode");

        let profile = detect_profile(".")?;
        let registry = default_registry();
        let skills = SkillRegistry::load_dir("skills")?;
        let skill = resolve_skill(&skills, context.skill.as_deref());
        let memory = MemoryState::new(profile.name.clone());

        println!("Task: {}", context.task);
        println!("Profile: {}", profile.name);
        println!("Available tools: {}", registry.names().join(", "));

        if let Some(skill) = skill {
            println!("Skill: {}", skill.name);
        }

        println!("Memory summary: {}", memory.summary());
        println!("Runtime status: scaffold only, model loop not connected yet.");

        let store = SessionStore::new(self.config.workspace.session_dir());
        let snapshot = SessionSnapshot::new(context.task, profile.name);
        store.save(&snapshot)?;

        Ok(())
    }
}

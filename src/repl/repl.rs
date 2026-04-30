use crate::config::types::AppConfig;
use crate::error::{app_error, AppResult};
use crate::repl::transcript::Transcript;

pub const DEFAULT_BUDGET: usize = 20;

pub enum ControlFlow {
    Continue,
    Quit,
}

pub struct Repl {
    pub config: AppConfig,
    pub transcript: Transcript,
    pub budget: usize,
    pub skill: Option<String>,
    pub tokens_prompt: u64,
    pub tokens_completion: u64,
}

impl Repl {
    pub fn new(config: AppConfig, skill: Option<String>) -> Self {
        Self {
            config,
            transcript: Transcript::default(),
            budget: DEFAULT_BUDGET,
            skill,
            tokens_prompt: 0,
            tokens_completion: 0,
        }
    }

    pub fn run(&mut self) -> AppResult<()> {
        Err(app_error("dscode chat REPL is implemented in a later task"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::AppConfig;

    #[test]
    fn new_starts_with_default_budget_and_empty_transcript() {
        let r = Repl::new(AppConfig::default(), None);
        assert_eq!(r.budget, DEFAULT_BUDGET);
        assert!(r.transcript.turns.is_empty());
        assert_eq!(r.tokens_prompt, 0);
        assert_eq!(r.tokens_completion, 0);
        assert!(r.skill.is_none());
    }

    #[test]
    fn new_keeps_skill_when_provided() {
        let r = Repl::new(AppConfig::default(), Some("pr-review".to_string()));
        assert_eq!(r.skill.as_deref(), Some("pr-review"));
    }
}

use crate::core::todos::{Todo, TodoStatus};

#[derive(Debug, Clone)]
pub struct SkillSpec {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub system_append: String,
    pub suggested_steps: Vec<String>,
    pub triggers: Vec<String>,
    pub initial_todos: Vec<TodoSeed>,
    pub references: Vec<String>,
    pub policy: SkillPolicy,
}

#[derive(Debug, Clone)]
pub struct SkillPolicy {
    pub require_write_confirmation: bool,
    pub require_shell_confirmation: bool,
    pub shell_allowlist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSeed {
    pub content: String,
    pub active_form: String,
    pub status: TodoStatus,
}

impl TodoSeed {
    pub fn to_todo(&self) -> Todo {
        Todo {
            content: self.content.clone(),
            active_form: self.active_form.clone(),
            status: self.status,
        }
    }
}

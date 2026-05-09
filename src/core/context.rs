#[derive(Debug, Clone)]
pub struct TaskContext {
    pub task: String,
    pub skill: Option<String>,
}

impl TaskContext {
    pub fn new(task: String, skill: Option<String>) -> Self {
        Self { task, skill }
    }
}

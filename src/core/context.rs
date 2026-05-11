use crate::model::protocol::ImageInput;

#[derive(Debug, Clone)]
pub struct TaskContext {
    pub task: String,
    pub skill: Option<String>,
    pub image_inputs: Vec<ImageInput>,
}

impl TaskContext {
    pub fn new(task: String, skill: Option<String>) -> Self {
        Self {
            task,
            skill,
            image_inputs: Vec::new(),
        }
    }

    pub fn with_image_inputs(
        task: String,
        skill: Option<String>,
        image_inputs: Vec<ImageInput>,
    ) -> Self {
        Self {
            task,
            skill,
            image_inputs,
        }
    }
}

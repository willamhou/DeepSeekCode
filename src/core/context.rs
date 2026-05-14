use crate::model::protocol::ImageInput;

#[derive(Debug, Clone)]
pub struct TaskContext {
    pub task: String,
    pub skill: Option<String>,
    pub image_inputs: Vec<ImageInput>,
    pub translation_target_language: Option<String>,
}

impl TaskContext {
    pub fn new(task: String, skill: Option<String>) -> Self {
        Self {
            task,
            skill,
            image_inputs: Vec::new(),
            translation_target_language: None,
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
            translation_target_language: None,
        }
    }

    pub fn with_translation_target_language(mut self, target_language: impl Into<String>) -> Self {
        let target_language = target_language.into();
        if !target_language.trim().is_empty() {
            self.translation_target_language = Some(target_language);
        }
        self
    }
}

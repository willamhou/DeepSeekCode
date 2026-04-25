use crate::config::types::ModelConfig;
use crate::error::AppResult;
use crate::model::client::ModelClient;
use crate::model::protocol::{ModelRequest, ModelResponse};

pub struct DeepSeekClient {
    pub config: ModelConfig,
}

impl ModelClient for DeepSeekClient {
    fn respond(&self, input: ModelRequest) -> AppResult<ModelResponse> {
        let message = format!(
            "stub response from {} for prompt: {}",
            self.config.model, input.user_prompt
        );
        Ok(ModelResponse { message })
    }
}

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub system_prompt: String,
    pub user_prompt: String,
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub message: String,
}


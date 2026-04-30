use crate::error::AppResult;
use crate::model::protocol::{ModelRequest, ModelResponse, TokenUsage};

pub trait ModelClient {
    fn respond(&self, input: ModelRequest) -> AppResult<(ModelResponse, Option<TokenUsage>)>;
}

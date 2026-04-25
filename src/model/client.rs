use crate::error::AppResult;
use crate::model::protocol::{ModelRequest, ModelResponse};

pub trait ModelClient {
    fn respond(&self, input: ModelRequest) -> AppResult<ModelResponse>;
}

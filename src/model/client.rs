use crate::error::AppResult;
use crate::model::protocol::{ModelRequest, ModelResponse, TokenUsage};
use crate::ui::stream::StreamEvents;

pub trait ModelClient {
    fn respond(
        &self,
        input: ModelRequest,
        events: &mut dyn StreamEvents,
    ) -> AppResult<(ModelResponse, Option<TokenUsage>)>;
}

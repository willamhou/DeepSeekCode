use crate::error::AppResult;
use crate::model::protocol::{ModelRequest, ModelResponse, TokenUsage};
use crate::ui::stream::StreamEvents;
use crate::util::cancel::CancellationCheck;

pub trait ModelClient {
    fn respond(
        &self,
        input: ModelRequest,
        events: &mut dyn StreamEvents,
    ) -> AppResult<(ModelResponse, Option<TokenUsage>)>;

    fn respond_with_cancel(
        &self,
        input: ModelRequest,
        events: &mut dyn StreamEvents,
        _cancel_check: Option<&mut dyn CancellationCheck>,
    ) -> AppResult<(ModelResponse, Option<TokenUsage>)> {
        self.respond(input, events)
    }
}

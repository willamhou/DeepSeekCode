use crate::error::{app_error, AppResult};

pub trait CancellationCheck {
    fn is_cancelled(&mut self) -> AppResult<bool>;
}

pub fn check_cancelled(cancel_check: Option<&mut dyn CancellationCheck>) -> AppResult<()> {
    if let Some(check) = cancel_check {
        if check.is_cancelled()? {
            return Err(app_error("agent run cancelled"));
        }
    }
    Ok(())
}

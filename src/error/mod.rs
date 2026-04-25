use std::error::Error;
use std::fmt::{Display, Formatter};

pub type AppResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub struct AppError {
    message: String,
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for AppError {}

pub fn app_error(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(AppError {
        message: message.into(),
    })
}

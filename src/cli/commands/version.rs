use crate::error::AppResult;

pub fn run() -> AppResult<()> {
    println!("deepseek {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

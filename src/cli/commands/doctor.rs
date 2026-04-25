use crate::cli::app::DoctorArgs;
use crate::config::load::load_or_default;
use crate::error::AppResult;

pub fn run(_args: DoctorArgs) -> AppResult<()> {
    let config = load_or_default()?;
    println!("DeepseekCode doctor");
    println!("Config path: {}", config.workspace.config_path().display());
    println!("Session dir: {}", config.workspace.session_dir().display());
    println!("Model: {}", config.model.model);
    Ok(())
}

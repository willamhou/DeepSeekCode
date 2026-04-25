use crate::cli::app::ConfigArgs;
use crate::config::load::load_or_default;
use crate::error::AppResult;

pub fn run(args: ConfigArgs) -> AppResult<()> {
    let config = load_or_default()?;
    if args.print_default {
        println!("model.base_url = {}", config.model.base_url);
        println!("model.model = {}", config.model.model);
        println!("model.api_key_env = {}", config.model.api_key_env);
        println!(
            "approval.require_write_confirmation = {}",
            config.approval.require_write_confirmation
        );
        println!(
            "approval.require_shell_confirmation = {}",
            config.approval.require_shell_confirmation
        );
        println!("workspace.config_dir = {}", config.workspace.config_dir);
        println!("workspace.session_dir = {}", config.workspace.session_dir);
    } else {
        println!("Config file path: {}", config.workspace.config_path().display());
    }
    Ok(())
}

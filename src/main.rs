mod config;
mod secret_key;

use anyhow::{Error, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::time::sleep;
use tracing::info;
use crate::config::Config;

fn startup_error(spinner: &ProgressBar, context: &str, err: Error) -> Error {
    spinner.finish_and_clear();
    eprintln!("  {} {}: {}", "✗".red(), context, err);
    err
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("  Deputy Ver. {}", env!("CARGO_PKG_VERSION"));

    sleep(std::time::Duration::from_secs(1)).await;
    println!();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("  {spinner} Starting deputy...")?,
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let _config = Config::load()
        .map_err(|err| startup_error(&spinner, "Failed to load configuration", err))?;
    info!("Configuration loaded successfully");

    Ok(())
}

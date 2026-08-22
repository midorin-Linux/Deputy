pub mod core;
pub mod features;

use core::{config::Config, registry::Registry, store::NoopStore, telemetry::init_tracing};
use std::sync::Arc;

use anyhow::{Error, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serenity::all::{Client, GatewayIntents};
use tracing::{error, info};

fn startup_error(spinner: &ProgressBar, context: &str, err: Error) -> Error {
    spinner.finish_and_clear();
    error!(error = ?err, "{context}");
    eprintln!("  {} {}: {}", "✗".red(), context, err);
    err
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("  Deputy Ver. {}", env!("CARGO_PKG_VERSION"));
    println!();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("  {spinner} {msg}")?,
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    // Tracingの初期化
    spinner.set_message("Initializing tracing...");
    let _guard = init_tracing()
        .map_err(|err| startup_error(&spinner, "Failed to initialize tracing", err))?;
    info!("Tracing initialized successfully");

    // Configの読み込み
    spinner.set_message("Loading configuration...");
    let config = Config::load()
        .map_err(|err| startup_error(&spinner, "Failed to load configuration", err))?;
    info!("Configuration loaded successfully");

    // 機能の組み立て（有効/無効は設定ファイルで完結する）
    spinner.set_message("Building features...");
    let store = Arc::new(NoopStore);
    let feats = features::all(&config, store)
        .map_err(|err| startup_error(&spinner, "Failed to build features", err))?;
    for feature in &feats {
        info!(feature = feature.name(), "feature enabled");
    }
    let registry = Registry::new(feats);

    let intents =
        GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS | GatewayIntents::GUILD_VOICE_STATES;

    spinner.set_message("Connecting to Discord...");
    let mut client = Client::builder(config.discord.token.expose(), intents)
        .event_handler(registry)
        .await
        .map_err(|err| startup_error(&spinner, "Failed to build Discord client", err.into()))?;

    // 終了処理
    spinner.finish_and_clear();
    info!("Startup completed successfully");
    println!("  {} Startup completed successfully", "✓".green());

    if let Err(err) = client.start().await {
        error!(error = ?err, "gateway client stopped with error");
        return Err(err.into());
    }

    Ok(())
}

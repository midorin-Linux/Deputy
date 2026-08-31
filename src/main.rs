pub mod core;
pub mod features;
pub mod services;

use core::{config::Config, registry::Registry, store::NoopStore, telemetry::init_tracing};
use std::sync::Arc;

use anyhow::{Error, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serenity::all::{Client, GatewayIntents};
use songbird::SerenityInit;
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

    // rustlsの既定CryptoProviderを明示する。依存ツリーでring（serenity/songbird系）と
    // aws-lc-rs（reqwest 0.13系）の両featureが有効なため自動選択できず、未指定のままだと
    // VC接続（songbirdのTLS確立）時にpanicする。失敗は「設定済み」を意味するので無視してよい。
    let _ = rustls::crypto::ring::default_provider().install_default();

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

    // `GUILD_MEMBERS`と`MESSAGE_CONTENT`は特権インテント。Developer Portalで有効化しないと
    // ゲートウェイがclose code 4014で切断される。機能の有効/無効に関わらず常に要求するため、
    // honeypotを使わない構成でも`MESSAGE_CONTENT`の有効化が要る（README参照）。
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    spinner.set_message("Connecting to Discord...");
    let mut client = Client::builder(config.discord.token.expose(), intents)
        .event_handler(registry)
        .register_songbird()
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

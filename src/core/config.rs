use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use config::{Config as ConfigBuilder, File};
use serde::{Deserialize, de::DeserializeOwned};
use tracing::{debug, info};

use crate::core::{error::ConfigError, secret_key::SecretKey};

/// 設定ファイルのパス。`Config`と`logging`の軽量読み取りで共有する。
pub const SETTINGS_FILE: &str = "settings.yml";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub env: EnvConfig,
    pub discord: DiscordConfig,
    pub ai: AiConfig,

    /// 機能ごとの設定セクション。`core`は中身を知らず、各`features/<name>/config.rs`が
    /// 自分の名前のキーを取り出してデシリアライズする。
    #[serde(default)]
    pub features: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for EnvConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscordConfig {
    pub token: SecretKey,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiConfig {
    pub api_key: SecretKey,
    pub base_url: String,
    pub model_id: String,

    #[serde(default = "default_support_image")]
    pub support_image: bool,

    /// AIプロバイダへのリクエストタイムアウト（秒）。ハング時に判定が無期限ブロックするのを防ぐ。
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_support_image() -> bool {
    false
}

fn default_request_timeout_secs() -> u64 {
    300
}

impl Config {
    pub fn load() -> Result<Self> {
        info!("loading configuration file");

        let settings_path = PathBuf::from(SETTINGS_FILE);

        let config = ConfigBuilder::builder()
            .add_source(
                File::from(settings_path)
                    .format(config::FileFormat::Yaml)
                    .required(true),
            )
            .build()
            .context("failed to build config")?;

        debug!("configuration source parsed");

        let parsed: Self = config.try_deserialize()?;

        parsed.validate()?;

        info!("configuration deserialized successfully");

        Ok(parsed)
    }

    pub fn validate(&self) -> Result<()> {
        if self.discord.token.expose().trim().is_empty() {
            return Err(ConfigError::MissingDiscordToken.into());
        }

        Ok(())
    }
}

/// `enabled` / `log_channel`という同じ形を持つ機能設定が実装するトレイト。
/// `load_feature_config`はこれだけを使って有効/無効判定とチャンネルIDの検証を行う。
pub trait FeatureToggle {
    fn is_enabled(&self) -> bool;
    /// 検証前の生のチャンネルID。0は「未指定」を意味する。
    fn raw_log_channel(&self) -> u64;
}

/// `cfg.features`から`name`セクションを取り出し、`enabled` / `log_channel`の
/// 共通ルールで検証する。各`features/<name>/config.rs`の`load`はこれへ委譲するだけでよい。
///
/// ルール（member_log・voice_logの両方で必要だったため、ここへ一本化する）：
/// - セクションが無い、または`enabled = false`なら`None`。
/// - 有効なのに`log_channel`が0（未指定）なら`ConfigError::InvalidLogChannel`。
///   `ChannelId::new(0)`は実行時にpanicするため、起動時に前倒しで弾く。
pub fn load_feature_config<T>(cfg: &Config, name: &'static str) -> Result<Option<T>>
where
    T: DeserializeOwned + FeatureToggle,
{
    let Some(raw) = cfg.features.get(name) else {
        return Ok(None);
    };

    let parsed: T = serde_json::from_value(raw.clone())
        .with_context(|| format!("failed to parse features.{name}"))?;

    if !parsed.is_enabled() {
        return Ok(None);
    }

    if parsed.raw_log_channel() == 0 {
        return Err(ConfigError::InvalidLogChannel { feature: name }.into());
    }

    Ok(Some(parsed))
}

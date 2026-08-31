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
    /// 検証前の生のチャンネルID。
    /// - `Some(0)`: 「未指定」。有効時は`ConfigError::InvalidLogChannel`。
    /// - `Some(n)` (n != 0): 検証OK。
    /// - `None`: そもそもログチャンネルを持たない機能（TTS等）。検証自体をスキップする。
    fn raw_log_channel(&self) -> Option<u64>;
}

/// `cfg.features`から`name`セクションを取り出し、`enabled` / `log_channel`の
/// 共通ルールで検証する。各`features/<name>/config.rs`の`load`はこれへ委譲するだけでよい。
///
/// ルール（member_log・voice_logの両方で必要だったため、ここへ一本化する）：
/// - セクションが無い、または`enabled = false`なら`None`。
/// - 有効なのに`log_channel`が`Some(0)`（未指定）なら`ConfigError::InvalidLogChannel`。
///   `ChannelId::new(0)`は実行時にpanicするため、起動時に前倒しで弾く。
/// - `log_channel`が`None`（そのfeatureがログチャンネルを持たない）ならこの検証はスキップする。
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

    if let Some(0) = parsed.raw_log_channel() {
        return Err(ConfigError::InvalidLogChannel { feature: name }.into());
    }

    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config(token: &str) -> Config {
        serde_json::from_value(serde_json::json!({
            "env": { "log_level": "info" },
            "discord": { "token": token },
            "ai": { "api_key": "k", "base_url": "u", "model_id": "m" },
            "features": {},
        }))
        .expect("base config must deserialize")
    }

    #[test]
    fn validate_accepts_non_empty_token() {
        assert!(base_config("t").validate().is_ok());
    }

    #[test]
    fn validate_rejects_empty_token() {
        let err = base_config("").validate().unwrap_err();
        assert!(err.to_string().contains("discord.token"));
    }

    #[test]
    fn validate_rejects_whitespace_only_token() {
        let err = base_config("   ").validate().unwrap_err();
        assert!(err.to_string().contains("discord.token"));
    }

    #[derive(Debug, Clone, Deserialize)]
    struct DummyFeatureConfig {
        #[serde(default)]
        enabled: bool,
        #[serde(default)]
        log_channel: u64,
    }

    impl FeatureToggle for DummyFeatureConfig {
        fn is_enabled(&self) -> bool {
            self.enabled
        }

        fn raw_log_channel(&self) -> Option<u64> {
            Some(self.log_channel)
        }
    }

    #[test]
    fn load_feature_config_reports_malformed_section() {
        let mut cfg = base_config("t");
        // log_channelが文字列など、期待する型と合わない場合はデシリアライズ失敗として弾く。
        cfg.features.insert(
            "dummy".to_string(),
            serde_json::json!({ "enabled": true, "log_channel": "not-a-number" }),
        );
        let err = load_feature_config::<DummyFeatureConfig>(&cfg, "dummy").unwrap_err();
        assert!(err.to_string().contains("dummy"), "unexpected error: {err}");
    }

    /// `raw_log_channel`が`None`を返す機能（TTS等、ログチャンネルを持たない）は
    /// チャンネル検証自体をスキップし、`log_channel`が無くても`Some(_)`でロードされること。
    #[derive(Debug, Clone, Deserialize)]
    struct DummyNoChannelFeatureConfig {
        #[serde(default)]
        enabled: bool,
    }

    impl FeatureToggle for DummyNoChannelFeatureConfig {
        fn is_enabled(&self) -> bool {
            self.enabled
        }

        fn raw_log_channel(&self) -> Option<u64> {
            None
        }
    }

    #[test]
    fn load_feature_config_skips_channel_validation_when_raw_log_channel_is_none() {
        let mut cfg = base_config("t");
        cfg.features.insert(
            "dummy_no_channel".to_string(),
            serde_json::json!({ "enabled": true }),
        );
        let loaded =
            load_feature_config::<DummyNoChannelFeatureConfig>(&cfg, "dummy_no_channel").unwrap();
        assert!(loaded.is_some());
    }
}

use serde::Deserialize;
use serenity::all::ChannelId;

use crate::core::config::{Config, FeatureToggle, load_feature_config};

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceLogConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 通知先チャンネルID。`enabled: false`なら省略可（未指定は0となり、
    /// 有効時のみ`load`の検証でエラーになる）。
    #[serde(default)]
    pub log_channel: u64,
}

impl FeatureToggle for VoiceLogConfig {
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn raw_log_channel(&self) -> Option<u64> {
        Some(self.log_channel)
    }
}

impl VoiceLogConfig {
    /// `cfg.features`から`voice_log`セクションを取り出し、`enabled` / `log_channel`の
    /// 共通ルール（`core::config::load_feature_config`）で検証する。
    /// セクションが無い、または`enabled = false`なら`None`。
    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        load_feature_config(cfg, "voice_log")
    }

    /// 通知先チャンネル。`load`で0でないことを検証済みのため、ここではpanicしない。
    pub fn log_channel(&self) -> ChannelId {
        ChannelId::new(self.log_channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(section: serde_json::Value) -> Config {
        let mut cfg: Config = serde_json::from_value(serde_json::json!({
            "env": { "log_level": "info" },
            "discord": { "token": "t" },
            "ai": { "api_key": "k", "base_url": "u", "model_id": "m" },
            "features": {},
        }))
        .expect("base config must deserialize");
        cfg.features.insert("voice_log".to_string(), section);
        cfg
    }

    #[test]
    fn missing_section_is_disabled() {
        let mut cfg = config_with(serde_json::json!({}));
        cfg.features.remove("voice_log");
        assert!(VoiceLogConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn disabled_section_without_log_channel_is_accepted() {
        // 無効化の自然な書き方（enabledだけ残してlog_channel行を消す）で起動が止まらないこと。
        let cfg = config_with(serde_json::json!({ "enabled": false }));
        assert!(VoiceLogConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn enabled_section_without_log_channel_is_rejected_at_load() {
        let cfg = config_with(serde_json::json!({ "enabled": true }));
        let err = VoiceLogConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("log_channel"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_log_channel_is_rejected_at_load() {
        // 実行時のpanicではなく、起動時のエラーになること。
        let cfg = config_with(serde_json::json!({ "enabled": true, "log_channel": 0 }));
        let err = VoiceLogConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("log_channel"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn disabled_feature_skips_channel_validation() {
        let cfg = config_with(serde_json::json!({ "enabled": false, "log_channel": 0 }));
        assert!(VoiceLogConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn valid_section_yields_channel() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "log_channel": 42 }));
        let loaded = VoiceLogConfig::load(&cfg).unwrap().unwrap();
        assert_eq!(loaded.log_channel().get(), 42);
    }
}

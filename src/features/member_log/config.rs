use serde::Deserialize;
use serenity::all::ChannelId;

use crate::core::config::{Config, FeatureToggle, load_feature_config};

#[derive(Debug, Clone, Deserialize)]
pub struct MemberLogConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 通知先チャンネルID。`enabled: false`なら省略可（未指定は0となり、
    /// 有効時のみ`load`の検証でエラーになる）。
    #[serde(default)]
    pub log_channel: u64,
    #[serde(default = "default_new_account_warn_days")]
    pub new_account_warn_days: i64,
}

fn default_new_account_warn_days() -> i64 {
    7
}

impl FeatureToggle for MemberLogConfig {
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn raw_log_channel(&self) -> u64 {
        self.log_channel
    }
}

impl MemberLogConfig {
    /// `cfg.features`から`member_log`セクションを取り出し、`enabled` / `log_channel`の
    /// 共通ルール（`core::config::load_feature_config`）で検証する。
    /// セクションが無い、または`enabled = false`なら`None`。
    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        load_feature_config(cfg, "member_log")
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
        cfg.features.insert("member_log".to_string(), section);
        cfg
    }

    #[test]
    fn missing_section_is_disabled() {
        let mut cfg = config_with(serde_json::json!({}));
        cfg.features.remove("member_log");
        assert!(MemberLogConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn disabled_section_without_log_channel_is_accepted() {
        // 無効化の自然な書き方（enabledだけ残してlog_channel行を消す）で起動が止まらないこと。
        let cfg = config_with(serde_json::json!({ "enabled": false }));
        assert!(MemberLogConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn enabled_section_without_log_channel_is_rejected_at_load() {
        // 有効なのに通知先が無いのは設定ミスなので、起動時に明確なエラーで止める。
        let cfg = config_with(serde_json::json!({ "enabled": true }));
        let err = MemberLogConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("log_channel"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_log_channel_is_rejected_at_load() {
        // 実行時のpanicではなく、起動時のエラーになること。
        let cfg = config_with(serde_json::json!({ "enabled": true, "log_channel": 0 }));
        let err = MemberLogConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("log_channel"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn disabled_feature_skips_channel_validation() {
        // 無効なら通知先を持たないので、0でもエラーにしない。
        let cfg = config_with(serde_json::json!({ "enabled": false, "log_channel": 0 }));
        assert!(MemberLogConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn valid_section_yields_channel() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "log_channel": 42 }));
        let loaded = MemberLogConfig::load(&cfg).unwrap().unwrap();
        assert_eq!(loaded.log_channel().get(), 42);
        assert_eq!(loaded.new_account_warn_days, 7);
    }
}

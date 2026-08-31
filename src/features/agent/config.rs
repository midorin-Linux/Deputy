use serde::Deserialize;

use crate::core::{
    config::{Config, FeatureToggle, load_feature_config},
    error::ConfigError,
};

pub const FEATURE_NAME: &str = "agent";

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    enabled: bool,

    /// 使用するモデルID。省略時は共通の`ai.model_id`を使う。
    #[serde(default)]
    model: Option<String>,

    /// チャンネルごとに保持する会話履歴の件数（ユーザー発言・応答をあわせた件数）。
    #[serde(default = "default_history_limit")]
    history_limit: usize,

    /// ユーザー単位のクールダウン秒数。
    #[serde(default = "default_cooldown_secs")]
    cooldown_secs: u64,
}

fn default_history_limit() -> usize {
    20
}

fn default_cooldown_secs() -> u64 {
    15
}

impl FeatureToggle for AgentConfig {
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// agentも通知先チャンネルを持たない機能のため、検証自体をスキップさせる。
    fn raw_log_channel(&self) -> Option<u64> {
        None
    }
}

impl AgentConfig {
    /// `cfg.features`から`agent`セクションを取り出し、`enabled`の共通ルールで検証したうえで、
    /// この機能固有の設定値も起動時に検証する。
    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        let Some(parsed) = load_feature_config::<Self>(cfg, FEATURE_NAME)? else {
            return Ok(None);
        };

        parsed.validate()?;

        Ok(Some(parsed))
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.history_limit == 0 {
            return Err(self.invalid("history_limit must be greater than 0"));
        }

        // cooldown_secs=0は「クールダウン無効」として許容する
        // （ban_trigger.mention_thresholdの0=offと同じ慣習に合わせている）。
        Ok(())
    }

    fn invalid(&self, detail: &str) -> ConfigError {
        ConfigError::InvalidFeatureSetting {
            feature: FEATURE_NAME,
            detail: detail.to_string(),
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn history_limit(&self) -> usize {
        self.history_limit
    }

    pub fn cooldown_secs(&self) -> u64 {
        self.cooldown_secs
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
        cfg.features.insert(FEATURE_NAME.to_string(), section);
        cfg
    }

    #[test]
    fn missing_section_is_disabled() {
        let mut cfg = config_with(serde_json::json!({ "enabled": true }));
        cfg.features.remove(FEATURE_NAME);
        assert!(AgentConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn disabled_section_is_accepted_without_any_other_key() {
        let cfg = config_with(serde_json::json!({ "enabled": false }));
        assert!(AgentConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn defaults_match_settings_example() {
        let cfg = config_with(serde_json::json!({ "enabled": true }));
        let loaded = AgentConfig::load(&cfg).unwrap().unwrap();

        assert_eq!(loaded.model(), None);
        assert_eq!(loaded.history_limit(), 20);
        assert_eq!(loaded.cooldown_secs(), 15);
    }

    #[test]
    fn model_override_is_kept() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "model": "gpt-4o-mini" }));
        let loaded = AgentConfig::load(&cfg).unwrap().unwrap();

        assert_eq!(loaded.model(), Some("gpt-4o-mini"));
    }

    #[test]
    fn zero_history_limit_is_rejected() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "history_limit": 0 }));
        let err = AgentConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("history_limit"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_cooldown_secs_is_accepted_as_disabled() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "cooldown_secs": 0 }));
        let loaded = AgentConfig::load(&cfg).unwrap().unwrap();
        assert_eq!(loaded.cooldown_secs(), 0);
    }

    /// 配布している`settings.example.yml`のagentセクションが、実際に読める形のまま
    /// 保たれていることを確かめる。
    #[test]
    fn shipped_example_settings_are_loadable() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("settings.example.yml");

        let mut cfg: Config = config::Config::builder()
            .add_source(config::File::from(path).format(config::FileFormat::Yaml))
            .build()
            .expect("example settings must build")
            .try_deserialize()
            .expect("example settings must deserialize into Config");

        let section = cfg
            .features
            .get_mut(FEATURE_NAME)
            .expect("example settings must contain an agent section");

        // 例では無効にしてあるため、有効化したうえで検証まで通ることを確かめる。
        section["enabled"] = serde_json::json!(true);

        let loaded = AgentConfig::load(&cfg)
            .expect("example agent section must be valid")
            .expect("agent must be enabled");

        assert_eq!(loaded.history_limit(), 20);
        assert_eq!(loaded.cooldown_secs(), 15);
    }
}

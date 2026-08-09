use anyhow::Context;
use serde::Deserialize;

use crate::core::config::Config;

#[derive(Debug, Clone, Deserialize)]
pub struct VoiceLogConfig {
    #[serde(default)]
    pub enabled: bool,
    pub log_channel: u64,
}

impl VoiceLogConfig {
    /// `cfg.features`から`voice_log`セクションを取り出す。
    /// セクションが無い、または`enabled = false`なら`None`。
    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        let Some(raw) = cfg.features.get("voice_log") else {
            return Ok(None);
        };

        let parsed: Self =
            serde_json::from_value(raw.clone()).context("failed to parse features.voice_log")?;

        Ok(parsed.enabled.then_some(parsed))
    }
}

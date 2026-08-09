use anyhow::Context;
use serde::Deserialize;

use crate::core::config::Config;

#[derive(Debug, Clone, Deserialize)]
pub struct MemberLogConfig {
    #[serde(default)]
    pub enabled: bool,
    pub log_channel: u64,
    #[serde(default = "default_new_account_warn_days")]
    pub new_account_warn_days: i64,
}

fn default_new_account_warn_days() -> i64 {
    7
}

impl MemberLogConfig {
    /// `cfg.features`から`member_log`セクションを取り出す。
    /// セクションが無い、または`enabled = false`なら`None`。
    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        let Some(raw) = cfg.features.get("member_log") else {
            return Ok(None);
        };

        let parsed: Self =
            serde_json::from_value(raw.clone()).context("failed to parse features.member_log")?;

        Ok(parsed.enabled.then_some(parsed))
    }
}

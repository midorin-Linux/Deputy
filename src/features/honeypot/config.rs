use serde::Deserialize;
use serenity::all::ChannelId;

use crate::core::{
    config::{Config, FeatureToggle, load_feature_config},
    error::ConfigError,
};

pub const FEATURE_NAME: &str = "honeypot";

/// Discordの仕様上、BAN時にさかのぼって削除できるメッセージは最大7日分。
const MAX_DELETE_MESSAGE_DAYS: u8 = 7;

#[derive(Debug, Clone, Deserialize)]
pub struct HoneypotConfig {
    #[serde(default)]
    pub enabled: bool,

    /// BAN実行・AI判定失敗を通知する管理者向けチャンネル。
    #[serde(default)]
    pub log_channel: u64,

    /// 監視対象のハニーポットチャンネルID。1サーバー内の複数チャンネルを見張れる。
    #[serde(default)]
    pub honeypot_channels: Vec<u64>,

    /// 有効にすると、スパム判定がついても実際にはBANせず判定結果のみを通知する（検証用）。
    #[serde(default)]
    pub debug_mode: bool,

    /// 監視チャンネルへ投稿したアカウントを、`ban_trigger`もAI判定も一切問わず即BANする。
    /// 強力なため既定は無効。
    #[serde(default)]
    pub unconditional_ban: bool,

    /// AIによるスパム判定を行うか。falseなら`ban_trigger`の条件のみで判定する。
    #[serde(default = "default_enable_ai_judgment")]
    pub enable_ai_judgment: bool,

    /// BAN時にさかのぼって削除するメッセージの日数（0〜7）。
    #[serde(default = "default_delete_message_days")]
    pub delete_message_days: u8,

    /// このロールを持つメンバーは判定対象外。管理者・モデレータの誤BAN事故を防ぐ。
    #[serde(default)]
    pub exempt_roles: Vec<u64>,

    /// AI呼び出し前の高速フィルタ。
    #[serde(default)]
    pub ban_trigger: BanTriggerConfig,

    /// AI判定が失敗したときの方針。
    #[serde(default)]
    pub ai_failure_policy: AiFailurePolicy,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BanTriggerConfig {
    /// 招待リンク（discord.gg/* など）が含まれている場合にBANする。
    #[serde(default = "default_true")]
    pub has_invite_link: bool,

    /// 全体メンションまたはロールメンションが含まれている場合にBANする。
    #[serde(default = "default_true")]
    pub has_role_mention: bool,

    /// メンション数の閾値（0でオフ）。
    #[serde(default = "default_mention_threshold")]
    pub mention_threshold: u64,
}

/// AI判定が失敗したときの扱い。判定チェーンの最後がAIのため、
/// どちらを選んでも「そのメッセージはBANしない」点は変わらず、差は運用者への伝え方にある。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AiFailurePolicy {
    /// エラーログのみ残して見逃す（移植元と同じ挙動）。
    Skip,
    /// 見逃したうえで、管理チャンネルへ警告を通知する。
    #[default]
    Notify,
}

impl Default for BanTriggerConfig {
    fn default() -> Self {
        Self {
            has_invite_link: default_true(),
            has_role_mention: default_true(),
            mention_threshold: default_mention_threshold(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_mention_threshold() -> u64 {
    3
}

fn default_enable_ai_judgment() -> bool {
    true
}

fn default_delete_message_days() -> u8 {
    1
}

impl FeatureToggle for HoneypotConfig {
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn raw_log_channel(&self) -> Option<u64> {
        Some(self.log_channel)
    }
}

impl HoneypotConfig {
    /// `features.honeypot`セクションを共通ルール（`enabled` / `log_channel`）で検証したうえで、
    /// この機能固有の設定値も起動時に検証する。
    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        let Some(parsed) = load_feature_config::<Self>(cfg, FEATURE_NAME)? else {
            return Ok(None);
        };

        parsed.validate(cfg)?;

        Ok(Some(parsed))
    }

    fn validate(&self, cfg: &Config) -> Result<(), ConfigError> {
        // `log_channel`と同じく、`ChannelId::new(0)`の実行時panicを起動時エラーへ前倒しする。
        if self.honeypot_channels.is_empty() {
            return Err(self.invalid("honeypot_channels must not be empty"));
        }

        if self.honeypot_channels.contains(&0) {
            return Err(self.invalid("honeypot_channels must not contain 0"));
        }

        if self.delete_message_days > MAX_DELETE_MESSAGE_DAYS {
            return Err(self.invalid("delete_message_days must be between 0 and 7"));
        }

        // AI判定を使うなら、AIセクションが実際に使える値になっているかまでここで確かめる。
        // 起動できてしまうと、スパム投稿が来た瞬間に初めて失敗が露見することになる。
        if self.enable_ai_judgment {
            if cfg.ai.base_url.trim().is_empty() {
                return Err(self.invalid("ai.base_url must not be empty when enable_ai_judgment"));
            }

            if cfg.ai.model_id.trim().is_empty() {
                return Err(self.invalid("ai.model_id must not be empty when enable_ai_judgment"));
            }

            if cfg.ai.api_key.expose().trim().is_empty() {
                return Err(self.invalid("ai.api_key must not be empty when enable_ai_judgment"));
            }

            if cfg.ai.request_timeout_secs == 0 {
                return Err(self.invalid(
                    "ai.request_timeout_secs must be greater than 0 when enable_ai_judgment",
                ));
            }
        }

        Ok(())
    }

    fn invalid(&self, detail: &str) -> ConfigError {
        ConfigError::InvalidFeatureSetting {
            feature: FEATURE_NAME,
            detail: detail.to_string(),
        }
    }

    /// 管理者向け通知チャンネル。`load`で0でないことを検証済みのため、ここではpanicしない。
    pub fn log_channel(&self) -> ChannelId {
        ChannelId::new(self.log_channel)
    }

    /// 監視対象チャンネルかどうか。
    pub fn is_watched(&self, channel_id: ChannelId) -> bool {
        self.honeypot_channels.contains(&channel_id.get())
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

    fn valid_section() -> serde_json::Value {
        serde_json::json!({
            "enabled": true,
            "log_channel": 1,
            "honeypot_channels": [2, 3],
        })
    }

    #[test]
    fn missing_section_is_disabled() {
        let mut cfg = config_with(valid_section());
        cfg.features.remove(FEATURE_NAME);
        assert!(HoneypotConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn disabled_section_skips_all_validation() {
        // 無効なら監視チャンネルも通知先も持たないので、欠けていてもエラーにしない。
        let cfg = config_with(serde_json::json!({ "enabled": false }));
        assert!(HoneypotConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn defaults_follow_the_least_destructive_option() {
        let cfg = config_with(valid_section());
        let loaded = HoneypotConfig::load(&cfg).unwrap().unwrap();

        assert!(!loaded.unconditional_ban, "即BANは既定で無効であること");
        assert!(!loaded.debug_mode);
        assert!(loaded.enable_ai_judgment);
        assert_eq!(loaded.delete_message_days, 1);
        assert!(loaded.exempt_roles.is_empty());
        assert!(loaded.ban_trigger.has_invite_link);
        assert!(loaded.ban_trigger.has_role_mention);
        assert_eq!(loaded.ban_trigger.mention_threshold, 3);
        assert_eq!(loaded.ai_failure_policy, AiFailurePolicy::Notify);
    }

    #[test]
    fn enabled_section_without_log_channel_is_rejected() {
        let cfg = config_with(serde_json::json!({
            "enabled": true,
            "honeypot_channels": [2],
        }));
        let err = HoneypotConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("log_channel"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn empty_honeypot_channels_is_rejected() {
        let cfg = config_with(serde_json::json!({
            "enabled": true,
            "log_channel": 1,
            "honeypot_channels": [],
        }));
        let err = HoneypotConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("honeypot_channels"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_in_honeypot_channels_is_rejected() {
        // `ChannelId::new(0)`は実行時にpanicするため、起動時に弾く。
        let cfg = config_with(serde_json::json!({
            "enabled": true,
            "log_channel": 1,
            "honeypot_channels": [2, 0],
        }));
        let err = HoneypotConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("honeypot_channels"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn delete_message_days_above_discord_limit_is_rejected() {
        let mut section = valid_section();
        section["delete_message_days"] = serde_json::json!(8);
        let err = HoneypotConfig::load(&config_with(section)).unwrap_err();
        assert!(
            err.to_string().contains("delete_message_days"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn ai_settings_are_validated_only_when_ai_judgment_is_enabled() {
        let mut cfg = config_with(valid_section());
        cfg.ai.base_url = "  ".to_string();

        let err = HoneypotConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("ai.base_url"),
            "unexpected error: {err}"
        );

        // AI判定を使わないなら、AI設定が空でも起動できる。
        let mut section = valid_section();
        section["enable_ai_judgment"] = serde_json::json!(false);
        cfg.features.insert(FEATURE_NAME.to_string(), section);
        assert!(HoneypotConfig::load(&cfg).unwrap().is_some());
    }

    #[test]
    fn zero_request_timeout_is_rejected_when_ai_judgment_is_enabled() {
        let mut cfg = config_with(valid_section());
        cfg.ai.request_timeout_secs = 0;
        let err = HoneypotConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("request_timeout_secs"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn is_watched_matches_only_configured_channels() {
        let loaded = HoneypotConfig::load(&config_with(valid_section()))
            .unwrap()
            .unwrap();
        assert!(loaded.is_watched(ChannelId::new(2)));
        assert!(loaded.is_watched(ChannelId::new(3)));
        assert!(!loaded.is_watched(ChannelId::new(4)));
    }

    /// 配布している`settings.example.yml`のhoneypotセクションが、実際に読める形のまま
    /// 保たれていることを確かめる（キー名の綴り違いや値の書式ミスをここで検出する）。
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
            .expect("example settings must contain a honeypot section");

        // 例では無効にしてあるため、有効化したうえで検証まで通ることを確かめる。
        section["enabled"] = serde_json::json!(true);

        let loaded = HoneypotConfig::load(&cfg)
            .expect("example honeypot section must be valid")
            .expect("honeypot must be enabled");

        assert!(!loaded.honeypot_channels.is_empty());
        assert_eq!(loaded.ai_failure_policy, AiFailurePolicy::Notify);
    }

    #[test]
    fn unknown_ai_failure_policy_is_rejected() {
        let mut section = valid_section();
        section["ai_failure_policy"] = serde_json::json!("explode");
        let err = HoneypotConfig::load(&config_with(section)).unwrap_err();
        assert!(
            err.to_string().contains("honeypot"),
            "unexpected error: {err}"
        );
    }
}

use serde::Deserialize;

use crate::core::{
    config::{Config, FeatureToggle, load_feature_config},
    error::ConfigError,
};

pub const FEATURE_NAME: &str = "tts";

#[derive(Debug, Clone, Deserialize)]
pub struct TtsConfig {
    #[serde(default)]
    enabled: bool,

    /// VOICEVOX ENGINEのベースURL。
    #[serde(default = "default_voicevox_url")]
    voicevox_url: String,

    /// `/speaker`で個別設定していないユーザーに使う話者ID。
    #[serde(default = "default_speaker")]
    default_speaker: u32,

    /// 1メッセージあたりの読み上げ文字数の上限。超えた分は切り捨てる。
    #[serde(default = "default_max_chars")]
    max_chars: usize,

    /// VOICEVOX ENGINEへのリクエストタイムアウト（秒）。
    #[serde(default = "default_request_timeout_secs")]
    request_timeout_secs: u64,
}

fn default_voicevox_url() -> String {
    "http://127.0.0.1:50021".to_string()
}

fn default_speaker() -> u32 {
    3
}

fn default_max_chars() -> usize {
    120
}

fn default_request_timeout_secs() -> u64 {
    30
}

impl FeatureToggle for TtsConfig {
    fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// ttsは通知先チャンネルを持たない機能のため、検証自体をスキップさせる。
    fn raw_log_channel(&self) -> Option<u64> {
        None
    }
}

impl TtsConfig {
    /// `cfg.features`から`tts`セクションを取り出し、`enabled`の共通ルールで検証したうえで、
    /// この機能固有の設定値も起動時に検証する。
    pub fn load(cfg: &Config) -> anyhow::Result<Option<Self>> {
        let Some(parsed) = load_feature_config::<Self>(cfg, FEATURE_NAME)? else {
            return Ok(None);
        };

        parsed.validate()?;

        Ok(Some(parsed))
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.voicevox_url.trim().is_empty() {
            return Err(self.invalid("voicevox_url must not be empty"));
        }

        if self.max_chars == 0 {
            return Err(self.invalid("max_chars must be greater than 0"));
        }

        if self.request_timeout_secs == 0 {
            return Err(self.invalid("request_timeout_secs must be greater than 0"));
        }

        // ponytail: VOICEVOXの話者IDに使えるIDの範囲はENGINE/辞書構成によって変わり、
        // 設定ファイルだけからは妥当性を判定できない（型がu32のため負値・非数値はここへ来る前に
        // serdeのデシリアライズ段階で弾かれる）。存在しないIDは実行時に合成失敗として扱う
        // （README記載の仕様どおり）。ENGINEの`/speakers`を起動時に問い合わせて検証したくなったら
        // ここへ追加する。
        Ok(())
    }

    fn invalid(&self, detail: &str) -> ConfigError {
        ConfigError::InvalidFeatureSetting {
            feature: FEATURE_NAME,
            detail: detail.to_string(),
        }
    }

    pub fn voicevox_url(&self) -> &str {
        &self.voicevox_url
    }

    pub fn default_speaker(&self) -> u32 {
        self.default_speaker
    }

    pub fn max_chars(&self) -> usize {
        self.max_chars
    }

    pub fn request_timeout_secs(&self) -> u64 {
        self.request_timeout_secs
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
        assert!(TtsConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn disabled_section_is_accepted_without_any_other_key() {
        let cfg = config_with(serde_json::json!({ "enabled": false }));
        assert!(TtsConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn defaults_match_settings_example() {
        let cfg = config_with(serde_json::json!({ "enabled": true }));
        let loaded = TtsConfig::load(&cfg).unwrap().unwrap();

        assert_eq!(loaded.voicevox_url(), "http://127.0.0.1:50021");
        assert_eq!(loaded.default_speaker(), 3);
        assert_eq!(loaded.max_chars(), 120);
        assert_eq!(loaded.request_timeout_secs(), 30);
    }

    #[test]
    fn blank_voicevox_url_is_rejected() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "voicevox_url": "   " }));
        let err = TtsConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("voicevox_url"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_max_chars_is_rejected() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "max_chars": 0 }));
        let err = TtsConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("max_chars"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn zero_request_timeout_is_rejected() {
        let cfg = config_with(serde_json::json!({ "enabled": true, "request_timeout_secs": 0 }));
        let err = TtsConfig::load(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("request_timeout_secs"),
            "unexpected error: {err}"
        );
    }

    /// 配布している`settings.example.yml`のttsセクションが、実際に読める形のまま
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
            .expect("example settings must contain a tts section");

        // 例では無効にしてあるため、有効化したうえで検証まで通ることを確かめる。
        section["enabled"] = serde_json::json!(true);

        let loaded = TtsConfig::load(&cfg)
            .expect("example tts section must be valid")
            .expect("tts must be enabled");

        assert_eq!(loaded.voicevox_url(), "http://127.0.0.1:50021");
        assert_eq!(loaded.default_speaker(), 3);
        assert_eq!(loaded.max_chars(), 120);
        assert_eq!(loaded.request_timeout_secs(), 30);
    }
}

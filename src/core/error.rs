use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("discord.token must not be empty")]
    MissingDiscordToken,

    /// `ChannelId::new(0)`はpanicするため、IDは起動時に検証して弾く。
    /// 実行時（イベント処理中）のpanicを設定読み込み時のエラーへ前倒しする。
    /// （`log_channel`未指定はデフォルトの0として扱われ、ここに合流する。）
    #[error(
        "features.{feature}.log_channel must be set to a valid Discord channel ID (missing or 0)"
    )]
    InvalidLogChannel { feature: &'static str },

    /// `log_channel`以外の、機能固有の設定値が不正な場合。
    /// 共通ルール（`FeatureToggle`）に載らない検証は各`features/<name>/config.rs`が行い、
    /// この型でまとめて起動時エラーにする。
    #[error("features.{feature}: {detail}")]
    InvalidFeatureSetting {
        feature: &'static str,
        detail: String,
    },
}

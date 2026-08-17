use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("discord.token must not be empty")]
    MissingDiscordToken,

    /// `ChannelId::new(0)`はpanicするため、IDは起動時に検証して弾く。
    /// 実行時（イベント処理中）のpanicを設定読み込み時のエラーへ前倒しする。
    #[error("features.{feature}.log_channel must be a valid Discord channel ID (got 0)")]
    InvalidLogChannel { feature: &'static str },
}

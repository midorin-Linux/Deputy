use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("discord.token must not be empty")]
    MissingDiscordToken,
}

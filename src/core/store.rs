use std::sync::Arc;

use chrono::{DateTime, Utc};
use serenity::all::{GuildId, UserId};
use tracing::warn;

/// 機能が記録する1件のログ。永続化するかどうかは`LogStore`の実装に委ねる。
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub feature: &'static str,
    pub guild_id: GuildId,
    pub user_id: Option<UserId>,
    pub message: String,
    pub timestamp: DateTime<Utc>,
}

impl LogEntry {
    pub fn new(feature: &'static str, guild_id: GuildId, message: impl Into<String>) -> Self {
        Self {
            feature,
            guild_id,
            user_id: None,
            message: message.into(),
            timestamp: Utc::now(),
        }
    }

    pub fn with_user(mut self, user_id: UserId) -> Self {
        self.user_id = Some(user_id);
        self
    }
}

/// ログ永続化ポート。SQLite等の実装差し替えを可能にする。
#[async_trait::async_trait]
pub trait LogStore: Send + Sync {
    async fn record(&self, entry: LogEntry) -> anyhow::Result<()>;
}

/// 既定の実装：何もしない。永続化するかどうか未定のまま、機能側の実装を先に進められる。
pub struct NoopStore;

#[async_trait::async_trait]
impl LogStore for NoopStore {
    async fn record(&self, _entry: LogEntry) -> anyhow::Result<()> {
        Ok(())
    }
}

/// `store.record(entry)`を呼び、失敗しても機能本体には伝播させずログだけ残す。
/// 永続化はベストエフォートであり、通知（Discordへのメッセージ送信）の成否とは独立させるため。
pub async fn record_or_warn(store: &Arc<dyn LogStore>, entry: LogEntry) {
    let feature = entry.feature;
    if let Err(err) = store.record(entry).await {
        warn!(feature, error = %err, "failed to persist log entry");
    }
}

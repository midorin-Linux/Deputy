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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn entry() -> LogEntry {
        LogEntry::new("test_feature", GuildId::new(1), "message")
    }

    struct FailingStore;

    #[async_trait::async_trait]
    impl LogStore for FailingStore {
        async fn record(&self, _entry: LogEntry) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("boom"))
        }
    }

    struct CountingStore(AtomicUsize);

    #[async_trait::async_trait]
    impl LogStore for CountingStore {
        async fn record(&self, _entry: LogEntry) -> anyhow::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn store_failure_is_swallowed_not_propagated() {
        // 戻り値は()なので、パニックせず戻ってくること自体がテストの主張。
        let store: Arc<dyn LogStore> = Arc::new(FailingStore);
        record_or_warn(&store, entry()).await;
    }

    #[tokio::test]
    async fn entry_is_forwarded_to_store() {
        let store = Arc::new(CountingStore(AtomicUsize::new(0)));
        let dyn_store: Arc<dyn LogStore> = store.clone();
        record_or_warn(&dyn_store, entry()).await;
        assert_eq!(store.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn noop_store_accepts_entry() {
        let store: Arc<dyn LogStore> = Arc::new(NoopStore);
        record_or_warn(&store, entry()).await;
    }
}

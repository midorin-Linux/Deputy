//! チャンネル単位の会話履歴。プロセス内のみで完結する揮発性のもので、永続化はしない。

use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

use serenity::all::ChannelId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub role: Role,
    pub content: String,
}

/// チャンネルごとの会話履歴。`limit`件を超えたら古いものから捨てる。
pub struct History {
    entries: Mutex<HashMap<ChannelId, VecDeque<HistoryEntry>>>,
    limit: usize,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            limit,
        }
    }

    /// 履歴に1件追加する。`limit`を超えた分は古いものから捨てる。
    pub fn push(&self, channel_id: ChannelId, role: Role, content: String) {
        let mut entries = self.lock();
        let deque = entries.entry(channel_id).or_default();

        deque.push_back(HistoryEntry { role, content });
        while deque.len() > self.limit {
            deque.pop_front();
        }
    }

    /// 現時点の履歴のスナップショット（古い順）。
    pub fn snapshot(&self, channel_id: ChannelId) -> Vec<HistoryEntry> {
        self.lock()
            .get(&channel_id)
            .map(|deque| deque.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// チャンネルの履歴をクリアする。
    pub fn clear(&self, channel_id: ChannelId) {
        self.lock().remove(&channel_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ChannelId, VecDeque<HistoryEntry>>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: u64) -> ChannelId {
        ChannelId::new(id)
    }

    #[test]
    fn snapshot_is_empty_for_unknown_channel() {
        let history = History::new(10);
        assert!(history.snapshot(channel(1)).is_empty());
    }

    #[test]
    fn push_appends_in_order() {
        let history = History::new(10);
        history.push(channel(1), Role::User, "hello".to_string());
        history.push(channel(1), Role::Assistant, "hi".to_string());

        let snapshot = history.snapshot(channel(1));
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].role, Role::User);
        assert_eq!(snapshot[0].content, "hello");
        assert_eq!(snapshot[1].role, Role::Assistant);
        assert_eq!(snapshot[1].content, "hi");
    }

    #[test]
    fn channels_are_independent() {
        let history = History::new(10);
        history.push(channel(1), Role::User, "a".to_string());
        history.push(channel(2), Role::User, "b".to_string());

        assert_eq!(history.snapshot(channel(1)).len(), 1);
        assert_eq!(history.snapshot(channel(2)).len(), 1);
    }

    #[test]
    fn oldest_entries_are_dropped_once_limit_is_exceeded() {
        let history = History::new(2);
        history.push(channel(1), Role::User, "1".to_string());
        history.push(channel(1), Role::Assistant, "2".to_string());
        history.push(channel(1), Role::User, "3".to_string());

        let snapshot = history.snapshot(channel(1));
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].content, "2");
        assert_eq!(snapshot[1].content, "3");
    }

    #[test]
    fn clear_removes_only_the_target_channel() {
        let history = History::new(10);
        history.push(channel(1), Role::User, "a".to_string());
        history.push(channel(2), Role::User, "b".to_string());

        history.clear(channel(1));

        assert!(history.snapshot(channel(1)).is_empty());
        assert_eq!(history.snapshot(channel(2)).len(), 1);
    }
}

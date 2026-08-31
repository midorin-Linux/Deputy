//! ユーザー単位のクールダウン。honeypot/dedup.rsの`HandledUsers`と同じ
//! 「TTL付き有界Mutex<HashMap>」パターンを流用している。

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use serenity::all::UserId;

/// 保持する最大件数。超えたら最も古いものから捨てる。
const DEFAULT_CAPACITY: usize = 1024;

/// ユーザー単位のクールダウン管理。
pub struct Cooldown {
    entries: Mutex<HashMap<UserId, Instant>>,
    duration: Duration,
    capacity: usize,
}

impl Cooldown {
    pub fn new(duration: Duration) -> Self {
        Self::with_capacity(duration, DEFAULT_CAPACITY)
    }

    fn with_capacity(duration: Duration, capacity: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            duration,
            capacity,
        }
    }

    /// クールダウン中でなければユーザーを記録して`true`を返す（＝呼び出し元がLLMを呼んでよい）。
    /// クールダウン中なら記録を更新せず`false`を返す。
    pub fn try_acquire(&self, user_id: UserId) -> bool {
        self.try_acquire_at(user_id, Instant::now())
    }

    fn try_acquire_at(&self, user_id: UserId, now: Instant) -> bool {
        let mut entries = self.lock();

        entries.retain(|_, marked_at| now.saturating_duration_since(*marked_at) < self.duration);

        if entries.contains_key(&user_id) {
            return false;
        }

        if entries.len() >= self.capacity
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, marked_at)| **marked_at)
                .map(|(user_id, _)| *user_id)
        {
            entries.remove(&oldest);
        }

        entries.insert(user_id, now);

        true
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<UserId, Instant>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DURATION: Duration = Duration::from_secs(15);

    fn user(id: u64) -> UserId {
        UserId::new(id)
    }

    #[test]
    fn first_call_is_allowed_and_second_is_blocked() {
        let cooldown = Cooldown::with_capacity(DURATION, 8);
        let now = Instant::now();

        assert!(cooldown.try_acquire_at(user(1), now));
        assert!(!cooldown.try_acquire_at(user(1), now));
    }

    #[test]
    fn different_users_are_independent() {
        let cooldown = Cooldown::with_capacity(DURATION, 8);
        let now = Instant::now();

        assert!(cooldown.try_acquire_at(user(1), now));
        assert!(cooldown.try_acquire_at(user(2), now));
    }

    #[test]
    fn acquire_is_allowed_again_after_duration_elapses() {
        let cooldown = Cooldown::with_capacity(DURATION, 8);
        let now = Instant::now();
        cooldown.try_acquire_at(user(1), now);

        assert!(!cooldown.try_acquire_at(user(1), now + DURATION / 2));
        assert!(cooldown.try_acquire_at(user(1), now + DURATION));
    }

    #[test]
    fn capacity_evicts_the_oldest_entry() {
        let cooldown = Cooldown::with_capacity(DURATION, 2);
        let base = Instant::now();

        cooldown.try_acquire_at(user(1), base);
        cooldown.try_acquire_at(user(2), base + Duration::from_secs(1));
        cooldown.try_acquire_at(user(3), base + Duration::from_secs(2));

        let now = base + Duration::from_secs(2);
        assert!(
            cooldown.try_acquire_at(user(1), now),
            "最古が押し出されて再取得できること"
        );
    }
}

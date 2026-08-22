//! 同一ユーザーの連投に対する重複判定・重複BANの防止。
//!
//! 移植元はプロセス内の`HashSet<UserId>`で、稼働時間に比例して単調増加していた。
//! Deputyは常駐する統合ボットのため、TTLと件数上限を持たせて有界にしている。

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use serenity::all::UserId;

/// 登録を保持する時間。連投スパムの重複を潰すにはこの程度で足りる。
const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);
/// 保持する最大件数。超えたら最も古いものから捨てる。
const DEFAULT_CAPACITY: usize = 1024;

/// 処理済みユーザーの有界キャッシュ。
pub struct HandledUsers {
    entries: Mutex<HashMap<UserId, Instant>>,
    ttl: Duration,
    capacity: usize,
}

impl Default for HandledUsers {
    fn default() -> Self {
        Self::new(DEFAULT_TTL, DEFAULT_CAPACITY)
    }
}

impl HandledUsers {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
            capacity,
        }
    }

    /// 既に処理済みとして登録されているか（AI判定などの重い処理を省くための事前確認）。
    pub fn contains(&self, user_id: UserId) -> bool {
        self.contains_at(user_id, Instant::now())
    }

    /// ユーザーを処理済みとして登録する。まだ登録されていなければ`true`
    /// （＝この呼び出しがBANを担当する）、既に登録済みなら`false`。
    /// 判定と挿入をロック内でアトミックに行い、並行到達時の二重BANを防ぐ。
    pub fn mark(&self, user_id: UserId) -> bool {
        self.mark_at(user_id, Instant::now())
    }

    /// 登録を取り消す（誤BANのunban時に、同じユーザーを再び判定対象へ戻すため）。
    pub fn forget(&self, user_id: UserId) {
        self.lock().remove(&user_id);
    }

    fn contains_at(&self, user_id: UserId, now: Instant) -> bool {
        let entries = self.lock();
        entries
            .get(&user_id)
            .is_some_and(|marked_at| !Self::is_expired(*marked_at, now, self.ttl))
    }

    fn mark_at(&self, user_id: UserId, now: Instant) -> bool {
        let mut entries = self.lock();

        entries.retain(|_, marked_at| !Self::is_expired(*marked_at, now, self.ttl));

        if let Some(marked_at) = entries.get_mut(&user_id) {
            // 連投されている間は期限を延長し、沈黙してから初めて期限切れになるようにする。
            *marked_at = now;
            return false;
        }

        // 期限切れを掃除してもなお上限に達しているなら、最も古い登録を押し出す。
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

    fn is_expired(marked_at: Instant, now: Instant, ttl: Duration) -> bool {
        now.saturating_duration_since(marked_at) >= ttl
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<UserId, Instant>> {
        // ロック中にpanicする処理を挟んでいないため、毒された状態は起こらない想定。
        // 万一起きた場合も判定を続けられるよう、中身をそのまま取り出す。
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(60);

    fn user(id: u64) -> UserId {
        UserId::new(id)
    }

    #[test]
    fn first_mark_wins_and_second_loses() {
        let cache = HandledUsers::new(TTL, 8);
        let now = Instant::now();

        assert!(cache.mark_at(user(1), now));
        assert!(!cache.mark_at(user(1), now));
    }

    #[test]
    fn different_users_are_independent() {
        let cache = HandledUsers::new(TTL, 8);
        let now = Instant::now();

        assert!(cache.mark_at(user(1), now));
        assert!(cache.mark_at(user(2), now));
    }

    #[test]
    fn contains_reflects_marking() {
        let cache = HandledUsers::new(TTL, 8);
        let now = Instant::now();

        assert!(!cache.contains_at(user(1), now));
        cache.mark_at(user(1), now);
        assert!(cache.contains_at(user(1), now));
    }

    #[test]
    fn entry_expires_after_ttl() {
        let cache = HandledUsers::new(TTL, 8);
        let now = Instant::now();
        cache.mark_at(user(1), now);

        let later = now + TTL;
        assert!(!cache.contains_at(user(1), later));
        assert!(
            cache.mark_at(user(1), later),
            "期限切れ後は再びBAN担当になれること"
        );
    }

    #[test]
    fn repeated_posts_extend_the_entry() {
        let cache = HandledUsers::new(TTL, 8);
        let now = Instant::now();
        cache.mark_at(user(1), now);

        let half = now + TTL / 2;
        assert!(!cache.mark_at(user(1), half));
        // 連投で期限が延びるため、最初の登録から TTL 経過しただけでは切れない。
        assert!(cache.contains_at(user(1), now + TTL));
    }

    #[test]
    fn capacity_evicts_the_oldest_entry() {
        let cache = HandledUsers::new(TTL, 2);
        let base = Instant::now();

        cache.mark_at(user(1), base);
        cache.mark_at(user(2), base + Duration::from_secs(1));
        cache.mark_at(user(3), base + Duration::from_secs(2));

        let now = base + Duration::from_secs(2);
        assert!(!cache.contains_at(user(1), now), "最古が押し出されること");
        assert!(cache.contains_at(user(2), now));
        assert!(cache.contains_at(user(3), now));
    }

    #[test]
    fn expired_entries_are_swept_before_eviction() {
        let cache = HandledUsers::new(TTL, 2);
        let base = Instant::now();

        cache.mark_at(user(1), base);
        cache.mark_at(user(2), base);

        // 全て期限切れになったあとの登録では、上限による押し出しは起きない。
        let later = base + TTL;
        cache.mark_at(user(3), later);
        assert!(cache.contains_at(user(3), later));
        assert!(!cache.contains_at(user(1), later));
        assert!(!cache.contains_at(user(2), later));
    }

    #[test]
    fn forget_allows_the_user_to_be_judged_again() {
        let cache = HandledUsers::new(TTL, 8);
        let now = Instant::now();

        cache.mark_at(user(1), now);
        cache.forget(user(1));

        assert!(!cache.contains_at(user(1), now));
        assert!(cache.mark_at(user(1), now));
    }
}

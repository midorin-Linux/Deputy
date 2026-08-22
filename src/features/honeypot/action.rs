//! 処分の実行と、その記録・通知。

use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use rand::seq::IndexedRandom;
use serenity::all::UserId;

use crate::features::honeypot::verdict::VerdictSource;

/// BAN理由の最大文字数。serenityは監査ログの理由が512文字を超えると`ExceededLimit`を
/// 返すため、余裕を持たせた上限に切り詰めてから渡す。
pub const MAX_BAN_REASON_LEN: usize = 100;

/// `/honeypot list`で遡れる件数。誤BANに気付いてから取り消すまでの導線に足りればよい。
const RECENT_BANS_CAPACITY: usize = 20;

/// 実行した処分の記録。`/honeypot list`と`/honeypot unban`の材料になる。
#[derive(Debug, Clone)]
pub struct BanRecord {
    pub user_id: UserId,
    pub user_tag: String,
    pub reason: String,
    pub source: VerdictSource,
    pub at: DateTime<Utc>,
    /// `debug_mode`で実BANしなかった場合は`false`。
    pub executed: bool,
}

/// 直近の処分の有界リスト。新しいものが先頭。
pub struct RecentBans {
    entries: Mutex<VecDeque<BanRecord>>,
    capacity: usize,
}

impl Default for RecentBans {
    fn default() -> Self {
        Self::new(RECENT_BANS_CAPACITY)
    }
}

impl RecentBans {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            capacity,
        }
    }

    pub fn record(&self, record: BanRecord) {
        let mut entries = self.lock();

        entries.push_front(record);

        while entries.len() > self.capacity {
            entries.pop_back();
        }
    }

    pub fn list(&self) -> Vec<BanRecord> {
        self.lock().iter().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<BanRecord>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// AI判定失敗の通知間隔。AIプロバイダが落ちている間はメッセージごとに失敗するため、
/// そのまま通知すると管理チャンネルが埋まる。最初の1件だけ通知し、以降は間隔を空ける。
const FAILURE_NOTIFY_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// 同種の通知を出しすぎないための門番。
pub struct NotifyCooldown {
    last: Mutex<Option<Instant>>,
    interval: Duration,
}

impl Default for NotifyCooldown {
    fn default() -> Self {
        Self::new(FAILURE_NOTIFY_INTERVAL)
    }
}

impl NotifyCooldown {
    pub fn new(interval: Duration) -> Self {
        Self {
            last: Mutex::new(None),
            interval,
        }
    }

    /// 通知してよければ`true`を返し、同時に「今通知した」ものとして記録する。
    pub fn allow(&self) -> bool {
        self.allow_at(Instant::now())
    }

    fn allow_at(&self, now: Instant) -> bool {
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(last_at) = *last
            && now.saturating_duration_since(last_at) < self.interval
        {
            return false;
        }

        *last = Some(now);

        true
    }
}

/// 監査ログへ載せるBAN理由を組み立てる。判定経路が分かるよう接頭辞を付け、
/// 理由本文だけを`MAX_BAN_REASON_LEN`へ切り詰める。
pub fn ban_reason(source: VerdictSource, reason: &str) -> String {
    format!("honeypot({}): {}", source.as_str(), truncate_reason(reason))
}

/// 理由を`MAX_BAN_REASON_LEN`文字以内へ切り詰める（文字境界を尊重）。
/// 切り詰めた場合は末尾を省略記号にする。
fn truncate_reason(reason: &str) -> String {
    let reason = reason.trim();

    if reason.chars().count() <= MAX_BAN_REASON_LEN {
        return reason.to_string();
    }

    let truncated: String = reason.chars().take(MAX_BAN_REASON_LEN - 1).collect();

    format!("{truncated}…")
}

const SALVATION_REPLIES: [&str; 9] = [
    "# 撃ちーかたはじめー！",
    "# やることはシンプルだ！\n# 命令を受け アカウントを消す！",
    "# いいぞ～貴官も救済の一部だ！\n# BANされて来い！ 脱退を許可する！",
    "# 救済だ！",
    "# 貴様に美しさの何が分かる！",
    "# 必要なのだスパムアカウントのBANが！！",
    "# 想像せよ ギルドメンバー諸君！\n# BANで1000万人が救済される！",
    "# 汚しやがって",
    "# 目標はスパムアカウント {account_name}",
];

/// BAN直前に送る演出用のリプライ。
pub fn salvation_reply(account_name: &str) -> String {
    SALVATION_REPLIES
        .choose(&mut rand::rng())
        .expect("SALVATION_REPLIES must be non-empty")
        .replace("{account_name}", account_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: u64) -> BanRecord {
        BanRecord {
            user_id: UserId::new(id),
            user_tag: format!("user{id}"),
            reason: "reason".to_string(),
            source: VerdictSource::Rule,
            at: Utc::now(),
            executed: true,
        }
    }

    #[test]
    fn short_reason_is_kept_as_is() {
        assert_eq!(truncate_reason("nitro scam"), "nitro scam");
    }

    #[test]
    fn reason_is_trimmed_before_measuring() {
        assert_eq!(truncate_reason("  nitro scam \n"), "nitro scam");
    }

    #[test]
    fn long_reason_is_truncated_to_the_limit() {
        let truncated = truncate_reason(&"a".repeat(MAX_BAN_REASON_LEN * 2));
        assert_eq!(truncated.chars().count(), MAX_BAN_REASON_LEN);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn reason_exactly_at_the_limit_is_not_truncated() {
        let reason = "a".repeat(MAX_BAN_REASON_LEN);
        assert_eq!(truncate_reason(&reason), reason);
    }

    #[test]
    fn truncation_respects_multibyte_boundaries() {
        // バイト単位で切ると壊れる文字列でもpanicせず、文字数で切り詰められること。
        let truncated = truncate_reason(&"あ".repeat(MAX_BAN_REASON_LEN * 2));
        assert_eq!(truncated.chars().count(), MAX_BAN_REASON_LEN);
    }

    #[test]
    fn ban_reason_carries_the_verdict_source() {
        let reason = ban_reason(VerdictSource::Ai, "nitro scam");
        assert!(reason.starts_with("honeypot(ai): "));
        assert!(reason.ends_with("nitro scam"));
    }

    #[test]
    fn salvation_reply_substitutes_the_account_name() {
        // 埋め込み対象の文言が選ばれたときにプレースホルダが残らないこと。
        for _ in 0 .. 200 {
            assert!(!salvation_reply("spammer").contains("{account_name}"));
        }
    }

    #[test]
    fn recent_bans_are_listed_newest_first() {
        let bans = RecentBans::new(4);
        bans.record(record(1));
        bans.record(record(2));

        let listed = bans.list();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].user_id, UserId::new(2));
        assert_eq!(listed[1].user_id, UserId::new(1));
    }

    #[test]
    fn recent_bans_drop_the_oldest_beyond_capacity() {
        let bans = RecentBans::new(2);
        bans.record(record(1));
        bans.record(record(2));
        bans.record(record(3));

        let listed = bans.list();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].user_id, UserId::new(3));
        assert_eq!(listed[1].user_id, UserId::new(2));
    }

    #[test]
    fn recent_bans_start_empty() {
        assert!(RecentBans::default().list().is_empty());
    }

    #[test]
    fn first_notification_is_always_allowed() {
        let gate = NotifyCooldown::new(Duration::from_secs(60));
        assert!(gate.allow_at(Instant::now()));
    }

    #[test]
    fn notifications_within_the_interval_are_suppressed() {
        let interval = Duration::from_secs(60);
        let gate = NotifyCooldown::new(interval);
        let now = Instant::now();

        assert!(gate.allow_at(now));
        assert!(!gate.allow_at(now + interval / 2));
    }

    #[test]
    fn notification_is_allowed_again_after_the_interval() {
        let interval = Duration::from_secs(60);
        let gate = NotifyCooldown::new(interval);
        let now = Instant::now();

        assert!(gate.allow_at(now));
        assert!(gate.allow_at(now + interval));
    }

    #[test]
    fn suppressed_attempts_do_not_extend_the_interval() {
        // 抑制された呼び出しで期限が延びると、通知が永久に出なくなる。
        let interval = Duration::from_secs(60);
        let gate = NotifyCooldown::new(interval);
        let now = Instant::now();

        assert!(gate.allow_at(now));
        assert!(!gate.allow_at(now + interval / 2));
        assert!(gate.allow_at(now + interval));
    }
}

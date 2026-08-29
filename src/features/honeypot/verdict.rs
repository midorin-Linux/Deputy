//! 判定結果と、AI呼び出し前のルールチェーン。
//!
//! 判定順序は移植元と同じフォールチェーン：
//! `unconditional_ban` → 招待リンク → ロール/@everyoneメンション → メンション数 →（AI判定）。
//! ここではAIの手前までを純関数として扱い、AI判定は呼び出し側（`mod.rs`）が行う。

use crate::features::honeypot::{
    config::HoneypotConfig,
    rules::{has_invite_link, has_many_mention, has_role_mention},
};

/// 判定がどこで確定したか。管理チャンネルへの通知に載せる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictSource {
    /// `unconditional_ban`による無条件BAN。
    Unconditional,
    /// ルールベースの高速フィルタ。
    Rule,
    /// LLMによる判定。
    Ai,
}

impl VerdictSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unconditional => "unconditional",
            Self::Rule => "rule",
            Self::Ai => "ai",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub is_spam: bool,
    pub reason: String,
    pub source: VerdictSource,
}

impl Verdict {
    pub fn spam(source: VerdictSource, reason: impl Into<String>) -> Self {
        Self {
            is_spam: true,
            reason: reason.into(),
            source,
        }
    }

    pub fn clean(source: VerdictSource, reason: impl Into<String>) -> Self {
        Self {
            is_spam: false,
            reason: reason.into(),
            source,
        }
    }
}

/// ルール判定に必要な情報だけを抜き出したもの。`Message`への依存をここで断ち切る。
#[derive(Debug, Clone, Copy)]
pub struct MessageFacts<'a> {
    pub content: &'a str,
    pub mention_roles: usize,
    pub mention_everyone: bool,
    pub mentions: usize,
}

/// AI手前までのチェーンを評価する。
/// `Some`ならその時点で確定、`None`ならAI判定へ委ねる（AI判定が無効なら非スパム確定）。
pub fn rule_verdict(facts: &MessageFacts<'_>, cfg: &HoneypotConfig) -> Option<Verdict> {
    if cfg.unconditional_ban {
        return Some(Verdict::spam(
            VerdictSource::Unconditional,
            "unconditional ban enabled",
        ));
    }

    if cfg.ban_trigger.has_invite_link && has_invite_link(facts.content) {
        return Some(Verdict::spam(
            VerdictSource::Rule,
            "discord invite link detected",
        ));
    }

    if cfg.ban_trigger.has_role_mention
        && has_role_mention(facts.mention_roles, facts.mention_everyone)
    {
        return Some(Verdict::spam(
            VerdictSource::Rule,
            "role/everyone mention detected",
        ));
    }

    if has_many_mention(facts.mentions, cfg.ban_trigger.mention_threshold) {
        return Some(Verdict::spam(VerdictSource::Rule, "many mentions detected"));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::honeypot::config::HoneypotConfig;

    fn config(section: serde_json::Value) -> HoneypotConfig {
        let mut base = serde_json::json!({
            "enabled": true,
            "log_channel": 1,
            "honeypot_channels": [2],
        });
        for (key, value) in section.as_object().expect("section must be an object") {
            base[key] = value.clone();
        }
        serde_json::from_value(base).expect("config must deserialize")
    }

    fn facts(content: &str) -> MessageFacts<'_> {
        MessageFacts {
            content,
            mention_roles: 0,
            mention_everyone: false,
            mentions: 0,
        }
    }

    #[test]
    fn unconditional_ban_wins_over_everything() {
        let cfg = config(serde_json::json!({ "unconditional_ban": true }));
        let verdict = rule_verdict(&facts("こんにちは"), &cfg).expect("must be decided");
        assert!(verdict.is_spam);
        assert_eq!(verdict.source, VerdictSource::Unconditional);
    }

    #[test]
    fn invite_link_is_banned_when_enabled() {
        let cfg = config(serde_json::json!({}));
        let verdict = rule_verdict(&facts("https://discord.gg/x"), &cfg).expect("must be decided");
        assert!(verdict.is_spam);
        assert_eq!(verdict.source, VerdictSource::Rule);
        assert!(verdict.reason.contains("invite"));
    }

    #[test]
    fn invite_link_is_ignored_when_trigger_disabled() {
        let cfg = config(serde_json::json!({
            "ban_trigger": { "has_invite_link": false }
        }));
        assert!(rule_verdict(&facts("https://discord.gg/x"), &cfg).is_none());
    }

    #[test]
    fn role_mention_is_banned_when_enabled() {
        let cfg = config(serde_json::json!({}));
        let mut f = facts("");
        f.mention_everyone = true;
        let verdict = rule_verdict(&f, &cfg).expect("must be decided");
        assert!(verdict.is_spam);
        assert!(verdict.reason.contains("mention"));
    }

    #[test]
    fn role_mention_is_ignored_when_trigger_disabled() {
        let cfg = config(serde_json::json!({
            "ban_trigger": { "has_role_mention": false }
        }));
        let mut f = facts("");
        f.mention_roles = 3;
        assert!(rule_verdict(&f, &cfg).is_none());
    }

    #[test]
    fn mention_count_threshold_is_applied() {
        let cfg = config(serde_json::json!({
            "ban_trigger": { "mention_threshold": 3 }
        }));

        let mut below = facts("");
        below.mentions = 2;
        assert!(rule_verdict(&below, &cfg).is_none());

        let mut at = facts("");
        at.mentions = 3;
        assert!(rule_verdict(&at, &cfg).expect("must be decided").is_spam);
    }

    #[test]
    fn zero_threshold_disables_mention_count_rule() {
        let cfg = config(serde_json::json!({
            "ban_trigger": { "mention_threshold": 0 }
        }));
        let mut f = facts("");
        f.mentions = 100;
        assert!(rule_verdict(&f, &cfg).is_none());
    }

    #[test]
    fn benign_message_falls_through_to_ai() {
        let cfg = config(serde_json::json!({}));
        assert!(rule_verdict(&facts("おはようございます"), &cfg).is_none());
    }
}

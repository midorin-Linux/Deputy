//! AI呼び出し前の高速フィルタ。いずれもDiscordの型に依存しない純関数として書き、
//! `Message`を組み立てずに単体テストできるようにしている。

/// Discordの招待リンクとみなすドメイン。
const INVITE_DOMAINS: [&str; 3] = [
    "discord.gg/",
    "discord.com/invite/",
    "discordapp.com/invite/",
];

/// 本文に招待リンクが含まれるか。
pub fn has_invite_link(content: &str) -> bool {
    let lower = content.to_lowercase();
    INVITE_DOMAINS.iter().any(|domain| lower.contains(domain))
}

/// ロールメンションまたは`@everyone`/`@here`が含まれるか。
pub fn has_role_mention(mention_roles: usize, mention_everyone: bool) -> bool {
    mention_roles > 0 || mention_everyone
}

/// メンション数が閾値以上か。閾値0はこの判定自体を使わない意味なので、常に偽を返す。
pub fn has_many_mention(mentions: usize, threshold: u64) -> bool {
    threshold != 0 && mentions as u64 >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_invite_domain() {
        assert!(has_invite_link("見てね https://discord.gg/abcdef"));
        assert!(has_invite_link("https://discord.com/invite/abcdef"));
        assert!(has_invite_link("http://discordapp.com/invite/abcdef"));
    }

    #[test]
    fn invite_detection_ignores_case() {
        assert!(has_invite_link("HTTPS://DISCORD.GG/ABCDEF"));
    }

    #[test]
    fn plain_text_is_not_an_invite() {
        assert!(!has_invite_link("こんにちは"));
        assert!(!has_invite_link(""));
    }

    #[test]
    fn domain_without_invite_path_is_not_an_invite() {
        // 招待リンクではない通常のDiscordリンクを誤検知しないこと。
        assert!(!has_invite_link(
            "https://discord.com/channels/1/2/3 を見て"
        ));
    }

    #[test]
    fn role_mention_is_detected_from_either_signal() {
        assert!(has_role_mention(1, false));
        assert!(has_role_mention(0, true));
        assert!(has_role_mention(2, true));
        assert!(!has_role_mention(0, false));
    }

    #[test]
    fn many_mention_triggers_at_threshold() {
        assert!(!has_many_mention(2, 3));
        assert!(has_many_mention(3, 3));
        assert!(has_many_mention(4, 3));
    }

    #[test]
    fn zero_threshold_disables_mention_count_rule() {
        assert!(!has_many_mention(0, 0));
        assert!(!has_many_mention(100, 0));
    }
}

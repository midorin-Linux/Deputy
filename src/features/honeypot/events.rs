//! `Message`から判定に必要な情報だけを取り出す層。
//!
//! ここでDiscordの型への依存を断ち切ることで、判定チェーン（`verdict.rs`）と
//! ルール（`rules.rs`）を`Message`なしでテストできるようにしている。

use serenity::all::Message;

use crate::features::honeypot::verdict::MessageFacts;

/// ルール判定に使う値を取り出す。
pub fn message_facts(msg: &Message) -> MessageFacts<'_> {
    MessageFacts {
        content: &msg.content,
        mention_roles: msg.mention_roles.len(),
        mention_everyone: msg.mention_everyone,
        mentions: msg.mentions.len(),
    }
}

/// 投稿者が持つロールID。ギルド外の投稿など、メンバー情報が無い場合は空。
pub fn member_role_ids(msg: &Message) -> Vec<u64> {
    msg.member
        .as_ref()
        .map(|member| member.roles.iter().map(|role| role.get()).collect())
        .unwrap_or_default()
}

/// 除外ロールを1つでも持っていれば判定対象外。
/// 管理者・モデレータがハニーポットへ書き込んでも処分されないようにするための安全弁。
pub fn is_exempt(member_roles: &[u64], exempt_roles: &[u64]) -> bool {
    if exempt_roles.is_empty() {
        return false;
    }

    member_roles.iter().any(|role| exempt_roles.contains(role))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の最小限のメッセージ。`Message`は公開のコンストラクタを持たないため、
    /// Discordのゲートウェイが送るのと同じJSON表現から組み立てる。
    fn message(extra: serde_json::Value) -> Message {
        let mut value = serde_json::json!({
            "id": "1",
            "channel_id": "2",
            "author": {
                "id": "3",
                "username": "spammer",
                "discriminator": "0001",
                "avatar": null,
                "bot": false,
            },
            "content": "hello",
            "timestamp": "2024-01-01T00:00:00.000000+00:00",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0,
        });

        for (key, patch) in extra.as_object().expect("extra must be an object") {
            value[key] = patch.clone();
        }

        serde_json::from_value(value).expect("message must deserialize")
    }

    fn user(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "username": "u",
            "discriminator": "0001",
            "avatar": null,
            "bot": false,
        })
    }

    #[test]
    fn facts_are_extracted_from_message() {
        let msg = message(serde_json::json!({
            "content": "https://discord.gg/x",
            "mention_everyone": true,
            "mention_roles": ["10", "11"],
            "mentions": [user("4"), user("5"), user("6")],
        }));

        let facts = message_facts(&msg);

        assert_eq!(facts.content, "https://discord.gg/x");
        assert!(facts.mention_everyone);
        assert_eq!(facts.mention_roles, 2);
        assert_eq!(facts.mentions, 3);
    }

    #[test]
    fn plain_message_yields_zeroed_facts() {
        let msg = message(serde_json::json!({}));
        let facts = message_facts(&msg);

        assert_eq!(facts.content, "hello");
        assert!(!facts.mention_everyone);
        assert_eq!(facts.mention_roles, 0);
        assert_eq!(facts.mentions, 0);
    }

    #[test]
    fn roles_are_read_from_the_partial_member() {
        let msg = message(serde_json::json!({
            "member": { "roles": ["10", "11"], "deaf": false, "mute": false },
        }));

        assert_eq!(member_role_ids(&msg), vec![10, 11]);
    }

    #[test]
    fn message_without_member_has_no_roles() {
        assert!(member_role_ids(&message(serde_json::json!({}))).is_empty());
    }

    #[test]
    fn no_exempt_roles_configured_exempts_nobody() {
        assert!(!is_exempt(&[1, 2, 3], &[]));
    }

    #[test]
    fn member_without_exempt_role_is_not_exempt() {
        assert!(!is_exempt(&[1, 2], &[9]));
    }

    #[test]
    fn single_matching_role_is_enough() {
        assert!(is_exempt(&[1, 9], &[9]));
    }

    #[test]
    fn member_without_any_role_is_not_exempt() {
        assert!(!is_exempt(&[], &[9]));
    }
}

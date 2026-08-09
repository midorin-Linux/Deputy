use serenity::all::ChannelId;

use super::diff::VoiceDiff;

/// `VoiceDiff`から通知タイトルと本文を組み立てる。`Unchanged`は通知不要なので`None`。
pub fn describe(user_tag: &str, diff: VoiceDiff) -> Option<(&'static str, String)> {
    match diff {
        VoiceDiff::Joined(to) => Some((
            "VC参加",
            format!("{user_tag} が {} に参加しました。", mention(to)),
        )),
        VoiceDiff::Left(from) => Some((
            "VC退出",
            format!("{user_tag} が {} から退出しました。", mention(from)),
        )),
        VoiceDiff::Moved { from, to } => Some((
            "VC移動",
            format!(
                "{user_tag} が {} から {} に移動しました。",
                mention(from),
                mention(to)
            ),
        )),
        VoiceDiff::Unchanged => None,
    }
}

fn mention(channel: ChannelId) -> String {
    format!("<#{channel}>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_produces_no_notification() {
        assert!(describe("user#0001", VoiceDiff::Unchanged).is_none());
    }

    #[test]
    fn joined_mentions_destination_channel() {
        let (title, description) =
            describe("user#0001", VoiceDiff::Joined(ChannelId::new(42))).unwrap();
        assert_eq!(title, "VC参加");
        assert!(description.contains("<#42>"));
    }

    #[test]
    fn left_mentions_source_channel() {
        let (title, description) =
            describe("user#0001", VoiceDiff::Left(ChannelId::new(42))).unwrap();
        assert_eq!(title, "VC退出");
        assert!(description.contains("<#42>"));
    }

    #[test]
    fn moved_mentions_both_channels() {
        let (title, description) = describe(
            "user#0001",
            VoiceDiff::Moved {
                from: ChannelId::new(1),
                to: ChannelId::new(2),
            },
        )
        .unwrap();
        assert_eq!(title, "VC移動");
        assert!(description.contains("<#1>"));
        assert!(description.contains("<#2>"));
    }
}

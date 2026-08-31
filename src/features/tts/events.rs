//! `Message`から判定に必要な情報だけを取り出す、Discordの実処理を伴わない純粋なロジック層。

use serenity::all::{ChannelId, GuildId};

/// 投稿を読み上げ対象にするかどうか。
/// - bot自身の発言は対象外
/// - ギルド外（DM等）の投稿は対象外
/// - `/join`で紐付けたチャンネル以外への投稿は対象外
pub fn is_speakable(
    author_is_bot: bool,
    guild_id: Option<GuildId>,
    channel_id: ChannelId,
    bound_channel: Option<ChannelId>,
) -> bool {
    if author_is_bot || guild_id.is_none() {
        return false;
    }

    bound_channel == Some(channel_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild(id: u64) -> GuildId {
        GuildId::new(id)
    }

    fn channel(id: u64) -> ChannelId {
        ChannelId::new(id)
    }

    #[test]
    fn bot_authored_message_is_not_speakable() {
        assert!(!is_speakable(
            true,
            Some(guild(1)),
            channel(1),
            Some(channel(1))
        ));
    }

    #[test]
    fn message_without_guild_is_not_speakable() {
        assert!(!is_speakable(false, None, channel(1), Some(channel(1))));
    }

    #[test]
    fn message_outside_bound_channel_is_not_speakable() {
        assert!(!is_speakable(
            false,
            Some(guild(1)),
            channel(2),
            Some(channel(1))
        ));
    }

    #[test]
    fn unbound_guild_is_not_speakable() {
        assert!(!is_speakable(false, Some(guild(1)), channel(1), None));
    }

    #[test]
    fn message_in_bound_channel_is_speakable() {
        assert!(is_speakable(
            false,
            Some(guild(1)),
            channel(1),
            Some(channel(1))
        ));
    }
}

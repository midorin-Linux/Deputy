use serenity::all::ChannelId;

/// 旧/新のVoiceStateを比較した結果、起きた遷移の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceDiff {
    Joined(ChannelId),
    Left(ChannelId),
    Moved {
        from: ChannelId,
        to: ChannelId,
    },
    /// チャンネルの変化なし（ミュート切替などVC移動を伴わない更新）。
    Unchanged,
}

/// 旧チャンネルIDと新チャンネルIDから遷移の種類を判定する純関数。
pub fn diff_channel(old: Option<ChannelId>, new: Option<ChannelId>) -> VoiceDiff {
    match (old, new) {
        (None, Some(to)) => VoiceDiff::Joined(to),
        (Some(from), None) => VoiceDiff::Left(from),
        (Some(from), Some(to)) if from != to => VoiceDiff::Moved { from, to },
        _ => VoiceDiff::Unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_channel_to_channel_is_joined() {
        let to = ChannelId::new(1);
        assert_eq!(diff_channel(None, Some(to)), VoiceDiff::Joined(to));
    }

    #[test]
    fn channel_to_no_channel_is_left() {
        let from = ChannelId::new(1);
        assert_eq!(diff_channel(Some(from), None), VoiceDiff::Left(from));
    }

    #[test]
    fn different_channels_is_moved() {
        let from = ChannelId::new(1);
        let to = ChannelId::new(2);
        assert_eq!(
            diff_channel(Some(from), Some(to)),
            VoiceDiff::Moved { from, to }
        );
    }

    #[test]
    fn same_channel_is_unchanged() {
        let channel = ChannelId::new(1);
        assert_eq!(
            diff_channel(Some(channel), Some(channel)),
            VoiceDiff::Unchanged
        );
    }

    #[test]
    fn no_channel_both_sides_is_unchanged() {
        assert_eq!(diff_channel(None, None), VoiceDiff::Unchanged);
    }
}

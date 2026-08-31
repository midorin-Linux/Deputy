// ponytail: 合成が並走すると稀に読み上げ順が入れ替わる。問題になったらGuild単位のmpscワーカーへ。

mod commands;
pub mod config;
mod events;
mod sanitize;
mod speakers;

use std::{collections::HashMap, path::PathBuf, sync::Mutex};

use serenity::all::{ChannelId, CommandInteraction, Context, CreateCommand, FullEvent, GuildId};
use tracing::warn;

use crate::{
    core::feature::{Feature, Flow},
    features::tts::{config::TtsConfig, speakers::SpeakerStore},
    services::voicevox::VoicevoxClient,
};

/// 話者設定の永続化先。`settings.yml`・`logs/`と同じくプロセスのカレントディレクトリ基準。
const SPEAKER_STORE_PATH: &str = "data/tts_speakers.json";

/// VOICEVOXによる読み上げ機能。`/join`したテキストチャンネルへの投稿を音声合成し、
/// 対応するボイスチャンネルで読み上げる。
pub struct Tts {
    cfg: TtsConfig,
    voicevox: VoicevoxClient,
    /// ギルドごとの読み上げ対象テキストチャンネル（`/join`で登録、永続化はしない）。
    bound: Mutex<HashMap<GuildId, ChannelId>>,
    speakers: SpeakerStore,
}

impl Tts {
    pub fn new(cfg: TtsConfig) -> anyhow::Result<Self> {
        let voicevox =
            VoicevoxClient::new(cfg.voicevox_url().to_string(), cfg.request_timeout_secs())?;
        let speakers = SpeakerStore::load(PathBuf::from(SPEAKER_STORE_PATH));

        Ok(Self {
            cfg,
            voicevox,
            bound: Mutex::new(HashMap::new()),
            speakers,
        })
    }

    fn bound_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        self.bound
            .lock()
            .expect("tts bound mutex poisoned")
            .get(&guild_id)
            .copied()
    }

    /// テキストチャンネルへの投稿を読み上げる。合成失敗・VC未接続は`warn!`ログのみに留め、
    /// 読み上げの失敗で機能全体を止めない。
    async fn handle_message(&self, ctx: &Context, msg: &serenity::all::Message) {
        let Some(guild_id) = msg.guild_id else {
            return;
        };

        let bound = self.bound_channel(guild_id);

        if !events::is_speakable(msg.author.bot, Some(guild_id), msg.channel_id, bound) {
            return;
        }

        let Some(text) = sanitize::sanitize(&msg.content, self.cfg.max_chars()) else {
            return;
        };

        // VCに接続していなければ合成しても捨てるだけなので、VOICEVOX呼び出しより先に確かめる。
        let Some(manager) = songbird::get(ctx).await else {
            warn!("songbirdマネージャーを取得できませんでした");
            return;
        };

        let Some(call) = manager.get(guild_id) else {
            warn!(guild_id = %guild_id, "VCに接続していないため読み上げをスキップしました");
            return;
        };

        let speaker = self.speakers.get(msg.author.id, self.cfg.default_speaker());

        let wav = match self.voicevox.synthesize(&text, speaker).await {
            Ok(wav) => wav,
            Err(err) => {
                warn!(error = ?err, guild_id = %guild_id, "VOICEVOXでの音声合成に失敗しました");
                return;
            }
        };

        let input: songbird::input::Input = wav.into();
        call.lock().await.enqueue_input(input).await;
    }

    /// ボットが接続しているVCから人間が誰もいなくなったら自動退出し、`bound`の紐付けも解除する。
    async fn handle_voice_state_update(&self, ctx: &Context, new: &serenity::all::VoiceState) {
        let Some(guild_id) = new.guild_id else {
            return;
        };

        let Some(manager) = songbird::get(ctx).await else {
            return;
        };

        let Some(call) = manager.get(guild_id) else {
            return;
        };

        let Some(bot_channel) = call.lock().await.current_channel() else {
            return;
        };
        let bot_channel_raw = bot_channel.0.get();

        // キャッシュのガードは`!Send`のため、awaitをまたがないよう関数呼び出しの中だけで使い切る。
        // キャッシュからギルドを引けなかった場合（None）は判定不能なので残留する。
        if human_remains(ctx, guild_id, bot_channel_raw).unwrap_or(true) {
            return;
        }

        if let Err(err) = manager.leave(guild_id).await {
            warn!(error = ?err, guild_id = %guild_id, "VCからの自動退出に失敗しました");
        }

        self.bound
            .lock()
            .expect("tts bound mutex poisoned")
            .remove(&guild_id);
    }
}

/// ボットのいるVCに、ボット自身を除く人間が残っているかどうか。
/// キャッシュのガードを持ち越さないよう、awaitを含まない同期関数として切り出している。
fn human_remains(ctx: &Context, guild_id: GuildId, bot_channel_raw: u64) -> Option<bool> {
    let guild = ctx.cache.guild(guild_id)?;
    let my_id = ctx.cache.current_user().id;

    Some(guild.voice_states.values().any(|vs| {
        vs.user_id != my_id
            && vs.channel_id.map(|c| c.get()) == Some(bot_channel_raw)
            // メンバーキャッシュ未取得の在室者は人間側へ倒す。誤って人間のいるVCから
            // 退出するより、bot同士で残留する方が安全（/leaveで手動退出できる）。
            && !guild
                .members
                .get(&vs.user_id)
                .map(|member| member.user.bot)
                .unwrap_or(false)
    }))
}

#[async_trait::async_trait]
impl Feature for Tts {
    fn name(&self) -> &'static str {
        config::FEATURE_NAME
    }

    fn priority(&self) -> i32 {
        50
    }

    fn commands(&self) -> Vec<CreateCommand> {
        commands::command_definitions()
    }

    async fn on_event(&self, ctx: &Context, ev: &FullEvent) -> anyhow::Result<Flow> {
        match ev {
            FullEvent::Message { new_message } => self.handle_message(ctx, new_message).await,
            FullEvent::VoiceStateUpdate { new, .. } => {
                self.handle_voice_state_update(ctx, new).await
            }
            _ => {}
        }

        Ok(Flow::Continue)
    }

    async fn on_command(&self, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
        commands::on_command(self, ctx, ic).await
    }
}

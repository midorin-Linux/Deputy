pub mod config;
mod diff;
mod events;

use std::sync::Arc;

use anyhow::Context as _;
use config::VoiceLogConfig;
use diff::diff_channel;
use serenity::all::{ChannelId, CommandInteraction, Context, CreateMessage, FullEvent, VoiceState};

use crate::core::{
    discord::embed::log_embed,
    feature::{Feature, Flow},
    store::{LogEntry, LogStore, record_or_warn},
};

/// VC入退出ログ機能。チャンネルの変化のみを通知し、ミュート切替などは無視する。
pub struct VoiceLog {
    cfg: VoiceLogConfig,
    store: Arc<dyn LogStore>,
}

impl VoiceLog {
    pub fn new(cfg: VoiceLogConfig, store: Arc<dyn LogStore>) -> Self {
        Self { cfg, store }
    }

    fn log_channel(&self) -> ChannelId {
        self.cfg.log_channel()
    }

    async fn handle_update(
        &self,
        ctx: &Context,
        old: &Option<VoiceState>,
        new: &VoiceState,
    ) -> anyhow::Result<()> {
        let diff = diff_channel(
            old.as_ref().and_then(|state| state.channel_id),
            new.channel_id,
        );

        let Some((title, description)) = events::describe(&user_tag(new), diff) else {
            return Ok(());
        };

        self.log_channel()
            .send_message(
                &ctx.http,
                CreateMessage::new().embed(log_embed(title, description)),
            )
            .await
            .with_context(|| {
                format!(
                    "failed to send voice log message: guild={:?} channel={}",
                    new.guild_id,
                    self.log_channel()
                )
            })?;

        if let Some(guild_id) = new.guild_id {
            let entry = LogEntry::new("voice_log", guild_id, title).with_user(new.user_id);
            record_or_warn(&self.store, entry).await;
        }

        Ok(())
    }
}

fn user_tag(state: &VoiceState) -> String {
    state
        .member
        .as_ref()
        .map(|member| member.user.tag())
        .unwrap_or_else(|| format!("<@{}>", state.user_id))
}

#[async_trait::async_trait]
impl Feature for VoiceLog {
    fn name(&self) -> &'static str {
        "voice_log"
    }

    async fn on_event(&self, ctx: &Context, ev: &FullEvent) -> anyhow::Result<Flow> {
        if let FullEvent::VoiceStateUpdate { old, new } = ev {
            self.handle_update(ctx, old, new).await?;
        }

        Ok(Flow::Continue)
    }

    async fn on_command(&self, _ctx: &Context, _ic: &CommandInteraction) -> anyhow::Result<()> {
        Ok(())
    }
}

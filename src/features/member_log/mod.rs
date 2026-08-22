pub mod config;
mod events;

use std::sync::Arc;

use anyhow::Context as _;
use config::MemberLogConfig;
use serenity::all::{
    ChannelId, CommandInteraction, Context, CreateMessage, FullEvent, Member, Timestamp, User,
};
use tracing::{debug, info};

use crate::core::{
    discord::embed::{log_embed, warn_embed},
    feature::{Feature, Flow},
    store::{LogEntry, LogStore, record_or_warn},
};

/// ギルド入退出ログ機能。`core/discord/embed.rs`の共通ビルダを使い、
/// 新規垢（`new_account_warn_days`未満）の入室を警告色で通知する。
pub struct MemberLog {
    cfg: MemberLogConfig,
    store: Arc<dyn LogStore>,
}

impl MemberLog {
    pub fn new(cfg: MemberLogConfig, store: Arc<dyn LogStore>) -> Self {
        Self { cfg, store }
    }

    fn log_channel(&self) -> ChannelId {
        self.cfg.log_channel()
    }

    async fn handle_addition(&self, ctx: &Context, member: &Member) -> anyhow::Result<()> {
        let is_new = events::is_new_account(
            member.user.id.created_at().unix_timestamp(),
            Timestamp::now().unix_timestamp(),
            self.cfg.new_account_warn_days,
        );

        debug!(
            guild_id = %member.guild_id,
            user_id = %member.user.id,
            is_new_account = is_new,
            "member joined"
        );

        let description = format!(
            "{} ({}) がサーバーに参加しました。\nアカウント作成日: {}",
            member.user.tag(),
            member.user.id,
            member.user.id.created_at(),
        );

        let embed = if is_new {
            warn_embed("新規アカウントの参加", description)
        } else {
            log_embed("メンバー参加", description)
        };

        self.log_channel()
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await
            .with_context(|| {
                format!(
                    "failed to send member join log: guild={} channel={}",
                    member.guild_id,
                    self.log_channel()
                )
            })?;

        let entry =
            LogEntry::new("member_log", member.guild_id, "member joined").with_user(member.user.id);
        record_or_warn(&self.store, entry).await;

        info!(guild_id = %member.guild_id, user_id = %member.user.id, "member join logged");

        Ok(())
    }

    async fn handle_removal(
        &self,
        ctx: &Context,
        guild_id: serenity::all::GuildId,
        user: &User,
    ) -> anyhow::Result<()> {
        let description = format!("{} ({}) がサーバーから退出しました。", user.tag(), user.id);

        self.log_channel()
            .send_message(
                &ctx.http,
                CreateMessage::new().embed(log_embed("メンバー退出", description)),
            )
            .await
            .with_context(|| {
                format!(
                    "failed to send member leave log: guild={} channel={}",
                    guild_id,
                    self.log_channel()
                )
            })?;

        let entry = LogEntry::new("member_log", guild_id, "member left").with_user(user.id);
        record_or_warn(&self.store, entry).await;

        info!(guild_id = %guild_id, user_id = %user.id, "member leave logged");

        Ok(())
    }
}

#[async_trait::async_trait]
impl Feature for MemberLog {
    fn name(&self) -> &'static str {
        "member_log"
    }

    async fn on_event(&self, ctx: &Context, ev: &FullEvent) -> anyhow::Result<Flow> {
        match ev {
            FullEvent::GuildMemberAddition { new_member } => {
                self.handle_addition(ctx, new_member).await?;
            }
            FullEvent::GuildMemberRemoval { guild_id, user, .. } => {
                self.handle_removal(ctx, *guild_id, user).await?;
            }
            _ => {}
        }

        Ok(Flow::Continue)
    }

    async fn on_command(&self, _ctx: &Context, _ic: &CommandInteraction) -> anyhow::Result<()> {
        Ok(())
    }
}

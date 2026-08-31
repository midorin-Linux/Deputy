mod action;
mod agent;
pub mod config;
mod dedup;
mod events;
mod image;
mod rules;
mod verdict;

use std::sync::Arc;

use anyhow::Context as _;
use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateEmbed, CreateMessage, FullEvent, GuildId, Message, Permissions, ResolvedOption,
    ResolvedValue, UserId,
};
use tracing::{debug, error, info, warn};

use crate::{
    core::{
        config::AiConfig,
        discord::{
            embed::{log_embed, warn_embed},
            interaction::ephemeral_respond,
        },
        feature::{Feature, Flow},
        store::{LogEntry, LogStore, record_or_warn},
    },
    features::honeypot::{
        action::{BanRecord, NotifyCooldown, RecentBans, ban_reason, salvation_reply},
        agent::Agent,
        config::{AiFailurePolicy, FEATURE_NAME, HoneypotConfig},
        dedup::HandledUsers,
        events::{is_exempt, member_role_ids, message_facts},
        image::download_image_attachments,
        verdict::{Verdict, VerdictSource, rule_verdict},
    },
};

/// スラッシュコマンド名。誤BAN時の確認・取り消し導線。
const COMMAND_NAME: &str = "honeypot";

/// ハニーポット機能。監視チャンネルへの投稿をルールとLLMで判定し、スパムをBANする。
///
/// 処分を実行したメッセージは後続の機能へ流さない（`Flow::Consume`）。
pub struct Honeypot {
    cfg: HoneypotConfig,
    /// `enable_ai_judgment`が無効なら`None`。
    agent: Option<Agent>,
    store: Arc<dyn LogStore>,
    handled: HandledUsers,
    recent_bans: RecentBans,
    /// AI判定失敗の通知が連発しないようにするための門番。
    ai_failure_notify: NotifyCooldown,
}

impl Honeypot {
    pub fn new(
        cfg: HoneypotConfig,
        ai: &AiConfig,
        store: Arc<dyn LogStore>,
    ) -> anyhow::Result<Self> {
        // AI判定を使わない構成では、APIキーもPROMPT.mdも無いまま起動できるようにする。
        let agent = if cfg.enable_ai_judgment {
            Some(Agent::new(ai).context("failed to build honeypot ai agent")?)
        } else {
            None
        };

        Ok(Self {
            cfg,
            agent,
            store,
            handled: HandledUsers::default(),
            recent_bans: RecentBans::default(),
            ai_failure_notify: NotifyCooldown::default(),
        })
    }

    async fn handle_message(&self, ctx: &Context, msg: &Message) -> anyhow::Result<Flow> {
        if msg.author.bot || !self.cfg.is_watched(msg.channel_id) {
            return Ok(Flow::Continue);
        }

        let Some(guild_id) = msg.guild_id else {
            warn!(channel_id = %msg.channel_id, "honeypot message had no guild_id; cannot ban");
            return Ok(Flow::Continue);
        };

        if is_exempt(&member_role_ids(msg), &self.cfg.exempt_roles) {
            debug!(user_id = %msg.author.id, "honeypot skipped exempt member");
            return Ok(Flow::Continue);
        }

        // 既に処理済みのユーザーは、連投されても再判定しない（AI呼び出しを無駄に増やさない）。
        // debug_mode中は検証のため、処理済みユーザーでも毎回判定し直す。
        if !self.cfg.debug_mode && self.handled.contains(msg.author.id) {
            return Ok(Flow::Continue);
        }

        let Some(verdict) = self.judge(ctx, msg).await? else {
            return Ok(Flow::Continue);
        };

        if !verdict.is_spam {
            debug!(user_id = %msg.author.id, reason = %verdict.reason, "honeypot judged message as clean");

            // debug_mode中は判定の裏取りができるよう、シロの判定も管理チャンネルへ残す。
            if self.cfg.debug_mode {
                self.notify(
                    ctx,
                    log_embed(
                        "スパムではないと判定しました（debug_mode）",
                        format!(
                            "対象: {} ({})\n判定経路: {}\n理由: {}\nチャンネル: <#{}>",
                            msg.author.tag(),
                            msg.author.id,
                            verdict.source.as_str(),
                            verdict.reason,
                            msg.channel_id,
                        ),
                    ),
                )
                .await;
            }

            return Ok(Flow::Continue);
        }

        self.punish(ctx, msg, guild_id, verdict).await
    }

    /// 判定チェーンを評価する。`None`は「判定できなかったので何もしない」を意味する。
    async fn judge(&self, ctx: &Context, msg: &Message) -> anyhow::Result<Option<Verdict>> {
        if let Some(verdict) = rule_verdict(&message_facts(msg), &self.cfg) {
            return Ok(Some(verdict));
        }

        let Some(agent) = &self.agent else {
            return Ok(Some(Verdict::clean(VerdictSource::Rule, "no rule matched")));
        };

        let images = if agent.support_image() {
            download_image_attachments(msg).await
        } else {
            Vec::new()
        };

        match agent.judge_spam(&msg.content, &images).await {
            Ok(spam_verdict) => Ok(Some(if spam_verdict.is_spam {
                Verdict::spam(VerdictSource::Ai, spam_verdict.reason)
            } else {
                Verdict::clean(VerdictSource::Ai, spam_verdict.reason)
            })),
            Err(err) => {
                // 判定チェーンの最後がAIのため、失敗＝そのメッセージは処分されない（見逃し）。
                // 気付かないまま素通りし続けるのを防ぐため、既定では管理チャンネルへ通知する。
                error!(error = ?err, user_id = %msg.author.id, "failed to judge message with ai; skipping");

                // プロバイダ障害中はメッセージごとに失敗するため、通知は間隔を空けて出す。
                if self.cfg.ai_failure_policy == AiFailurePolicy::Notify
                    && self.ai_failure_notify.allow()
                {
                    self.notify(
                        ctx,
                        warn_embed(
                            "AI判定に失敗しました",
                            format!(
                                "{} ({}) の投稿を判定できず、処分せず見逃しました。\nチャンネル: <#{}>\nエラー: {err}",
                                msg.author.tag(),
                                msg.author.id,
                                msg.channel_id,
                            ),
                        ),
                    )
                    .await;
                }

                Ok(None)
            }
        }
    }

    /// スパム判定が確定したメッセージを処分する。
    async fn punish(
        &self,
        ctx: &Context,
        msg: &Message,
        guild_id: GuildId,
        verdict: Verdict,
    ) -> anyhow::Result<Flow> {
        let reason = ban_reason(verdict.source, &verdict.reason);

        // BAN実行前にアトミックに登録する。並行して届いた同一ユーザーのメッセージが
        // 同時にここへ到達しても、実際に処分するのは最初の1件だけになる。
        if !self.cfg.debug_mode && !self.handled.mark(msg.author.id) {
            return Ok(Flow::Consume);
        }

        if self.cfg.debug_mode {
            info!(
                user = %msg.author.name,
                user_id = %msg.author.id,
                reason = %reason,
                "debug_mode enabled; skipping actual ban"
            );

            if let Err(err) = msg.reply(&ctx.http, "You have been banned.").await {
                warn!(error = %err, "failed to send debug_mode ban notice");
            }

            self.record_ban(
                msg.author.id,
                msg.author.tag(),
                &reason,
                verdict.source,
                false,
            );
            self.notify_ban(ctx, msg, &reason, verdict.source, false)
                .await;

            return Ok(Flow::Consume);
        }

        info!(
            user = %msg.author.name,
            user_id = %msg.author.id,
            reason = %reason,
            "banning spammer detected in honeypot channel"
        );

        if let Err(err) = msg
            .reply(&ctx.http, salvation_reply(&msg.author.name))
            .await
        {
            warn!(error = %err, "failed to send salvation reply before ban");
        }

        if let Err(err) = guild_id
            .ban_with_reason(
                &ctx.http,
                msg.author.id,
                self.cfg.delete_message_days,
                &reason,
            )
            .await
        {
            // 処分できなかったユーザーは登録を取り消し、次の投稿で再挑戦できるようにする。
            // 権限不足のまま登録だけ残ると、権限を直しても当分の間そのユーザーを見逃し続ける。
            self.handled.forget(msg.author.id);

            self.notify(
                ctx,
                warn_embed(
                    "BANに失敗しました",
                    format!(
                        "{} ({}) をBANできませんでした。BAN_MEMBERS権限を確認してください。\nエラー: {err}",
                        msg.author.tag(),
                        msg.author.id,
                    ),
                ),
            )
            .await;

            return Err(anyhow::Error::new(err).context(format!(
                "failed to ban user (check BAN_MEMBERS permission): guild={guild_id} user={}",
                msg.author.id
            )));
        }

        self.record_ban(
            msg.author.id,
            msg.author.tag(),
            &reason,
            verdict.source,
            true,
        );
        self.notify_ban(ctx, msg, &reason, verdict.source, true)
            .await;

        let entry = LogEntry::new(FEATURE_NAME, guild_id, reason).with_user(msg.author.id);
        record_or_warn(&self.store, entry).await;

        Ok(Flow::Consume)
    }

    fn record_ban(
        &self,
        user_id: UserId,
        user_tag: String,
        reason: &str,
        source: VerdictSource,
        executed: bool,
    ) {
        self.recent_bans.record(BanRecord {
            user_id,
            user_tag,
            reason: reason.to_string(),
            source,
            at: chrono::Utc::now(),
            executed,
        });
    }

    async fn notify_ban(
        &self,
        ctx: &Context,
        msg: &Message,
        reason: &str,
        source: VerdictSource,
        executed: bool,
    ) {
        let title = if executed {
            "スパムアカウントをBANしました"
        } else {
            "スパム判定（debug_mode: BANは実行していません）"
        };

        self.notify(
            ctx,
            warn_embed(
                title,
                format!(
                    "対象: {} ({})\n判定経路: {}\n理由: {reason}\nチャンネル: <#{}>",
                    msg.author.tag(),
                    msg.author.id,
                    source.as_str(),
                    msg.channel_id,
                ),
            ),
        )
        .await;
    }

    /// 管理チャンネルへの通知。失敗しても処分自体は成立しているため、警告ログに留める。
    async fn notify(&self, ctx: &Context, embed: CreateEmbed) {
        if let Err(err) = self
            .cfg
            .log_channel()
            .send_message(&ctx.http, CreateMessage::new().embed(embed))
            .await
        {
            warn!(
                error = %err,
                channel = %self.cfg.log_channel(),
                "failed to send honeypot notification"
            );
        }
    }

    async fn handle_list(&self, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
        let records = self.recent_bans.list();

        let embed = if records.is_empty() {
            log_embed("直近の処分", "まだ処分の記録はありません。")
        } else {
            let body = records
                .iter()
                .map(|record| {
                    format!(
                        "- {} (`{}`) {} / {} / <t:{}:R>\n  {}",
                        record.user_tag,
                        record.user_id,
                        if record.executed {
                            "BAN済み"
                        } else {
                            "判定のみ"
                        },
                        record.source.as_str(),
                        record.at.timestamp(),
                        record.reason,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");

            log_embed("直近の処分", body)
        };

        self.respond(ctx, ic, embed).await
    }

    async fn handle_unban(
        &self,
        ctx: &Context,
        ic: &CommandInteraction,
        user_id: UserId,
    ) -> anyhow::Result<()> {
        let Some(guild_id) = ic.guild_id else {
            return self
                .respond(
                    ctx,
                    ic,
                    warn_embed(
                        "解除できません",
                        "このコマンドはサーバー内で実行してください。",
                    ),
                )
                .await;
        };

        let embed = match guild_id.unban(&ctx.http, user_id).await {
            Ok(()) => {
                // 再び判定対象へ戻す。戻さないと、同じユーザーの投稿が
                // 重複防止キャッシュに阻まれて判定されないままになる。
                self.handled.forget(user_id);

                info!(user_id = %user_id, moderator = %ic.user.id, "honeypot ban revoked");

                log_embed(
                    "BANを解除しました",
                    format!("<@{user_id}> (`{user_id}`) のBANを解除しました。"),
                )
            }
            Err(err) => {
                warn!(error = %err, user_id = %user_id, "failed to unban user");

                warn_embed(
                    "解除に失敗しました",
                    format!(
                        "<@{user_id}> (`{user_id}`) のBANを解除できませんでした。\nエラー: {err}"
                    ),
                )
            }
        };

        self.respond(ctx, ic, embed).await
    }

    /// コマンドへの応答は実行者にだけ見せる（処分の記録を公開チャンネルへ晒さない）。
    async fn respond(
        &self,
        ctx: &Context,
        ic: &CommandInteraction,
        embed: CreateEmbed,
    ) -> anyhow::Result<()> {
        ephemeral_respond(ctx, ic, embed).await
    }
}

#[async_trait::async_trait]
impl Feature for Honeypot {
    fn name(&self) -> &'static str {
        FEATURE_NAME
    }

    /// 処分対象の投稿を他機能へ流さないため、Message系では最優先で評価する。
    fn priority(&self) -> i32 {
        100
    }

    fn commands(&self) -> Vec<CreateCommand> {
        vec![
            CreateCommand::new(COMMAND_NAME)
                .description("ハニーポットの処分履歴の確認とBAN解除")
                // 誤BANの取り消し導線のため、BAN権限を持つ運営だけに見せる。
                .default_member_permissions(Permissions::BAN_MEMBERS)
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "list",
                    "直近の処分を確認する",
                ))
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "unban",
                        "誤BANしたユーザーのBANを解除する",
                    )
                    .add_sub_option(
                        CreateCommandOption::new(
                            CommandOptionType::User,
                            "user",
                            "解除するユーザー",
                        )
                        .required(true),
                    ),
                ),
        ]
    }

    async fn on_event(&self, ctx: &Context, ev: &FullEvent) -> anyhow::Result<Flow> {
        let FullEvent::Message { new_message } = ev else {
            return Ok(Flow::Continue);
        };

        self.handle_message(ctx, new_message).await
    }

    async fn on_command(&self, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
        match ic.data.options().as_slice() {
            [
                ResolvedOption {
                    name: "list",
                    value: ResolvedValue::SubCommand(_),
                    ..
                },
            ] => self.handle_list(ctx, ic).await,
            [
                ResolvedOption {
                    name: "unban",
                    value: ResolvedValue::SubCommand(options),
                    ..
                },
            ] => {
                let Some(user_id) = options.iter().find_map(|option| match option.value {
                    ResolvedValue::User(user, _) => Some(user.id),
                    _ => None,
                }) else {
                    return self
                        .respond(
                            ctx,
                            ic,
                            warn_embed("解除できません", "ユーザーが指定されていません。"),
                        )
                        .await;
                };

                self.handle_unban(ctx, ic, user_id).await
            }
            _ => {
                warn!(command = %ic.data.name, "unknown honeypot subcommand");
                Ok(())
            }
        }
    }
}

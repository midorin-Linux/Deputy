mod commands;
pub mod config;
mod history;
mod llm;
mod ratelimit;

use std::time::Duration;

use serenity::all::{ChannelId, CommandInteraction, Context, CreateCommand, FullEvent, Message};
use tracing::{error, warn};

use crate::{
    core::{
        config::AiConfig,
        feature::{Feature, Flow},
    },
    features::agent::{
        config::AgentConfig,
        history::{History, Role},
        llm::LlmClient,
        ratelimit::Cooldown,
    },
};

/// Discordの1メッセージあたりの文字数上限。超えた分は切り詰めて送信する。
const DISCORD_MESSAGE_LIMIT: usize = 2000;

/// LLMによる対話機能。ボットへのメンションまたは`/ask`で呼びかけられた発言に応答する。
pub struct Agent {
    llm: LlmClient,
    history: History,
    cooldown: Cooldown,
}

impl Agent {
    pub fn new(cfg: AgentConfig, ai: &AiConfig) -> anyhow::Result<Self> {
        let llm = LlmClient::new(ai, cfg.model())?;
        let history = History::new(cfg.history_limit());
        let cooldown = Cooldown::new(Duration::from_secs(cfg.cooldown_secs()));

        Ok(Self {
            llm,
            history,
            cooldown,
        })
    }

    /// メンションによる呼びかけを処理する。ボット・他ボットの発言や、メンションを含まない
    /// 発言は無視する。
    async fn handle_message(&self, ctx: &Context, msg: &Message) {
        if msg.author.bot {
            return;
        }

        let mentioned = msg.mentions_me(ctx).await.unwrap_or(false);
        if !mentioned {
            return;
        }

        if !self.cooldown.try_acquire(msg.author.id) {
            if let Err(err) = msg
                .reply(
                    &ctx.http,
                    "クールダウン中です。少し待ってから話しかけてください。",
                )
                .await
            {
                warn!(error = ?err, "agentのクールダウン通知の送信に失敗しました");
            }
            return;
        }

        // 失敗しても応答自体は続けるため、結果は無視してよい。
        let _ = ctx.http.broadcast_typing(msg.channel_id).await;

        let content = self.reply_content(msg.channel_id, &msg.content).await;

        if let Err(err) = msg.reply(&ctx.http, content).await {
            warn!(error = ?err, "agentの応答送信に失敗しました");
        }
    }

    /// 履歴へユーザー発言を積んでLLMへ問い合わせ、応答も履歴へ積んだうえで送信用に整形する。
    /// エラー時は簡潔な日本語メッセージを返し、詳細はログへ残す。
    async fn reply_content(&self, channel_id: ChannelId, user_content: &str) -> String {
        match self.generate_reply(channel_id, user_content).await {
            Ok(reply) => truncate(&reply),
            Err(err) => {
                error!(error = ?err, channel_id = %channel_id, "agentの応答生成に失敗しました");
                "応答の生成に失敗しました。しばらくしてから再度お試しください。".to_string()
            }
        }
    }

    async fn generate_reply(
        &self,
        channel_id: ChannelId,
        user_content: &str,
    ) -> anyhow::Result<String> {
        self.history
            .push(channel_id, Role::User, user_content.to_string());

        let snapshot = self.history.snapshot(channel_id);
        let reply = self.llm.respond(&snapshot).await?;

        self.history
            .push(channel_id, Role::Assistant, reply.clone());

        Ok(reply)
    }
}

/// Discordの文字数制限に収まるよう、超えた分は切り詰める。
fn truncate(content: &str) -> String {
    if content.chars().count() <= DISCORD_MESSAGE_LIMIT {
        return content.to_string();
    }

    content.chars().take(DISCORD_MESSAGE_LIMIT).collect()
}

#[async_trait::async_trait]
impl Feature for Agent {
    fn name(&self) -> &'static str {
        config::FEATURE_NAME
    }

    fn priority(&self) -> i32 {
        10
    }

    fn commands(&self) -> Vec<CreateCommand> {
        commands::command_definitions()
    }

    async fn on_event(&self, ctx: &Context, ev: &FullEvent) -> anyhow::Result<Flow> {
        if let FullEvent::Message { new_message } = ev {
            self.handle_message(ctx, new_message).await;
        }

        Ok(Flow::Continue)
    }

    async fn on_command(&self, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
        commands::on_command(self, ctx, ic).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_is_not_truncated() {
        assert_eq!(truncate("hello"), "hello");
    }

    #[test]
    fn long_content_is_truncated_to_the_discord_limit() {
        let long = "a".repeat(DISCORD_MESSAGE_LIMIT + 100);
        let truncated = truncate(&long);
        assert_eq!(truncated.chars().count(), DISCORD_MESSAGE_LIMIT);
    }
}

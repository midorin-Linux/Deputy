use anyhow::Context as _;
use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseFollowup, CreateInteractionResponseMessage,
    ResolvedOption, ResolvedValue,
};
use tracing::{error, warn};

use crate::{
    core::discord::{
        embed::{log_embed, warn_embed},
        interaction::ephemeral_respond,
    },
    features::agent::Agent,
};

const ASK: &str = "ask";
const RESET: &str = "reset";

pub fn command_definitions() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new(ASK)
            .description("AIエージェントに質問する")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "text", "質問内容")
                    .required(true),
            ),
        CreateCommand::new(RESET).description("このチャンネルの会話履歴をクリアする"),
    ]
}

pub async fn on_command(
    agent: &Agent,
    ctx: &Context,
    ic: &CommandInteraction,
) -> anyhow::Result<()> {
    match ic.data.name.as_str() {
        ASK => handle_ask(agent, ctx, ic).await,
        RESET => handle_reset(agent, ctx, ic).await,
        other => {
            warn!(command = %other, "未知のagentコマンドを受信しました");
            Ok(())
        }
    }
}

async fn handle_ask(agent: &Agent, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
    let Some(text) = ic.data.options().iter().find_map(|option| match option {
        ResolvedOption {
            name: "text",
            value: ResolvedValue::String(value),
            ..
        } => Some(value.to_string()),
        _ => None,
    }) else {
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed("応答できません", "質問内容が指定されていません。"),
        )
        .await;
    };

    if !agent.cooldown.try_acquire(ic.user.id) {
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed(
                "少し待ってください",
                "クールダウン中です。しばらくしてから再度お試しください。",
            ),
        )
        .await;
    }

    // LLMの応答生成は時間がかかるため、先にdeferしてインタラクションの初回応答期限（3秒）を回避する。
    ic.create_response(
        &ctx.http,
        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
    )
    .await
    .context("failed to defer ask command")?;

    let content = match agent.generate_reply(ic.channel_id, &text).await {
        Ok(reply) => super::truncate(&reply),
        Err(err) => {
            error!(error = ?err, channel_id = %ic.channel_id, "agentの応答生成に失敗しました");
            "応答の生成に失敗しました。しばらくしてから再度お試しください。".to_string()
        }
    };

    ic.create_followup(
        &ctx.http,
        CreateInteractionResponseFollowup::new().content(content),
    )
    .await
    .map(|_| ())
    .context("failed to send deferred ask response")
}

async fn handle_reset(agent: &Agent, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
    agent.history.clear(ic.channel_id);

    ephemeral_respond(
        ctx,
        ic,
        log_embed(
            "クリアしました",
            "このチャンネルの会話履歴をクリアしました。",
        ),
    )
    .await
}

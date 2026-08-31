//! スラッシュコマンド応答の共通ヘルパー。応答は実行者にだけ見せる（ephemeral）。

use anyhow::Context as _;
use serenity::all::{
    CommandInteraction, Context, CreateEmbed, CreateInteractionResponse,
    CreateInteractionResponseFollowup, CreateInteractionResponseMessage,
};

/// コマンドへのephemeral応答。実行者にだけ見せる。
pub async fn ephemeral_respond(
    ctx: &Context,
    ic: &CommandInteraction,
    embed: CreateEmbed,
) -> anyhow::Result<()> {
    ic.create_response(
        &ctx.http,
        CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .ephemeral(true),
        ),
    )
    .await
    .with_context(|| format!("failed to respond to command: {}", ic.data.name))
}

/// `defer`で応答を保留した後の本応答。`defer`と同じく実行者にだけ見せる。
pub async fn ephemeral_followup(
    ctx: &Context,
    ic: &CommandInteraction,
    embed: CreateEmbed,
) -> anyhow::Result<()> {
    ic.create_followup(
        &ctx.http,
        CreateInteractionResponseFollowup::new()
            .embed(embed)
            .ephemeral(true),
    )
    .await
    .map(|_| ())
    .with_context(|| format!("failed to send deferred command response: {}", ic.data.name))
}

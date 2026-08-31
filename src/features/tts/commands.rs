use std::sync::Arc;

use anyhow::Context as _;
use serenity::all::{
    CommandInteraction, CommandOptionType, Context, CreateCommand, CreateCommandOption,
    CreateInteractionResponse, CreateInteractionResponseMessage, GuildId, ResolvedOption,
    ResolvedValue,
};
use songbird::Songbird;
use tracing::warn;

use crate::{
    core::discord::{
        embed::{log_embed, warn_embed},
        interaction::{ephemeral_followup, ephemeral_respond},
    },
    features::tts::Tts,
};

const JOIN: &str = "join";
const LEAVE: &str = "leave";
const SKIP: &str = "skip";
const SPEAKER: &str = "speaker";

pub fn command_definitions() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new(JOIN).description("実行者が入っているVCへ接続し、コマンドを実行したテキストチャンネルを読み上げ対象にする"),
        CreateCommand::new(LEAVE).description("VCから退出し、読み上げを終了する"),
        CreateCommand::new(SKIP).description("読み上げ中の1件をスキップする"),
        CreateCommand::new(SPEAKER)
            .description("自分の発言に使う話者IDを設定する")
            .add_option(
                CreateCommandOption::new(CommandOptionType::Integer, "id", "話者ID")
                    .required(true)
                    .min_int_value(0),
            ),
    ]
}

pub async fn on_command(tts: &Tts, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
    match ic.data.name.as_str() {
        JOIN => handle_join(tts, ctx, ic).await,
        LEAVE => handle_leave(tts, ctx, ic).await,
        SKIP => handle_skip(tts, ctx, ic).await,
        SPEAKER => handle_speaker(tts, ctx, ic).await,
        other => {
            warn!(command = %other, "未知のttsコマンドを受信しました");
            Ok(())
        }
    }
}

/// ギルド外での実行を弾く。ギルド外ならその場でエラー応答し`None`を返す。
async fn require_guild(
    ctx: &Context,
    ic: &CommandInteraction,
    title: &str,
) -> anyhow::Result<Option<GuildId>> {
    match ic.guild_id {
        Some(guild_id) => Ok(Some(guild_id)),
        None => {
            ephemeral_respond(
                ctx,
                ic,
                warn_embed(title, "このコマンドはサーバー内で実行してください。"),
            )
            .await?;
            Ok(None)
        }
    }
}

/// songbirdマネージャーを取得する。取得できなければその場でエラー応答し`None`を返す。
async fn require_manager(
    ctx: &Context,
    ic: &CommandInteraction,
    title: &str,
) -> anyhow::Result<Option<Arc<Songbird>>> {
    match songbird::get(ctx).await {
        Some(manager) => Ok(Some(manager)),
        None => {
            ephemeral_respond(
                ctx,
                ic,
                warn_embed(title, "songbirdマネージャーを取得できませんでした。"),
            )
            .await?;
            Ok(None)
        }
    }
}

async fn handle_join(tts: &Tts, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
    let Some(guild_id) = require_guild(ctx, ic, "接続できません").await? else {
        return Ok(());
    };

    let member_voice_channel = ctx.cache.guild(guild_id).and_then(|guild| {
        guild
            .voice_states
            .get(&ic.user.id)
            .and_then(|vs| vs.channel_id)
    });

    let Some(voice_channel) = member_voice_channel else {
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed(
                "接続できません",
                "先にボイスチャンネルへ参加してから実行してください。",
            ),
        )
        .await;
    };

    let Some(manager) = require_manager(ctx, ic, "接続できません").await? else {
        return Ok(());
    };

    // VCへの接続は音声ゲートウェイの応答待ちを含み、インタラクションの初回応答期限（3秒）を
    // 超えることがある。先に`defer`で受け付けを返し、結果はfollowupで伝える。
    ic.create_response(
        &ctx.http,
        CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new().ephemeral(true)),
    )
    .await
    .context("failed to defer tts join command")?;

    if let Err(err) = manager.join(guild_id, voice_channel).await {
        warn!(error = ?err, guild_id = %guild_id, "VCへの接続に失敗しました");
        return ephemeral_followup(
            ctx,
            ic,
            warn_embed(
                "接続できません",
                format!("VCへの接続に失敗しました。\nエラー: {err}"),
            ),
        )
        .await;
    }

    tts.bound
        .lock()
        .expect("tts bound mutex poisoned")
        .insert(guild_id, ic.channel_id);

    ephemeral_followup(
        ctx,
        ic,
        log_embed(
            "接続しました",
            format!("<#{}> の投稿を読み上げます。", ic.channel_id),
        ),
    )
    .await
}

async fn handle_leave(tts: &Tts, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
    let Some(guild_id) = require_guild(ctx, ic, "退出できません").await? else {
        return Ok(());
    };

    let Some(manager) = require_manager(ctx, ic, "退出できません").await? else {
        return Ok(());
    };

    if manager.get(guild_id).is_none() {
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed("退出できません", "VCに接続していません。"),
        )
        .await;
    }

    if let Err(err) = manager.leave(guild_id).await {
        warn!(error = ?err, guild_id = %guild_id, "VCからの退出に失敗しました");
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed(
                "退出できません",
                format!("VCからの退出に失敗しました。\nエラー: {err}"),
            ),
        )
        .await;
    }

    tts.bound
        .lock()
        .expect("tts bound mutex poisoned")
        .remove(&guild_id);

    ephemeral_respond(
        ctx,
        ic,
        log_embed("退出しました", "読み上げを終了しました。"),
    )
    .await
}

async fn handle_skip(_tts: &Tts, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
    let Some(guild_id) = require_guild(ctx, ic, "スキップできません").await? else {
        return Ok(());
    };

    let Some(manager) = require_manager(ctx, ic, "スキップできません").await? else {
        return Ok(());
    };

    let Some(call) = manager.get(guild_id) else {
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed("スキップできません", "VCに接続していません。"),
        )
        .await;
    };

    // ロックはスキップ操作の間だけ保持する。Discordへの応答（HTTP往復）をまたいで持つと、
    // 同じギルドの読み上げ投入・自動退出判定が応答完了までブロックされる。
    let skip_result = {
        let call = call.lock().await;

        if call.queue().current_queue().is_empty() {
            None
        } else {
            Some(call.queue().skip())
        }
    };

    match skip_result {
        None => {
            ephemeral_respond(
                ctx,
                ic,
                warn_embed("スキップできません", "再生中の読み上げがありません。"),
            )
            .await
        }
        Some(Err(err)) => {
            warn!(error = ?err, guild_id = %guild_id, "読み上げのスキップに失敗しました");
            ephemeral_respond(
                ctx,
                ic,
                warn_embed(
                    "スキップできません",
                    format!("スキップに失敗しました。\nエラー: {err}"),
                ),
            )
            .await
        }
        Some(Ok(())) => {
            ephemeral_respond(
                ctx,
                ic,
                log_embed("スキップしました", "再生中の読み上げをスキップしました。"),
            )
            .await
        }
    }
}

async fn handle_speaker(tts: &Tts, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()> {
    let Some(id) = ic.data.options().iter().find_map(|option| match option {
        ResolvedOption {
            name: "id",
            value: ResolvedValue::Integer(value),
            ..
        } => Some(*value),
        _ => None,
    }) else {
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed("設定できません", "話者IDが指定されていません。"),
        )
        .await;
    };

    let Ok(speaker) = u32::try_from(id) else {
        return ephemeral_respond(ctx, ic, warn_embed("設定できません", "話者IDが不正です。"))
            .await;
    };

    if let Err(err) = tts.speakers.set(ic.user.id, speaker).await {
        warn!(error = ?err, user_id = %ic.user.id, "tts話者設定の保存に失敗しました");
        return ephemeral_respond(
            ctx,
            ic,
            warn_embed(
                "設定できません",
                format!("話者設定の保存に失敗しました。\nエラー: {err}"),
            ),
        )
        .await;
    }

    ephemeral_respond(
        ctx,
        ic,
        log_embed(
            "設定しました",
            format!("話者IDを {speaker} に設定しました。"),
        ),
    )
    .await
}

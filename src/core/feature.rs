use serenity::all::{CommandInteraction, Context, CreateCommand, FullEvent};

/// イベントを消費したか、後続の機能へ流すかを表す。
pub enum Flow {
    Continue,
    /// 例: honeypotが検知したらtts / agentへは流さない。
    Consume,
}

#[async_trait::async_trait]
pub trait Feature: Send + Sync {
    fn name(&self) -> &'static str;

    /// Message系イベントの処理順。大きいほど先。既定0。
    fn priority(&self) -> i32 {
        0
    }

    /// この機能が提供するスラッシュコマンド定義。
    fn commands(&self) -> Vec<CreateCommand> {
        vec![]
    }

    /// Gatewayイベントの処理。
    async fn on_event(&self, ctx: &Context, ev: &FullEvent) -> anyhow::Result<Flow>;

    /// 自機能のコマンドが呼ばれたときの処理。
    async fn on_command(&self, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()>;
}

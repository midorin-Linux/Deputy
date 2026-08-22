use serenity::all::{
    Command, Context, CreateCommand, EventHandler, FullEvent, GuildId, Interaction, Member,
    Message, Ready, User, VoiceState,
};
use tracing::{error, info, warn};

use crate::core::{
    commands::CommandRouter,
    feature::{Feature, Flow},
};

/// 唯一の`EventHandler`実装。イベント配送・コマンドルーティング・エラー隔離だけを担う。
/// 「何をするか」は一切知らず、機構のみを持つ。
pub struct Registry {
    features: Vec<Box<dyn Feature>>,
    commands: Vec<CreateCommand>,
    router: CommandRouter,
}

impl Registry {
    pub fn new(mut features: Vec<Box<dyn Feature>>) -> Self {
        // priority降順で並べておき、配送時はそのまま先頭から呼べばよいようにする。
        features.sort_by_key(|f| std::cmp::Reverse(f.priority()));

        let (commands, router) = CommandRouter::build(&features);

        Self {
            features,
            commands,
            router,
        }
    }

    /// `FullEvent`をpriority降順で各機能へ配送する。
    /// `Flow::Consume`が返れば打ち切り、`Err`は機能単位でログして次へ進む（bulkhead）。
    async fn dispatch(&self, ctx: &Context, ev: FullEvent) {
        for feature in &self.features {
            match feature.on_event(ctx, &ev).await {
                Ok(Flow::Continue) => {}
                Ok(Flow::Consume) => break,
                Err(err) => {
                    error!(feature = feature.name(), error = ?err, "feature failed to handle event");
                }
            }
        }
    }
}

/// 各`EventHandler`メソッドは対応する`FullEvent`へ詰め直して`dispatch`へ渡すだけ。
///
/// 機能が`on_event`で受け取れるのは、ここで明示的に転送したイベントに限られる。
/// 新しい`FullEvent`を扱う機能を追加したら、対応するメソッドをここへ実装すること
/// （未実装だと`EventHandler`の既定実装が黙って握り潰し、機能が沈黙する）。
#[async_trait::async_trait]
impl EventHandler for Registry {
    async fn ready(&self, ctx: Context, data_about_bot: Ready) {
        info!(user = %data_about_bot.user.name, "gateway ready");

        // 空配列でのPUTは既存のグローバルコマンドを全削除してしまうため、何か登録がある時だけ呼ぶ。
        if !self.commands.is_empty()
            && let Err(err) = Command::set_global_commands(&ctx.http, self.commands.clone()).await
        {
            error!(error = %err, "failed to register global commands");
        }

        self.dispatch(&ctx, FullEvent::Ready { data_about_bot })
            .await;
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        self.dispatch(&ctx, FullEvent::Message { new_message })
            .await;
    }

    async fn guild_member_addition(&self, ctx: Context, new_member: Member) {
        self.dispatch(&ctx, FullEvent::GuildMemberAddition { new_member })
            .await;
    }

    async fn guild_member_removal(
        &self,
        ctx: Context,
        guild_id: GuildId,
        user: User,
        member_data_if_available: Option<Member>,
    ) {
        self.dispatch(
            &ctx,
            FullEvent::GuildMemberRemoval {
                guild_id,
                user,
                member_data_if_available,
            },
        )
        .await;
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        self.dispatch(&ctx, FullEvent::VoiceStateUpdate { old, new })
            .await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = &interaction {
            match self.router.route(&command.data.name) {
                Some(index) => {
                    if let Err(err) = self.features[index].on_command(&ctx, command).await {
                        error!(
                            feature = self.features[index].name(),
                            error = ?err,
                            "feature failed to handle command"
                        );
                    }
                }
                None => {
                    warn!(command = %command.data.name, "no feature registered for this command");
                }
            }

            // コマンドは担当機能にのみ渡し、他の機能へは流さない。
            return;
        }

        self.dispatch(&ctx, FullEvent::InteractionCreate { interaction })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use serenity::all::{CommandInteraction, Context, FullEvent};

    use super::*;

    struct StubFeature {
        name: &'static str,
        priority: i32,
    }

    #[async_trait::async_trait]
    impl Feature for StubFeature {
        fn name(&self) -> &'static str {
            self.name
        }

        fn priority(&self) -> i32 {
            self.priority
        }

        async fn on_event(&self, _ctx: &Context, _ev: &FullEvent) -> anyhow::Result<Flow> {
            Ok(Flow::Continue)
        }

        async fn on_command(&self, _ctx: &Context, _ic: &CommandInteraction) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn features_are_ordered_by_priority_descending() {
        let features: Vec<Box<dyn Feature>> = vec![
            Box::new(StubFeature {
                name: "low",
                priority: 0,
            }),
            Box::new(StubFeature {
                name: "high",
                priority: 10,
            }),
            Box::new(StubFeature {
                name: "mid",
                priority: 5,
            }),
        ];

        let registry = Registry::new(features);

        let names: Vec<&str> = registry.features.iter().map(|f| f.name()).collect();
        assert_eq!(names, vec!["high", "mid", "low"]);
    }

    #[test]
    fn equal_priority_preserves_input_order() {
        let features: Vec<Box<dyn Feature>> = vec![
            Box::new(StubFeature {
                name: "first",
                priority: 0,
            }),
            Box::new(StubFeature {
                name: "second",
                priority: 0,
            }),
        ];

        let registry = Registry::new(features);

        let names: Vec<&str> = registry.features.iter().map(|f| f.name()).collect();
        assert_eq!(names, vec!["first", "second"]);
    }
}

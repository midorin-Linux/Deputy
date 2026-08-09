use std::collections::HashMap;

use serenity::all::CreateCommand;

use crate::core::feature::Feature;

/// スラッシュコマンド名 → 機能インデックスのルーティング表。
pub struct CommandRouter {
    routes: HashMap<String, usize>,
}

impl CommandRouter {
    /// 各機能の`commands()`を集約し、Discordへ一括登録するコマンド一覧とルーティング表を作る。
    pub fn build(features: &[Box<dyn Feature>]) -> (Vec<CreateCommand>, Self) {
        let mut commands = Vec::new();
        let mut routes = HashMap::new();

        for (index, feature) in features.iter().enumerate() {
            for command in feature.commands() {
                if let Some(name) = command_name(&command) {
                    routes.insert(name, index);
                }
                commands.push(command);
            }
        }

        (commands, Self { routes })
    }

    /// コマンド名から担当する機能のインデックスを引く。
    pub fn route(&self, command_name: &str) -> Option<usize> {
        self.routes.get(command_name).copied()
    }
}

/// `CreateCommand`は名前を公開フィールドとして持たないため、Discordへ送るのと同じ
/// JSON表現から取り出す。
fn command_name(command: &CreateCommand) -> Option<String> {
    serde_json::to_value(command)
        .ok()?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use serenity::all::{CommandInteraction, Context, FullEvent};

    use super::*;
    use crate::core::feature::Flow;

    struct StubFeature;

    #[async_trait::async_trait]
    impl Feature for StubFeature {
        fn name(&self) -> &'static str {
            "stub"
        }

        fn commands(&self) -> Vec<CreateCommand> {
            vec![CreateCommand::new("ask")]
        }

        async fn on_event(&self, _ctx: &Context, _ev: &FullEvent) -> anyhow::Result<Flow> {
            Ok(Flow::Continue)
        }

        async fn on_command(&self, _ctx: &Context, _ic: &CommandInteraction) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn routes_command_name_to_owning_feature_index() {
        let features: Vec<Box<dyn Feature>> = vec![Box::new(StubFeature)];
        let (commands, router) = CommandRouter::build(&features);

        assert_eq!(commands.len(), 1);
        assert_eq!(router.route("ask"), Some(0));
        assert_eq!(router.route("missing"), None);
    }
}

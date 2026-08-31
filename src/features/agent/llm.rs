//! LLM呼び出し。honeypot/agent.rsの`async-openai`クライアント構築パターン
//! （`OpenAIConfig::with_api_base` + APIキー、OpenRouter用ヘッダー、backoffリトライ、timeout）を
//! 踏襲しているが、JSON強制なしの自由文応答で画像も扱わない。

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage,
        CreateChatCompletionRequestArgs,
    },
};
use backoff::{ExponentialBackoffBuilder, future::retry};
use tracing::warn;

use crate::{
    core::config::AiConfig,
    features::agent::history::{HistoryEntry, Role},
};

/// システムプロンプトのパス。`PROMPT.md`（honeypot）と同じくプロセスのカレントディレクトリ基準。
pub const PROMPT_FILE: &str = "AGENT_PROMPT.md";

/// `AGENT_PROMPT.md`が無い・読めない場合に使う既定のシステムプロンプト。
/// VOICEVOX未起動時の扱いと同じく、起動時エラーにはせず警告ログのうえで動作を継続する。
const DEFAULT_SYSTEM_PROMPT: &str = "あなたはDiscordサーバーに常駐する親しみやすいアシスタントです。\
簡潔で分かりやすい日本語で回答してください。";

/// リトライの初期待機時間（ミリ秒）。以降は指数的に増加する。
const RETRY_INITIAL_DELAY_MS: u64 = 1000;
/// リトライを打ち切るまでの総経過時間（秒）。これを超えると最後のエラーを返す。
const RETRY_MAX_ELAPSED_SECS: u64 = 15;

pub struct LlmClient {
    client: Client<OpenAIConfig>,
    model: String,
    system_prompt: String,
    request_timeout: Duration,
}

impl LlmClient {
    /// `model_override`（`AgentConfig.model`）があれば共通の`ai.model_id`より優先する。
    pub fn new(ai: &AiConfig, model_override: Option<&str>) -> Result<Self> {
        let system_prompt = load_system_prompt();

        let openai_config = OpenAIConfig::new()
            .with_api_base(ai.base_url.clone())
            .with_api_key(ai.api_key.expose())
            .with_header("HTTP-Referer", "https://github.com/midorin-Linux/Deputy")?
            .with_header("X-OpenRouter-Title", "Deputy")?
            .with_header("X-OpenRouter-Categories", "personal-agent")?;

        Ok(Self {
            client: Client::with_config(openai_config),
            model: model_override
                .map(str::to_string)
                .unwrap_or_else(|| ai.model_id.clone()),
            system_prompt,
            request_timeout: Duration::from_secs(ai.request_timeout_secs),
        })
    }

    /// 会話履歴（最新のユーザー発言を含む）からLLMの応答を得る。
    /// 一時的な失敗は指数バックオフでリトライする。
    pub async fn respond(&self, history: &[HistoryEntry]) -> Result<String> {
        let backoff = ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(RETRY_INITIAL_DELAY_MS))
            .with_max_elapsed_time(Some(Duration::from_secs(RETRY_MAX_ELAPSED_SECS)))
            .build();

        retry(backoff, || async {
            self.respond_once(history)
                .await
                .map_err(backoff::Error::transient)
        })
        .await
    }

    async fn respond_once(&self, history: &[HistoryEntry]) -> Result<String> {
        let mut messages = vec![ChatCompletionRequestMessage::System(
            ChatCompletionRequestSystemMessage::from(self.system_prompt.as_str()),
        )];
        messages.extend(history.iter().map(to_chat_message));

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages)
            .build()
            .context("failed to build chat completion request")?;

        let response =
            tokio::time::timeout(self.request_timeout, self.client.chat().create(request))
                .await
                .context("chat completion request timed out")?
                .context("chat completion request failed")?;

        response
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .context("chat completion response had no content")
    }
}

fn to_chat_message(entry: &HistoryEntry) -> ChatCompletionRequestMessage {
    match entry.role {
        Role::User => ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage::from(
            entry.content.as_str(),
        )),
        Role::Assistant => ChatCompletionRequestMessage::Assistant(
            ChatCompletionRequestAssistantMessage::from(entry.content.as_str()),
        ),
    }
}

fn load_system_prompt() -> String {
    let path = PathBuf::from(PROMPT_FILE);

    match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            warn!(
                error = ?err,
                path = %path.display(),
                "AGENT_PROMPT.mdを読み込めなかったため既定のシステムプロンプトを使用します"
            );
            DEFAULT_SYSTEM_PROMPT.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_system_prompt_is_not_empty() {
        assert!(!DEFAULT_SYSTEM_PROMPT.trim().is_empty());
    }

    #[test]
    fn user_entry_becomes_user_message() {
        let entry = HistoryEntry {
            role: Role::User,
            content: "hello".to_string(),
        };
        assert!(matches!(
            to_chat_message(&entry),
            ChatCompletionRequestMessage::User(_)
        ));
    }

    #[test]
    fn assistant_entry_becomes_assistant_message() {
        let entry = HistoryEntry {
            role: Role::Assistant,
            content: "hi".to_string(),
        };
        assert!(matches!(
            to_chat_message(&entry),
            ChatCompletionRequestMessage::Assistant(_)
        ));
    }
}

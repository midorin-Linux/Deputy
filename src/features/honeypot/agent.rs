//! LLMによるスパム判定。移植元（honeypot v0.3.2）の`agent/`をほぼそのまま持ち込み、
//! 設定の受け取りだけを`core`の`AiConfig`へ差し替えている。

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartImage,
        ChatCompletionRequestMessageContentPartText, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageArgs,
        ChatCompletionRequestUserMessageContentPart, CreateChatCompletionRequestArgs, ImageUrl,
        ResponseFormat,
    },
};
use backoff::{ExponentialBackoffBuilder, future::retry};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::Deserialize;
use tracing::debug;

use crate::core::config::AiConfig;

/// システムプロンプトのパス。`settings.yml`と同じくプロセスのカレントディレクトリ基準。
pub const PROMPT_FILE: &str = "PROMPT.md";

/// リトライの初期待機時間（ミリ秒）。以降は指数的に増加する。
const RETRY_INITIAL_DELAY_MS: u64 = 1000;
/// リトライを打ち切るまでの総経過時間（秒）。これを超えると最後のエラーを返す。
const RETRY_MAX_ELAPSED_SECS: u64 = 15;

/// 判定に添える画像。DiscordのCDNリンクは有効期限が切れるため、URLではなく
/// base64データURLとして埋め込む。
pub struct ImageAttachment {
    pub data: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Deserialize)]
pub struct SpamVerdict {
    pub is_spam: bool,
    pub reason: String,
}

pub struct Agent {
    client: Client<OpenAIConfig>,
    model: String,
    support_image: bool,
    system_prompt: String,
    request_timeout: Duration,
}

impl Agent {
    pub fn new(cfg: &AiConfig) -> Result<Self> {
        let system_prompt = load_system_prompt()?;

        let openai_config = OpenAIConfig::new()
            .with_api_base(cfg.base_url.clone())
            .with_api_key(cfg.api_key.expose())
            .with_header("HTTP-Referer", "https://github.com/midorin-Linux/Deputy")?
            .with_header("X-OpenRouter-Title", "Deputy")?
            .with_header("X-OpenRouter-Categories", "personal-agent")?;

        Ok(Self {
            client: Client::with_config(openai_config),
            model: cfg.model_id.clone(),
            support_image: cfg.support_image,
            system_prompt,
            request_timeout: Duration::from_secs(cfg.request_timeout_secs),
        })
    }

    pub fn support_image(&self) -> bool {
        self.support_image
    }

    /// スパム判定を行う。一時的な失敗は指数バックオフでリトライする。
    pub async fn judge_spam(
        &self,
        content: &str,
        images: &[ImageAttachment],
    ) -> Result<SpamVerdict> {
        let backoff = ExponentialBackoffBuilder::new()
            .with_initial_interval(Duration::from_millis(RETRY_INITIAL_DELAY_MS))
            .with_max_elapsed_time(Some(Duration::from_secs(RETRY_MAX_ELAPSED_SECS)))
            .build();

        retry(backoff, || async {
            self.judge_spam_once(content, images)
                .await
                .map_err(backoff::Error::transient)
        })
        .await
    }

    async fn judge_spam_once(
        &self,
        content: &str,
        images: &[ImageAttachment],
    ) -> Result<SpamVerdict> {
        let user_message = self.build_user_message(content, images)?;

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(vec![
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage::from(
                    self.system_prompt.as_str(),
                )),
                user_message,
            ])
            .response_format(ResponseFormat::JsonObject)
            .build()
            .context("failed to build chat completion request")?;

        let response =
            tokio::time::timeout(self.request_timeout, self.client.chat().create(request))
                .await
                .context("chat completion request timed out")?
                .context("chat completion request failed")?;

        let content = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .context("chat completion response had no content")?;

        let verdict: SpamVerdict =
            serde_json::from_str(extract_json(content)).with_context(|| {
                format!("failed to parse spam verdict json from response: {content}")
            })?;

        debug!(reason = %verdict.reason, "spam verdict reason");

        Ok(verdict)
    }

    fn build_user_message(
        &self,
        content: &str,
        images: &[ImageAttachment],
    ) -> Result<ChatCompletionRequestMessage> {
        if !self.support_image || images.is_empty() {
            return Ok(ChatCompletionRequestMessage::User(
                ChatCompletionRequestUserMessage::from(content),
            ));
        }

        let mut parts: Vec<ChatCompletionRequestUserMessageContentPart> =
            vec![ChatCompletionRequestMessageContentPartText::from(content).into()];

        for image in images {
            let data_url = format!(
                "data:{};base64,{}",
                image.content_type,
                BASE64.encode(&image.data)
            );

            parts.push(
                ChatCompletionRequestMessageContentPartImage::from(ImageUrl {
                    url: data_url,
                    detail: None,
                })
                .into(),
            );
        }

        Ok(ChatCompletionRequestUserMessageArgs::default()
            .content(parts)
            .build()
            .context("failed to build user message with images")?
            .into())
    }
}

fn load_system_prompt() -> Result<String> {
    let path = PathBuf::from(PROMPT_FILE);

    std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read system prompt from {}", path.display()))
}

/// LLM応答からJSONオブジェクト部分を抽出する。
/// `response_format`にJSONモードを指定していても、指示追従性の低いモデルは
/// Markdownコードフェンス(```json ... ```)や前置き・後置きテキストを付けることがある。
/// 最初の`{`から最後の`}`までを切り出すことで、そうした装飾を許容する。
fn extract_json(content: &str) -> &str {
    let trimmed = content.trim();

    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(start), Some(end)) if end > start => &trimmed[start ..= end],
        _ => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_json_passes_through() {
        let raw = r#"{"is_spam": true, "reason": "x"}"#;
        assert_eq!(extract_json(raw), raw);
    }

    #[test]
    fn code_fence_is_stripped() {
        let raw = "```json\n{\"is_spam\": false, \"reason\": \"x\"}\n```";
        assert_eq!(extract_json(raw), "{\"is_spam\": false, \"reason\": \"x\"}");
    }

    #[test]
    fn surrounding_prose_is_stripped() {
        let raw = "Here is my answer: {\"is_spam\": true, \"reason\": \"x\"} Hope this helps!";
        assert_eq!(extract_json(raw), "{\"is_spam\": true, \"reason\": \"x\"}");
    }

    #[test]
    fn nested_object_keeps_the_outermost_braces() {
        let raw = r#"{"is_spam": true, "meta": {"score": 1}}"#;
        assert_eq!(extract_json(raw), raw);
    }

    #[test]
    fn text_without_json_is_returned_trimmed() {
        assert_eq!(extract_json("  no json here  "), "no json here");
    }

    #[test]
    fn extracted_json_deserializes_into_verdict() {
        let raw = "```json\n{\"is_spam\": true, \"reason\": \"nitro scam\"}\n```";
        let verdict: SpamVerdict = serde_json::from_str(extract_json(raw)).unwrap();
        assert!(verdict.is_spam);
        assert_eq!(verdict.reason, "nitro scam");
    }
}

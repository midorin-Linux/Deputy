use anyhow::{Context, Result};

/// VOICEVOX ENGINE（`/audio_query` → `/synthesis`）を呼び出すクライアント。
/// `src/services`配下のためserenityへは依存しない（依存方向: `main → features → core / services`）。
pub struct VoicevoxClient {
    http: reqwest::Client,
    base_url: String,
}

impl VoicevoxClient {
    /// `base_url`末尾のスラッシュは取り除いて保持する（`{base}/audio_query`のように連結するため）。
    pub fn new(base_url: String, timeout_secs: u64) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .context("failed to build reqwest client for VOICEVOX")?;

        Ok(Self {
            http,
            base_url: normalize_base_url(&base_url),
        })
    }

    /// テキストを音声合成し、WAVバイト列を返す。
    /// `/audio_query`のレスポンス（クエリJSON）は中身を解釈せずそのまま`/synthesis`へ渡す。
    pub async fn synthesize(&self, text: &str, speaker: u32) -> Result<Vec<u8>> {
        let speaker_str = speaker.to_string();

        let query: serde_json::Value = self
            .http
            .post(format!("{}/audio_query", self.base_url))
            .query(&[("text", text), ("speaker", &speaker_str)])
            .send()
            .await
            .context("failed to call VOICEVOX /audio_query")?
            .error_for_status()
            .context("VOICEVOX /audio_query returned an error status")?
            .json()
            .await
            .context("failed to parse VOICEVOX /audio_query response as JSON")?;

        let wav = self
            .http
            .post(format!("{}/synthesis", self.base_url))
            .query(&[("speaker", &speaker_str)])
            .json(&query)
            .send()
            .await
            .context("failed to call VOICEVOX /synthesis")?
            .error_for_status()
            .context("VOICEVOX /synthesis returned an error status")?
            .bytes()
            .await
            .context("failed to read VOICEVOX /synthesis response body")?;

        Ok(wav.to_vec())
    }
}

/// 末尾のスラッシュを取り除く。無ければそのまま。
fn normalize_base_url(base_url: &str) -> String {
    base_url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_base_url_strips_trailing_slash() {
        assert_eq!(
            normalize_base_url("http://localhost:50021/"),
            "http://localhost:50021"
        );
    }

    #[test]
    fn normalize_base_url_strips_multiple_trailing_slashes() {
        assert_eq!(
            normalize_base_url("http://localhost:50021//"),
            "http://localhost:50021"
        );
    }

    #[test]
    fn normalize_base_url_keeps_url_without_trailing_slash() {
        assert_eq!(
            normalize_base_url("http://localhost:50021"),
            "http://localhost:50021"
        );
    }
}

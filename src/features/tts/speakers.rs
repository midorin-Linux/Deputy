// ponytail: 更新のたびに全量を書き出す。ユーザー数が増えたらSQLiteへ。

use std::{collections::HashMap, fs, path::PathBuf, sync::Mutex};

use anyhow::{Context, Result};
use serenity::all::UserId;
use tracing::warn;

/// ユーザーごとの話者ID設定。`data/tts_speakers.json`（カレントディレクトリ基準）へ永続化する。
pub struct SpeakerStore {
    path: PathBuf,
    map: Mutex<HashMap<UserId, u32>>,
}

impl SpeakerStore {
    /// ファイルを読む。無ければ空で開始する。壊れたJSONは警告ログを出して空で開始する
    /// （読み上げ機能全体を起動不能にしないため）。
    pub fn load(path: PathBuf) -> Self {
        let map = match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<HashMap<String, u32>>(&content) {
                Ok(raw) => parse_user_ids(raw),
                Err(err) => {
                    warn!(
                        error = %err,
                        path = %path.display(),
                        "tts話者設定ファイルの読み込みに失敗しました。空の状態で開始します"
                    );
                    HashMap::new()
                }
            },
            Err(_) => HashMap::new(),
        };

        Self {
            path,
            map: Mutex::new(map),
        }
    }

    /// 設定済みならその話者ID、未設定なら`default`。
    pub fn get(&self, user: UserId, default: u32) -> u32 {
        self.map
            .lock()
            .expect("SpeakerStore mutex poisoned")
            .get(&user)
            .copied()
            .unwrap_or(default)
    }

    /// 更新してファイルへ全量書き出す。書き込み失敗はエラーを返す。
    ///
    /// ロックは更新とシリアライズの間だけ保持し、ディスクへの書き込みは`tokio::fs`で行う
    /// （同期I/Oでtokioのワーカースレッドを塞がないため）。並行する`set`同士は
    /// 後勝ちのスナップショット書き出しになるが、更新はマップへ反映済みのため欠落はしない。
    pub async fn set(&self, user: UserId, speaker: u32) -> Result<()> {
        let json = {
            let mut map = self.map.lock().expect("SpeakerStore mutex poisoned");
            map.insert(user, speaker);

            let raw: HashMap<String, u32> = map
                .iter()
                .map(|(user_id, speaker)| (user_id.get().to_string(), *speaker))
                .collect();

            serde_json::to_string_pretty(&raw)
                .context("failed to serialize tts speaker settings")?
        };

        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }

        tokio::fs::write(&self.path, json).await.with_context(|| {
            format!(
                "failed to write tts speaker settings to {}",
                self.path.display()
            )
        })
    }
}

fn parse_user_ids(raw: HashMap<String, u32>) -> HashMap<UserId, u32> {
    raw.into_iter()
        .filter_map(|(id, speaker)| id.parse::<u64>().ok().map(|id| (UserId::new(id), speaker)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト間で衝突しない一意なテンポラリディレクトリを作る。
    /// `tempfile`クレートは依存に無いため、PIDとナノ秒時刻で手作りする。
    fn temp_path(name: &str) -> PathBuf {
        let unique = format!(
            "deputy_tts_speakers_test_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name,
        );
        std::env::temp_dir().join(unique).join("tts_speakers.json")
    }

    #[test]
    fn unset_user_falls_back_to_default() {
        let path = temp_path("fallback");
        let store = SpeakerStore::load(path.clone());

        assert_eq!(store.get(UserId::new(1), 3), 3);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn saved_value_survives_reload() {
        let path = temp_path("roundtrip");
        let store = SpeakerStore::load(path.clone());
        store
            .set(UserId::new(42), 7)
            .await
            .expect("set must succeed");

        let reloaded = SpeakerStore::load(path.clone());
        assert_eq!(reloaded.get(UserId::new(42), 3), 7);
        // 保存していないユーザーは引き続きdefaultへフォールバックする。
        assert_eq!(reloaded.get(UserId::new(99), 3), 3);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_starts_empty_without_warning() {
        let path = temp_path("missing");
        // 親ディレクトリ自体を作らず、ファイルが存在しない状態から読む。
        let store = SpeakerStore::load(path.clone());
        assert_eq!(store.get(UserId::new(1), 5), 5);
    }

    #[test]
    fn corrupted_file_starts_empty() {
        let path = temp_path("corrupted");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "not json").unwrap();

        let store = SpeakerStore::load(path.clone());
        assert_eq!(store.get(UserId::new(1), 5), 5);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}

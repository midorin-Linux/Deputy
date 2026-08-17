use anyhow::{Context, Result};
use config::{Config as ConfigBuilder, File};
pub use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::EnvFilter;

use crate::core::{config::SETTINGS_FILE, secret_key::TruncatingEventFormat};

/// ログの出力先ディレクトリ。プロセスのカレントディレクトリ基準。
/// `.gitignore`の`/logs`と対応させるため、リポジトリ相対で解決させる。
const LOG_DIR: &str = "logs";

/// 既定のログレベル。`env.log_level`が未設定・解釈不能なときのフォールバック。
const DEFAULT_LOG_LEVEL: &str = "info";

fn read_log_level() -> String {
    ConfigBuilder::builder()
        .add_source(
            File::from(std::path::PathBuf::from(SETTINGS_FILE))
                .format(config::FileFormat::Yaml)
                .required(false),
        )
        .build()
        .ok()
        .and_then(|config| config.get_string("env.log_level").ok())
        .unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string())
}

/// `env.log_level`として単体で書ける値。`settings.example.yml`の記述と対応する
/// （`off`は意図的に全ログを止めたい場合のために許可する）。
const VALID_LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

/// `env.log_level`を`EnvFilter`へ変換する。
///
/// 素直に`EnvFilter::new`へ渡すと、打ち間違いが「ログが一切出ない」状態に化ける。
/// `EnvFilter`の構文では裸の単語は*ターゲット名*として解釈されるため、`inof`は
/// 「`inof`というターゲットをtraceで有効化」という妥当な指定として通ってしまい、
/// 結果として`deputy`のログだけが黙って消える。
///
/// そこで、単純なレベル名（`info`など）として書かれている場合は綴りを検証し、
/// `deputy=debug,serenity=warn`のような複合指定のみ`parse`へ委ねる。
/// 解釈できなければ既定値へ落としてstderrで知らせる
/// （この時点ではsubscriber未設定なので`tracing`では通知できない）。
fn build_env_filter(directives: &str) -> EnvFilter {
    let trimmed = directives.trim();

    // 空文字は`parse`が「指定なし＝全部無効」として受理してしまうので、未設定と同じ扱いにする。
    if trimmed.is_empty() {
        return EnvFilter::new(DEFAULT_LOG_LEVEL);
    }

    let is_compound = trimmed.contains(['=', ',']);
    if !is_compound
        && !VALID_LEVELS
            .iter()
            .any(|level| level.eq_ignore_ascii_case(trimmed))
    {
        eprintln!(
            "  warning: unknown env.log_level {trimmed:?}; falling back to {DEFAULT_LOG_LEVEL:?} \
             (expected one of {VALID_LEVELS:?})"
        );
        return EnvFilter::new(DEFAULT_LOG_LEVEL);
    }

    match EnvFilter::builder().parse(trimmed) {
        Ok(filter) => filter,
        Err(err) => {
            eprintln!(
                "  warning: invalid env.log_level {trimmed:?} ({err}); falling back to {DEFAULT_LOG_LEVEL:?}"
            );
            EnvFilter::new(DEFAULT_LOG_LEVEL)
        }
    }
}

pub fn init_tracing() -> Result<WorkerGuard> {
    // `create_dir_all`は既存ディレクトリに対しても`Ok`を返すため、事前の存在確認は不要。
    // 確認と作成を分けるとその隙間で状態が変わりうる分、こちらのほうが素直で安全。
    std::fs::create_dir_all(LOG_DIR)
        .with_context(|| format!("Failed to create logs directory: {LOG_DIR}"))?;

    let appender = rolling::daily(LOG_DIR, "deputy.log");
    let (non_blocking, guard) = non_blocking(appender);

    let env_filter = build_env_filter(&read_log_level());

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(env_filter)
        .with_ansi(false)
        .event_format(TruncatingEventFormat)
        .init();

    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_level_is_accepted() {
        for level in VALID_LEVELS {
            assert_eq!(build_env_filter(level).to_string(), level);
        }
        // 前後の空白や大文字表記も受け付ける。
        assert_eq!(build_env_filter("  DEBUG  ").to_string(), "debug");
    }

    #[test]
    fn compound_directives_are_passed_through() {
        // 複合指定は`EnvFilter`へ委ねる（出力順は保証されないので個別に確認する）。
        let filter = build_env_filter("deputy=debug,serenity=warn").to_string();
        assert!(filter.contains("deputy=debug"), "got {filter:?}");
        assert!(filter.contains("serenity=warn"), "got {filter:?}");
    }

    #[test]
    fn misspelled_level_falls_back_to_default_instead_of_silencing_logs() {
        // `EnvFilter`は裸の単語をターゲット名として受理するため、`parse`は成功してしまう。
        // 素通しすると`deputy`のログが黙って全部消えるので、既定値へ落ちることを保証する。
        assert_eq!(build_env_filter("inof").to_string(), DEFAULT_LOG_LEVEL);
        assert_eq!(build_env_filter("verbose").to_string(), DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn empty_level_falls_back_to_default_instead_of_silencing_logs() {
        // `parse("")`は「指定なし＝全部無効」として成功してしまうため、明示的に弾く。
        assert_eq!(build_env_filter("").to_string(), DEFAULT_LOG_LEVEL);
        assert_eq!(build_env_filter("   ").to_string(), DEFAULT_LOG_LEVEL);
    }
}

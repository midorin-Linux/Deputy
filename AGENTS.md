# AGENTS.md

個人用DiscordボットアプリケーションDeputy（Rust, edition 2024）。`serenity`でGatewayに接続し、
入退出ログ（member_log）・VC入退出ログ（voice_log）・ハニーポットによるスパム自動BAN（honeypot）を提供する。
AI連携（`AiConfig`/`async-openai`）はhoneypotのスパム判定で使用する。

## よく使うコマンド
- ビルド: `cargo build`
- 実行: `cargo run`（カレントディレクトリに`settings.yml`が必要。無ければ`settings.example.yml`をコピー）
- テスト: `cargo test --workspace`
- フォーマット: `just fmt`（内部で`cargo +nightly fmt --all`。nightly toolchainが必須）
    - フォーマットのみ`+nightly`、他はstableでよい（CIの`fmt`/`clippy`/`test`ジョブが分かれている）
- lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- CIは`.github/workflows/ci.yml`：fmt(nightly) / clippy(stable) / test(stable) / gitleaks・cargo audit(security)

## アーキテクチャ
- `src/main.rs`: エントリーポイント。tracing初期化 → `Config::load()` → `features::all()` でFeature一覧構築 →
  `Registry`をEventHandlerとしてserenity Clientに登録 → 起動。
- `src/core/`: 「仕組み」だけを持ち、機能の中身を一切知らない層。
    - `feature.rs`: `Feature`トレイト（`on_event` / `on_command` / `priority` / `commands`）と`Flow`（Continue/Consume）。
    - `registry.rs`: 唯一の`EventHandler`実装。`FullEvent`をpriority降順で各Featureへ配送し、
      エラーはfeature単位でログして次へ進む（bulkhead、1機能の失敗が他へ波及しない）。
      **新しい`FullEvent`種別を扱うfeatureを追加したら、対応する`EventHandler`メソッドをここへ実装すること**
      （未実装だと既定実装が黙ってイベントを握り潰す）。
    - `commands.rs`: スラッシュコマンド名→feature indexのルーティング表を構築。
    - `config.rs`: 全体設定のロード（`settings.yml`固定パス）。`features.<name>`は`serde_json::Value`のまま
      保持し、各`features/<name>/config.rs`が`load_feature_config()`経由で自分のセクションだけを取り出す。
      `enabled=false`または未設定なら`None`（feature自体を作らない）。有効なのに`log_channel=0`は起動時エラー
      （`ChannelId::new(0)`が実行時にpanicするのを前倒しで防ぐため）。
    - `secret_key.rs`: `SecretKey`（トークン等）。`Debug`は完全にマスクし長さも漏らさない。
      tracing出力は`TruncatingEventFormat`で各フィールド値を100文字に切り詰める（秘密のマスクではなく長文防止）。
    - `store.rs`: `LogStore`トレイト（永続化ポート、現状`NoopStore`のみ）。`record_or_warn()`で失敗は
      warnログに留め、Discord通知の成否とは独立させる。
    - `discord/embed.rs`: 共通Embedビルダ（`log_embed`=青, `warn_embed`=橙）。
    - `telemetry.rs`: tracing初期化。ログは`logs/`（カレントディレクトリ基準）に日次ローテーション、
      14日保持。`env.log_level`のパース失敗時は黙って全ログ消失させず`info`にフォールバックする。
- `src/features/<name>/`: 機能の縦切り単位（vertical slice）。`mod.rs`（Feature実装）/`config.rs`（設定）/
  `events.rs`等。新機能追加は`src/features/mod.rs::all()`に1行足すだけで完結させる設計。
- `src/features/honeypot/`: 監視チャンネルへの投稿をルール（招待リンク/ロールメンション/メンション数）と
  LLMで判定し、スパムをBANする。`priority()`は100（Message系で最優先）、処分したメッセージは`Flow::Consume`で
  後続へ流さない。システムプロンプトは`PROMPT.md`（`settings.yml`と同じくカレントディレクトリ基準）。
  純ロジック（`rules.rs`/`verdict.rs`/`events.rs`/`dedup.rs`/`action.rs`）はDiscord接続なしでテストできる。
- 依存方向: `main → features → core`。`core`は`features`をimportしない。

## コード規約
- コミットメッセージ・コメント・ログ文言（Discord通知文含む）は日本語。コード識別子は英語。
- エラーは`anyhow::Result`+`.context()`が基本、ドメイン固有エラーのみ`thiserror`（`core/error.rs`の`ConfigError`）。
- 各モジュールに`#[cfg(test)] mod tests`でユニットテストを併設するのが標準（core配下はほぼ全てにある）。
- 設定の`enabled`/`log_channel`という形の共通ルールは`FeatureToggle`トレイト+`load_feature_config()`に
  一本化されている。新しいfeature設定を追加する際もこれに乗ること（重複実装しない）。

## 注意点
- `settings.yml`の読み込み先・`logs/`の出力先は実行ファイルの場所ではなく**プロセスのカレントディレクトリ**。
  `cargo run`はリポジトリルート、ビルド済みバイナリは`target/<profile>/`で実行する想定（README参照）。
- `settings.yml`と`logs/`は`.gitignore`済み。トークンを含むためコミットしない。
- Discordの`GUILD_MEMBERS`は特権インテント。Developer Portalで有効化しないとゲートウェイが
  close code 4014で切断され再試行を繰り返す（トークン自体は正しいまま失敗する）。
- 現在の実装は単一サーバー運用前提。`log_channel`はギルドごとではなくボット全体で1つ。
- `MESSAGE_CONTENT`も特権インテント。`GUILD_MEMBERS`同様、未有効化なら4014で切断される。
  honeypotが無効でもインテントは常に要求するため、有効化は必須。
- `Cargo.lock`は`.gitignore`済み（バイナリだが意図的に追跡しない）。

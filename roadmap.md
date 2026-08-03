# Discord Bot アーキテクチャ再設計（v2 / 単一クレート版）

保守性・機能追加のしやすさ・責任の分散を最優先にしつつ、構成は **単一クレート**、`src/main.rs` をエントリーポイントとする設計です。

---

## 1. 旧設計の課題整理

旧構成（`handler/` + `features/` + `commands/` + `services/`）は「層で切る」水平分割でした。動きはしますが、成長すると次の問題が出ます。

1. **1機能が3ディレクトリに散る** — TTS を触るとき `handler/message.rs`、`features/tts/`、`commands/tts.rs` を行き来する。機能追加のたびに横断修正が必要
2. **`handler/message.rs` が神クラス化する** — honeypot → tts → agent の分岐が中央に集中し、機能を増やすほどここが肥大化・壊れやすくなる
3. **グローバル共有状態への依存** — `TypeMapKey` の巨大な共有マップに全機能の状態が同居し、機能間の暗黙的な結合が生まれる
4. **機能の無効化・差し替えが構造に現れない** — 「この機能だけ止める」がコードのコメントアウトになりがち

v2 では **機能単位の縦切り（vertical slice）+ プラグイン機構** に切り替え、これらを構造レベルで解決します。

---

## 2. 設計原則

1. **1機能 = 1ディレクトリ = 1登録** — イベント処理・コマンド・設定・状態を機能ディレクトリに閉じ込める。機能追加時に触るのは「新ディレクトリの作成」と「登録リストへの1行追加」のみ
2. **中央は機構だけを持つ** — ディスパッチ・優先度・エラー隔離という「仕組み」は `core` モジュールが持ち、「何をするか」は一切知らない
3. **依存方向は一方向** — `main → features → core / services`。`services` は serenity を import しない、`core` は `features` を import しない、を規約として守る（§3.1）
4. **状態は各機能が私有する** — グローバル TypeMap を廃止。各 Feature 構造体が自分の `Arc<...>` / `DashMap` を内部に持つ
5. **機能間はイベントバス経由** — 機能同士の直接呼び出しを禁止し、疎結合な pub/sub で連携する
6. **失敗は機能単位で隔離（bulkhead）** — 1機能のエラーが他機能とボット本体を巻き込まない

---

## 3. ディレクトリ構造

```
discord-bot/
├── Cargo.toml
├── .env                        # DISCORD_TOKEN, OPENROUTER_API_KEY, VOICEVOX_URL
├── config.toml                 # 運用設定（機能ごとのセクション）
│
└── src/
    ├── main.rs                 # エントリーポイント：設定読込 → services 構築 → features::all() → Registry → Client 起動
    │
    ├── core/                   # 基盤機構（機能の中身を知らない）
    │   ├── mod.rs
    │   ├── feature.rs          # Feature trait / Flow
    │   ├── registry.rs         # 機能の登録・イベント配送・優先度・エラー隔離（唯一の EventHandler）
    │   ├── commands.rs         # スラッシュコマンドの一括登録と name→feature ルーティング
    │   ├── config.rs           # config.toml ローダ。[features.*] を各機能へ配布
    │   ├── bus.rs              # 機能間イベントバス（tokio::sync::broadcast）
    │   ├── store.rs            # 永続化ポート（trait LogStore）+ NoopStore
    │   ├── error.rs            # 共通エラー型（thiserror）
    │   └── discord/
    │       ├── mod.rs
    │       └── embed.rs        # ログ用 Embed の共通ビルダ（新規垢警告色など）
    │
    ├── services/               # 外部APIクライアント（serenity 非依存を規約とする）
    │   ├── mod.rs              # pub struct Services { voicevox, openrouter } の束ね
    │   ├── voicevox/
    │   │   ├── mod.rs          # VoicevoxClient: audio_query → synthesis → WAV bytes
    │   │   └── types.rs
    │   └── openrouter/
    │       ├── mod.rs          # OpenRouterClient: async-openai の api_base 差し替えを内包
    │       └── types.rs
    │
    └── features/               # 各機能 = 縦切りモジュール
        ├── mod.rs              # pub fn all(cfg, svc) -> Vec<Box<dyn Feature>>  ← 登録の唯一の場所
        │
        ├── member_log/         # ギルド入退出ログ
        │   ├── mod.rs          # Feature 実装
        │   ├── config.rs       # ログチャンネルID、新規垢しきい値
        │   └── events.rs       # GuildMemberAddition / Removal
        │
        ├── voice_log/          # VC入退出ログ
        │   ├── mod.rs
        │   ├── config.rs
        │   ├── events.rs       # VoiceStateUpdate
        │   └── diff.rs         # 旧/新 VoiceState の差分判定（純関数・単体テスト対象）
        │
        ├── honeypot/
        │   ├── mod.rs          # priority 最高。検知時 Flow::Consume を返す
        │   ├── config.rs       # 対象チャンネル、処分種別、除外ロール
        │   ├── events.rs
        │   └── action.rs       # Ban / Kick / Timeout の実行と管理ログ通報
        │
        ├── tts/
        │   ├── mod.rs
        │   ├── config.rs       # 対象チャンネル、既定話者、文字数上限
        │   ├── commands.rs     # /join /leave /speaker /skip
        │   ├── events.rs       # Message 検知、VC 無人時の自動退出
        │   ├── sanitize.rs     # URL省略・メンション展開など（純関数・単体テスト対象）
        │   ├── queue.rs        # Guild 単位 mpsc + 直列ワーカー
        │   └── playback.rs     # songbird への WAV 供給
        │
        └── agent/
            ├── mod.rs
            ├── config.rs       # モデル名、履歴上限、クールダウン秒
            ├── commands.rs     # /ask /reset
            ├── events.rs       # メンション契機
            ├── history.rs      # チャンネル単位の短期履歴（トリム）
            └── ratelimit.rs    # ユーザー単位クールダウン
```

### 3.1 依存方向の守り方（単一クレートでの規約）

```
main.rs ──▶ features ──▶ core
                  │
                  └────▶ services   （services は serenity を使わない）
```

単一クレートではコンパイラによる強制ができないため、次の軽い仕組みで規約を維持します。

- **`core/*.rs` に `use crate::features` を書かない / `services/*.rs` に `use serenity` を書かない** を自分ルールとして固定
- 気になる場合は `grep -r "use serenity" src/services/` を CI やコミット前フックに 1 行入れるだけで機械チェックになる
- モジュール境界は将来 workspace 分割する場合の crate 境界とそのまま一致しているため、必要になった時点での分割は機械的な移動だけで済む

### 3.2 main.rs の役割（配線のみ）

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let cfg = core::config::load("config.toml")?;          // 設定読込
    let svc = services::Services::init(&cfg)?;              // 外部APIクライアント構築
    let bus = core::bus::Bus::new();                        // 機能間バス
    let feats = features::all(&cfg, svc, bus.clone());      // 機能の組み立て
    let registry = core::registry::Registry::new(feats, bus);

    let mut client = serenity::Client::builder(&cfg.discord_token, intents())
        .event_handler(registry)
        .register_songbird()
        .await?;
    client.start().await?;
    Ok(())
}
```

ロジックは一切置かず、「読み込む・組み立てる・起動する」の 3 手順だけに保ちます。

---

## 4. 中核機構：Feature trait と Registry

### 4.1 Feature trait（core/feature.rs）

```rust
use serenity::all::{Context, FullEvent, CommandInteraction, CreateCommand};

/// イベントを消費したか、後続の機能へ流すか
pub enum Flow {
    Continue,
    Consume,   // 例: honeypot が検知したら tts / agent へは流さない
}

#[async_trait::async_trait]
pub trait Feature: Send + Sync {
    fn name(&self) -> &'static str;

    /// Message 系イベントの処理順。大きいほど先。既定 0
    fn priority(&self) -> i32 { 0 }

    /// この機能が提供するスラッシュコマンド定義
    fn commands(&self) -> Vec<CreateCommand> { vec![] }

    /// Gateway イベントの処理
    async fn on_event(&self, ctx: &Context, ev: &FullEvent) -> anyhow::Result<Flow>;

    /// 自機能のコマンドが呼ばれたときの処理
    async fn on_command(&self, ctx: &Context, ic: &CommandInteraction) -> anyhow::Result<()>;
}
```

### 4.2 Registry（core/registry.rs）

serenity の `EventHandler` を実装するのは **Registry ただ1つ**。役割は3つだけです。

1. **配送**: 受け取った `FullEvent` を `priority` 降順で各 Feature の `on_event` に渡す。`Flow::Consume` が返ったら打ち切り
   - 旧設計の「message.rs 内の if 分岐」は、`honeypot.priority() = 100`、`tts = 50`、`agent = 10` という宣言に置き換わる。**優先順位が各機能の自己申告になり、中央のコードを編集せずに済む**
2. **コマンドルーティング**: 起動時に `commands()` を集約して Discord に一括登録し、`コマンド名 → Feature` のマップを構築。`InteractionCreate` は該当機能の `on_command` にだけ渡す
3. **エラー隔離（bulkhead）**: 各 `on_event` 呼び出しを個別に `match` し、`Err` は `tracing::error!(feature = f.name(), ...)` でログして**次の機能へ進む**。1機能の障害でイベントループ全体が止まらない

### 4.3 登録（features/mod.rs — 機能追加時に触る唯一の中央ファイル）

```rust
pub fn all(cfg: &AppConfig, svc: Services, bus: Bus) -> Vec<Box<dyn Feature>> {
    let mut v: Vec<Box<dyn Feature>> = Vec::new();
    if cfg.features.member_log.enabled { v.push(Box::new(member_log::MemberLog::new(&cfg.features.member_log))); }
    if cfg.features.voice_log.enabled  { v.push(Box::new(voice_log::VoiceLog::new(&cfg.features.voice_log))); }
    if cfg.features.honeypot.enabled   { v.push(Box::new(honeypot::Honeypot::new(&cfg.features.honeypot))); }
    if cfg.features.tts.enabled        { v.push(Box::new(tts::Tts::new(&cfg.features.tts, svc.voicevox.clone(), bus.clone()))); }
    if cfg.features.agent.enabled      { v.push(Box::new(agent::Agent::new(&cfg.features.agent, svc.openrouter.clone(), bus.clone()))); }
    v
}
```

**機能の有効/無効が設定ファイルで完結**し、コードのコメントアウトが不要になります。

---

## 5. 設定：機能ごとのセクション所有

`config.toml` は機能名でネームスペースを切り、**各機能が自分の Config 構造体を `features/<name>/config.rs` に定義**します。`core/config.rs` は「読み込んで配るだけ」で、中身を知りません。

```toml
[features.member_log]
enabled = true
log_channel = 123456789
new_account_warn_days = 7

[features.honeypot]
enabled = true
channel = 123456789
action = "timeout"          # "ban" | "kick" | "timeout"
exempt_roles = [111, 222]

[features.tts]
enabled = true
default_speaker = 3
max_chars = 120

[features.agent]
enabled = true
model = "google/gemini-flash-1.5"
history_limit = 20
cooldown_secs = 15
speak_reply = false          # true で応答を TTS にも流す（§6）
```

新機能の設定追加で既存機能のパースが壊れることはありません（セクション独立 + `serde(default)`）。

---

## 6. 機能間連携：イベントバス

「エージェントの応答を VOICEVOX で読み上げる」（旧・検討事項）のような**機能をまたぐ連携**は、直接呼び出しではなく core のバスで行います。

```rust
// core/bus.rs
#[derive(Clone, Debug)]
pub enum BotEvent {
    AgentReplied { guild_id: GuildId, text: String },
    MemberPunished { guild_id: GuildId, user_id: UserId, reason: String },
    // 追加はここに列挙
}
// 実体は tokio::sync::broadcast::Sender<BotEvent>
```

- `agent` は応答後に `AgentReplied` を publish するだけ。**tts の存在を知らない**
- `tts` は `speak_reply = true` のとき subscribe してキューに積む
- 将来「処分を Webhook で外部通知する機能」を作るときも、`MemberPunished` を購読する新機能を置くだけで honeypot は無改造

---

## 7. 状態管理：TypeMap の廃止

旧設計の `Arc<RwLock<HashMap<GuildId, _>>>` を TypeMapKey で共有する方式をやめ、**状態は各 Feature 構造体のフィールド**にします。

```rust
pub struct Tts {
    cfg: TtsConfig,
    voicevox: VoicevoxClient,
    queues: DashMap<GuildId, mpsc::Sender<TtsJob>>,   // Tts の私有物
}
```

- 状態のスコープ = 機能のスコープになり、「この Mutex 誰が触ってる？」問題が消える
- Guild 単位のマップには `dashmap` を推奨（`RwLock<HashMap>` のロック粒度問題を回避）
- どうしても複数機能で共有したいもの（例: songbird の Call ハンドル）だけを core 管轄に置く

---

## 8. 永続化ポート（旧・検討事項への回答）

ログ永続化は**やるかどうか未定のまま構造だけ先に用意**できます。

```rust
// core/store.rs
#[async_trait::async_trait]
pub trait LogStore: Send + Sync {
    async fn record(&self, entry: LogEntry) -> anyhow::Result<()>;
}
pub struct NoopStore;          // 既定：何もしない
// 後日 core/store/sqlite.rs を追加して差し替え
```

機能側は `Arc<dyn LogStore>` を受け取るだけなので、SQLite 導入時に **member_log / voice_log / honeypot のコードは無変更**です。

---

## 9. テスト戦略（構造が可能にするもの）

| 対象 | 方法 | 場所 |
|---|---|---|
| VoiceState 差分判定 | 純関数の単体テスト | `features/voice_log/diff.rs` 内 `#[cfg(test)]` |
| 読み上げ整形 | 純関数の単体テスト | `features/tts/sanitize.rs` 内 `#[cfg(test)]` |
| 履歴トリム / クールダウン | 単体テスト | `features/agent/history.rs` `ratelimit.rs` |
| VOICEVOX / OpenRouter | serenity 非依存のまま結合テスト（要ローカルENGINE） | `tests/services_*.rs` |
| ディスパッチ・優先度・隔離 | モック Feature を Registry に登録して検証 | `tests/registry.rs` |

Discord 実サーバーが必要なのは最終結合のみで、ロジックの大半は `cargo test` で回ります。

---

## 10. 新機能を追加する手順（この設計の存在意義）

例:「リアクションロール」機能を足す場合。

1. `src/features/reaction_role/` を作成（`mod.rs` / `config.rs` / `events.rs`）
2. `Feature` trait を実装
3. `features/mod.rs` の `all()` に 1 行追加（+ `mod reaction_role;` 宣言）
4. `config.toml` に `[features.reaction_role]` セクションを追記

**既存機能のファイルには一切触れません。** 逆に機能を捨てるときもディレクトリ削除 + 宣言と登録の削除で完了します。

---

## 11. 開発の進め方（v2 版）

1. **骨組み**: `main.rs` + `core` の `Feature` / `Registry` / 設定ローダ → 空の Feature 1 個で `Ready` 疎通
2. **member_log**: 最小の Feature 実装例になる。`core/discord/embed.rs` の共通ビルダもここで作る
3. **voice_log**: `diff.rs` を純関数で書き、最初の単体テストを整備
4. **honeypot**: `priority` と `Flow::Consume` の動作確認、権限まわりの検証
5. **services/voicevox**: 単体で WAV 取得まで確認 → **tts**: playback → queue の順
6. **services/openrouter** → **agent**: 履歴・レート制限
7. **bus 連携**（`speak_reply`）、`store.rs` の実装判断、運用調整

---

## 12. 残る検討事項

- 読み上げ対象チャンネルの決め方（VC 紐付け固定か `/bind` コマンドでの任意指定か）— どちらでも `tts/config.rs` + `commands.rs` 内で完結
- ハニーポット処分の既定値 — 誤爆リスクを考えると初期値は `timeout` が安全
- 特権 Intents（`GUILD_MEMBERS` / `MESSAGE_CONTENT`）と Bot 権限は旧設計の一覧をそのまま踏襲

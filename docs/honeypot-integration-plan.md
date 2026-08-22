# Honeypot機能 統合計画（Deputy側）

`midorin-Linux/honeypot` v0.3.2 を、Deputyの`features/honeypot/`として統合するための実装計画。
元の「Honeypot 移植計画書」を、Deputyの実際のアーキテクチャ（`core`の`Feature`/`Registry`/
`load_feature_config`）に落とし込んだもの。

- 対象: `src/features/honeypot/`（新規）+ `core`側の最小限の拡張
- 前提: 単一ギルド運用。ギルド別設定の永続化（SQLite / `/config`）は移植しない

---

## 1. 現状の差分整理

| 観点 | honeypot（移植元） | Deputy（移植先） | 統合時の扱い |
|---|---|---|---|
| イベント処理 | `EventHandler`直実装（`message`） | `Registry`が唯一の`EventHandler`。`Message`は未転送 | **`Registry`に`message`メソッドを追加**（後述2.1） |
| 設定 | `app.*`をトップレベルに持つ | `features.<name>`セクション + `FeatureToggle` | `features.honeypot`へ再編（後述3） |
| 判定設定の出所 | SQLite `guild_configs` → YAMLフォールバック | なし | YAML単一系統へ統合。DB層は移植しない |
| コマンド | `guild_create`でギルド登録 | `Registry`が`ready`でグローバル一括登録 | Deputyの機構に一本化。`guild_create`ループは不要 |
| Intents | `GUILDS`/`GUILD_MESSAGES`/`MESSAGE_CONTENT` | `GUILDS`/`GUILD_MEMBERS`/`GUILD_VOICE_STATES` | **`GUILD_MESSAGES`と`MESSAGE_CONTENT`を追加**（特権） |
| AIクライアント | `Agent`を`main`で構築し`Handler`が保持 | `AiConfig`は存在するがクライアント未構築 | `features::all()`内でhoneypotが自前に構築（`AiConfig`は`core`から受け取る） |
| 秘密情報 | `SecretKey`（config.rs内に同居） | `core/secret_key.rs`に分離済み | Deputy側をそのまま使う。移植不要 |
| ロギング | `telemetry.rs`（日次ローテ・フィールド切り詰め） | 同等のものが`core/telemetry.rs`に存在 | Deputy側をそのまま使う。移植不要 |
| 永続化 | なし（DBは設定用のみ） | `LogStore`ポート（`NoopStore`） | BAN記録は`record_or_warn()`経由で流す |

移植で実際に「コードを持ち込む」のは **`moderation/`（ルール・パイプライン）と `agent/`（LLM判定）と `PROMPT.md`** のみ。
それ以外（設定・ロギング・秘密情報・コマンド登録・エラー隔離）はDeputyの既存機構に載せ替える。

---

## 2. `core`側に必要な変更

honeypotはDeputy初の「Messageイベントを扱うfeature」であり、`core`にも最小限の追加が要る。

### 2.1 `core/registry.rs`: `message`の転送（必須）

`Registry`は明示的に実装した`EventHandler`メソッドしかFeatureへ届けない。現状`message`が
未実装のため、このままではhoneypotの`on_event`は永遠に呼ばれない（CLAUDE.mdの注意点そのもの）。

```rust
async fn message(&self, ctx: Context, new_message: Message) {
    self.dispatch(&ctx, FullEvent::Message { new_message }).await;
}
```

### 2.2 `main.rs`: Intentsの追加（必須）

```rust
let intents = GatewayIntents::GUILDS
    | GatewayIntents::GUILD_MEMBERS
    | GatewayIntents::GUILD_VOICE_STATES
    | GatewayIntents::GUILD_MESSAGES      // 追加
    | GatewayIntents::MESSAGE_CONTENT;    // 追加（特権）
```

`MESSAGE_CONTENT`は特権インテント。Developer Portalで有効化しないと`msg.content`が常に空になり、
招待リンク検知もLLM判定も無言で機能しなくなる（`GUILD_MEMBERS`未有効時の4014と違い、
**接続は成功したまま判定だけが壊れる**ため気付きにくい）。READMEのインテント表にも追記する。

なお、honeypotを`enabled: false`にしていても特権インテントは常時要求される形になる。
インテントを機能側から申告させる仕組み（`Feature::intents()`）はスコープ外とし、
READMEに「honeypotを使わない場合もMESSAGE_CONTENTの有効化が必要」と明記する方針とする。

### 2.3 `core/config.rs`: `FeatureToggle`の扱い（要判断）

`load_feature_config()`は`enabled` + `log_channel != 0`を共通ルールとして強制する。
honeypotの通知先（管理者向けBAN通知チャンネル）を`log_channel`という同じ名前で持たせれば、
**このトレイトにそのまま乗れる**ので`core`の変更は不要。監視対象は別名`honeypot_channels`とする。

→ 方針: `log_channel`（通知先）は必須、`honeypot_channels`（監視対象）は`load`後に
`config.rs`側で追加検証（空配列・0を含む場合はエラー）する。

### 2.4 `Cargo.toml`

すでに`async-openai` / `backoff` / `base64` / `image` / `rand`は宣言済み（未使用）。
honeypot統合で全て実際に使われるようになる。`sqlx`はDB層を移植しないため**削除する**
（現状も未使用のまま残っている）。

### 2.5 `build.rs`

`PROMPT.md`を`target/<profile>/`へコピーする処理を追加する（honeypotの`build.rs`と同様）。
`settings.yml`と同じく**カレントディレクトリ基準**で読まれるため、READMEの実行手順の前提も同じ。

---

## 3. 設定モデル

`settings.example.yml`へ追加するセクション。

```yaml
features:
  honeypot:
    enabled: true

    # BAN実行・判定失敗を通知する管理者向けチャンネル（core共通ルールにより必須）
    log_channel: 123456789012345678

    # 監視対象のハニーポットチャンネルID（複数可）
    honeypot_channels: [123456789012345678]

    # 実BANせず判定のみ行う検証モード
    debug_mode: false

    # 投稿即BANの緊急モード。他の条件より優先される
    unconditional_ban: false

    # AI判定を行うか。falseならban_triggerのみで判定
    enable_ai_judgment: true

    # BAN時にさかのぼって削除するメッセージの日数（0〜7）
    delete_message_days: 3

    # 判定から除外するロール（管理者・モデレータの事故防止）
    exempt_roles: []

    # 高速フィルタ（AI呼び出し前）
    ban_trigger:
      has_invite_link: true
      has_role_mention: true
      mention_threshold: 3   # 0でオフ

    # AI判定が失敗したときの方針: skip | rules_only | notify
    ai_failure_policy: notify
```

移植元との対応:
- `env.debug_mode` → `features.honeypot.debug_mode`（Bot全体ではなく機能単位のスイッチに降格。
  他機能の挙動に影響させないため）
- `env.database_url` → **廃止**
- `app.*` → `features.honeypot.*`
- `ai.*` → Deputyの`ai`（トップレベル）をそのまま使う。honeypotは`&AiConfig`を受け取る
- `exempt_roles` / `ai_failure_policy` は新規（4章）

起動時検証（`config.rs`の`load`内で`load_feature_config`のあとに実施）:
- `honeypot_channels`が空、または0を含む → エラー
- `delete_message_days > 7` → エラー
- `enable_ai_judgment`が真で`ai.base_url`が空、または`ai.request_timeout_secs == 0` → エラー

---

## 4. ディレクトリ構成と責務

```
src/features/honeypot/
├── mod.rs        # Feature実装。priorityは最高（100）。BAN実行時はFlow::Consume
├── config.rs     # HoneypotConfig / BanTriggerConfig / FeatureToggle実装 + 追加検証
├── events.rs     # メッセージ→判定対象かの絞り込み（純関数：bot判定・チャンネル一致・除外ロール）
├── rules.rs      # 招待リンク / ロールメンション / メンション数（純関数・テスト対象）
├── verdict.rs    # Verdict { is_spam, reason } と判定チェーン determine_verdict()
├── action.rs     # BAN実行・理由の切り詰め・重複防止・救済リプライ・管理チャンネル通知
├── agent.rs      # LLM判定（async-openai、指数バックオフ、タイムアウト、JSON抽出）
├── image.rs      # 添付画像のDL・サイズ上限・縮小/JPEG再エンコード
└── dedup.rs      # 重複BAN防止キャッシュ（TTL付き・上限付き）
```

移植元とのファイル対応:

| 移植元 | 移植先 | 変更点 |
|---|---|---|
| `moderation/rules/{invite_link,mention}.rs` | `rules.rs` | ロジックはそのまま。**テストを追加**（移植元に無い） |
| `moderation/rules/mod.rs::determine_ban_reason` | `verdict.rs` | 引数から`BanTriggerSettings`(DBモデル)を廃し、`&HoneypotConfig`のみ参照 |
| `moderation/pipeline.rs` | `agent.rs` + `image.rs` | 画像前処理を分離。`MAX_BAN_REASON_LEN`は`action.rs`へ |
| `agent/mod.rs` | `agent.rs` | `Config`全体ではなく`&AiConfig`を受け取る。ヘッダのRefererはDeputyのURLへ |
| `agent/prompt.rs` + `PROMPT.md` | 同左（`PROMPT.md`はリポジトリルート） | そのまま移植。判定基準・出力形式は変更しない |
| `discord/handler.rs::message` | `mod.rs` + `events.rs` + `action.rs` | DBフォールバック3分岐を削除。早期returnを`events.rs`の純関数へ切り出す |
| `discord/handler.rs::banned_users` | `dedup.rs` | 無制限の`HashSet`をTTL/上限付きへ（4.2） |
| `db/`, `migrations/`, `discord/commands/config.rs` | — | **移植しない** |
| `telemetry.rs`, `SecretKey` | — | Deputy側の既存実装を使う |

### 4.1 判定チェーン（`verdict.rs`）

移植元のフォールチェーンを維持する。設定の出所がYAML単一になるだけで順序は不変。

```
unconditional_ban
  → has_invite_link && ban_trigger.has_invite_link
  → has_role_mention && ban_trigger.has_role_mention
  → mention_threshold != 0 && mentions >= threshold
  → enable_ai_judgment なら LLM判定
  → いずれにも当たらなければ非スパム
```

`determine_verdict()`は`&Message`ではなく判定に必要な値（本文・メンション数・ロールメンション有無）を
受け取る形にしておくと、`Message`を組み立てずにチェーン全体をユニットテストできる。

### 4.2 重複BAN防止（`dedup.rs`）

移植元は`Mutex<HashSet<UserId>>`で単調増加する。長期稼働するDeputyに載せる以上、
`(UserId, Instant)`を保持し、挿入時に期限切れエントリを掃除する方式へ変更する。

- TTL: 既定10分（連投スパムの重複判定を潰すには十分）
- 上限: 既定1024件。超過時は最古から破棄
- 「挿入と判定をロック内でアトミックに行う」性質は維持する（並行到達時に実BANは1回だけ）

### 4.3 Flow

BANを実行した（または`debug_mode`で実行したことにした）場合のみ`Flow::Consume`を返す。
判定が非スパムなら`Flow::Continue`。将来tts/agentがMessageを扱うようになったとき、
処分対象の投稿を他機能へ流さないための取り決め（roadmap.md §4.1の意図に一致）。

---

## 5. 追加実装するもの

元計画書4章のうち、統合と同時に入れるべきものを優先度順に整理した。

| 優先度 | 項目 | 内容 |
|---|---|---|
| 高 | **除外ロール（`exempt_roles`）** | 元計画書には無いがroadmap.mdの`honeypot/config.rs`に記載あり。管理者を誤BANする事故を構造的に防ぐため最初から入れる |
| 高 | **管理チャンネルへのBAN通知** | `log_channel`へ`warn_embed`で「対象ユーザー・判定理由・判定経路（rule/LLM）」を通知。`core/discord/embed.rs`をそのまま使う |
| 高 | **AI判定失敗時の方針（`ai_failure_policy`）** | 移植元は`error!`ログのみで見逃す。`skip`（従来）/`rules_only`（ルール結果で確定）/`notify`（skip + 管理チャンネルへ警告）から選択。既定は`notify` |
| 高 | **ルール関数のユニットテスト** | 移植元に無い。`rules.rs` / `verdict.rs` / `dedup.rs` / `events.rs`に`#[cfg(test)] mod tests`を併設（Deputyの標準） |
| 中 | **`/honeypot` コマンド（unban導線）** | 直近N件のBAN（対象・理由・時刻）を表示し、その場でunbanする。誤BAN対応手段が現状ゼロなので価値が高い。`Feature::commands()`に載せるだけでよい |
| 中 | **`LogStore`への記録** | BAN実行を`LogEntry::new("honeypot", guild_id, reason).with_user(...)`で`record_or_warn()`。将来SQLite実装を入れた時点で自動的に永続化される |
| 低 | **実行時の閾値変更コマンド** | 元計画書の`/config-runtime`案（`Arc<RwLock<BanTriggerConfig>>`、永続化なし）。まずYAML + 再起動で運用し、必要になってから入れる |
| 低 | **統計・可観測性** | BAN件数、LLM判定のtrue/false内訳、平均レイテンシ。Deputyにステータスコマンドが出来た時点で合流させる |

「救済リプライ」（`SALVATION_REPLIES`）はBotの個性として維持する。ただし`debug_mode`では
実BANしないため、リプライも既存どおり検証用の文言に留める。

---

## 6. 実装ステップ

各ステップの終わりに`cargo clippy --workspace --all-targets --all-features -- -D warnings`と
`cargo test --workspace`が通ることを条件とする。

1. **`core`の下準備** — `Registry::message`の追加とそのテスト、`main.rs`のIntents追加、
   `Cargo.toml`から`sqlx`削除、`build.rs`の`PROMPT.md`コピー。これだけで独立してマージできる。
2. **設定の器** — `features/honeypot/config.rs`を作り、`load_feature_config`+追加検証と
   そのテストを先に書く。`settings.example.yml`とREADMEの設定項目も同時に更新。
3. **純ロジックの移植** — `rules.rs` / `verdict.rs` / `events.rs` / `dedup.rs`。
   Discordに繋がずテストできる範囲をここで固める（移植元に無いテストを補うのはこの段階）。
4. **LLM判定の移植** — `agent.rs` / `image.rs` / `PROMPT.md`。`AiConfig`から構築し、
   タイムアウト・バックオフ・JSON抽出はそのまま持ち込む。
5. **Feature実装と配線** — `mod.rs`（`on_event`）/ `action.rs`（BAN実行・通知）、
   `features/mod.rs::all()`へ1行追加。ここで初めてDiscordに繋いだ疎通確認を行う。
6. **追加機能** — 5章の「高」を実装（除外ロール・管理通知・失敗時方針は5より前倒しでも可）、
   続いて`/honeypot`のunban導線。
7. **検証・切り替え** — `debug_mode: true`でステージング運用し、判定精度と誤検知率を確認。
   問題なければ旧Honeypot Botを停止してDeputyへ一本化する。

1〜3はDiscordトークンもAPIキーも不要なので、そこまでを先にPRとして分けるとレビューしやすい。

---

## 7. リスク・留意点

- **`MESSAGE_CONTENT`の未有効化** — 接続は成功するのに`msg.content`が空になり、招待リンク検知と
  LLM判定が無言で死ぬ。起動時に「honeypotが有効なのに本文が空のメッセージを受信し続けている」
  ことを検知して警告するのは難しいため、READMEでの明示が実質唯一の防御になる。
- **`Flow::Consume`の影響範囲** — 現状Messageを扱う機能は他に無いため実害はないが、
  tts/agent追加時に「honeypotが握り潰す」前提が効いてくる。priorityは最高値で固定しておく。
- **誤BANの影響拡大** — Deputyは他機能でも同じギルドに常駐するため、誤BANはBot全体の信頼を損なう。
  `exempt_roles`とunban導線の優先度を高く置いているのはこのため。既定値は移植元どおり
  `unconditional_ban: false`を維持する。
- **AI APIの共用** — Deputyに将来agent機能が入ると`ai.*`を共有することになり、レート制限と
  コストが合算される。honeypotは低頻度想定だが、`request_timeout_secs`はhoneypot専用に
  上書きできる余地を残しておくとよい（今回はスコープ外）。
- **画像ダウンロードのメモリ** — 25MB上限・1024px縮小・JPEG再エンコードは移植元のまま維持する。
  Deputyは他機能と同一プロセスで動くため、上限を緩めないこと。
- **BAN理由の長さ** — serenityは監査ログ理由が512文字超で`ExceededLimit`。移植元の100文字
  切り詰め（文字境界尊重）をそのまま持ち込む。
- **`priority`の衝突** — `member_log` / `voice_log`は既定0。honeypotを100にしても現状の
  イベント種別は重ならないが、宣言としては最高値を明示しておく。

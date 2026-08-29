# Deputy
個人用のDiscordボットアプリケーション。管理人の代理人を務められるような機能を搭載。

## 開発環境構築
このプロジェクトでは以下の開発環境を使用してます。
- **Rust (安定版最新)**
- **rustfmt (nightly最新)**
```bash
rustup toolchain install nightly --component rustfmt
```
- **Justfile**
```bash
cargo install just 
```

## セットアップ

### 1. Discord側の準備（特権インテントの有効化）

本ボットは入退出ログのために `GUILD_MEMBERS` を、ハニーポットのメッセージ判定のために
`MESSAGE_CONTENT` を要求します。いずれも**特権インテント**のため、
[Discord Developer Portal](https://discord.com/developers/applications) での有効化が必須です。

`Bot` → `Privileged Gateway Intents` → **SERVER MEMBERS INTENT** と **MESSAGE CONTENT INTENT** を
オンにしてください。

どちらか一方でも有効化しないまま起動すると、ゲートウェイがクローズコード `4014`（Disallowed intent）で
切断され、接続の再試行を延々と繰り返します。トークンが正しくてもログインできないため、
この症状が出たらまずここを疑ってください。

要求しているインテントは以下の5つです。

| インテント | 用途 | 特権 |
| --- | --- | --- |
| `GUILDS` | ギルド情報・VoiceStateのキャッシュ | - |
| `GUILD_MEMBERS` | `member_log`（入退出ログ） | 要有効化 |
| `GUILD_VOICE_STATES` | `voice_log`（VC入退出ログ） | - |
| `GUILD_MESSAGES` | `honeypot`（メッセージ受信） | - |
| `MESSAGE_CONTENT` | `honeypot`（本文・添付の判定） | 要有効化 |

インテントは機能の有効/無効に関わらず常に要求します。そのため `honeypot` を使わない場合でも
**MESSAGE CONTENT INTENT の有効化が必要**です（有効化しないと上記の `4014` で起動できません）。

なお `MESSAGE_CONTENT` は、**要求せずに**接続した場合は切断されない代わりに `msg.content` が
常に空になります。Deputyは常に要求する側なので静かに壊れることはありませんが、インテントの
指定を減らす変更を加える際はこの挙動に注意してください。

### 2. 設定ファイルの配置

`cargo build` すると、`settings.example.yml` が `target/<profile>/settings.yml` として生成されます
（**既存の `settings.yml` は上書きしません**。トークンを書き込んだあとに再ビルドしても消えません）。

ただし設定の読み込み先は実行ファイルの場所ではなく、**プロセスのカレントディレクトリ**です。
同様にログの出力先もカレントディレクトリ配下の `logs/` になります。そのため、次のどちらかで実行してください。

```bash
# A. 生成された雛形をそのまま使う（target/debug で実行する）
cargo build
cd target/debug
# settings.yml を編集してトークン等を設定
./deputy

# B. リポジトリルートで実行する（settings.yml をルートに置く）
cp settings.example.yml settings.yml
# settings.yml を編集してトークン等を設定
cargo run
```

`settings.yml` と `logs/` はいずれも `.gitignore` 済みです。トークンを含むため、コミットしないでください。

### 3. 設定項目

各項目の説明は [`settings.example.yml`](settings.example.yml) のコメントを参照してください。
`features.*.log_channel` には通知先チャンネルのIDを設定します（`0` のままだと起動時にエラーで停止します）。

> **注記:** 現在の実装は**単一サーバーでの運用を前提**としています。
> `log_channel` はギルドごとではなくボット全体で1つのため、複数のサーバーに参加させると
> すべてのサーバーの入退出が同じチャンネルへ通知されます。

## 機能

### honeypot（スパムアカウントの自動BAN）

`honeypot_channels` に指定したチャンネル（正規の利用者が書き込む理由の無い「おとり」チャンネル）への
投稿を監視し、スパム判定されたアカウントをBANします。判定は次の順で評価され、先に当たったものが確定します。

1. `unconditional_ban` — 投稿しただけで即BAN（緊急用。既定は無効）
2. 招待リンクの検知（`ban_trigger.has_invite_link`）
3. ロール/@everyoneメンションの検知（`ban_trigger.has_role_mention`）
4. メンション数の閾値（`ban_trigger.mention_threshold`、0でオフ）
5. LLMによる判定（`enable_ai_judgment`。`ai.support_image` が有効なら添付画像も判定に含める）

2〜4はAI呼び出し前の高速フィルタで、コストと誤検知の両方を抑えます。
LLMへ渡すシステムプロンプトは [`PROMPT.md`](PROMPT.md) にあり、`settings.yml` と同じく
**プロセスのカレントディレクトリ**から読み込まれます（`cargo build` で `target/<profile>/` へも配置されます）。

処分の実行・`debug_mode` での判定・AI判定の失敗は、いずれも `log_channel` へ通知されます。

**誤BANへの備え**

- `exempt_roles` に指定したロールを持つメンバーは判定対象外になります。運営ロールを入れておいてください。
- `debug_mode: true` にすると、判定は行いますが実際のBANは行いません。導入直後の精度確認に使ってください。
- `/honeypot list` で直近20件の処分（対象・判定経路・理由）を確認し、`/honeypot unban <user>` で
  その場で解除できます。どちらもBAN権限を持つメンバーにのみ表示され、応答は実行者にしか見えません。

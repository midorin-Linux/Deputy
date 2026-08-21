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

本ボットは入退出ログのために `GUILD_MEMBERS` インテントを要求します。これは**特権インテント**のため、
[Discord Developer Portal](https://discord.com/developers/applications) での有効化が必須です。

`Bot` → `Privileged Gateway Intents` → **SERVER MEMBERS INTENT** をオンにしてください。

有効化しないまま起動すると、ゲートウェイがクローズコード `4014`（Disallowed intent）で切断され、
接続の再試行を延々と繰り返します。トークンが正しくてもログインできないため、この症状が出たらまずここを疑ってください。

要求しているインテントは以下の3つです（`SERVER MEMBERS INTENT` 以外は特権ではありません）。

| インテント | 用途 | 特権 |
| --- | --- | --- |
| `GUILDS` | ギルド情報・VoiceStateのキャッシュ | - |
| `GUILD_MEMBERS` | `member_log`（入退出ログ） | 要有効化 |
| `GUILD_VOICE_STATES` | `voice_log`（VC入退出ログ） | - |

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

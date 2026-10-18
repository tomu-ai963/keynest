# KeyNest

**APIキー管理に特化した、完全ローカル動作のデスクトップパスワードマネージャー**

A local-first desktop manager for API keys. No cloud, no telemetry — your keys never leave your machine.

## なぜ KeyNest か

開発者は数十個のAPIキーを `.env` ファイルに散らばらせがちです。KeyNest はそれらを一箇所に集約し、どのプロジェクトがどのキーを使っているかを管理します。

## セキュリティモデル

KeyNest は2層ストレージを採用しています。

- **キーの値そのもの** → OSのセキュアストレージに保存(Windows資格情報マネージャー / macOS Keychain / Linux secret-service)。暗号化はOSに委譲します。Linux では **Secret Service** を提供するデーモン(GNOME Keyring / KWallet 等)が起動している必要があります([前提パッケージ](#linux-の前提)を参照)。
- **メタデータ**(サービス名・環境変数名・有効期限・プロジェクト紐づけ等) → ローカルのSQLite。**キーの値は一切含みません。**

UIに表示されるのは常にマスク値(`sk-ant-…3f2x`)のみ。マスクで露出するのは**公知のプレフィックス**(`sk-ant-` 等)と**末尾4文字**だけで、末尾4文字を出すのは20文字以上のキーに限ります(それ未満は全マスク)。生の値がストレージから出るのは「.env への書き出し」と「キーの生死確認(プロバイダAPIへの検証リクエスト)」の操作時だけです。

なお、キーをローテーション・上書きすると、**旧値のマスク文字列**(上記と同じ露出範囲)がローテーション履歴として SQLite に保存されます。生の値そのものは保存しませんが、公知プレフィックス+末尾4文字の断片は履歴に残る点にご留意ください。

独自のマスターパスワードは持ちません(= OSログインと同じ強度)。ネットワーク通信は、ユーザーが明示的に実行するキーの生死確認のみです。

## 主な機能

- APIキーの登録・一覧・削除(値は常にマスク表示)
- プリセット30サービス(Anthropic / OpenAI / Cloudflare / Stripe / Resend など)+ カスタムサービス無制限
- プロジェクトへの紐づけと `.env` への書き出し(既存の `.env` は保持し対象変数のみ更新)
- `.env` 一括スキャン取り込み(プレビュー → 選択 → 適用のウィザード。既存キーとの重複・競合も判定)
- キーの生死確認(対応: Anthropic / Cloudflare / GitHub / OpenAI / Resend / Stripe)
- ローテーション履歴(旧値はマスクのみ保存)・有効期限アラート(期限切れ / 30日以内)

## Claude Code から使う(MCP サーバー)

`keynest-mcp` は KeyNest のキーを Claude Code などの MCP クライアントから使うための stdio サーバーです。**キーの値そのものは AI に渡しません。** 値を会話に載せると、API への送信内容やセッションのログに平文で残るためです。代わりに、KeyNest が値を `.env` やコマンドの環境変数に直接注入します。

| ツール | 内容 |
|---|---|
| `list_keys` | キー一覧(サービス・環境変数名・期限・使用プロジェクト。値は含まない) |
| `list_projects` | プロジェクトと紐づくキーの一覧 |
| `assign_key` | キーをプロジェクトに紐づける |
| `inject_env` | プロジェクトのキーを `<プロジェクト>/.env` に書き出す(返すのは変数名のみ) |
| `run_with_keys` | キーを環境変数に注入してプロジェクトのディレクトリでコマンドを実行する。出力に値が含まれていれば `[REDACTED:環境変数名]` に置き換えて返す |

### セットアップ

```sh
# ビルド(現時点ではインストーラに同梱していません)
cargo build --release --manifest-path src-tauri/Cargo.toml --bin keynest-mcp

# Claude Code に登録
claude mcp add keynest -- /path/to/keynest/src-tauri/target/release/keynest-mcp
```

GUI と同じ DB(`<データディレクトリ>/com.tomuai.keynest/keynest.db`、環境変数 `KEYNEST_DB` で上書き可)を使います。先に KeyNest アプリを一度起動して DB を作成しておいてください。`run_with_keys` で使えるのは、パスが設定されたプロジェクトに紐づけたキーだけです。

### 注意(脅威モデル)

伏せ字への置き換えは「**値がうっかり会話に載る**」のを防ぐ仕組みで、AI の挙動を制限する境界ではありません。`run_with_keys` は任意のコマンドを実行でき、エンコードして出力するなど意図的に持ち出す手段は残ります。また `inject_env` で書き出した `.env` は、AI がファイルとして読める場所にあります。

- `run_with_keys` / `inject_env` / `assign_key` は **常時許可(allow)に入れず、呼び出しごとに承認する**ことを推奨します。読み取り専用の `list_keys` / `list_projects` だけを許可するのが目安です。
- 承認時は、実行されるコマンドの内容を確認してください。

## 制限事項

- **Windows: 1キーあたり約2.5KBまで。** キーの値は Windows 資格情報マネージャーに保存されるため、1エントリの上限(約2,560バイト)を超える値(サービスアカウントの JSON キーなどの長大なシークレット)は保存できません。超過時は「キーが長すぎてこのOSのセキュアストレージに保存できません(上限 n バイト)」というエラーになります。macOS Keychain / Linux Secret Service の上限は十分大きいため、通常は影響しません。

## インストール

[Releases](https://github.com/tomu-ai963/keynest/releases) から各OS向けのインストーラをダウンロードしてください。

- Windows: `.msi` / `.exe`
- macOS: `.dmg`(ユニバーサルバイナリ。署名なしのため初回起動時に Gatekeeper の許可が必要です)
- Linux: `.deb` / `.rpm` / `.AppImage`

### Linux の前提

キーの保存に OS の Secret Service を使うため、以下のいずれかが起動している必要があります。起動していないとキーの保存・取得時に「OSのセキュアストレージにアクセスできませんでした」というエラーになります。

- GNOME 系: `gnome-keyring`(多くのデスクトップ環境に既定で同梱)
- KDE 系: `KWallet`
- 軽量WM / ヘッドレス環境: `gnome-keyring` などを別途インストール・起動してください。

```sh
# Debian / Ubuntu 例
sudo apt-get install gnome-keyring
```

## 技術スタック

- [Tauri 2](https://tauri.app/)(Rust)+ React + TypeScript + Vite
- メタデータ: SQLite(rusqlite)
- シークレット保存: [keyring](https://crates.io/crates/keyring) クレート(OSネイティブのセキュアストレージ)

## 開発環境のセットアップ

必要なもの:

- Node.js 20+
- Rust(stable)— [rustup](https://rustup.rs/) でインストール
- Windows: Visual Studio Build Tools(C++ ワークロード)/ Linux: [Tauri の前提パッケージ](https://tauri.app/start/prerequisites/)

```sh
npm install
npm run tauri dev    # 開発モードで起動
npm run tauri build  # 配布用バイナリのビルド
```

テスト:

```sh
cargo test --manifest-path src-tauri/Cargo.toml
npm run build        # tsc + vite build(型チェック込み)
```

変更履歴は [CHANGELOG.md](CHANGELOG.md) を参照してください。

## ライセンス

[MIT](LICENSE)

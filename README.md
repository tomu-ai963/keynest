# KeyNest

**APIキー管理に特化した、完全ローカル動作のデスクトップパスワードマネージャー**

A local-first desktop manager for API keys. No cloud, no telemetry — your keys never leave your machine.

> 🚧 開発初期段階です。v0.1.0 に向けて開発中。

## なぜ KeyNest か

開発者は数十個のAPIキーを `.env` ファイルに散らばらせがちです。KeyNest はそれらを一箇所に集約し、どのプロジェクトがどのキーを使っているかを管理します。

## セキュリティモデル

KeyNest は2層ストレージを採用しています。

- **キーの値そのもの** → OSのセキュアストレージに保存(Windows資格情報マネージャー / macOS Keychain / Linux secret-service)。暗号化はOSに委譲します。
- **メタデータ**(サービス名・環境変数名・有効期限・プロジェクト紐づけ等) → ローカルのSQLite。**キーの値は一切含みません。**

UIに表示されるのは常にマスク値(`sk-a…3f2x`)のみ。生の値がストレージから出るのは「.env への書き出し」と「クリップボードへのコピー」の操作時だけです。

独自のマスターパスワードは持ちません(= OSログインと同じ強度)。ネットワーク通信は、ユーザーが明示的に実行するキーの生死確認(各プロバイダAPIへの検証リクエスト)のみです。

## 主な機能(計画)

- [x] APIキーの登録・一覧・削除(値は常にマスク表示)
- [x] プリセット30サービス(Anthropic / OpenAI / Cloudflare / Stripe / Resend など)
- [x] カスタムサービスの追加(無制限)
- [x] プロジェクトへの紐づけと `.env` への書き出し(既存の `.env` は保持し対象変数のみ更新)
- [ ] `.env` 一括スキャン取り込み(プレビュー → 選択 → 適用のウィザード)
- [ ] キーの生死確認(プロバイダAPIで検証)
- [ ] ローテーション履歴・有効期限アラート

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

## ロードマップ

| Phase | 内容 | 状態 |
|---|---|---|
| 0 | プロジェクト足場(Tauri + React + CI) | ✅ |
| 1 | vault コア(2層ストレージ、キーCRUD、最小UI) | ✅ |
| 2 | プリセット30サービス、プロジェクト紐づけ、.env 書き出し | ✅ |
| 3 | .env 一括スキャン取り込みウィザード | 🚧 |
| 4 | 生死確認、ローテーション履歴、有効期限アラート | — |
| 5 | クロスプラットフォーム検証、v0.1.0 リリース | — |

## ライセンス

[MIT](LICENSE)

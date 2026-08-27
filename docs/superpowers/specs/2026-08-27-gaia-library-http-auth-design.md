# gaia-library サブプロジェクト B: HTTP トランスポート＋認証 設計書（2026-08-27）

## 1. 概要

サブプロジェクト A（基盤）で構築した `ToolService` を、stdio に加えて **ローカル常駐の Streamable HTTP** で公開し、**API キー（bearer）でクライアントを識別**する。デスクトップアプリ（サブプロジェクト C）はこの HTTP サーバーをプロセス内で起動し、エージェント（Claude Code / Codex 等）は HTTP または従来どおり stdio で接続する。

- 前提: `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`（A。§7.1 の「stdio は起動時識別・キー検証は HTTP で追加」を本書が引き受ける）
- 本書の範囲: HTTP トランスポート、キー認証、識別のリクエスト毎注入、CLI のキー発行・接続設定生成
- 範囲外: キーチェーン保管（C のアプリが担当）、リモート公開（バインドは 127.0.0.1 固定）

## 2. 決定事項

| 項目 | 決定 | 理由 |
| --- | --- | --- |
| エンドポイント | `http://127.0.0.1:4111/mcp`（既定。候補 4112〜4114 へフォールバック、config で変更可） | solo-eikaiwa の 3111 系と衝突しない近傍。localhost バインド固定 |
| 認証方式 | `Authorization: Bearer <key>`。config には **SHA-256 ハッシュのみ**保存 | 平文キーを設定ファイルに残さない |
| キー形式 | `gaia_<ASCII 安全な識別接頭辞>_<32 桁 hex 乱数>`（OS 乱数） | 日本語や空白を含むクライアント名でも HTTP ヘッダーに使える。主体は接頭辞ではなくハッシュ照合で識別 |
| 平文キーの保管 | 発行時に 1 度だけ表示。アプリ（C）は OS キーチェーンに保管して接続スニペットを再表示できる | Notion 確定「キーはエージェント設定とキーチェーンの外に置かない」を具体化 |
| human キー | **既定では発行しない**。アプリ内承認はプロセス内呼び出し、CLI 承認は DB 直開きでキー不要。HTTP 経由の human 操作が必要になった場合のみ発行 | 攻撃面と管理物を減らす |
| 認証失敗 | 401（本文に詳細を書かない）。stderr ログのみで audit_log には書かない | 無認証経路から DB 書き込みを発生させない |
| 識別の注入 | axum middleware が Bearer 検証 → `ClientIdentity` を request extensions へ。ハンドラは `RequestContext.extensions` 内の `http::request::Parts` から取り出す | rmcp 3.1.4 の実装（tower.rs が Parts を注入）を調査・実行確認済み |
| セッション | rmcp `StreamableHttpService` ＋所有者を記録する `LocalSessionManager` ラッパー。全 POST / GET / DELETE でクライアント名を照合し、異なる主体には 404。既定の allowed_hosts も維持 | Bearer が有効でも別クライアントの応答再取得やセッション削除を許可しない |
| get_server_info | `protocol.transports` を `["stdio", "http"]` に更新（スキーマ変更なし・contract_version 据え置き） | 値の追加のみ |

## 3. 変更点一覧

| 場所 | 変更 |
| --- | --- |
| `gaia-core` config | `Config` に `[keys]` テーブル（クライアント名 → SHA-256 hex）と `[server] port: Option<u16>` を追加。`ClientIdentity` は変更しない（識別と認証材料を分離し、既存の構築箇所を壊さない） |
| `gaia-core` 新規 | `auth` モジュール: キー生成 `generate_key(name) -> (plaintext, hash)`、`hash_key(&str) -> String`（SHA-256 hex）、`AuthTable`（`[keys]` と `[[clients]]` の突合。`verify(bearer) -> Option<ClientIdentity>`、比較は constant-time） |
| `gaia-mcp` | `GaiaServer` の識別を `IdentitySource::{Fixed(ClientIdentity), FromRequest}` に拡張（FromRequest は extensions から取り出し、無ければ JSON-RPC `-32001`）。新規 `http.rs`: `serve_http(service: Arc<ToolService>, auth: Arc<AuthTable>, port: Option<u16>) -> Result<BoundServer, HttpServeError>`（async。`BoundServer.local_addr()` で実ポートを返し、`shutdown()` で停止。`--port 0` で ephemeral 可） |
| `gaia` CLI | `gaia serve --http [--port N]`（127.0.0.1 バインド。ポート未指定は 4111→4114 フォールバック）／`gaia client add ... --generate-key`／`gaia client keygen <name>`（再発行）／`gaia client mcp-config <name> [--transport stdio\|http]`（`.mcp.json` 用スニペット出力） |
| contracts | 変更なし（`transports` の値のみ更新） |
| 依存追加 | `axum`（workspace）、`sha2`、`rand`、`subtle`（constant-time 比較）、`tokio-util`（CancellationToken）。rmcp features に `transport-streamable-http-server` を追加。dev: `ureq`（HTTP 統合テスト） |
| AGENTS.md | トランスポート節と接続手順を更新 |

## 4. 詳細設計

### 4.1 認証（gaia-core::auth）

- `generate_key(name: &str) -> (String, String)`: クライアント名を ASCII 安全・長さ制限付きの接頭辞に変換し、平文 `gaia_{prefix}_{hex32}`（`rand` の OS 乱数 16 バイト）とその SHA-256 hex を返す。元のクライアント名や役割は変更しない
- `AuthTable::from_config(&Config) -> AuthTable`: `[keys]` と `[[clients]]` を突合し、不正ハッシュ・不明クライアント・重複ハッシュを含む設定では認証しない。`verify(&self, bearer: &str) -> Option<ClientIdentity>` は入力の SHA-256 を全エントリと `subtle::ConstantTimeEq` で比較（早期 return しない）。HTTP は `from_path` で設定を認証ごとに再読込し、キー・役割・scope の変更や削除を反映する
- `gaia client add --generate-key` / `gaia client keygen`: config のロック付き原子的更新が成功した後、平文を stdout に 1 度だけ表示し、config には hash を保存。`--json` 時はキーを含む JSON を stdout へ出力する。stderr やログにはキーを出さない。keygen は既存 hash を置き換える（旧キーは即失効）

### 4.2 HTTP サーバー（gaia-mcp::http）

- axum の `/mcp` に `StreamableHttpService` を登録し、Bearer 認証とセッション所有者検査の middleware を適用する。セッション管理は `LocalSessionManager` のラッパーで、作成時の枠確保・initialize 時の所有者記録・終了時の解放を SDK の寿命に合わせる
- `bearer_middleware`: `Authorization: Bearer` を取り出し `AuthTable::verify` → 成功なら identity を request extensions に設定、失敗は 401。既存セッションを指定した POST / GET / DELETE は所有者も検査し、不明・期限切れ・別主体なら 404。同一クライアントの新キーは既存セッションを利用できる
- factory は `GaiaServer::new_http(service.clone())`（IdentitySource::FromRequest）を返す軽量クロージャ（rmcp はセッション毎＋ツールスキーマ照会で factory を呼ぶため、重い初期化を置かない）
- `GaiaServer` の `list_tools` / `call_tool` は識別解決を `resolve_identity` に集約（Fixed は clone、FromRequest は `ctx.extensions.get::<http::request::Parts>()` → `parts.extensions.get::<ClientIdentity>()`）。HTTP の `get_tool` は SDK の事前スキーマ検査用に役割非依存で定義を返し、一覧と実行の権限は各リクエストで検査する。stdio のツール一覧・スキーマは固定した役割で絞り込む
- 起動: `--port` → config の `server.port` → 4111..=4114 の順で選択する。明示ポートが使用中なら失敗し、別ポートへ切り替えない。起動成功時の URL は通常 stderr、`--json` 時は stdout の `{"status":"listening","url":"..."}` で返す
- 終了: `CancellationToken` で graceful shutdown（axum `with_graceful_shutdown`）。開いているセッションも明示終了し、SSE 接続が残っていても停止を完了する

### 4.3 接続スニペット（gaia client mcp-config）

- stdio: `{"mcpServers": {"gaia_library": {"command": "gaia", "args": ["--config", "<absolute-config-path>", "--client", "<name>", "serve", "--stdio"], "env": {"GAIA_DB": "<absolute-db-path>"}}}}` — 生成時の設定と実効 DB の絶対パスを保持し、接続元の作業ディレクトリや環境変数に依存しない。DB を移動した場合は再生成する
- http: `{"mcpServers": {"gaia_library": {"type": "http", "url": "http://127.0.0.1:<port>/mcp", "headers": {"Authorization": "Bearer <key>"}}}}` — 有効な平文キーを `--key-stdin` または互換用の `--key` で受け取る（両者は排他）。出力前に指定クライアントの有効なキーか検証する。config からは復元できず、設定の生成だけで勝手に再発行はしない

### 4.4 テスト

- 単体: `generate_key` / `hash_key` / `AuthTable::verify`（一致・不一致・未知名・constant-time パスの成立）／identity 解決（Fixed / FromRequest 欠落時 -32001）
- 統合（`crates/gaia/tests/http.rs`）: `gaia serve --http --port 0` を子プロセス起動 → stderr から URL を取得 → `ureq` で initialize / tools/list / tools/call（agent キー: approve 系が見えない・search が通る／不正キー: 401／human キー発行時: approve が見える）
- 既存 stdio テスト・全ゲートが引き続き通ること

## 5. リスクと残論点

- rmcp の Streamable HTTP はセッション管理（`Mcp-Session-Id`）を含む。Claude Code / Codex の HTTP MCP クライアント実装との互換は統合テスト＋実機（Claude Code から接続）で確認する
- `Authorization` ヘッダを固定で送れないクライアントが現れた場合はクエリパラメータ認証を足さず、stdio を案内する（キーの URL 混入を避ける）
- 同時接続が増えた場合の `Mutex<Connection>` 直列化は A の既知事項のまま（実測で問題になってから接続プール化）

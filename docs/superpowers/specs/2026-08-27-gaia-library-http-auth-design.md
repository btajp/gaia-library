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
| キー形式 | `gaia_<client名>_<32 桁 hex 乱数>`（OS 乱数） | 目視でどのクライアントのキーか分かる |
| 平文キーの保管 | 発行時に 1 度だけ表示。アプリ（C）は OS キーチェーンに保管して接続スニペットを再表示できる | Notion 確定「キーはエージェント設定とキーチェーンの外に置かない」を具体化 |
| human キー | **既定では発行しない**。アプリ内承認はプロセス内呼び出し、CLI 承認は DB 直開きでキー不要。HTTP 経由の human 操作が必要になった場合のみ発行 | 攻撃面と管理物を減らす |
| 認証失敗 | 401（本文に詳細を書かない）。stderr ログのみで audit_log には書かない | 無認証経路から DB 書き込みを発生させない |
| 識別の注入 | axum middleware が Bearer 検証 → `ClientIdentity` を request extensions へ。ハンドラは `RequestContext.extensions` 内の `http::request::Parts` から取り出す | rmcp 3.1.4 の実装（tower.rs が Parts を注入）を調査・実行確認済み |
| セッション | rmcp `StreamableHttpService` ＋ `LocalSessionManager`、`StreamableHttpServerConfig::default()`（allowed_hosts が localhost 既定で DNS rebinding を遮断） | 既定が安全側 |
| get_server_info | `protocol.transports` を `["stdio", "http"]` に更新（スキーマ変更なし・contract_version 据え置き） | 値の追加のみ |

## 3. 変更点一覧

| 場所 | 変更 |
| --- | --- |
| `gaia-core` config | `ClientIdentity` に `key_hash: Option<String>` を追加（認証材料。ツール出力 `ClientInfo` には含めない）。`[server] port: Option<u16>` を追加 |
| `gaia-core` 新規 | `auth` モジュール: キー生成 `generate_key(name) -> (plaintext, hash)`、`hash_key(&str) -> String`（SHA-256 hex）、`AuthTable`（name→(hash, identity)。`verify(bearer) -> Option<ClientIdentity>`、比較は constant-time） |
| `gaia-mcp` | `GaiaServer` の識別を `IdentitySource::{Fixed(ClientIdentity), FromRequest}` に拡張（FromRequest は extensions から取り出し、無ければ JSON-RPC `-32001`）。新規 `http.rs`: `serve_http(service: Arc<ToolService>, auth: Arc<AuthTable>, addr: SocketAddr, shutdown: CancellationToken) -> Result<BoundServer, ServeError>`（`BoundServer.local_addr()` で実ポートを返す。`--port 0` で ephemeral 可） |
| `gaia` CLI | `gaia serve --http [--port N]`（127.0.0.1 バインド。ポート未指定は 4111→4114 フォールバック）／`gaia client add ... --generate-key`／`gaia client keygen <name>`（再発行）／`gaia client mcp-config <name> [--transport stdio\|http]`（`.mcp.json` 用スニペット出力） |
| contracts | 変更なし（`transports` の値のみ更新） |
| 依存追加 | `axum`（workspace）、`sha2`、`rand`、`subtle`（constant-time 比較）、`tokio-util`（CancellationToken）。rmcp features に `transport-streamable-http-server` を追加。dev: `ureq`（HTTP 統合テスト） |
| AGENTS.md | トランスポート節と接続手順を更新 |

## 4. 詳細設計

### 4.1 認証（gaia-core::auth）

- `generate_key(name: &str) -> (String, String)`: 平文 `gaia_{name}_{hex32}`（`rand` の OS 乱数 16 バイト）とその SHA-256 hex を返す
- `AuthTable::from_config(&Config) -> AuthTable`: `key_hash` を持つクライアントだけを収載。`verify(&self, bearer: &str) -> Option<ClientIdentity>` は `subtle::ConstantTimeEq` でハッシュ比較
- `gaia client add --generate-key` / `gaia client keygen`: 平文を stderr に 1 度だけ表示し、config には hash を保存。keygen は既存 hash を置き換える（旧キーは即失効）

### 4.2 HTTP サーバー（gaia-mcp::http）

- axum `Router::new().nest_service("/mcp", StreamableHttpService::new(factory, LocalSessionManager::default().into(), StreamableHttpServerConfig::default())).layer(middleware::from_fn_with_state(auth, bearer_middleware))`
- `bearer_middleware`: `Authorization: Bearer` を取り出し `AuthTable::verify` → 成功なら `req.extensions_mut().insert(identity)`、失敗は 401
- factory は `GaiaServer::new_http(service.clone())`（IdentitySource::FromRequest）を返す軽量クロージャ（rmcp はセッション毎＋ツールスキーマ照会で factory を呼ぶため、重い初期化を置かない）
- `GaiaServer` の `list_tools` / `call_tool` / `get_tool` は識別解決を `fn resolve_identity(&self, ctx) -> Result<ClientIdentity, ErrorData>` に集約（Fixed は clone、FromRequest は `ctx.extensions.get::<http::request::Parts>()` → `parts.extensions.get::<ClientIdentity>()`）
- 起動: `--port` 指定なしは 4111..=4114 を順に bind 試行。起動成功時に stderr へ `listening on http://127.0.0.1:<port>/mcp` を 1 行出す（テスト・アプリがこれを読む）
- 終了: `CancellationToken` で graceful shutdown（axum `with_graceful_shutdown`）

### 4.3 接続スニペット（gaia client mcp-config）

- stdio: `{"mcpServers": {"gaia_library": {"command": "gaia", "args": ["serve", "--stdio", "--client", "<name>"]}}}`
- http: `{"mcpServers": {"gaia_library": {"type": "http", "url": "http://127.0.0.1:<port>/mcp", "headers": {"Authorization": "Bearer <key>"}}}}` — 平文キーが必要なため、キーを引数 `--key` で受けるか、その場で `keygen` するかを選ばせる（config からは復元できない）

### 4.4 テスト

- 単体: `generate_key` / `hash_key` / `AuthTable::verify`（一致・不一致・未知名・constant-time パスの成立）／identity 解決（Fixed / FromRequest 欠落時 -32001）
- 統合（`crates/gaia/tests/http.rs`）: `gaia serve --http --port 0` を子プロセス起動 → stderr から URL を取得 → `ureq` で initialize / tools/list / tools/call（agent キー: approve 系が見えない・search が通る／不正キー: 401／human キー発行時: approve が見える）
- 既存 stdio テスト・全ゲートが引き続き通ること

## 5. リスクと残論点

- rmcp の Streamable HTTP はセッション管理（`Mcp-Session-Id`）を含む。Claude Code / Codex の HTTP MCP クライアント実装との互換は統合テスト＋実機（Claude Code から接続）で確認する
- `Authorization` ヘッダを固定で送れないクライアントが現れた場合はクエリパラメータ認証を足さず、stdio を案内する（キーの URL 混入を避ける）
- 同時接続が増えた場合の `Mutex<Connection>` 直列化は A の既知事項のまま（実測で問題になってから接続プール化）

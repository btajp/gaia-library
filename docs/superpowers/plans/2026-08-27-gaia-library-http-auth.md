# gaia-library HTTP＋認証 実装計画（サブプロジェクト B）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `ToolService` を Streamable HTTP（127.0.0.1、bearer キー認証）でも公開し、キー発行・接続設定生成を CLI に追加する。

**Architecture:** 認証材料は `gaia-core::auth`（`[keys]` テーブル＝名前→SHA-256、constant-time 照合）。`gaia-mcp` は識別源を `Fixed(stdio)` / `FromRequest(HTTP)` に抽象化し、axum middleware が Bearer 検証済みの `ClientIdentity` を request extensions 経由で注入する（rmcp が `http::request::Parts` を `RequestContext.extensions` に運ぶ、調査で実行確認済みの経路）。

**Tech Stack:** A のスタック ＋ axum 0.8、sha2、rand 0.9、subtle、tokio-util（CancellationToken）、http 1。rmcp features に `transport-streamable-http-server` を追加。dev: ureq 3

**Spec:** `docs/superpowers/specs/2026-08-27-gaia-library-http-auth-design.md`（前提: A の仕様書と実装が完了していること）

## Global Constraints

- A の計画の Global Constraints をすべて引き継ぐ（コミット規約・ゲート・依存方向・scope 規則・stderr ログ）
- バインドは 127.0.0.1 固定。0.0.0.0 や外部公開のオプションを作らない
- 平文キーは保存しない（config には SHA-256 hex のみ）。平文の表示は発行時の 1 度だけ
- 認証失敗は 401 のみ（本文に理由を書かない・audit_log に書かない・stderr ログのみ）
- 契約ファイルは変更しない（`get_server_info` の `transports` は値の更新のみ）

---

### Task B1: gaia-core::auth と config 拡張

**Files:**
- Create: `crates/gaia-core/src/auth.rs`
- Modify: `crates/gaia-core/src/config.rs`, `crates/gaia-core/src/lib.rs`, `Cargo.toml`（workspace deps）, `crates/gaia-core/Cargo.toml`

**Interfaces:**
- Produces: `gaia_core::auth::{generate_key(name: &str) -> (String, String), hash_key(&str) -> String, AuthTable}`
  - `AuthTable::from_config(&Config) -> AuthTable`、`AuthTable::verify(&self, bearer: &str) -> Option<ClientIdentity>`、`AuthTable::is_empty(&self) -> bool`
- `Config` に追加: `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")] pub keys: BTreeMap<String, String>`（クライアント名 → SHA-256 hex）と `#[serde(default)] pub server: ServerConfig`、`ServerConfig { pub port: Option<u16> }`（derive は Config と同じ）

- [ ] **Step 1: workspace 依存を追加する**

ルート `Cargo.toml` の `[workspace.dependencies]` に追加し、rmcp の features を拡張:

```toml
axum = "0.8"
http = "1"
rand = "0.9"
sha2 = "0.10"
subtle = "2"
tokio-util = "0.7"
ureq = { version = "3", default-features = false, features = ["json"] }
```

```toml
rmcp = { version = "3.1", features = ["server", "transport-io", "transport-streamable-http-server"] }
```

`crates/gaia-core/Cargo.toml` の `[dependencies]` に `rand.workspace = true`、`sha2.workspace = true`、`subtle.workspace = true` を追加。

- [ ] **Step 2: auth.rs をテスト込みで実装する**

```rust
//! API キー認証の材料。仕様書 B §4.1。平文キーは保存せず、config の [keys] に SHA-256 hex だけを置く。
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{config::Config, identity::ClientIdentity};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

pub fn hash_key(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

/// 平文キー `gaia_<name>_<32hex>` とその SHA-256 hex を返す。平文は発行時に 1 度だけ表示する。
pub fn generate_key(name: &str) -> (String, String) {
    let mut raw = [0u8; 16];
    rand::rng().fill_bytes(&mut raw);
    let plaintext = format!("gaia_{name}_{}", hex(&raw));
    let hash = hash_key(&plaintext);
    (plaintext, hash)
}

/// `[keys]` と `[[clients]]` を突合した認証表。
pub struct AuthTable {
    entries: Vec<(Vec<u8>, ClientIdentity)>,
}

impl AuthTable {
    pub fn from_config(config: &Config) -> Self {
        let entries = config
            .keys
            .iter()
            .filter_map(|(name, hash)| {
                let identity = config.client(name)?.clone();
                Some((decode_hex(hash)?, identity))
            })
            .collect();
        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全エントリと constant-time 比較する（早期 return しない）。
    pub fn verify(&self, bearer: &str) -> Option<ClientIdentity> {
        let candidate = Sha256::digest(bearer.as_bytes());
        let mut found: Option<&ClientIdentity> = None;
        for (hash, identity) in &self.entries {
            let matches = hash.len() == candidate.len() && bool::from(hash.as_slice().ct_eq(candidate.as_slice()));
            if matches {
                found = Some(identity);
            }
        }
        found.cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Role;

    fn config_with_key() -> (Config, String) {
        let mut config = Config::default();
        config
            .add_client(ClientIdentity { name: "bot".into(), role: Role::Agent, default_scope: Some("cn".into()) })
            .unwrap();
        let (plaintext, hash) = generate_key("bot");
        config.keys.insert("bot".into(), hash);
        (config, plaintext)
    }

    #[test]
    fn generated_key_verifies_and_wrong_key_does_not() {
        let (config, plaintext) = config_with_key();
        let table = AuthTable::from_config(&config);
        assert!(!table.is_empty());
        let id = table.verify(&plaintext).expect("valid key");
        assert_eq!(id.name, "bot");
        assert!(table.verify("gaia_bot_deadbeef").is_none());
        assert!(table.verify("").is_none());
    }

    #[test]
    fn keys_without_matching_client_are_ignored() {
        let mut config = Config::default();
        config.keys.insert("ghost".into(), hash_key("gaia_ghost_x"));
        assert!(AuthTable::from_config(&config).is_empty());
    }

    #[test]
    fn key_format_and_hash_are_stable() {
        let (plaintext, hash) = generate_key("claude-code");
        assert!(plaintext.starts_with("gaia_claude-code_"));
        assert_eq!(plaintext.len(), "gaia_claude-code_".len() + 32);
        assert_eq!(hash, hash_key(&plaintext));
        assert_eq!(hash.len(), 64);
        let (second, _) = generate_key("claude-code");
        assert_ne!(plaintext, second, "乱数で毎回異なる");
    }
}
```

（`rand::rng()` は rand 0.9 の API。0.8 系に解決された場合は `rand::thread_rng()`。）

- [ ] **Step 3: config.rs に keys / server を追加する**

`Config` 構造体（既存 derive・`deny_unknown_fields` のまま）:

```rust
use std::collections::BTreeMap;

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, String>,
    #[serde(default)]
    pub server: ServerConfig,
```

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}
```

config テストに追記:

```rust
    #[test]
    fn keys_and_server_round_trip_and_default_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.add_client(human("me")).unwrap();
        cfg.keys.insert("me".into(), "ab".repeat(32));
        cfg.server.port = Some(4200);
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), cfg);
        // 旧形式（keys / server なし）も読める
        std::fs::write(&path, "[[clients]]\nname = \"x\"\nrole = \"human\"\n").unwrap();
        let old = Config::load(&path).unwrap();
        assert!(old.keys.is_empty());
        assert_eq!(old.server.port, None);
    }
```

`lib.rs` に `pub mod auth;` を追加。

- [ ] **Step 4: テスト・ゲートを実行しコミット**

Run: `cargo test -p gaia-core auth config && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: すべて成功

```bash
git add Cargo.toml Cargo.lock crates/gaia-core
git commit -m "feat(core): add key auth material and config keys/server sections" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task B2: gaia-mcp の識別源抽象（Fixed / FromRequest）

**Files:**
- Modify: `crates/gaia-mcp/src/server.rs`, `crates/gaia-mcp/Cargo.toml`（`http.workspace = true` 追加）

**Interfaces:**
- Produces: `GaiaServer::new(service, identity)`（従来どおり stdio 用・挙動不変）、`GaiaServer::new_http(service: Arc<ToolService>) -> GaiaServer`、内部 `fn resolve_identity(&self, ctx: &RequestContext<RoleServer>) -> Result<ClientIdentity, ErrorData>`
- 挙動変更: `get_tool` はロールで絞らず enabled な契約を返す（rmcp が HTTP ヘッダ検証にだけ使う。認可は call_tool が強制）

- [ ] **Step 1: server.rs を拡張する**

```rust
enum IdentitySource {
    Fixed(ClientIdentity),
    FromRequest,
}

pub struct GaiaServer {
    service: Arc<ToolService>,
    identity: IdentitySource,
}

impl GaiaServer {
    pub fn new(service: Arc<ToolService>, identity: ClientIdentity) -> Self {
        Self { service, identity: IdentitySource::Fixed(identity) }
    }

    /// HTTP 用: 識別はリクエスト毎に bearer middleware が注入した ClientIdentity を使う。
    pub fn new_http(service: Arc<ToolService>) -> Self {
        Self { service, identity: IdentitySource::FromRequest }
    }

    fn resolve_identity(&self, ctx: &RequestContext<RoleServer>) -> Result<ClientIdentity, ErrorData> {
        match &self.identity {
            IdentitySource::Fixed(id) => Ok(id.clone()),
            IdentitySource::FromRequest => ctx
                .extensions
                .get::<http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<ClientIdentity>())
                .cloned()
                .ok_or_else(|| {
                    ErrorData::new(RpcErrorCode(-32001), "unauthenticated: missing client identity", None)
                }),
        }
    }
}
```

- `list_tools`: `let identity = self.resolve_identity(&context)?;` → `self.service.visible_tools(identity.role)`
- `call_tool`: `let identity = self.resolve_identity(&context)?;` → `self.service.call(&identity, ...)`
- `get_tool`: `self.service.catalog().get(name).filter(|s| s.enabled).map(to_tool)`（ロール非依存に変更。理由コメントを書く）
- 既存テスト（to_tool）はそのまま。`resolve_identity` の Fixed パスは既存 stdio 統合テストが回帰検証する

- [ ] **Step 2: ゲートを実行しコミット**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功（stdio 統合テスト含む）

```bash
git add crates/gaia-mcp Cargo.lock
git commit -m "feat(mcp): abstract identity source for per-request HTTP identity" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task B3: gaia-mcp::http（serve_http ＋ bearer middleware）

**Files:**
- Create: `crates/gaia-mcp/src/http.rs`
- Modify: `crates/gaia-mcp/src/lib.rs`, `crates/gaia-mcp/Cargo.toml`（axum / tokio-util / gaia-core 経由 auth）, `crates/gaia-core/src/tools/server_info.rs`（transports を `["stdio", "http"]` に）

**Interfaces:**
- Produces: `gaia_mcp::http::{serve_http, BoundServer, HttpServeError, DEFAULT_PORTS}`
  - `async fn serve_http(service: Arc<ToolService>, auth: Arc<AuthTable>, port: Option<u16>) -> Result<BoundServer, HttpServeError>`（`port: None` は 4111→4114 を順に試す。`Some(0)` は ephemeral）
  - `BoundServer { pub fn url(&self) -> String; pub fn local_addr(&self) -> SocketAddr; pub async fn shutdown(self) -> Result<(), HttpServeError>; pub async fn wait(self) -> Result<(), HttpServeError> }`
- バインドは常に 127.0.0.1。起動側（CLI / アプリ）が `listening on <url>` を stderr に出す

- [ ] **Step 1: http.rs を実装する**

```rust
//! Streamable HTTP サーバー。仕様書 B §4.2。バインドは 127.0.0.1 固定。
//! 認証は axum middleware（Bearer → AuthTable::verify → ClientIdentity を extensions へ）。
//! rmcp は http::request::Parts を RequestContext.extensions に運ぶので、
//! GaiaServer(FromRequest) がそこから識別を取り出す。
use std::{net::SocketAddr, sync::Arc};

use axum::{Router, middleware};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use gaia_core::{auth::AuthTable, identity::ClientIdentity, tools::ToolService};

use crate::server::GaiaServer;

pub const DEFAULT_PORTS: [u16; 4] = [4111, 4112, 4113, 4114];

#[derive(Debug, thiserror::Error)]
pub enum HttpServeError {
    #[error("auth table is empty: issue a key first (gaia client keygen <name>)")]
    NoKeys,
    #[error("no port available (tried {0:?})")]
    NoPort(Vec<u16>),
    #[error("bind failed on {addr}: {source}")]
    Bind { addr: SocketAddr, source: std::io::Error },
    #[error("server failed: {0}")]
    Serve(String),
}

pub struct BoundServer {
    local_addr: SocketAddr,
    ct: CancellationToken,
    handle: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl BoundServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.local_addr)
    }

    /// graceful shutdown（アプリ終了・テスト用）。
    pub async fn shutdown(self) -> Result<(), HttpServeError> {
        self.ct.cancel();
        self.join().await
    }

    /// サーバーが終わるまで待つ（CLI のフォアグラウンド運転用）。
    pub async fn wait(self) -> Result<(), HttpServeError> {
        self.join().await
    }

    async fn join(self) -> Result<(), HttpServeError> {
        match self.handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(HttpServeError::Serve(e.to_string())),
            Err(e) => Err(HttpServeError::Serve(e.to_string())),
        }
    }
}

async fn bearer_middleware(
    axum::extract::State(auth): axum::extract::State<Arc<AuthTable>>,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, axum::http::StatusCode> {
    let identity: Option<ClientIdentity> = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|token| auth.verify(token));
    match identity {
        Some(id) => {
            req.extensions_mut().insert(id);
            Ok(next.run(req).await)
        }
        None => {
            // 詳細は返さない（仕様 B: 401 のみ・audit_log に書かない）
            tracing::warn!("http: rejected request without a valid bearer key");
            Err(axum::http::StatusCode::UNAUTHORIZED)
        }
    }
}

pub async fn serve_http(
    service: Arc<ToolService>,
    auth: Arc<AuthTable>,
    port: Option<u16>,
) -> Result<BoundServer, HttpServeError> {
    if auth.is_empty() {
        return Err(HttpServeError::NoKeys);
    }
    let candidates: Vec<u16> = match port {
        Some(p) => vec![p],
        None => DEFAULT_PORTS.to_vec(),
    };
    let mut listener = None;
    let mut last: Option<(SocketAddr, std::io::Error)> = None;
    for p in &candidates {
        let addr = SocketAddr::from(([127, 0, 0, 1], *p));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => {
                listener = Some(l);
                break;
            }
            Err(e) => last = Some((addr, e)),
        }
    }
    let Some(listener) = listener else {
        return match (port, last) {
            (Some(_), Some((addr, source))) => Err(HttpServeError::Bind { addr, source }),
            _ => Err(HttpServeError::NoPort(candidates)),
        };
    };
    let local_addr = listener.local_addr().map_err(|e| HttpServeError::Serve(e.to_string()))?;

    let mcp: StreamableHttpService<GaiaServer, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(GaiaServer::new_http(service.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let app = Router::new()
        .nest_service("/mcp", mcp)
        .layer(middleware::from_fn_with_state(auth, bearer_middleware));

    let ct = CancellationToken::new();
    let shutdown = ct.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
            .await
    });
    Ok(BoundServer { local_addr, ct, handle })
}
```

`lib.rs` に `pub mod http;` と `pub use http::{BoundServer, HttpServeError, serve_http};` を追加。`crates/gaia-mcp/Cargo.toml` に `axum.workspace = true`、`http.workspace = true`、`tokio-util.workspace = true` を追加（tokio は既存）。

- [ ] **Step 2: server_info.rs の transports を更新する**

```rust
        protocol: ServerProtocolInfo { transports: vec!["stdio".to_string(), "http".to_string()] },
```

- [ ] **Step 3: ゲートを実行しコミット**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功（HTTP の実挙動は Task B5 の統合テストで検証）

```bash
git add crates/gaia-mcp crates/gaia-core Cargo.lock
git commit -m "feat(mcp): add streamable HTTP server with bearer auth" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task B4: CLI（serve --http / keygen / mcp-config）

**Files:**
- Modify: `crates/gaia/src/cli/serve.rs`, `crates/gaia/src/cli/admin_cmd.rs`, `crates/gaia/src/cli/mod.rs`（Client variants の dispatch は admin_cmd 内で完結）

**Interfaces:**
- `gaia serve --http [--port N]`（`--stdio` と排他。identity は使わない）。起動成功時に stderr へ `gaia_library listening on http://127.0.0.1:<port>/mcp` を 1 行出し、Ctrl-C まで待つ
- `gaia client add <name> --role <r> [--default-scope s] [--generate-key]`／`gaia client keygen <name>`（平文キーを **stdout に 1 行**出力し、config の `[keys]` を置換）／`gaia client mcp-config <name> [--transport stdio|http] [--key <plaintext>] [--port N]`（スニペットを stdout に出力。http で `--key` 省略はエラーにし `gaia client keygen` を案内）

- [ ] **Step 1: serve.rs を拡張する**

```rust
#[derive(Args)]
pub struct ServeArgs {
    /// stdio トランスポートで起動する
    #[arg(long, conflicts_with = "http")]
    pub stdio: bool,
    /// Streamable HTTP（127.0.0.1）で起動する
    #[arg(long)]
    pub http: bool,
    /// HTTP のポート（省略時は config → 4111..4114。0 で空きポート）
    #[arg(long)]
    pub port: Option<u16>,
}

pub fn serve(app: App, cli_client: Option<&str>, args: &ServeArgs) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    if args.http {
        let auth = std::sync::Arc::new(gaia_core::auth::AuthTable::from_config(&app.config));
        let port = args.port.or(app.config.server.port);
        let service = std::sync::Arc::new(app.service);
        return runtime.block_on(async move {
            let bound = gaia_mcp::serve_http(service, auth, port).await?;
            eprintln!("gaia_library listening on {}", bound.url());
            tokio::select! {
                r = bound.wait() => r?,
                _ = tokio::signal::ctrl_c() => {}
            }
            Ok(())
        });
    }
    if !args.stdio {
        anyhow::bail!("--stdio か --http を指定してください");
    }
    // （既存の stdio 経路はそのまま）
    ...
}
```

（`tokio::select!` で `bound.wait()` を使うと move の都合が悪ければ、`ctrl_c().await` 後に `bound.shutdown().await` する素直な 2 段でよい。）

- [ ] **Step 2: admin_cmd.rs にキー系を追加する**

```rust
#[derive(Subcommand)]
pub enum ClientCmd {
    Add {
        name: String,
        #[arg(long)]
        role: Role,
        #[arg(long)]
        default_scope: Option<String>,
        /// 追加と同時に API キーを発行する（平文は stdout に 1 度だけ出力）
        #[arg(long)]
        generate_key: bool,
    },
    List,
    /// API キーを（再）発行する。旧キーは即失効
    Keygen { name: String },
    /// MCP クライアント設定のスニペットを出力する
    McpConfig {
        name: String,
        #[arg(long, default_value = "stdio")]
        transport: String,
        /// http 用の平文キー（keygen の出力）
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        port: Option<u16>,
    },
}
```

実装（`client` 関数に追記。`use gaia_core::auth;` と `use serde_json::json;`）:

```rust
        ClientCmd::Add { name, role, default_scope, generate_key } => {
            config.add_client(ClientIdentity { name: name.clone(), role: *role, default_scope: default_scope.clone() })?;
            if *generate_key {
                let (plaintext, hash) = auth::generate_key(name);
                config.keys.insert(name.clone(), hash);
                println!("{plaintext}");
                eprintln!("API キーを発行しました（この 1 回しか表示されません。config にはハッシュのみ保存）");
            }
            config.save(config_path)?;
            eprintln!("クライアント `{name}` を追加しました（role={role}）");
        }
        ClientCmd::Keygen { name } => {
            if config.client(name).is_none() {
                anyhow::bail!("クライアント `{name}` がありません（gaia client add で作成）");
            }
            let (plaintext, hash) = auth::generate_key(name);
            config.keys.insert(name.clone(), hash);
            config.save(config_path)?;
            println!("{plaintext}");
            eprintln!("API キーを発行しました（旧キーは失効。この 1 回しか表示されません）");
        }
        ClientCmd::McpConfig { name, transport, key, port } => {
            if config.client(name).is_none() {
                anyhow::bail!("クライアント `{name}` がありません");
            }
            let snippet = match transport.as_str() {
                "stdio" => json!({"mcpServers": {"gaia_library": {"command": "gaia", "args": ["serve", "--stdio", "--client", name]}}}),
                "http" => {
                    let key = key.clone().ok_or_else(|| anyhow::anyhow!(
                        "--key <平文キー> が必要です（`gaia client keygen {name}` で発行できます）"
                    ))?;
                    let port = port.or(config.server.port).unwrap_or(4111);
                    json!({"mcpServers": {"gaia_library": {"type": "http", "url": format!("http://127.0.0.1:{port}/mcp"), "headers": {"Authorization": format!("Bearer {key}")}}}})
                }
                other => anyhow::bail!("未知の transport `{other}`（stdio | http）"),
            };
            print_json(&snippet, compact);
        }
```

- [ ] **Step 3: 手動スモークとゲート**

Run:

```bash
D="$(mktemp -d)"; export GAIA_CONFIG="$D/config.toml" GAIA_DB="$D/gaia.db"
cargo run -p gaia -- init --affiliation cloudnative --client-name tester
KEY=$(cargo run -q -p gaia -- client add bot --role agent --default-scope cloudnative --generate-key)
cargo run -p gaia -- client mcp-config bot --transport http --key "$KEY"
( cargo run -p gaia -- serve --http --port 0 & sleep 3; kill %1 ) 2>&1 | grep listening
unset GAIA_CONFIG GAIA_DB
```

Expected: スニペットに Bearer キー入り URL、serve が `listening on http://127.0.0.1:<port>/mcp` を出力

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/gaia
git commit -m "feat(cli): add http serve, key issuance and mcp-config snippets" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task B5: HTTP 統合テスト

**Files:**
- Create: `crates/gaia/tests/http.rs`
- Modify: `crates/gaia/Cargo.toml`（dev-dependencies に `ureq.workspace = true`）

**Interfaces:**
- 検証: 起動 → 401（キー無し・不正キー）→ initialize（セッション ID 取得）→ tools/list（agent に承認系が見えない）→ tools/call search_context。応答は SSE（`data:` 行）と素の JSON の両対応でパースする

- [ ] **Step 1: http.rs テストを書く**

```rust
//! HTTP トランスポートの一気通貫。gaia serve --http --port 0 を起動し ureq で JSON-RPC を送る。
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

struct HttpServer {
    child: Child,
    url: String,
}

impl HttpServer {
    fn start(dir: &std::path::Path) -> HttpServer {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gaia"))
            .args(["serve", "--http", "--port", "0"])
            .env("GAIA_CONFIG", dir.join("config.toml"))
            .env("GAIA_DB", dir.join("gaia.db"))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn gaia serve --http");
        let stderr = child.stderr.take().unwrap();
        let mut reader = BufReader::new(stderr);
        let mut url = None;
        for _ in 0..200 {
            let mut line = String::new();
            let n = reader.read_line(&mut line).unwrap();
            if n == 0 {
                break;
            }
            if let Some(rest) = line.trim().strip_prefix("gaia_library listening on ") {
                url = Some(rest.to_string());
                break;
            }
        }
        // stderr を読み続けるスレッド（パイプ詰まり防止）
        std::thread::spawn(move || {
            let mut sink = String::new();
            while let Ok(n) = reader.read_line(&mut sink) {
                if n == 0 {
                    break;
                }
                sink.clear();
            }
        });
        HttpServer { child, url: url.expect("listening line") }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .env("GAIA_CONFIG", dir.join("config.toml"))
        .env("GAIA_DB", dir.join("gaia.db"))
        .output()
        .unwrap()
}

fn setup(dir: &std::path::Path) -> String {
    for args in [
        vec!["init", "--affiliation", "cloudnative", "--client-name", "tester"],
        vec!["add", "person", "--name", "岡村 慎太郎", "--alias", "okash1n"],
    ] {
        let out = cli(dir, &args);
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    }
    let out = cli(dir, &["client", "add", "bot", "--role", "agent", "--default-scope", "cloudnative", "--generate-key"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// SSE（`data:` 行）と素の JSON の両対応で JSON-RPC 応答を取り出す。
fn parse_body(text: &str) -> serde_json::Value {
    for line in text.lines() {
        if let Some(data) = line.trim().strip_prefix("data:") {
            if let Ok(v) = serde_json::from_str(data.trim()) {
                return v;
            }
        }
    }
    serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("unparseable body ({e}): {text}"))
}

struct Rpc {
    url: String,
    key: String,
    session: Option<String>,
}

impl Rpc {
    fn post(&mut self, body: serde_json::Value) -> (u16, serde_json::Value, Option<String>) {
        let mut req = ureq::post(&self.url)
            .header("Authorization", &format!("Bearer {}", self.key))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("MCP-Protocol-Version", "2025-06-18");
        if let Some(s) = &self.session {
            req = req.header("Mcp-Session-Id", s);
        }
        match req.send(body.to_string()) {
            Ok(mut res) => {
                let session = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(String::from);
                let text = res.body_mut().read_to_string().unwrap_or_default();
                (res.status().as_u16(), if text.is_empty() { serde_json::Value::Null } else { parse_body(&text) }, session)
            }
            Err(ureq::Error::StatusCode(code)) => (code, serde_json::Value::Null, None),
            Err(e) => panic!("request failed: {e}"),
        }
    }

    fn request(&mut self, id: i64, method: &str, params: serde_json::Value) -> serde_json::Value {
        let (status, body, session) = self.post(serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        assert_eq!(status, 200, "{method}: {body}");
        if session.is_some() {
            self.session = session;
        }
        body
    }
}

#[test]
fn http_auth_filters_and_serves() {
    let dir = tempfile::tempdir().unwrap();
    let key = setup(dir.path());
    assert!(key.starts_with("gaia_bot_"), "keygen stdout: {key}");
    let server = HttpServer::start(dir.path());

    // 不正キーは 401
    let mut bad = Rpc { url: server.url.clone(), key: "gaia_bot_wrong".into(), session: None };
    let (status, _, _) = bad.post(serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}));
    assert_eq!(status, 401);

    // 正キー: initialize → initialized → tools/list → tools/call
    let mut rpc = Rpc { url: server.url.clone(), key, session: None };
    let init = rpc.request(1, "initialize", serde_json::json!({
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "it", "version": "0"}
    }));
    assert_eq!(init["result"]["serverInfo"]["name"], "gaia_library");
    let (status, _, _) = rpc.post(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert!(status == 200 || status == 202, "initialized: {status}");

    let listed = rpc.request(2, "tools/list", serde_json::json!({}));
    let names: Vec<&str> =
        listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"search_context"));
    assert!(!names.contains(&"approve_proposal"), "agent には承認系が見えない: {names:?}");

    let called = rpc.request(3, "tools/call", serde_json::json!({"name": "search_context", "arguments": {"query": "okash1n"}}));
    assert_eq!(called["result"]["isError"], serde_json::json!(false));
    assert_eq!(called["result"]["structuredContent"]["entities"][0]["type"], "person");
}
```

（ureq 3 の API 名が違う場合は docs.rs で確認して合わせる。挙動のアサートは変えない。）

- [ ] **Step 2: テスト・ゲートを実行しコミット**

Run: `cargo test -p gaia --test http && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功

```bash
git add crates/gaia Cargo.lock
git commit -m "test: add HTTP transport integration test" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task B6: ドキュメント整合

**Files:**
- Modify: `AGENTS.md`（トランスポート節: stdio ＋ HTTP、キー発行と mcp-config の手順）, `README.md`（HTTP 接続例）, `docs/superpowers/specs/2026-08-27-gaia-library-http-auth-design.md`（§5 に実績追記）

- [ ] **Step 1: AGENTS.md / README.md を更新する**

- AGENTS.md「技術スタック」のトランスポート行を「stdio（`gaia serve --stdio --client <name>`）と Streamable HTTP（`gaia serve --http`、bearer キー。キーは `gaia client keygen`、config にはハッシュのみ）」に更新
- README の接続節に http の例を追加:

````markdown
### HTTP で接続する場合

```sh
gaia client add claude-code --role agent --default-scope <所属元名> --generate-key  # キーが 1 度だけ表示される
gaia serve --http   # 127.0.0.1:4111
gaia client mcp-config claude-code --transport http --key <表示されたキー>
```
````

- [ ] **Step 2: 仕様書 §5 に実績（乖離があれば）を追記し、最終ゲートを実行してコミット**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add AGENTS.md README.md docs
git commit -m "docs: document HTTP transport and key issuance" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

# gaia-library 基盤 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** JSON Schema 契約を正本とする Rust 製ローカル MCP サーバー gaia-library の基盤（契約・SQLite・scope 強制・提案キュー・全ツール・stdio・CLI）を、テスト付きで動く状態にする。

**Architecture:** Cargo workspace 3 crate。`gaia-core`（契約ロード・型生成・SQLite・ドメイン・`ToolService`）→ `gaia-mcp`（rmcp の `ServerHandler` 手動実装、stdio）→ `gaia`（clap CLI）。CLI も MCP も `ToolService::call(client, tool, args)` だけを入口にする。書き込みは提案キュー経由、承認は human ロールのみ、内容層の読み取りは常に `ScopeSet` 付き。

**Tech Stack:** Rust 1.96 stable / edition 2024、rmcp 3.1.4、rusqlite 0.40.2（bundled SQLite 3.53.2、FTS5 trigram）、rusqlite_migration 2.6、jsonschema 0.51、typify 0.7（build.rs）、clap 4、tokio 1、serde、toml 1、unicode-normalization 0.1

**Spec:** `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`（以下「仕様書」。各タスクは仕様書の該当節を参照する）

## Global Constraints

- edition 2024、`rust-version = "1.95"`、`resolver = "3"`
- 依存方向は `gaia` → `gaia-mcp` → `gaia-core` の一方向。`gaia-core` は rmcp を依存に持たない。`gaia-mcp` と `gaia` は `gaia_core::tools::ToolService`、`gaia_core::contracts`、`gaia_core::config`、`gaia_core::identity`、`gaia_core::admin`、`gaia_core::storage::Db` 以外の core API を使わない
- 契約は `contracts/` が正本。変更は契約 → `cargo build`（型・スキーマ再生成）→ 実装の順。生成物（`OUT_DIR`）はコミットしない
- 契約スキーマで使ってよいキーワード: `type` / `properties` / `required` / `additionalProperties` / `enum` / `oneOf` / `items` / `minItems` / `default`（整数のみ）/ `description` / `$ref`（`../defs/common.json#/$defs/X` 形式のみ）。`minLength` / `minimum` / `maximum` / `pattern` / `format` / `if` / `prefixItems` / `unevaluatedProperties` / `dependentSchemas` / `$anchor` / ツールファイル内 `$defs` は使わない。enum は必ず `$defs` に名前付きで定義する
- 内容層（engagements / engagement_people / interactions / facts / refs / glossary / proposals）への SELECT は必ず `scope IN (SELECT value FROM json_each(?))` を付ける。scope なしの読み取り関数を作らない
- `INSERT OR REPLACE` / `REPLACE INTO` は使わない（FTS 索引が壊れる）。`ON CONFLICT ... DO UPDATE` か `UPDATE` を使う
- 全書き込み（propose / approve / reject / admin_write）と複数 scope の読み取りは `audit_log` に actor 付きで残す
- ログは stderr のみ（stdout は MCP の JSON-RPC 専用）
- コミットは Conventional Commits（`feat:` / `fix:` / `test:` / `docs:` / `chore:`）。`Co-Authored-By` は付けない。コミットメッセージ末尾に `Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR` を 1 行入れる
- リポジトリ内の実コード・ドキュメント・コミットに、リポジトリ外の作業用ディレクトリのパスや内容を含めない
- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace` が各タスク末尾で通ること

---

## ファイル構成（全タスク共通の地図）

```
gaia-library/
├── AGENTS.md                                   # Task 1（Task 21 で最終確認）
├── CLAUDE.md -> AGENTS.md                      # Task 1
├── README.md                                   # Task 1 / Task 21
├── Cargo.toml                                  # Task 1（workspace）
├── rust-toolchain.toml                         # Task 1
├── .gitignore                                  # Task 1
├── .github/workflows/ci.yml                    # Task 1
├── scripts/dev.sh                              # Task 1
├── contracts/
│   ├── manifest.json                           # Task 3
│   ├── defs/common.json                        # Task 3
│   └── tools/*.json（13）                       # Task 3（2 件）/ Task 4（11 件）
├── crates/gaia-core/
│   ├── Cargo.toml                              # Task 1 / Task 3（build.rs 追加）
│   ├── build.rs                                # Task 3
│   ├── migrations/0001_init.sql                # Task 5
│   └── src/
│       ├── lib.rs                              # 各タスクで pub mod を追加
│       ├── error.rs                            # Task 2: ErrorCode / ToolError
│       ├── identity.rs                         # Task 2: Role / ClientIdentity
│       ├── contracts/mod.rs                    # Task 3: Catalog / ToolSpec / types
│       ├── storage/mod.rs                      # Task 5: Db / MIGRATIONS / StorageError / like_pattern
│       ├── config.rs                           # Task 6
│       ├── domain/normalize.rs                 # Task 7
│       ├── domain/predicates.rs                # Task 7
│       ├── storage/affiliations.rs             # Task 8
│       ├── storage/audit.rs                    # Task 8
│       ├── scope.rs                            # Task 8: ScopeSet
│       ├── storage/organizations.rs            # Task 9
│       ├── storage/people.rs                   # Task 9
│       ├── storage/entities.rs                 # Task 9
│       ├── storage/targets.rs                  # Task 10
│       ├── storage/engagements.rs              # Task 10
│       ├── storage/interactions.rs             # Task 10
│       ├── storage/facts.rs                    # Task 10
│       ├── storage/refs.rs                     # Task 10
│       ├── storage/glossary.rs                 # Task 10
│       ├── storage/proposals.rs                # Task 11
│       ├── domain/proposals.rs                 # Task 11: patch 検証・適用
│       ├── admin.rs                            # Task 12: affiliation 管理（提案キューの唯一の例外）
│       ├── tools/mod.rs                        # Task 12: ToolService / CallContext / dispatch
│       ├── tools/server_info.rs                # Task 12
│       ├── tools/job_status.rs                 # Task 12
│       ├── tools/propose.rs                    # Task 13: propose_update / list / approve / reject
│       ├── tools/get_person.rs                 # Task 14
│       ├── tools/get_organization.rs           # Task 14
│       ├── tools/get_engagement.rs             # Task 14
│       ├── tools/get_glossary.rs               # Task 15
│       ├── tools/resolve_speakers.rs           # Task 15
│       └── tools/search_context.rs             # Task 16
├── crates/gaia-mcp/src/{lib.rs, server.rs, stdio.rs}   # Task 17
├── crates/gaia/src/main.rs                     # Task 18 / Task 19
├── crates/gaia/src/cli/{mod.rs, app.rs, admin.rs, serve.rs, query.rs, write.rs}  # Task 18 / 19
└── crates/gaia/tests/{cli_flow.rs, stdio.rs}   # Task 20
```

各タスクの「Interfaces」に、隣接タスクが使う正確な関数名・型名を書く。実装者は自分のタスクだけを読む前提なので、名前は必ずこの計画のとおりにする。

---

### Task 1: workspace の雛形・AGENTS.md・CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `README.md`, `AGENTS.md`, `CLAUDE.md`（シンボリックリンク）, `.github/workflows/ci.yml`, `scripts/dev.sh`
- Create: `crates/gaia-core/Cargo.toml`, `crates/gaia-core/src/lib.rs`
- Create: `crates/gaia-mcp/Cargo.toml`, `crates/gaia-mcp/src/lib.rs`
- Create: `crates/gaia/Cargo.toml`, `crates/gaia/src/main.rs`

**Interfaces:**
- Produces: workspace 名 `gaia-core` / `gaia-mcp` / `gaia`（bin 名 `gaia`）。`[workspace.dependencies]` に後続タスクが使う全 crate を登録済み

- [ ] **Step 1: ルート Cargo.toml を書く**

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
license = "MIT"
repository = "https://github.com/btajp/gaia-library"

[workspace.dependencies]
gaia-core = { path = "crates/gaia-core" }
gaia-mcp = { path = "crates/gaia-mcp" }

anyhow = "1"
clap = { version = "4", features = ["derive"] }
jsonschema = { version = "0.51", default-features = false }
rmcp = { version = "3.1", features = ["server", "transport-io"] }
rusqlite = { version = "0.40", features = ["bundled", "serde_json"] }
rusqlite_migration = "2.6"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "io-std"] }
toml = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
unicode-normalization = "0.1"
uuid = { version = "1", features = ["v4"] }
tempfile = "3"

# build-dependencies（gaia-core の build.rs）
typify = "0.7"
schemars = "0.8.22"
prettyplease = "0.2"
syn = { version = "2", features = ["full"] }
```

- [ ] **Step 2: rust-toolchain.toml と .gitignore を書く**

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
```

`.gitignore`:

```
/target
*.db
*.db-wal
*.db-shm
.env
```

- [ ] **Step 3: 3 つの crate を作る**

`crates/gaia-core/Cargo.toml`:

```toml
[package]
name = "gaia-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
jsonschema.workspace = true
rusqlite.workspace = true
rusqlite_migration.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
toml.workspace = true
tracing.workspace = true
unicode-normalization.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/gaia-core/src/lib.rs`:

```rust
//! gaia-library のコア: 契約・ストレージ・ドメイン・ToolService。
//! MCP と CLI はこの crate の `tools::ToolService` だけを入口にする。
```

`crates/gaia-mcp/Cargo.toml`:

```toml
[package]
name = "gaia-mcp"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
gaia-core.workspace = true
rmcp.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
```

`crates/gaia-mcp/src/lib.rs`:

```rust
//! rmcp の ServerHandler を gaia_core::tools::ToolService に接続する薄い層。
```

`crates/gaia/Cargo.toml`:

```toml
[package]
name = "gaia"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "gaia"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
clap.workspace = true
gaia-core.workspace = true
gaia-mcp.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
uuid.workspace = true

[dev-dependencies]
tempfile.workspace = true
serde_json.workspace = true
```

`crates/gaia/src/main.rs`:

```rust
fn main() {
    println!("gaia {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 4: ビルドが通ることを確認する**

Run: `cargo build --workspace && cargo run -p gaia`
Expected: `gaia 0.1.0`

- [ ] **Step 5: AGENTS.md を書き、CLAUDE.md をシンボリックリンクにする**

`AGENTS.md`（仕様書 §12 の項目をすべて含む）:

````markdown
# gaia-library（ガイアライブラリー）

仕事の記憶の索引 MCP サーバー。実記憶の全文ではなく「思い出し方」を保存し、問い合わせには要点＋解決可能な参照からなる「回答の設計図」を返す。相棒は narumi（議事録生成）だが排他ではなく、Claude / Codex など任意のクライアントから使われる。

設計の正本: `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`

## 技術スタック
- Rust（edition 2024、MSRV 1.95）。MCP は公式 rust-sdk（rmcp 3.x）、DB は rusqlite（bundled。FTS5 trigram）。検索の格上げ経路は lindera-sqlite（形態素解析。実際に検索が失敗し始めるまで入れない）
- contracts/: JSON Schema（1 ツール 1 ファイル＋共通 defs、contract_version=semver）。契約が正本。変更は契約 → `cargo build` → 実装の順。型生成は typify（build.rs。生成物はコミットしない）
- トランスポート: ローカル常駐 Streamable HTTP（今後）。現在は stdio（`gaia serve --stdio --client <name>`）

## リポ構成（workspace）
- crates/gaia-core: 契約ロード（`contracts::Catalog`）・SQLite（`storage`）・ドメイン（`domain`）・scope（`scope::ScopeSet`）・`tools::ToolService`。rmcp を知らない
- crates/gaia-mcp: rmcp の `ServerHandler` 手動実装と stdio 起動。依存は gaia-core のみ
- crates/gaia: CLI `gaia`。全コマンドが `ToolService::call` を呼ぶ（設定ファイルと `affiliations` の管理コマンドだけが例外）
- 依存方向は gaia → gaia-mcp → gaia-core の一方向。gaia-mcp と gaia は `ToolService` / `contracts` / `config` / `identity` / `admin` / `storage::Db` 以外の core API を使わない

## データモデル（DDL v1）
- 名寄せ層（共有・scope なし）: people / person_aliases / organizations / entities / affiliations（scope の値域定義）
- 内容層（scope NOT NULL）: engagements / engagement_people / interactions / facts / refs / glossary / proposals。規則は「名寄せは共有、内容は境界内」
- facts: statement（自由文）必須＋predicate / value は任意構造化。kind = fact | inference を必ず区別。上書き・履歴は superseded_by。構造化 predicate の初期レジストリは role / status / interest / decision（`domain::predicates`）。レジストリ外の predicate は拒否し、自由文のみで登録する
- refs: エンティティにもファクトにも紐付け可（polymorphic）。source / provenance は refs に 1 系統化。参照は自己記述的に（system / uri / title / note / snapshot / last_verified）。URI だけの参照は登録禁止
- polymorphic（entity_type＋entity_id / target_type＋target_id）の整合はアプリ層で書き込み時に検証する
- 人物の name と alias は正規化した文字列（`domain::normalize::normalize_name`）を kind='normalized' の行として自動登録する

## 絶対原則
1. 書き込みは提案キュー（propose_update）経由のみ。承認（approve / reject）は human ロールのみ — manifest の roles で tool list から隠し、かつ `ToolService::call` でも認可チェックする。唯一の例外は affiliations の管理コマンド（機密境界の定義そのものなので `admin` 経由で直接書き、audit_log(admin_write) に残す）
2. scope は default deny / explicit allow。内容層の SELECT には必ず `scope IN (SELECT value FROM json_each(?))` を付ける。複数 scope の明示指定時のみ横断し、横断は audit_log(cross_scope_read) に記録。省略時はクライアントの default_scope を使う
3. 参照は必ず解決可能に: 辿れない参照だけを返すサーバーにしない。到達不能時は要点スナップショットをフォールバックに。到達可能性 > 正本一元化（二重管理は許容）
4. RAG・埋め込みは入れない（FTS で実際に困るまで）
5. stdio の役割分離は「エージェントが MCP 経由で誤って承認する」ことを防ぐ仕組みであり、同一 OS ユーザーのシェルから human 識別で起動することは防げない。API キー検証は HTTP 実装時に追加する

## 公開ツール（v1 契約、contracts/manifest.json が正本）
- 参照系（readOnlyHint）: search_context / get_person / get_organization / get_engagement / get_glossary / resolve_speakers / resolve_source（契約のみ。未登録）
- 提案系: propose_update / list_proposals / approve_proposal（human）/ reject_proposal（human）
- 共通: get_server_info / get_job_status（v1 は常に not_found）
- 書き込みはクライアント発番の request_id で冪等化。エラーは構造化コード（not_found / scope_denied / unauthorized / invalid_params / contract_mismatch / conflict / busy / not_implemented / internal）

## 開発ルール
- テスト: `cargo test --workspace`。lint: `cargo fmt --all --check` と `cargo clippy --workspace --all-targets -- -D warnings`
- `scripts/dev.sh` は narumi をサブプロセス起動する（NARUMI_BIN 未設定ならスキップ）。narumi 無しでも全テストが通ること（任意依存）
- 契約の書き方: `contracts/tools/<name>.json` は MCP の Tool オブジェクト 1 つ。共通型は `../defs/common.json#/$defs/X` で参照。`minLength` / `minimum` / `maximum` / `pattern` / `format` / `if` / `prefixItems` は使わない（typify の制約）。enum は必ず `$defs` に定義する
- FTS: `INSERT OR REPLACE` 禁止（`ON CONFLICT DO UPDATE` を使う）。同期はトリガで行う
- 検索は person_aliases の完全一致（正規化済み別行）＋ facts_fts（trigram。3 文字未満は LIKE）の併用
- ログは stderr のみ（stdout は JSON-RPC 専用）
- コミット: Conventional Commits。`Co-Authored-By` は付けない
````

Run: `ln -s AGENTS.md CLAUDE.md && ls -la CLAUDE.md`
Expected: `CLAUDE.md -> AGENTS.md`

- [ ] **Step 6: README.md、CI、dev.sh を書く**

`README.md`:

```markdown
# gaia-library

仕事の記憶の「思い出し方」を索引として保存し、問い合わせに要点＋解決可能な参照からなる「回答の設計図」を返すローカル MCP サーバー。

- 設計: `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`
- エージェント向け指示: `AGENTS.md`

## ビルドとテスト

```sh
cargo build --workspace
cargo test --workspace
```
```

`.github/workflows/ci.yml`:

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```

`scripts/dev.sh`（`chmod +x`）:

```bash
#!/usr/bin/env bash
# narumi をサブプロセス起動してから gaia を実行する開発用スクリプト。
# narumi は任意依存: NARUMI_BIN が未設定または実行不可ならスキップして続行する。
set -euo pipefail
if [[ -n "${NARUMI_BIN:-}" && -x "${NARUMI_BIN}" ]]; then
  "${NARUMI_BIN}" serve --stdio &
  NARUMI_PID=$!
  trap 'kill "${NARUMI_PID}" 2>/dev/null || true' EXIT
  echo "narumi started (pid ${NARUMI_PID})" >&2
else
  echo "narumi not found (NARUMI_BIN unset); continuing without it" >&2
fi
exec cargo run -p gaia -- "$@"
```

- [ ] **Step 7: lint とテストを通す**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功（テストは 0 件で OK）

- [ ] **Step 8: コミット**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore README.md AGENTS.md CLAUDE.md .github scripts crates
git commit -m "chore: scaffold workspace, AGENTS.md and CI" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 2: ErrorCode / ToolError / Role / ClientIdentity

**Files:**
- Create: `crates/gaia-core/src/error.rs`, `crates/gaia-core/src/identity.rs`
- Modify: `crates/gaia-core/src/lib.rs`

**Interfaces:**
- Produces: `gaia_core::error::{ErrorCode, ToolError}`、`gaia_core::identity::{Role, ClientIdentity}`
  - `ErrorCode::{NotFound, ScopeDenied, Unauthorized, InvalidParams, ContractMismatch, Conflict, Busy, NotImplemented, Internal}`、`as_str()`、`is_protocol_error()`
  - `ToolError { code, message, details: Option<Value> }`、コンストラクタ `not_found / scope_denied / unauthorized / invalid_params / conflict / busy / not_implemented / internal`、`with_details(Value)`、`to_json() -> Value`
  - `Role::{Human, Agent}`（serde は小文字）、`as_str()`、`FromStr`
  - `ClientIdentity { name: String, role: Role, default_scope: Option<String> }`

- [ ] **Step 1: テストを書く（error.rs の末尾に置く前提で先に用意）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_serializes_as_snake_case() {
        assert_eq!(serde_json::to_value(ErrorCode::ScopeDenied).unwrap(), "scope_denied");
        assert_eq!(ErrorCode::NotFound.as_str(), "not_found");
    }

    #[test]
    fn protocol_errors_are_unauthorized_invalid_params_contract_mismatch() {
        assert!(ErrorCode::Unauthorized.is_protocol_error());
        assert!(ErrorCode::InvalidParams.is_protocol_error());
        assert!(ErrorCode::ContractMismatch.is_protocol_error());
        assert!(!ErrorCode::NotFound.is_protocol_error());
        assert!(!ErrorCode::ScopeDenied.is_protocol_error());
    }

    #[test]
    fn tool_error_to_json_includes_details() {
        let e = ToolError::invalid_params("bad").with_details(serde_json::json!({"path": "/x"}));
        let v = e.to_json();
        assert_eq!(v["code"], "invalid_params");
        assert_eq!(v["message"], "bad");
        assert_eq!(v["details"]["path"], "/x");
        assert_eq!(e.to_string(), "invalid_params: bad");
    }

    #[test]
    fn busy_sqlite_error_maps_to_busy() {
        let e = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".into()),
        );
        assert_eq!(ToolError::from(e).code, ErrorCode::Busy);
    }
}
```

- [ ] **Step 2: error.rs を実装する**

```rust
//! ツール層の構造化エラー。仕様書 §8.2。
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    ScopeDenied,
    Unauthorized,
    InvalidParams,
    ContractMismatch,
    Conflict,
    Busy,
    NotImplemented,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::ScopeDenied => "scope_denied",
            Self::Unauthorized => "unauthorized",
            Self::InvalidParams => "invalid_params",
            Self::ContractMismatch => "contract_mismatch",
            Self::Conflict => "conflict",
            Self::Busy => "busy",
            Self::NotImplemented => "not_implemented",
            Self::Internal => "internal",
        }
    }

    /// JSON-RPC エラーとして返すべき「プロトコル違反」か（それ以外は isError の結果として返す）。
    pub fn is_protocol_error(self) -> bool {
        matches!(self, Self::Unauthorized | Self::InvalidParams | Self::ContractMismatch)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<Value>,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), details: None }
    }
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
    pub fn not_found(m: impl Into<String>) -> Self { Self::new(ErrorCode::NotFound, m) }
    pub fn scope_denied(m: impl Into<String>) -> Self { Self::new(ErrorCode::ScopeDenied, m) }
    pub fn unauthorized(m: impl Into<String>) -> Self { Self::new(ErrorCode::Unauthorized, m) }
    pub fn invalid_params(m: impl Into<String>) -> Self { Self::new(ErrorCode::InvalidParams, m) }
    pub fn conflict(m: impl Into<String>) -> Self { Self::new(ErrorCode::Conflict, m) }
    pub fn busy(m: impl Into<String>) -> Self { Self::new(ErrorCode::Busy, m) }
    pub fn not_implemented(m: impl Into<String>) -> Self { Self::new(ErrorCode::NotImplemented, m) }
    pub fn internal(m: impl Into<String>) -> Self { Self::new(ErrorCode::Internal, m) }

    /// 契約の ErrorObject 形 `{ code, message, details }`。
    pub fn to_json(&self) -> Value {
        json!({ "code": self.code.as_str(), "message": self.message, "details": self.details })
    }
}

impl From<rusqlite::Error> for ToolError {
    fn from(e: rusqlite::Error) -> Self {
        let busy = matches!(
            &e,
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::DatabaseBusy || f.code == rusqlite::ErrorCode::DatabaseLocked
        );
        if busy { Self::busy(e.to_string()) } else { Self::internal(format!("sqlite: {e}")) }
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        Self::internal(format!("json: {e}"))
    }
}
```

（`rusqlite::ffi::Error::new(SQLITE_BUSY)` はテスト用。`rusqlite::ffi` は `libsqlite3_sys` の再 export で、`Error::new(code: c_int)` がある。）

- [ ] **Step 3: identity.rs を実装する**

```rust
//! クライアント識別。仕様書 §7.1。
use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Human,
    Agent,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "human" => Ok(Self::Human),
            "agent" => Ok(Self::Agent),
            other => Err(format!("unknown role `{other}` (expected human|agent)")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientIdentity {
    pub name: String,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_scope: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_round_trips_lowercase() {
        assert_eq!("agent".parse::<Role>().unwrap(), Role::Agent);
        assert_eq!(serde_json::to_value(Role::Human).unwrap(), "human");
        assert!("admin".parse::<Role>().is_err());
    }

    #[test]
    fn client_identity_omits_missing_default_scope() {
        let c = ClientIdentity { name: "x".into(), role: Role::Agent, default_scope: None };
        assert_eq!(serde_json::to_value(&c).unwrap(), serde_json::json!({"name": "x", "role": "agent"}));
    }
}
```

`lib.rs` に追加:

```rust
pub mod error;
pub mod identity;
```

- [ ] **Step 4: テストを実行する**

Run: `cargo test -p gaia-core`
Expected: 6 tests passed

- [ ] **Step 5: コミット**

```bash
git add crates/gaia-core
git commit -m "feat(core): add ErrorCode/ToolError and Role/ClientIdentity" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 3: 契約パイプライン（manifest / common.json / 2 ツール / build.rs / Catalog）

**Files:**
- Create: `contracts/manifest.json`, `contracts/defs/common.json`, `contracts/tools/get_server_info.json`, `contracts/tools/get_job_status.json`
- Create: `crates/gaia-core/build.rs`, `crates/gaia-core/src/contracts/mod.rs`
- Modify: `crates/gaia-core/Cargo.toml`（build-dependencies）, `crates/gaia-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `gaia_core::contracts::types::*`（typify 生成。ツール `foo_bar` → `FooBarInput` / `FooBarOutput`。`$defs` の名前がそのまま型名）
  - `gaia_core::contracts::{Catalog, ToolSpec, ToolAnnotationsSpec, ContractError}`
  - `Catalog::embedded() -> Result<Catalog, ContractError>`、`Catalog::get(&self, name: &str) -> Option<&ToolSpec>`、`Catalog::tools(&self) -> &[ToolSpec]`、`Catalog::visible(&self, role: Role) -> Vec<&ToolSpec>`、フィールド `contract_version: String`、`server_name: String`
  - `ToolSpec { name, title: Option<String>, description, roles: Vec<Role>, enabled: bool, annotations: ToolAnnotationsSpec, input_schema: Value, output_schema: Option<Value> }`、`allows(&self, role) -> bool`、`validate_input(&self, &Value) -> Result<(), ToolError>`、`validate_output(&self, &Value) -> Result<(), ToolError>`
  - `ToolAnnotationsSpec { read_only_hint, destructive_hint, idempotent_hint, open_world_hint: bool }`

- [ ] **Step 1: contracts/manifest.json を書く（13 ツール分。ファイルは Task 4 で揃うが、build.rs は存在しないファイルをエラーにするので、このタスクでは 2 件だけ `tools` に載せ、Task 4 で残りを追加する）**

```json
{
  "contract_version": "1.0.0",
  "server_name": "gaia_library",
  "defs": "defs/common.json",
  "tools": [
    { "name": "get_server_info", "file": "tools/get_server_info.json", "roles": ["human", "agent"], "enabled": true },
    { "name": "get_job_status",  "file": "tools/get_job_status.json",  "roles": ["human", "agent"], "enabled": true }
  ]
}
```

- [ ] **Step 2: contracts/defs/common.json を書く（全ツールが使う共通型。ここで全部定義する）**

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "gaia-library common definitions",
  "$defs": {
    "ScopeInput": {
      "description": "所属元 scope。単一名か配列。配列で 2 つ以上指定したときだけ横断し、監査ログに残る。省略時はクライアントの既定 scope",
      "oneOf": [
        { "type": "string" },
        { "type": "array", "items": { "type": "string" }, "minItems": 1 }
      ]
    },
    "Kind": { "type": "string", "enum": ["fact", "inference"], "description": "fact=確認済みの事実 / inference=推測" },
    "EntityType": { "type": "string", "enum": ["person", "organization", "engagement", "interaction", "entity"] },
    "RefTargetType": { "type": "string", "enum": ["person", "organization", "engagement", "interaction", "entity", "fact"] },
    "ProposalTargetType": { "type": "string", "enum": ["person", "organization", "engagement", "interaction", "entity", "fact", "ref", "glossary"] },
    "ProposalAction": { "type": "string", "enum": ["insert", "update", "supersede"], "description": "supersede は fact のみ（旧 fact を superseded_by で置き換える）" },
    "ProposalStatus": { "type": "string", "enum": ["pending", "approved", "rejected"] },
    "SearchType": { "type": "string", "enum": ["person", "organization", "engagement", "entity", "interaction", "glossary"] },
    "SearchEntityType": { "type": "string", "enum": ["person", "organization", "engagement", "entity", "interaction"] },
    "SpeakerStatus": { "type": "string", "enum": ["matched", "ambiguous", "unmatched"] },
    "ErrorCode": { "type": "string", "enum": ["not_found", "scope_denied", "unauthorized", "invalid_params", "contract_mismatch", "conflict", "busy", "not_implemented", "internal"] },
    "ErrorObject": {
      "type": "object",
      "required": ["code", "message"],
      "properties": {
        "code": { "$ref": "#/$defs/ErrorCode" },
        "message": { "type": "string" },
        "details": {}
      }
    },
    "Alias": {
      "type": "object",
      "required": ["alias"],
      "additionalProperties": false,
      "properties": {
        "alias": { "type": "string", "description": "表示名・ローマ字・ニックネーム" },
        "kind": { "type": "string", "description": "display_name / romaji / nickname など任意" }
      }
    },
    "PersonSummary": {
      "type": "object",
      "required": ["id", "name", "aliases"],
      "properties": {
        "id": { "type": "integer" },
        "name": { "type": "string" },
        "org_id": { "type": "integer" },
        "org_name": { "type": "string" },
        "role": { "type": "string" },
        "first_met": { "type": "string" },
        "last_seen": { "type": "string" },
        "aliases": { "type": "array", "items": { "$ref": "#/$defs/Alias" } }
      }
    },
    "OrganizationSummary": {
      "type": "object",
      "required": ["id", "name"],
      "properties": {
        "id": { "type": "integer" },
        "name": { "type": "string" },
        "kind": { "type": "string", "description": "customer / partner / affiliation など" }
      }
    },
    "EngagementSummary": {
      "type": "object",
      "required": ["id", "name", "scope"],
      "properties": {
        "id": { "type": "integer" },
        "name": { "type": "string" },
        "org_id": { "type": "integer" },
        "org_name": { "type": "string" },
        "scope": { "type": "string" },
        "status": { "type": "string" },
        "started_at": { "type": "string" },
        "ended_at": { "type": "string" }
      }
    },
    "EngagementPerson": {
      "type": "object",
      "required": ["person"],
      "properties": {
        "person": { "$ref": "#/$defs/PersonSummary" },
        "role": { "type": "string", "description": "key_person / member / contact など" }
      }
    },
    "InteractionSummary": {
      "type": "object",
      "required": ["id", "kind", "occurred_at", "summary", "scope", "person_ids"],
      "properties": {
        "id": { "type": "integer" },
        "kind": { "type": "string", "description": "meeting / call / chat / mail など" },
        "occurred_at": { "type": "string", "description": "ISO 8601" },
        "summary": { "type": "string" },
        "engagement_id": { "type": "integer" },
        "scope": { "type": "string" },
        "person_ids": { "type": "array", "items": { "type": "integer" } }
      }
    },
    "EntitySummary": {
      "type": "object",
      "required": ["id", "type", "name", "attrs"],
      "properties": {
        "id": { "type": "integer" },
        "type": { "type": "string" },
        "name": { "type": "string" },
        "attrs": { "type": "object" }
      }
    },
    "Fact": {
      "type": "object",
      "required": ["id", "entity_type", "entity_id", "statement", "kind", "scope", "created_at"],
      "properties": {
        "id": { "type": "integer" },
        "entity_type": { "$ref": "#/$defs/EntityType" },
        "entity_id": { "type": "integer" },
        "statement": { "type": "string" },
        "predicate": { "type": "string" },
        "value": { "type": "string" },
        "kind": { "$ref": "#/$defs/Kind" },
        "scope": { "type": "string" },
        "valid_from": { "type": "string" },
        "superseded_by": { "type": "integer" },
        "created_at": { "type": "string" }
      }
    },
    "Reference": {
      "type": "object",
      "required": ["id", "target_type", "target_id", "system", "uri", "note", "scope", "created_at"],
      "properties": {
        "id": { "type": "integer" },
        "target_type": { "$ref": "#/$defs/RefTargetType" },
        "target_id": { "type": "integer" },
        "system": { "type": "string", "description": "notion / box / minutes / mail / file / url など" },
        "uri": { "type": "string" },
        "title": { "type": "string" },
        "note": { "type": "string", "description": "何が・どの粒度で・いつ時点の情報か" },
        "snapshot": { "type": "string", "description": "登録時点の要点（到達不能時のフォールバック）" },
        "scope": { "type": "string" },
        "last_verified": { "type": "string" },
        "created_at": { "type": "string" }
      }
    },
    "GlossaryTerm": {
      "type": "object",
      "required": ["id", "term", "scope"],
      "properties": {
        "id": { "type": "integer" },
        "term": { "type": "string" },
        "reading": { "type": "string" },
        "definition": { "type": "string" },
        "engagement_id": { "type": "integer" },
        "scope": { "type": "string" }
      }
    },
    "Provenance": {
      "type": "object",
      "additionalProperties": false,
      "description": "出所。ref_id で既存 ref を指すか、system/uri/note（title/snapshot 任意）で新規 ref を承認時に登録する",
      "properties": {
        "ref_id": { "type": "integer" },
        "system": { "type": "string" },
        "uri": { "type": "string" },
        "title": { "type": "string" },
        "note": { "type": "string" },
        "snapshot": { "type": "string" }
      }
    },
    "Proposal": {
      "type": "object",
      "required": ["id", "action", "target_type", "patch", "kind", "scope", "proposed_by", "request_id", "status", "created_at"],
      "properties": {
        "id": { "type": "integer" },
        "action": { "$ref": "#/$defs/ProposalAction" },
        "target_type": { "$ref": "#/$defs/ProposalTargetType" },
        "target_id": { "type": "integer" },
        "patch": { "type": "object" },
        "kind": { "$ref": "#/$defs/Kind" },
        "scope": { "type": "string" },
        "provenance": { "$ref": "#/$defs/Provenance" },
        "provenance_id": { "type": "integer" },
        "proposed_by": { "type": "string" },
        "request_id": { "type": "string" },
        "status": { "$ref": "#/$defs/ProposalStatus" },
        "result_id": { "type": "integer" },
        "decision_note": { "type": "string" },
        "created_at": { "type": "string" },
        "decided_at": { "type": "string" },
        "decided_by": { "type": "string" }
      }
    },
    "ApplyResult": {
      "type": "object",
      "required": ["target_type", "id"],
      "properties": {
        "target_type": { "$ref": "#/$defs/ProposalTargetType" },
        "id": { "type": "integer" }
      }
    },
    "PersonPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert 時は name 必須。aliases は追加のみ",
      "properties": {
        "name": { "type": "string" },
        "org_id": { "type": "integer" },
        "role": { "type": "string" },
        "aliases": { "type": "array", "items": { "$ref": "#/$defs/Alias" } },
        "first_met": { "type": "string" },
        "last_seen": { "type": "string" }
      }
    },
    "OrganizationPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert 時は name 必須",
      "properties": {
        "name": { "type": "string" },
        "kind": { "type": "string" }
      }
    },
    "EngagementPersonInput": {
      "type": "object",
      "required": ["person_id"],
      "additionalProperties": false,
      "properties": {
        "person_id": { "type": "integer" },
        "role": { "type": "string" }
      }
    },
    "EngagementPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert 時は name 必須。people は追加のみ",
      "properties": {
        "name": { "type": "string" },
        "org_id": { "type": "integer" },
        "status": { "type": "string" },
        "started_at": { "type": "string" },
        "ended_at": { "type": "string" },
        "people": { "type": "array", "items": { "$ref": "#/$defs/EngagementPersonInput" } }
      }
    },
    "InteractionPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert 時は kind / occurred_at / summary 必須。person_ids は追加のみ",
      "properties": {
        "kind": { "type": "string" },
        "occurred_at": { "type": "string" },
        "summary": { "type": "string" },
        "engagement_id": { "type": "integer" },
        "person_ids": { "type": "array", "items": { "type": "integer" } }
      }
    },
    "EntityPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert 時は type / name 必須",
      "properties": {
        "type": { "type": "string" },
        "name": { "type": "string" },
        "attrs": { "type": "object" }
      }
    },
    "FactPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert / supersede 時は entity_type / entity_id / statement 必須。predicate はレジストリ（role / status / interest / decision）にあるときのみ許可し value 必須",
      "properties": {
        "entity_type": { "$ref": "#/$defs/EntityType" },
        "entity_id": { "type": "integer" },
        "statement": { "type": "string" },
        "predicate": { "type": "string" },
        "value": { "type": "string" },
        "valid_from": { "type": "string" }
      }
    },
    "RefPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert 時は target_type / target_id / system / uri / note 必須（URI だけの参照は禁止）",
      "properties": {
        "target_type": { "$ref": "#/$defs/RefTargetType" },
        "target_id": { "type": "integer" },
        "system": { "type": "string" },
        "uri": { "type": "string" },
        "title": { "type": "string" },
        "note": { "type": "string" },
        "snapshot": { "type": "string" },
        "last_verified": { "type": "string" }
      }
    },
    "GlossaryPatch": {
      "type": "object",
      "additionalProperties": false,
      "description": "insert 時は term 必須",
      "properties": {
        "term": { "type": "string" },
        "reading": { "type": "string" },
        "definition": { "type": "string" },
        "engagement_id": { "type": "integer" }
      }
    },
    "SearchEntity": {
      "type": "object",
      "required": ["type", "id", "name", "summary", "score", "matched_on", "facts", "refs"],
      "properties": {
        "type": { "$ref": "#/$defs/SearchEntityType" },
        "id": { "type": "integer" },
        "name": { "type": "string" },
        "summary": { "type": "string" },
        "score": { "type": "number" },
        "matched_on": { "type": "array", "items": { "type": "string" } },
        "facts": { "type": "array", "items": { "$ref": "#/$defs/Fact" } },
        "refs": { "type": "array", "items": { "$ref": "#/$defs/Reference" } }
      }
    },
    "SpeakerCandidate": {
      "type": "object",
      "required": ["person_id", "name", "confidence", "reason"],
      "properties": {
        "person_id": { "type": "integer" },
        "name": { "type": "string" },
        "confidence": { "type": "number" },
        "reason": { "type": "string" }
      }
    },
    "SpeakerResult": {
      "type": "object",
      "required": ["input", "normalized", "status", "confidence", "candidates"],
      "properties": {
        "input": { "type": "string" },
        "normalized": { "type": "string" },
        "status": { "$ref": "#/$defs/SpeakerStatus" },
        "person": { "$ref": "#/$defs/PersonSummary" },
        "confidence": { "type": "number" },
        "candidates": { "type": "array", "items": { "$ref": "#/$defs/SpeakerCandidate" } }
      }
    },
    "ServerProtocolInfo": {
      "type": "object",
      "required": ["transports"],
      "properties": { "transports": { "type": "array", "items": { "type": "string" } } }
    },
    "SearchCapabilities": {
      "type": "object",
      "required": ["fts"],
      "properties": { "fts": { "type": "string" } }
    },
    "ServerCapabilitiesInfo": {
      "type": "object",
      "required": ["tools", "resolvers", "search"],
      "properties": {
        "tools": { "type": "array", "items": { "type": "string" } },
        "resolvers": { "type": "array", "items": { "type": "string" } },
        "search": { "$ref": "#/$defs/SearchCapabilities" }
      }
    },
    "ClientInfo": {
      "type": "object",
      "required": ["name", "role"],
      "properties": {
        "name": { "type": "string" },
        "role": { "type": "string" },
        "default_scope": { "type": "string" }
      }
    }
  }
}
```

- [ ] **Step 3: 最初の 2 ツールの契約ファイルを書く**

`contracts/tools/get_server_info.json`:

```json
{
  "name": "get_server_info",
  "title": "サーバー情報",
  "description": "gaia_library の版・契約版（contract_version）・利用できる能力と、接続中クライアントの識別（name / role / default_scope）を返す。互換性確認と、どの scope が既定かの確認に使う。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": { "type": "object", "additionalProperties": false, "properties": {} },
  "outputSchema": {
    "type": "object",
    "required": ["name", "version", "contract_version", "protocol", "capabilities", "client"],
    "properties": {
      "name": { "type": "string" },
      "version": { "type": "string" },
      "contract_version": { "type": "string" },
      "protocol": { "$ref": "../defs/common.json#/$defs/ServerProtocolInfo" },
      "capabilities": { "$ref": "../defs/common.json#/$defs/ServerCapabilitiesInfo" },
      "client": { "$ref": "../defs/common.json#/$defs/ClientInfo" }
    }
  }
}
```

`contracts/tools/get_job_status.json`:

```json
{
  "name": "get_job_status",
  "title": "ジョブ状態",
  "description": "長時間処理のジョブ状態を返す共通ツール。gaia_library v1 にはジョブが無いため常に not_found を返す（narumi と共通規約を揃えるために存在する）。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "required": ["job_id"],
    "additionalProperties": false,
    "properties": { "job_id": { "type": "string" } }
  },
  "outputSchema": {
    "type": "object",
    "required": ["job_id", "status"],
    "properties": {
      "job_id": { "type": "string" },
      "status": { "type": "string" },
      "result": {}
    }
  }
}
```

- [ ] **Step 4: gaia-core の Cargo.toml に build.rs と build-dependencies を追加する**

```toml
[package]
# （既存の行はそのまま）
build = "build.rs"

[build-dependencies]
typify.workspace = true
schemars.workspace = true
serde_json.workspace = true
prettyplease.workspace = true
syn.workspace = true
```

- [ ] **Step 5: build.rs を書く**

```rust
//! contracts/ を読み、(1) 外部 $ref を局所化して $defs を同梱した自己完結スキーマの束 contracts.json、
//! (2) typify による Rust 型 contract_types.rs を OUT_DIR に生成する。契約の誤りはビルドエラーにする。
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value, json};
use typify::{TypeSpace, TypeSpaceSettings};

/// `../defs/common.json#/$defs/X` → `#/$defs/X`
fn localize_refs(v: &mut Value) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get_mut("$ref") {
                if let Some(idx) = r.find('#') {
                    if idx > 0 {
                        *r = r[idx..].to_string();
                    }
                }
            }
            for (_, child) in map.iter_mut() {
                localize_refs(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(localize_refs),
        _ => {}
    }
}

/// スキーマが推移的に参照する $defs 名を集める。
fn collect_refs(v: &Value, pool: &Map<String, Value>, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(map) => {
            if let Some(Value::String(r)) = map.get("$ref") {
                if let Some(name) = r.strip_prefix("#/$defs/") {
                    if out.insert(name.to_string()) {
                        let def = pool
                            .get(name)
                            .unwrap_or_else(|| panic!("$ref to unknown $defs `{name}`"));
                        collect_refs(def, pool, out);
                    }
                }
            }
            for child in map.values() {
                collect_refs(child, pool, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_refs(i, pool, out)),
        _ => {}
    }
}

/// 参照している $defs だけを同梱した自己完結スキーマを返す。
fn self_contained(schema: &Value, pool: &Map<String, Value>) -> Value {
    let mut used = BTreeSet::new();
    collect_refs(schema, pool, &mut used);
    let mut out = schema.clone();
    if !used.is_empty() {
        let defs: Map<String, Value> = used.into_iter().map(|n| (n.clone(), pool[&n].clone())).collect();
        out["$defs"] = Value::Object(defs);
    }
    out
}

fn pascal(s: &str) -> String {
    s.split(['_', '-', '.'])
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let contracts = manifest_dir.join("../../contracts");
    println!("cargo:rerun-if-changed={}", contracts.display());

    let manifest = read_json(&contracts.join("manifest.json"));
    let mut common = read_json(&contracts.join(manifest["defs"].as_str().expect("manifest.defs")));
    localize_refs(&mut common);
    let pool: Map<String, Value> = common["$defs"].as_object().expect("common.json $defs").clone();

    let mut bundle_tools = Vec::new();
    let mut typed: Vec<(String, Value)> = Vec::new();
    for t in manifest["tools"].as_array().expect("manifest.tools") {
        let name = t["name"].as_str().expect("tool.name");
        let file = contracts.join(t["file"].as_str().expect("tool.file"));
        let mut tool = read_json(&file);
        assert_eq!(tool["name"].as_str(), Some(name), "{}: name mismatch with manifest", file.display());
        localize_refs(&mut tool);
        assert!(tool.get("$defs").is_none(), "{}: tool files must not define $defs", file.display());

        let input = tool.get("inputSchema").unwrap_or_else(|| panic!("{}: inputSchema missing", file.display()));
        assert_eq!(input["type"].as_str(), Some("object"), "{}: inputSchema.type must be object", file.display());
        let input_sc = self_contained(input, &pool);
        typed.push((format!("{}Input", pascal(name)), input_sc.clone()));

        let output_sc = tool.get("outputSchema").map(|o| self_contained(o, &pool));
        if let Some(o) = &output_sc {
            typed.push((format!("{}Output", pascal(name)), o.clone()));
        }

        bundle_tools.push(json!({
            "name": name,
            "title": tool.get("title"),
            "description": tool["description"].as_str().unwrap_or_else(|| panic!("{}: description missing", file.display())),
            "roles": t["roles"],
            "enabled": t["enabled"].as_bool().unwrap_or(true),
            "annotations": tool.get("annotations").cloned().unwrap_or(json!({})),
            "inputSchema": input_sc,
            "outputSchema": output_sc,
        }));
    }
    let bundle = json!({
        "contract_version": manifest["contract_version"],
        "server_name": manifest["server_name"],
        "tools": bundle_tools,
    });

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("contracts.json"), serde_json::to_string_pretty(&bundle).unwrap()).unwrap();

    // typify: 共通 $defs を 1 回だけ登録してから、ツールごとの Input/Output を名前付きで登録する。
    let mut settings = TypeSpaceSettings::default();
    settings.with_struct_builder(false).with_derive("PartialEq".to_string());
    let mut ts = TypeSpace::new(&settings);
    ts.add_ref_types(pool.iter().map(|(k, v)| {
        (
            k.clone(),
            serde_json::from_value::<schemars::schema::Schema>(v.clone())
                .unwrap_or_else(|e| panic!("$defs.{k} is not a valid schema: {e}")),
        )
    }))
    .expect("add common $defs");
    for (type_name, mut schema) in typed {
        // $defs は共通プールで登録済みなので、typify に渡す前に外す
        if let Value::Object(m) = &mut schema {
            m.remove("$defs");
        }
        let schema: schemars::schema::Schema =
            serde_json::from_value(schema).unwrap_or_else(|e| panic!("{type_name}: invalid schema: {e}"));
        ts.add_type_with_name(&schema, Some(type_name.clone()))
            .unwrap_or_else(|e| panic!("{type_name}: typify failed: {e}"));
    }
    let code = prettyplease::unparse(&syn::parse2::<syn::File>(ts.to_stream()).expect("generated code parses"));
    fs::write(out_dir.join("contract_types.rs"), code).unwrap();
    assert!(!ts.uses_regress(), "contracts must not use `pattern` (would require the regress crate)");
    assert!(!ts.uses_chrono(), "contracts must not use `format: date-time` (would require chrono)");
}
```

- [ ] **Step 6: contracts/mod.rs のテストを先に書く**

`crates/gaia-core/src/contracts/mod.rs` の末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn embedded_catalog_loads_all_tools() {
        let c = Catalog::embedded().expect("catalog");
        assert_eq!(c.server_name, "gaia_library");
        assert_eq!(c.contract_version, "1.0.0");
        assert!(c.get("get_server_info").is_some());
        assert!(c.get("get_job_status").is_some());
        assert!(c.get("nope").is_none());
    }

    #[test]
    fn schemas_are_self_contained() {
        let c = Catalog::embedded().unwrap();
        let text = serde_json::to_string(&c.get("get_server_info").unwrap().output_schema).unwrap();
        assert!(!text.contains("common.json"), "external $ref leaked: {text}");
        assert!(text.contains("\"ClientInfo\""));
    }

    #[test]
    fn validate_input_reports_path_and_message() {
        let c = Catalog::embedded().unwrap();
        let spec = c.get("get_job_status").unwrap();
        assert!(spec.validate_input(&json!({"job_id": "j1"})).is_ok());
        let err = spec.validate_input(&json!({"job_id": 1, "extra": true})).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidParams);
        let details = err.details.unwrap();
        let paths: Vec<&str> = details["errors"].as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"/job_id"), "{paths:?}");
    }

    #[test]
    fn visible_filters_by_role_and_enabled() {
        let c = Catalog::embedded().unwrap();
        let names: Vec<&str> = c.visible(Role::Agent).iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"get_server_info"));
    }

    #[test]
    fn generated_types_round_trip() {
        let v = json!({"job_id": "abc"});
        let input: types::GetJobStatusInput = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&input).unwrap(), v);
        let out = types::GetJobStatusOutput { job_id: "abc".into(), status: "unknown".into(), result: None };
        assert_eq!(serde_json::to_value(&out).unwrap()["status"], "unknown");
    }
}
```

- [ ] **Step 7: contracts/mod.rs を実装する**

```rust
//! 契約カタログ。build.rs が生成した自己完結スキーマの束（contracts.json）と typify 型（contract_types.rs）を読む。
use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{error::ToolError, identity::Role};

/// typify が契約から生成した型。ツール `foo_bar` → `FooBarInput` / `FooBarOutput`、`$defs` の名前はそのまま型名。
pub mod types {
    #![allow(clippy::all, dead_code, unused_imports)]
    include!(concat!(env!("OUT_DIR"), "/contract_types.rs"));
}

const BUNDLE: &str = include_str!(concat!(env!("OUT_DIR"), "/contracts.json"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolAnnotationsSpec {
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
}

impl Default for ToolAnnotationsSpec {
    fn default() -> Self {
        // MCP 仕様の既定値
        Self { read_only_hint: false, destructive_hint: true, idempotent_hint: false, open_world_hint: true }
    }
}

#[derive(Debug, Deserialize)]
struct RawBundle {
    contract_version: String,
    server_name: String,
    tools: Vec<RawTool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTool {
    name: String,
    #[serde(default)]
    title: Option<String>,
    description: String,
    roles: Vec<Role>,
    enabled: bool,
    #[serde(default)]
    annotations: ToolAnnotationsSpec,
    input_schema: Value,
    #[serde(default)]
    output_schema: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("contract bundle is invalid: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("tool `{name}` has an invalid {which} schema: {reason}")]
    Schema { name: String, which: &'static str, reason: String },
    #[error("duplicate tool name `{0}`")]
    Duplicate(String),
}

pub struct ToolSpec {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub roles: Vec<Role>,
    pub enabled: bool,
    pub annotations: ToolAnnotationsSpec,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    input_validator: jsonschema::Validator,
    output_validator: Option<jsonschema::Validator>,
}

impl ToolSpec {
    fn from_raw(raw: RawTool) -> Result<Self, ContractError> {
        let input_validator = jsonschema::validator_for(&raw.input_schema).map_err(|e| ContractError::Schema {
            name: raw.name.clone(),
            which: "input",
            reason: e.to_string(),
        })?;
        let output_validator = match &raw.output_schema {
            Some(s) => Some(jsonschema::validator_for(s).map_err(|e| ContractError::Schema {
                name: raw.name.clone(),
                which: "output",
                reason: e.to_string(),
            })?),
            None => None,
        };
        Ok(Self {
            name: raw.name,
            title: raw.title,
            description: raw.description,
            roles: raw.roles,
            enabled: raw.enabled,
            annotations: raw.annotations,
            input_schema: raw.input_schema,
            output_schema: raw.output_schema,
            input_validator,
            output_validator,
        })
    }

    pub fn allows(&self, role: Role) -> bool {
        self.enabled && self.roles.contains(&role)
    }

    pub fn validate_input(&self, args: &Value) -> Result<(), ToolError> {
        let errors: Vec<Value> = self
            .input_validator
            .iter_errors(args)
            .map(|e| json!({ "path": e.instance_path().to_string(), "message": e.to_string() }))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ToolError::invalid_params(format!("arguments for `{}` do not match the contract", self.name))
                .with_details(json!({ "errors": errors })))
        }
    }

    pub fn validate_output(&self, out: &Value) -> Result<(), ToolError> {
        let Some(v) = &self.output_validator else { return Ok(()) };
        let errors: Vec<String> = v.iter_errors(out).map(|e| format!("{}: {e}", e.instance_path())).collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ToolError::internal(format!("output of `{}` violates the contract: {}", self.name, errors.join("; "))))
        }
    }
}

pub struct Catalog {
    pub contract_version: String,
    pub server_name: String,
    tools: Vec<ToolSpec>,
    index: HashMap<String, usize>,
}

impl Catalog {
    pub fn embedded() -> Result<Self, ContractError> {
        Self::from_json(BUNDLE)
    }

    pub fn from_json(text: &str) -> Result<Self, ContractError> {
        let raw: RawBundle = serde_json::from_str(text)?;
        let mut tools = Vec::with_capacity(raw.tools.len());
        let mut index = HashMap::new();
        for t in raw.tools {
            if index.contains_key(&t.name) {
                return Err(ContractError::Duplicate(t.name));
            }
            index.insert(t.name.clone(), tools.len());
            tools.push(ToolSpec::from_raw(t)?);
        }
        Ok(Self { contract_version: raw.contract_version, server_name: raw.server_name, tools, index })
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.index.get(name).map(|i| &self.tools[*i])
    }

    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// role に見せてよい（enabled かつ roles に含む）ツール。manifest の順序を保つ。
    pub fn visible(&self, role: Role) -> Vec<&ToolSpec> {
        self.tools.iter().filter(|t| t.allows(role)).collect()
    }
}
```

`lib.rs` に `pub mod contracts;` を追加。

- [ ] **Step 8: ビルドとテストを通す**

Run: `cargo test -p gaia-core contracts`
Expected: 5 tests passed。失敗したら生成物 `target/debug/build/gaia-core-*/out/contract_types.rs` を開いて型名・フィールド型を確認し、テスト側を合わせる（契約は変えない）

- [ ] **Step 9: コミット**

```bash
git add contracts crates/gaia-core
git commit -m "feat(core): load contracts at build time (schemas + typify types)" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 4: 残り 11 ツールの契約ファイル

**Files:**
- Create: `contracts/tools/{search_context,get_person,get_organization,get_engagement,get_glossary,resolve_speakers,resolve_source,propose_update,list_proposals,approve_proposal,reject_proposal}.json`
- Modify: `contracts/manifest.json`, `crates/gaia-core/src/contracts/mod.rs`（テスト追加）

**Interfaces:**
- Produces: `types::{SearchContextInput, SearchContextOutput, GetPersonInput, GetPersonOutput, GetOrganizationInput, GetOrganizationOutput, GetEngagementInput, GetEngagementOutput, GetGlossaryInput, GetGlossaryOutput, ResolveSpeakersInput, ResolveSpeakersOutput, ResolveSourceInput, ResolveSourceOutput, ProposeUpdateInput, ProposeUpdateOutput, ListProposalsInput, ListProposalsOutput, ApproveProposalInput, ApproveProposalOutput, RejectProposalInput, RejectProposalOutput}` と、common.json 由来の全型（`ScopeInput::{String, Array}` など）

- [ ] **Step 1: manifest.json の tools を 13 件に置き換える**

```json
{
  "contract_version": "1.0.0",
  "server_name": "gaia_library",
  "defs": "defs/common.json",
  "tools": [
    { "name": "search_context",   "file": "tools/search_context.json",   "roles": ["human", "agent"], "enabled": true },
    { "name": "get_person",       "file": "tools/get_person.json",       "roles": ["human", "agent"], "enabled": true },
    { "name": "get_organization", "file": "tools/get_organization.json", "roles": ["human", "agent"], "enabled": true },
    { "name": "get_engagement",   "file": "tools/get_engagement.json",   "roles": ["human", "agent"], "enabled": true },
    { "name": "get_glossary",     "file": "tools/get_glossary.json",     "roles": ["human", "agent"], "enabled": true },
    { "name": "resolve_speakers", "file": "tools/resolve_speakers.json", "roles": ["human", "agent"], "enabled": true },
    { "name": "resolve_source",   "file": "tools/resolve_source.json",   "roles": ["human", "agent"], "enabled": false },
    { "name": "propose_update",   "file": "tools/propose_update.json",   "roles": ["human", "agent"], "enabled": true },
    { "name": "list_proposals",   "file": "tools/list_proposals.json",   "roles": ["human", "agent"], "enabled": true },
    { "name": "approve_proposal", "file": "tools/approve_proposal.json", "roles": ["human"],          "enabled": true },
    { "name": "reject_proposal",  "file": "tools/reject_proposal.json",  "roles": ["human"],          "enabled": true },
    { "name": "get_server_info",  "file": "tools/get_server_info.json",  "roles": ["human", "agent"], "enabled": true },
    { "name": "get_job_status",   "file": "tools/get_job_status.json",   "roles": ["human", "agent"], "enabled": true }
  ]
}
```

- [ ] **Step 2: 参照系 7 ファイルを書く**

`contracts/tools/search_context.json`:

```json
{
  "name": "search_context",
  "title": "コンテキスト横断検索",
  "description": "人物・組織・案件・汎用エンティティの名前と alias、facts の全文（trigram）、interactions の要約、用語集を横断検索し、「回答の設計図」を返す: 関連エンティティごとに facts の要点と、何がどこにあるかの注記付き参照（refs）。返った refs はクライアント側のコネクタ（Notion / Box / ファイル等）で辿ること。scope 省略時はクライアントの既定 scope。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "required": ["query"],
    "additionalProperties": false,
    "properties": {
      "query": { "type": "string", "description": "検索語。3 文字未満は部分一致にフォールバックする" },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" },
      "types": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/SearchType" }, "description": "省略時は全種別" },
      "limit": { "type": "integer", "default": 10, "description": "エンティティ件数の上限（1〜50）" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["query", "scopes", "cross_scope", "entities", "glossary", "interactions", "hints"],
    "properties": {
      "query": { "type": "string" },
      "scopes": { "type": "array", "items": { "type": "string" } },
      "cross_scope": { "type": "boolean" },
      "entities": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/SearchEntity" } },
      "glossary": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/GlossaryTerm" } },
      "interactions": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/InteractionSummary" } },
      "hints": { "type": "array", "items": { "type": "string" } }
    }
  }
}
```

`contracts/tools/get_person.json`:

```json
{
  "name": "get_person",
  "title": "人物の詳細",
  "description": "person_id または name（氏名・alias）で人物を特定し、所属組織・関わる案件・facts・refs・直近の interactions を scope 内で返す。name が複数件に該当した場合は conflict エラーで候補を返す。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "person_id": { "type": "integer" },
      "name": { "type": "string" },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["person", "engagements", "facts", "refs", "interactions"],
    "properties": {
      "person": { "$ref": "../defs/common.json#/$defs/PersonSummary" },
      "organization": { "$ref": "../defs/common.json#/$defs/OrganizationSummary" },
      "engagements": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/EngagementSummary" } },
      "facts": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/Fact" } },
      "refs": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/Reference" } },
      "interactions": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/InteractionSummary" } }
    }
  }
}
```

`contracts/tools/get_organization.json`:

```json
{
  "name": "get_organization",
  "title": "組織の詳細",
  "description": "organization_id または name で組織を特定し、所属する人物・案件（scope 内）・facts・refs を返す。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "organization_id": { "type": "integer" },
      "name": { "type": "string" },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["organization", "people", "engagements", "facts", "refs"],
    "properties": {
      "organization": { "$ref": "../defs/common.json#/$defs/OrganizationSummary" },
      "people": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/PersonSummary" } },
      "engagements": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/EngagementSummary" } },
      "facts": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/Fact" } },
      "refs": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/Reference" } }
    }
  }
}
```

`contracts/tools/get_engagement.json`:

```json
{
  "name": "get_engagement",
  "title": "案件の詳細",
  "description": "engagement_id または name で案件を特定し、相手組織・関係者（役割と alias 付き）・facts・refs・用語集・直近の interactions を返す。案件が scope 外なら not_found。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "engagement_id": { "type": "integer" },
      "name": { "type": "string" },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["engagement", "people", "facts", "refs", "glossary", "interactions"],
    "properties": {
      "engagement": { "$ref": "../defs/common.json#/$defs/EngagementSummary" },
      "organization": { "$ref": "../defs/common.json#/$defs/OrganizationSummary" },
      "people": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/EngagementPerson" } },
      "facts": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/Fact" } },
      "refs": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/Reference" } },
      "glossary": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/GlossaryTerm" } },
      "interactions": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/InteractionSummary" } }
    }
  }
}
```

`contracts/tools/get_glossary.json`:

```json
{
  "name": "get_glossary",
  "title": "用語集と語彙ヒント",
  "description": "案件（engagement_id 省略時は scope 内全体）の用語集と、文字起こしの語彙ヒント（用語・読み・関係者の名前と alias を平坦化した配列。Whisper の initial_prompt にそのまま使える）を返す。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "engagement_id": { "type": "integer" },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["terms", "vocabulary_hints"],
    "properties": {
      "terms": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/GlossaryTerm" } },
      "vocabulary_hints": { "type": "array", "items": { "type": "string" } }
    }
  }
}
```

`contracts/tools/resolve_speakers.json`:

```json
{
  "name": "resolve_speakers",
  "title": "表示名の人物突合",
  "description": "会議ツールの表示名（例: '岡村 慎太郎 (CloudNative)'）を正規化して people の alias と完全一致で突合し、matched / ambiguous / unmatched と候補・確度を返す。engagement_id を渡すとその案件の関係者を優先する（このとき scope が必要）。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "required": ["display_names"],
    "additionalProperties": false,
    "properties": {
      "display_names": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" },
      "engagement_id": { "type": "integer" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["results"],
    "properties": {
      "results": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/SpeakerResult" } }
    }
  }
}
```

`contracts/tools/resolve_source.json`（契約のみ。manifest で `enabled: false`）:

```json
{
  "name": "resolve_source",
  "title": "参照のサーバー側解決",
  "description": "ref_id または uri で指定した参照を、ローカル MCP サーバー（narumi 等）に問い合わせて実体を返す読み取り専用ツール。到達不能時は参照とスナップショットを resolved=false で返す。v1 では未登録。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": true },
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "ref_id": { "type": "integer" },
      "uri": { "type": "string" },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["reference", "resolved"],
    "properties": {
      "reference": { "$ref": "../defs/common.json#/$defs/Reference" },
      "resolved": { "type": "boolean" },
      "content": { "type": "string" },
      "reason": { "type": "string" }
    }
  }
}
```

- [ ] **Step 3: 提案系 4 ファイルを書く**

`contracts/tools/propose_update.json`:

```json
{
  "name": "propose_update",
  "title": "更新の提案",
  "description": "人物・組織・案件・interaction・汎用エンティティ・fact・ref・用語の追加/更新/置換（supersede は fact のみ）を提案キューに積む。直接の書き込みはできず、human が approve_proposal で承認して初めて反映される。patch は target_type ごとの Patch 型（PersonPatch / OrganizationPatch / EngagementPatch / InteractionPatch / EntityPatch / FactPatch / RefPatch / GlossaryPatch）。request_id はクライアント発番（8 文字以上）で、同じ request_id の再送は duplicate=true で既存の提案を返す。",
  "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "required": ["target_type", "action", "patch", "kind", "request_id"],
    "additionalProperties": false,
    "properties": {
      "target_type": { "$ref": "../defs/common.json#/$defs/ProposalTargetType" },
      "action": { "$ref": "../defs/common.json#/$defs/ProposalAction" },
      "target_id": { "type": "integer", "description": "update / supersede で必須" },
      "patch": { "type": "object", "description": "target_type ごとの Patch 型" },
      "kind": { "$ref": "../defs/common.json#/$defs/Kind" },
      "scope": { "type": "string", "description": "省略時はクライアントの既定 scope" },
      "provenance": { "$ref": "../defs/common.json#/$defs/Provenance" },
      "request_id": { "type": "string" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["proposal_id", "status", "duplicate"],
    "properties": {
      "proposal_id": { "type": "integer" },
      "status": { "$ref": "../defs/common.json#/$defs/ProposalStatus" },
      "duplicate": { "type": "boolean" }
    }
  }
}
```

`contracts/tools/list_proposals.json`:

```json
{
  "name": "list_proposals",
  "title": "提案の一覧",
  "description": "提案キューを status（省略時 pending）と scope で絞って新しい順に返す。",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "status": { "$ref": "../defs/common.json#/$defs/ProposalStatus" },
      "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" },
      "limit": { "type": "integer", "default": 50 }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["proposals"],
    "properties": {
      "proposals": { "type": "array", "items": { "$ref": "../defs/common.json#/$defs/Proposal" } }
    }
  }
}
```

`contracts/tools/approve_proposal.json`:

```json
{
  "name": "approve_proposal",
  "title": "提案の承認（human のみ）",
  "description": "pending の提案を検証して適用し、承認済みにする。human ロールのクライアントだけが呼べる。検証に失敗した場合は提案を pending のまま残してエラーを返す。",
  "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "required": ["proposal_id"],
    "additionalProperties": false,
    "properties": { "proposal_id": { "type": "integer" } }
  },
  "outputSchema": {
    "type": "object",
    "required": ["proposal_id", "status", "result"],
    "properties": {
      "proposal_id": { "type": "integer" },
      "status": { "$ref": "../defs/common.json#/$defs/ProposalStatus" },
      "result": { "$ref": "../defs/common.json#/$defs/ApplyResult" }
    }
  }
}
```

`contracts/tools/reject_proposal.json`:

```json
{
  "name": "reject_proposal",
  "title": "提案の却下（human のみ）",
  "description": "pending の提案を却下する。human ロールのクライアントだけが呼べる。",
  "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false },
  "inputSchema": {
    "type": "object",
    "required": ["proposal_id"],
    "additionalProperties": false,
    "properties": {
      "proposal_id": { "type": "integer" },
      "reason": { "type": "string" }
    }
  },
  "outputSchema": {
    "type": "object",
    "required": ["proposal_id", "status"],
    "properties": {
      "proposal_id": { "type": "integer" },
      "status": { "$ref": "../defs/common.json#/$defs/ProposalStatus" }
    }
  }
}
```

- [ ] **Step 4: テストを追加する（contracts/mod.rs の tests に追記）**

```rust
    #[test]
    fn all_thirteen_tools_load_and_roles_match_spec() {
        let c = Catalog::embedded().unwrap();
        assert_eq!(c.tools().len(), 13);
        let agent: Vec<&str> = c.visible(Role::Agent).iter().map(|t| t.name.as_str()).collect();
        assert!(!agent.contains(&"approve_proposal"));
        assert!(!agent.contains(&"reject_proposal"));
        assert!(!agent.contains(&"resolve_source"), "disabled tool must not be visible");
        let human: Vec<&str> = c.visible(Role::Human).iter().map(|t| t.name.as_str()).collect();
        assert!(human.contains(&"approve_proposal"));
        assert_eq!(human.len(), 12);
    }

    #[test]
    fn scope_input_accepts_string_and_array() {
        let a: types::ScopeInput = serde_json::from_value(json!("cloudnative")).unwrap();
        let b: types::ScopeInput = serde_json::from_value(json!(["a", "b"])).unwrap();
        assert!(matches!(a, types::ScopeInput::String(_)));
        assert!(matches!(b, types::ScopeInput::Array(_)));
        let input: types::SearchContextInput = serde_json::from_value(json!({"query": "q"})).unwrap();
        assert_eq!(input.limit, 10);
        assert!(input.types.is_empty());
    }

    #[test]
    fn input_validators_reject_unknown_fields() {
        let c = Catalog::embedded().unwrap();
        let err = c.get("propose_update").unwrap().validate_input(&json!({
            "target_type": "person", "action": "insert", "patch": {}, "kind": "fact", "request_id": "r-00000001", "bogus": 1
        })).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidParams);
    }
```

- [ ] **Step 5: ビルドとテストを通す**

Run: `cargo test -p gaia-core contracts`
Expected: 8 tests passed。typify が panic したら、そのスキーマが Global Constraints の禁止キーワードを使っていないか確認する

- [ ] **Step 6: コミット**

```bash
git add contracts crates/gaia-core
git commit -m "feat(contracts): add v1 tool schemas for all 13 tools" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 5: ストレージ基盤（Db / PRAGMA / マイグレーション / DDL v1）

**Files:**
- Create: `crates/gaia-core/migrations/0001_init.sql`, `crates/gaia-core/src/storage/mod.rs`
- Modify: `crates/gaia-core/src/lib.rs`

**Interfaces:**
- Consumes: `error::ToolError`（Task 2）
- Produces: `gaia_core::storage::{Db, StorageError, MIGRATIONS, like_pattern}`
  - `Db::open(path: &Path) -> Result<Db, StorageError>`（親ディレクトリ作成・PRAGMA・マイグレーション適用）
  - `Db::open_in_memory() -> Result<Db, StorageError>`（テスト用）
  - `Db::with_conn<T, E: From<StorageError>>(&self, f: impl FnOnce(&rusqlite::Connection) -> Result<T, E>) -> Result<T, E>`
  - `Db::with_tx<T, E: From<StorageError>>(&self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, E>) -> Result<T, E>`（`BEGIN IMMEDIATE`。`Err` なら rollback）
  - `StorageError::{Sqlite, Migration, Io, Json, NotFound(String), Integrity(String)}` と `impl From<StorageError> for ToolError`
  - `like_pattern(needle: &str) -> String`（`%` `_` `\` をエスケープして両端に `%`。SQL 側は `LIKE ?n ESCAPE '\'`）

- [ ] **Step 1: DDL v1 を書く（仕様書 §5.1 と同一。差分①〜⑤を含む）**

`crates/gaia-core/migrations/0001_init.sql`:

```sql
-- gaia-library DDL v1（仕様書 §5.1）。名寄せ層は共有、内容層は scope 必須。
-- 名寄せ層（共有・scope なし）
CREATE TABLE affiliations (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  identity   TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE organizations (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  kind       TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE people (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  org_id     INTEGER REFERENCES organizations(id),
  role       TEXT,
  first_met  TEXT,
  last_seen  TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE person_aliases (
  person_id  INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  alias      TEXT NOT NULL,
  kind       TEXT,
  PRIMARY KEY (person_id, alias)
);
CREATE TABLE entities (
  id         INTEGER PRIMARY KEY,
  type       TEXT NOT NULL,
  name       TEXT NOT NULL,
  attrs      TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 内容層（scope 必須）
CREATE TABLE engagements (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  org_id     INTEGER REFERENCES organizations(id),
  scope      TEXT NOT NULL REFERENCES affiliations(name),
  status     TEXT,
  started_at TEXT,
  ended_at   TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE engagement_people (
  engagement_id INTEGER NOT NULL REFERENCES engagements(id) ON DELETE CASCADE,
  person_id     INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  role          TEXT,
  PRIMARY KEY (engagement_id, person_id)
);
CREATE TABLE interactions (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,
  occurred_at   TEXT NOT NULL,
  summary       TEXT NOT NULL,
  engagement_id INTEGER REFERENCES engagements(id),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE interaction_people (
  interaction_id INTEGER NOT NULL REFERENCES interactions(id) ON DELETE CASCADE,
  person_id      INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  PRIMARY KEY (interaction_id, person_id)
);
CREATE TABLE facts (
  id            INTEGER PRIMARY KEY,
  entity_type   TEXT NOT NULL CHECK (entity_type IN ('person','organization','engagement','interaction','entity')),
  entity_id     INTEGER NOT NULL,
  statement     TEXT NOT NULL,
  predicate     TEXT,
  value         TEXT,
  kind          TEXT NOT NULL CHECK (kind IN ('fact','inference')),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  valid_from    TEXT,
  superseded_by INTEGER REFERENCES facts(id),
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE refs (
  id            INTEGER PRIMARY KEY,
  target_type   TEXT NOT NULL CHECK (target_type IN ('person','organization','engagement','interaction','entity','fact')),
  target_id     INTEGER NOT NULL,
  system        TEXT NOT NULL,
  uri           TEXT NOT NULL,
  title         TEXT,
  note          TEXT NOT NULL,
  snapshot      TEXT,
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  last_verified TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE glossary (
  id            INTEGER PRIMARY KEY,
  engagement_id INTEGER REFERENCES engagements(id),
  term          TEXT NOT NULL,
  reading       TEXT,
  definition    TEXT,
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE proposals (
  id            INTEGER PRIMARY KEY,
  action        TEXT NOT NULL CHECK (action IN ('insert','update','supersede')),
  target_type   TEXT NOT NULL,
  target_id     INTEGER,
  patch         TEXT NOT NULL,
  kind          TEXT NOT NULL CHECK (kind IN ('fact','inference')),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  provenance    TEXT,
  provenance_id INTEGER REFERENCES refs(id),
  proposed_by   TEXT NOT NULL,
  request_id    TEXT NOT NULL UNIQUE,
  status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
  result_id     INTEGER,
  decision_note TEXT,
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  decided_at    TEXT,
  decided_by    TEXT
);
CREATE TABLE audit_log (
  id     INTEGER PRIMARY KEY,
  actor  TEXT NOT NULL,
  action TEXT NOT NULL,
  detail TEXT NOT NULL,
  at     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_facts_target ON facts(entity_type, entity_id);
CREATE INDEX idx_refs_target  ON refs(target_type, target_id);
CREATE INDEX idx_facts_scope  ON facts(scope);
CREATE INDEX idx_refs_scope   ON refs(scope);
CREATE INDEX idx_alias_lookup ON person_aliases(alias);
CREATE INDEX idx_proposals_status ON proposals(status, scope);

-- 外部コンテンツ FTS（trigram）と同期トリガ
CREATE VIRTUAL TABLE facts_fts USING fts5(statement, content='facts', content_rowid='id', tokenize='trigram');
CREATE TRIGGER facts_ai AFTER INSERT ON facts BEGIN
  INSERT INTO facts_fts(rowid, statement) VALUES (new.id, new.statement);
END;
CREATE TRIGGER facts_ad AFTER DELETE ON facts BEGIN
  INSERT INTO facts_fts(facts_fts, rowid, statement) VALUES ('delete', old.id, old.statement);
END;
CREATE TRIGGER facts_au AFTER UPDATE OF statement ON facts BEGIN
  INSERT INTO facts_fts(facts_fts, rowid, statement) VALUES ('delete', old.id, old.statement);
  INSERT INTO facts_fts(rowid, statement) VALUES (new.id, new.statement);
END;
```

- [ ] **Step 2: テストを書く（storage/mod.rs の末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn open_in_memory_applies_schema_and_pragmas() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let fk: i64 = c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
            assert_eq!(fk, 1);
            let n: i64 = c.query_one(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('people','facts','proposals','audit_log','engagement_people')",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(n, 5);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn open_file_uses_wal_creates_parent_and_sets_user_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("sub").join("gaia.db")).unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let jm: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
            assert_eq!(jm, "wal");
            let uv: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(uv, 1);
            Ok(())
        })
        .unwrap();
        // 2 回目の open は冪等
        drop(db);
        Db::open(&dir.path().join("sub").join("gaia.db")).unwrap();
    }

    #[test]
    fn fts_stays_in_sync_through_insert_update_delete() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            c.execute_batch("INSERT INTO affiliations(name) VALUES ('t'); INSERT INTO people(name) VALUES ('p');")?;
            c.execute(
                "INSERT INTO facts(entity_type, entity_id, statement, kind, scope) VALUES ('person', 1, 'トライグラム検索の確認', 'fact', 't')",
                [],
            )?;
            let hit = |c: &Connection| -> Result<i64, StorageError> {
                Ok(c.query_one("SELECT count(*) FROM facts_fts WHERE facts_fts MATCH 'グラム'", [], |r| r.get(0))?)
            };
            assert_eq!(hit(c)?, 1);
            c.execute("UPDATE facts SET statement = '別の文' WHERE id = 1", [])?;
            assert_eq!(hit(c)?, 0);
            c.execute("DELETE FROM facts WHERE id = 1", [])?;
            // rank=1 の integrity-check は外部コンテンツ表と索引の不一致を検出する
            c.execute("INSERT INTO facts_fts(facts_fts, rank) VALUES ('integrity-check', 1)", [])?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn with_tx_rolls_back_on_error() {
        let db = Db::open_in_memory().unwrap();
        let r: Result<(), StorageError> = db.with_tx(|tx| {
            tx.execute("INSERT INTO organizations(name) VALUES ('x')", [])?;
            Err(StorageError::Integrity("boom".into()))
        });
        assert!(r.is_err());
        let n: i64 = db
            .with_conn::<_, StorageError>(|c| Ok(c.query_one("SELECT count(*) FROM organizations", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("a%b_c\\"), "%a\\%b\\_c\\\\%");
        assert_eq!(like_pattern("岡村"), "%岡村%");
    }

    #[test]
    fn storage_error_maps_to_tool_error_codes() {
        use crate::error::ErrorCode;
        assert_eq!(ToolError::from(StorageError::NotFound("person 1".into())).code, ErrorCode::NotFound);
        assert_eq!(ToolError::from(StorageError::Integrity("x".into())).code, ErrorCode::InvalidParams);
    }
}
```

- [ ] **Step 3: storage/mod.rs を実装する**

```rust
//! SQLite 接続・PRAGMA・マイグレーション。仕様書 §5。
//! 内容層への SELECT は必ず `scope IN (SELECT value FROM json_each(?))` を付ける（各リポジトリの責務）。
use std::{path::Path, sync::Mutex, time::Duration};

use rusqlite::{Connection, Transaction, TransactionBehavior};
use rusqlite_migration::{M, Migrations};

use crate::error::ToolError;

pub const MIGRATIONS: Migrations<'static> =
    Migrations::from_slice(&[M::up(include_str!("../../migrations/0001_init.sql"))]);

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] rusqlite_migration::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Integrity(String),
}

impl From<StorageError> for ToolError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::Sqlite(e) => ToolError::from(e),
            StorageError::NotFound(what) => ToolError::not_found(format!("{what} not found")),
            StorageError::Integrity(msg) => ToolError::invalid_params(msg),
            other => ToolError::internal(other.to_string()),
        }
    }
}

/// `Connection` は `Sync` ではないので Mutex で直列化する。個人 CRM 規模では単一接続で足りる。
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        configure(&mut conn)?;
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let mut conn = Connection::open_in_memory()?;
        configure(&mut conn)?;
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn with_conn<T, E: From<StorageError>>(&self, f: impl FnOnce(&Connection) -> Result<T, E>) -> Result<T, E> {
        let guard = self.conn.lock().map_err(|_| StorageError::Integrity("db mutex poisoned".into()))?;
        f(&guard)
    }

    /// `BEGIN IMMEDIATE` のトランザクション。閉包が `Err` を返すか panic すれば rollback される。
    pub fn with_tx<T, E: From<StorageError>>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T, E>) -> Result<T, E> {
        let mut guard = self.conn.lock().map_err(|_| StorageError::Integrity("db mutex poisoned".into()))?;
        let tx = guard.transaction().map_err(StorageError::from)?;
        let out = f(&tx)?;
        tx.commit().map_err(StorageError::from)?;
        Ok(out)
    }
}

fn configure(conn: &mut Connection) -> Result<(), StorageError> {
    // in-memory では "memory" が返るので戻り値は見ない
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.busy_timeout(Duration::from_millis(5000))?;
    conn.set_transaction_behavior(TransactionBehavior::Immediate);
    Ok(())
}

/// `LIKE ?n ESCAPE '\'` 用のパターン。`%` `_` `\` をエスケープして両端に `%` を付ける。
pub fn like_pattern(needle: &str) -> String {
    let mut out = String::with_capacity(needle.len() + 2);
    out.push('%');
    for ch in needle.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}
```

`lib.rs` に `pub mod storage;` を追加。

- [ ] **Step 4: テストを実行する**

Run: `cargo test -p gaia-core storage`
Expected: 7 tests passed

- [ ] **Step 5: コミット**

```bash
git add crates/gaia-core
git commit -m "feat(core): add SQLite storage with DDL v1 migration and FTS triggers" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 6: 設定ファイルとパス解決

**Files:**
- Create: `crates/gaia-core/src/config.rs`
- Modify: `crates/gaia-core/src/lib.rs`

**Interfaces:**
- Consumes: `identity::{ClientIdentity, Role}`
- Produces: `gaia_core::config::{Config, CliConfig, ConfigError, APP_DIR, config_path, config_path_with, db_path, db_path_with}`
  - `Config { db_path: Option<PathBuf>, cli: CliConfig, clients: Vec<ClientIdentity> }`（TOML）、`CliConfig { default_client: Option<String> }`
  - `config_path() -> Result<PathBuf, ConfigError>`（`GAIA_CONFIG` → `$XDG_CONFIG_HOME/gaia-library/config.toml` → `~/.config/gaia-library/config.toml`）
  - `db_path(config: &Config) -> Result<PathBuf, ConfigError>`（`GAIA_DB` → `config.db_path` → `$XDG_DATA_HOME/gaia-library/gaia.db` → `~/.local/share/gaia-library/gaia.db`）
  - `*_with(lookup: &dyn Fn(&str) -> Option<OsString>)` は環境変数の読み出しを差し替えられる版（テスト用。`config_path` / `db_path` はこれを `std::env::var_os` で呼ぶ）
  - `Config::load(&Path)`, `Config::load_or_default(&Path)`, `Config::save(&self, &Path)`（親ディレクトリ作成、unix では 0600）, `Config::client(&self, name) -> Option<&ClientIdentity>`, `Config::add_client(&mut self, ClientIdentity) -> Result<(), ConfigError>`, `Config::resolve_client(&self, name: Option<&str>) -> Result<&ClientIdentity, ConfigError>`

- [ ] **Step 1: テストを書く（config.rs の末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k: &str| map.get(k).map(OsString::from)
    }

    fn human(name: &str) -> ClientIdentity {
        ClientIdentity { name: name.into(), role: Role::Human, default_scope: Some("cn".into()) }
    }

    #[test]
    fn config_path_prefers_gaia_config_then_xdg_then_home() {
        let p = config_path_with(&env(&[("GAIA_CONFIG", "/x/c.toml"), ("HOME", "/h")])).unwrap();
        assert_eq!(p, PathBuf::from("/x/c.toml"));
        let p = config_path_with(&env(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/h")])).unwrap();
        assert_eq!(p, PathBuf::from("/xdg/gaia-library/config.toml"));
        let p = config_path_with(&env(&[("HOME", "/h")])).unwrap();
        assert_eq!(p, PathBuf::from("/h/.config/gaia-library/config.toml"));
        assert!(matches!(config_path_with(&env(&[])), Err(ConfigError::MissingHome)));
    }

    #[test]
    fn db_path_prefers_env_then_config_then_xdg_data() {
        let mut cfg = Config::default();
        let p = db_path_with(&cfg, &env(&[("GAIA_DB", "/x/g.db"), ("HOME", "/h")])).unwrap();
        assert_eq!(p, PathBuf::from("/x/g.db"));
        cfg.db_path = Some(PathBuf::from("/cfg/g.db"));
        let p = db_path_with(&cfg, &env(&[("HOME", "/h")])).unwrap();
        assert_eq!(p, PathBuf::from("/cfg/g.db"));
        cfg.db_path = None;
        let p = db_path_with(&cfg, &env(&[("HOME", "/h")])).unwrap();
        assert_eq!(p, PathBuf::from("/h/.local/share/gaia-library/gaia.db"));
    }

    #[test]
    fn save_and_load_round_trip_with_0600() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.toml");
        let mut cfg = Config::default();
        cfg.cli.default_client = Some("me".into());
        cfg.add_client(human("me")).unwrap();
        cfg.add_client(ClientIdentity { name: "bot".into(), role: Role::Agent, default_scope: None }).unwrap();
        cfg.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, cfg);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(matches!(cfg.add_client(human("me")), Err(ConfigError::DuplicateClient(_))));
        assert_eq!(Config::load_or_default(&dir.path().join("missing.toml")).unwrap(), Config::default());
    }

    #[test]
    fn resolve_client_uses_explicit_then_default_then_sole_human() {
        let mut cfg = Config::default();
        cfg.add_client(human("me")).unwrap();
        cfg.add_client(ClientIdentity { name: "bot".into(), role: Role::Agent, default_scope: None }).unwrap();
        assert_eq!(cfg.resolve_client(Some("bot")).unwrap().role, Role::Agent);
        assert!(matches!(cfg.resolve_client(Some("nope")), Err(ConfigError::UnknownClient(_))));
        // default_client 未設定・human が 1 人 → その人
        assert_eq!(cfg.resolve_client(None).unwrap().name, "me");
        cfg.add_client(human("other")).unwrap();
        assert!(matches!(cfg.resolve_client(None), Err(ConfigError::NoDefaultClient)));
        cfg.cli.default_client = Some("other".into());
        assert_eq!(cfg.resolve_client(None).unwrap().name, "other");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "bogus = 1\n").unwrap();
        assert!(matches!(Config::load(&path), Err(ConfigError::Parse { .. })));
    }
}
```

- [ ] **Step 2: config.rs を実装する**

```rust
//! 設定ファイル（TOML）とパス解決。仕様書 §7.1。XDG 配置を macOS でも使う。
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::identity::{ClientIdentity, Role};

pub const APP_DIR: &str = "gaia-library";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_path: Option<PathBuf>,
    #[serde(default)]
    pub cli: CliConfig,
    #[serde(default)]
    pub clients: Vec<ClientIdentity>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_client: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("HOME is not set; cannot resolve the config directory (set GAIA_CONFIG / GAIA_DB explicitly)")]
    MissingHome,
    #[error("cannot read config {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("cannot write config {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("config {path} is invalid: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("cannot serialize config: {0}")]
    Serialize(String),
    #[error("unknown client `{0}` (see [[clients]] in the config file)")]
    UnknownClient(String),
    #[error("client `{0}` already exists")]
    DuplicateClient(String),
    #[error("no default client: set [cli].default_client or pass --client")]
    NoDefaultClient,
}

type Lookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

fn home_dir(lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    lookup("HOME").map(PathBuf::from).ok_or(ConfigError::MissingHome)
}

pub fn config_path_with(lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    if let Some(p) = lookup("GAIA_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let base = match lookup("XDG_CONFIG_HOME") {
        Some(x) => PathBuf::from(x),
        None => home_dir(lookup)?.join(".config"),
    };
    Ok(base.join(APP_DIR).join("config.toml"))
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    config_path_with(&|k| std::env::var_os(k))
}

pub fn db_path_with(config: &Config, lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    if let Some(p) = lookup("GAIA_DB") {
        return Ok(PathBuf::from(p));
    }
    if let Some(p) = &config.db_path {
        return Ok(p.clone());
    }
    let base = match lookup("XDG_DATA_HOME") {
        Some(x) => PathBuf::from(x),
        None => home_dir(lookup)?.join(".local").join("share"),
    };
    Ok(base.join(APP_DIR).join("gaia.db"))
}

pub fn db_path(config: &Config) -> Result<PathBuf, ConfigError> {
    db_path_with(config, &|k| std::env::var_os(k))
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Read { path: path.to_path_buf(), source })?;
        toml::from_str(&text).map_err(|e| ConfigError::Parse { path: path.to_path_buf(), message: e.to_string() })
    }

    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() { Self::load(path) } else { Ok(Self::default()) }
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let text = toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Write { path: path.to_path_buf(), source })?;
        }
        fs::write(path, text).map_err(|source| ConfigError::Write { path: path.to_path_buf(), source })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|source| ConfigError::Write { path: path.to_path_buf(), source })?;
        }
        Ok(())
    }

    pub fn client(&self, name: &str) -> Option<&ClientIdentity> {
        self.clients.iter().find(|c| c.name == name)
    }

    pub fn add_client(&mut self, client: ClientIdentity) -> Result<(), ConfigError> {
        if self.client(&client.name).is_some() {
            return Err(ConfigError::DuplicateClient(client.name));
        }
        self.clients.push(client);
        Ok(())
    }

    /// `--client` 明示 → `[cli].default_client` → human が 1 人だけならその人 → エラー。
    pub fn resolve_client(&self, name: Option<&str>) -> Result<&ClientIdentity, ConfigError> {
        if let Some(n) = name {
            return self.client(n).ok_or_else(|| ConfigError::UnknownClient(n.to_string()));
        }
        if let Some(n) = &self.cli.default_client {
            return self.client(n).ok_or_else(|| ConfigError::UnknownClient(n.clone()));
        }
        let humans: Vec<&ClientIdentity> = self.clients.iter().filter(|c| c.role == Role::Human).collect();
        match humans.as_slice() {
            [only] => Ok(only),
            _ => Err(ConfigError::NoDefaultClient),
        }
    }
}
```

`lib.rs` に `pub mod config;` を追加。

- [ ] **Step 3: テストを実行する**

Run: `cargo test -p gaia-core config`
Expected: 5 tests passed

- [ ] **Step 4: コミット**

```bash
git add crates/gaia-core
git commit -m "feat(core): add config file and XDG path resolution" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 7: 名前の正規化と predicate レジストリ

**Files:**
- Create: `crates/gaia-core/src/domain/mod.rs`, `crates/gaia-core/src/domain/normalize.rs`, `crates/gaia-core/src/domain/predicates.rs`
- Modify: `crates/gaia-core/src/lib.rs`

**Interfaces:**
- Produces: `gaia_core::domain::normalize::normalize_name(&str) -> String`、`gaia_core::domain::predicates::{KNOWN_PREDICATES: &[&str], check(predicate: Option<&str>, value: Option<&str>) -> Result<(), ToolError>}`

- [ ] **Step 1: normalize のテストを書く（normalize.rs 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::normalize_name;

    #[test]
    fn strips_parenthesized_suffix_and_spaces() {
        assert_eq!(normalize_name("岡村 慎太郎 (CloudNative)"), "岡村慎太郎");
        assert_eq!(normalize_name("岡村　慎太郎（クラウドネイティブ）"), "岡村慎太郎");
    }

    #[test]
    fn lowercases_and_folds_fullwidth_via_nfkc() {
        assert_eq!(normalize_name("Okamura Shintaro"), "okamurashintaro");
        assert_eq!(normalize_name("Ｔａｎａｋａ Ｔａｒｏ"), "tanakataro");
        assert_eq!(normalize_name("ｵｶﾑﾗ ｼﾝﾀﾛｳ"), "オカムラシンタロウ");
    }

    #[test]
    fn strips_honorifics_only_when_something_remains() {
        assert_eq!(normalize_name("田中さん"), "田中");
        assert_eq!(normalize_name("田中 様"), "田中");
        assert_eq!(normalize_name("Tanaka-san"), "tanaka");
        assert_eq!(normalize_name("さん"), "さん");
    }

    #[test]
    fn empty_when_nothing_is_left() {
        assert_eq!(normalize_name("（外部）"), "");
        assert_eq!(normalize_name("   "), "");
    }
}
```

- [ ] **Step 2: normalize.rs を実装する**

```rust
//! 表示名の正規化。仕様書 §8.3 resolve_speakers。
//! NFKC → 括弧内除去 → 小文字化 → 前後空白除去 → 敬称除去 → 空白除去。
use unicode_normalization::UnicodeNormalization;

/// 末尾の敬称。空白を含む形は空白除去より前に処理する。
const HONORIFICS: &[&str] = &["さん", "様", "氏", "くん", "ちゃん", "先生", "-san", " san"];

pub fn normalize_name(input: &str) -> String {
    let nfkc: String = input.nfkc().collect();
    let mut without_parens = String::with_capacity(nfkc.len());
    let mut depth = 0usize;
    for ch in nfkc.chars() {
        match ch {
            '(' | '[' | '<' | '{' | '「' | '【' => depth += 1,
            ')' | ']' | '>' | '}' | '」' | '】' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ => without_parens.push(ch),
        }
    }
    let mut trimmed = without_parens.to_lowercase().trim().to_string();
    for suffix in HONORIFICS {
        if let Some(rest) = trimmed.strip_suffix(suffix) {
            if !rest.trim().is_empty() {
                trimmed = rest.trim().to_string();
                break;
            }
        }
    }
    trimmed.chars().filter(|c| !c.is_whitespace()).collect()
}
```

- [ ] **Step 3: predicates.rs をテスト込みで実装する**

```rust
//! 構造化 predicate の初期レジストリ。仕様書 §8.6。頻出したものだけ後払いで昇格する。
use crate::error::ToolError;

pub const KNOWN_PREDICATES: &[&str] = &["role", "status", "interest", "decision"];

/// 承認時の規則: レジストリにある predicate は value 必須、レジストリ外は拒否（自由文のみで登録し直す）。
pub fn check(predicate: Option<&str>, value: Option<&str>) -> Result<(), ToolError> {
    match predicate {
        None => Ok(()),
        Some(p) if KNOWN_PREDICATES.contains(&p) => {
            if value.map(|v| !v.trim().is_empty()).unwrap_or(false) {
                Ok(())
            } else {
                Err(ToolError::invalid_params(format!("predicate `{p}` requires a non-empty value")))
            }
        }
        Some(p) => Err(ToolError::invalid_params(format!(
            "unknown predicate `{p}`; allowed: {}. Register other facts with statement only",
            KNOWN_PREDICATES.join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn known_predicate_requires_value() {
        assert!(check(Some("role"), Some("CTO")).is_ok());
        assert_eq!(check(Some("role"), None).unwrap_err().code, ErrorCode::InvalidParams);
        assert_eq!(check(Some("role"), Some("  ")).unwrap_err().code, ErrorCode::InvalidParams);
    }

    #[test]
    fn unknown_predicate_is_rejected_and_none_is_free_text() {
        assert_eq!(check(Some("mood"), Some("x")).unwrap_err().code, ErrorCode::InvalidParams);
        assert!(check(None, None).is_ok());
    }
}
```

`domain/mod.rs`:

```rust
pub mod normalize;
pub mod predicates;
```

`lib.rs` に `pub mod domain;` を追加。

- [ ] **Step 4: テストを実行する**

Run: `cargo test -p gaia-core domain`
Expected: 6 tests passed

- [ ] **Step 5: コミット**

```bash
git add crates/gaia-core
git commit -m "feat(core): add name normalization and predicate registry" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 8: affiliations / 監査ログ / ScopeSet

**Files:**
- Create: `crates/gaia-core/src/storage/affiliations.rs`, `crates/gaia-core/src/storage/audit.rs`, `crates/gaia-core/src/scope.rs`
- Modify: `crates/gaia-core/src/storage/mod.rs`（`pub mod affiliations; pub mod audit;`）, `crates/gaia-core/src/lib.rs`（`pub mod scope;`）

**Interfaces:**
- Consumes: `storage::{StorageError, Db}`、`identity::ClientIdentity`、`contracts::types::ScopeInput`
- Produces:
  - `storage::affiliations::{Affiliation { id: i64, name: String, identity: Option<String> }, insert(conn, name: &str, identity: Option<&str>) -> Result<i64, StorageError>, exists(conn, name: &str) -> Result<bool, StorageError>, list(conn) -> Result<Vec<Affiliation>, StorageError>}`
  - `storage::audit::{AuditEntry { id: i64, actor: String, action: String, detail: serde_json::Value, at: String }, record(conn, actor: &str, action: &str, detail: &serde_json::Value) -> Result<i64, StorageError>, recent(conn, limit: usize) -> Result<Vec<AuditEntry>, StorageError>}`
  - `scope::ScopeSet`: `resolve(conn, client: &ClientIdentity, requested: Option<Vec<String>>) -> Result<ScopeSet, ToolError>`、`single(name: &str) -> ScopeSet`、`names(&self) -> &[String]`、`is_cross(&self) -> bool`、`contains(&self, name: &str) -> bool`、`as_json(&self) -> String`（`scope IN (SELECT value FROM json_each(?))` に渡す JSON 配列文字列）、`audit_cross_read(&self, conn, actor: &str, tool: &str) -> Result<(), ToolError>`
  - `scope::scope_input_to_vec(input: Option<&ScopeInput>) -> Option<Vec<String>>`

- [ ] **Step 1: affiliations.rs をテスト込みで実装する**

```rust
//! affiliations = scope の値域（機密境界の定義）。名寄せ層・共有。
use rusqlite::{Connection, OptionalExtension, params};

use super::StorageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Affiliation {
    pub id: i64,
    pub name: String,
    pub identity: Option<String>,
}

pub fn insert(conn: &Connection, name: &str, identity: Option<&str>) -> Result<i64, StorageError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StorageError::Integrity("affiliation name must not be empty".into()));
    }
    if exists(conn, name)? {
        return Err(StorageError::Integrity(format!("affiliation `{name}` already exists")));
    }
    conn.execute("INSERT INTO affiliations(name, identity) VALUES (?1, ?2)", params![name, identity])?;
    Ok(conn.last_insert_rowid())
}

pub fn exists(conn: &Connection, name: &str) -> Result<bool, StorageError> {
    let found: Option<i64> = conn
        .query_row("SELECT id FROM affiliations WHERE name = ?1", params![name], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

pub fn list(conn: &Connection) -> Result<Vec<Affiliation>, StorageError> {
    let mut stmt = conn.prepare("SELECT id, name, identity FROM affiliations ORDER BY name")?;
    let rows = stmt.query_map([], |r| Ok(Affiliation { id: r.get(0)?, name: r.get(1)?, identity: r.get(2)? }))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    #[test]
    fn insert_exists_list_and_reject_duplicates() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(c, "cloudnative", Some("CN"))?;
            assert!(id > 0);
            assert!(exists(c, "cloudnative")?);
            assert!(!exists(c, "other")?);
            assert!(matches!(insert(c, "cloudnative", None), Err(StorageError::Integrity(_))));
            assert!(matches!(insert(c, "  ", None), Err(StorageError::Integrity(_))));
            insert(c, "assoc", None)?;
            let names: Vec<String> = list(c)?.into_iter().map(|a| a.name).collect();
            assert_eq!(names, vec!["assoc", "cloudnative"]);
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 2: audit.rs をテスト込みで実装する**

```rust
//! 監査ログ。全書き込み・承認・横断読み取り・管理操作を actor 付きで残す。
use rusqlite::{Connection, params};
use serde_json::Value;

use super::StorageError;

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub detail: Value,
    pub at: String,
}

pub fn record(conn: &Connection, actor: &str, action: &str, detail: &Value) -> Result<i64, StorageError> {
    conn.execute(
        "INSERT INTO audit_log(actor, action, detail) VALUES (?1, ?2, ?3)",
        params![actor, action, detail.to_string()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn recent(conn: &Connection, limit: usize) -> Result<Vec<AuditEntry>, StorageError> {
    let mut stmt = conn.prepare("SELECT id, actor, action, detail, at FROM audit_log ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit as i64], |r| {
        let raw: String = r.get(3)?;
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, raw, r.get::<_, String>(4)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, actor, action, raw, at) = row?;
        out.push(AuditEntry { id, actor, action, detail: serde_json::from_str(&raw)?, at });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;
    use serde_json::json;

    #[test]
    fn record_and_read_back_newest_first() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            record(c, "me", "propose", &json!({"proposal_id": 1}))?;
            record(c, "bot", "cross_scope_read", &json!({"scopes": ["a", "b"]}))?;
            let entries = recent(c, 10)?;
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].actor, "bot");
            assert_eq!(entries[0].detail["scopes"][1], "b");
            assert_eq!(entries[1].action, "propose");
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 3: scope.rs のテストを書く（scope.rs 末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorCode, identity::Role, storage::{Db, StorageError, affiliations, audit}};

    fn client(default_scope: Option<&str>) -> ClientIdentity {
        ClientIdentity { name: "bot".into(), role: Role::Agent, default_scope: default_scope.map(String::from) }
    }

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "a", None)?;
            affiliations::insert(c, "b", None)?;
            Ok(())
        })
        .unwrap();
        db
    }

    #[test]
    fn falls_back_to_default_scope_and_is_not_cross() {
        let db = db();
        db.with_conn::<_, ToolError>(|c| {
            let s = ScopeSet::resolve(c, &client(Some("a")), None)?;
            assert_eq!(s.names(), ["a"]);
            assert!(!s.is_cross());
            assert_eq!(s.as_json(), "[\"a\"]");
            s.audit_cross_read(c, "bot", "search_context")?;
            assert!(audit::recent(c, 10)?.is_empty(), "single scope must not be audited");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn multiple_scopes_are_sorted_deduped_and_audited() {
        let db = db();
        db.with_conn::<_, ToolError>(|c| {
            let s = ScopeSet::resolve(c, &client(None), Some(vec!["b".into(), "a".into(), "b".into()]))?;
            assert_eq!(s.names(), ["a", "b"]);
            assert!(s.is_cross());
            assert!(s.contains("b"));
            s.audit_cross_read(c, "bot", "get_person")?;
            let entries = audit::recent(c, 10)?;
            assert_eq!(entries[0].action, "cross_scope_read");
            assert_eq!(entries[0].detail["tool"], "get_person");
            assert_eq!(entries[0].detail["scopes"], serde_json::json!(["a", "b"]));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn missing_scope_is_denied_and_unknown_scope_is_not_found() {
        let db = db();
        db.with_conn::<_, StorageError>(|c| {
            assert_eq!(ScopeSet::resolve(c, &client(None), None).unwrap_err().code, ErrorCode::ScopeDenied);
            assert_eq!(ScopeSet::resolve(c, &client(None), Some(vec![])).unwrap_err().code, ErrorCode::ScopeDenied);
            assert_eq!(
                ScopeSet::resolve(c, &client(Some("a")), Some(vec!["zzz".into()])).unwrap_err().code,
                ErrorCode::NotFound
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn scope_input_converts_string_and_array() {
        use crate::contracts::types::ScopeInput;
        assert_eq!(scope_input_to_vec(None), None);
        assert_eq!(scope_input_to_vec(Some(&ScopeInput::String("a".into()))), Some(vec!["a".to_string()]));
        assert_eq!(scope_input_to_vec(Some(&ScopeInput::Array(vec!["a".into(), "b".into()]))), Some(vec!["a".to_string(), "b".to_string()]));
    }
}
```

- [ ] **Step 4: scope.rs を実装する**

```rust
//! scope（所属元＝機密境界）の解決。仕様書 §7.2。default deny / explicit allow。
use rusqlite::Connection;
use serde_json::json;

use crate::{
    contracts::types::ScopeInput,
    error::ToolError,
    identity::ClientIdentity,
    storage::{affiliations, audit},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSet {
    scopes: Vec<String>,
}

impl ScopeSet {
    /// 引数 → クライアント既定 scope → `scope_denied`。各 scope は affiliations に存在すること（無ければ `not_found`）。
    pub fn resolve(conn: &Connection, client: &ClientIdentity, requested: Option<Vec<String>>) -> Result<Self, ToolError> {
        let mut scopes = match requested {
            Some(v) if !v.is_empty() => v,
            _ => vec![client.default_scope.clone().ok_or_else(|| {
                ToolError::scope_denied(format!(
                    "scope is required: pass `scope` or set default_scope for client `{}`",
                    client.name
                ))
            })?],
        };
        scopes.sort();
        scopes.dedup();
        for s in &scopes {
            if !affiliations::exists(conn, s)? {
                return Err(ToolError::not_found(format!("scope `{s}` (affiliation) not found")));
            }
        }
        Ok(Self { scopes })
    }

    /// 検証なしで 1 scope を包む（承認処理など scope が既に DB 由来のとき用）。
    pub fn single(name: &str) -> Self {
        Self { scopes: vec![name.to_string()] }
    }

    pub fn names(&self) -> &[String] {
        &self.scopes
    }

    pub fn is_cross(&self) -> bool {
        self.scopes.len() > 1
    }

    pub fn contains(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s == name)
    }

    /// `WHERE scope IN (SELECT value FROM json_each(?n))` に渡す JSON 配列。
    pub fn as_json(&self) -> String {
        serde_json::to_string(&self.scopes).expect("Vec<String> serializes")
    }

    /// 複数 scope の明示指定時だけ監査ログに残す。
    pub fn audit_cross_read(&self, conn: &Connection, actor: &str, tool: &str) -> Result<(), ToolError> {
        if self.is_cross() {
            audit::record(conn, actor, "cross_scope_read", &json!({ "tool": tool, "scopes": self.scopes }))?;
        }
        Ok(())
    }
}

pub fn scope_input_to_vec(input: Option<&ScopeInput>) -> Option<Vec<String>> {
    match input {
        None => None,
        Some(ScopeInput::String(s)) => Some(vec![s.clone()]),
        Some(ScopeInput::Array(v)) => Some(v.clone()),
    }
}
```

`storage/mod.rs` の先頭に `pub mod affiliations; pub mod audit;`、`lib.rs` に `pub mod scope;` を追加。

- [ ] **Step 5: テストを実行する**

Run: `cargo test -p gaia-core`
Expected: すべて成功（affiliations 1、audit 1、scope 4 を含む）

- [ ] **Step 6: コミット**

```bash
git add crates/gaia-core
git commit -m "feat(core): add affiliations, audit log and ScopeSet resolution" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 9: 名寄せ層リポジトリ（organizations / people / entities）

**Files:**
- Create: `crates/gaia-core/src/storage/organizations.rs`, `crates/gaia-core/src/storage/people.rs`, `crates/gaia-core/src/storage/entities.rs`
- Modify: `crates/gaia-core/src/storage/mod.rs`（`pub mod` 追加と共通ヘルパ）

**Interfaces:**
- Consumes: `contracts::types::{Alias, PersonPatch, PersonSummary, OrganizationPatch, OrganizationSummary, EntityPatch, EntitySummary}`、`domain::normalize::normalize_name`
- Produces（すべて `Result<_, StorageError>`）:
  - `storage::required(value: Option<&str>, what: &str) -> Result<&str, StorageError>`（trim して空なら Integrity）
  - `storage::parse_db_enum<T: FromStr>(raw: &str, what: &str) -> Result<T, StorageError>`（DB 文字列 → 生成 enum）
  - `organizations::{insert(conn, &OrganizationPatch) -> i64, update(conn, id, &OrganizationPatch), get(conn, id) -> Option<OrganizationSummary>, ensure(conn, id), find_by_name(conn, name) -> Vec<_>, search_like(conn, needle, limit) -> Vec<_>}`
  - `people::{insert(conn, &PersonPatch) -> i64, update(conn, id, &PersonPatch), add_alias(conn, person_id, alias, kind), get(conn, id) -> Option<PersonSummary>, ensure(conn, id), aliases(conn, person_id) -> Vec<Alias>, find_by_name(conn, name) -> Vec<PersonSummary>, find_by_alias_normalized(conn, normalized) -> Vec<PersonSummary>, search_like(conn, needle, limit) -> Vec<PersonSummary>, list_by_org(conn, org_id) -> Vec<PersonSummary>, load_many(conn, &[i64]) -> Vec<PersonSummary>}`
  - `entities::{insert(conn, &EntityPatch) -> i64, update(conn, id, &EntityPatch), get(conn, id) -> Option<EntitySummary>, ensure(conn, id), search_like(conn, needle, limit) -> Vec<EntitySummary>}`
- 注意: 契約の `type` フィールドは Rust 予約語のため typify が `type_` に生成する（`#[serde(rename = "type")]` 付き）。生成物 `contract_types.rs` で実名を確認してから書くこと

- [ ] **Step 1: storage/mod.rs に共通ヘルパを追加する**

```rust
/// insert 時の必須文字列。trim して空なら Integrity エラー。
pub(crate) fn required<'a>(value: Option<&'a str>, what: &str) -> Result<&'a str, StorageError> {
    match value.map(str::trim) {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(StorageError::Integrity(format!("{what} is required"))),
    }
}

/// DB の TEXT 列を契約 enum（typify 生成の FromStr 実装）へ変換する。
pub(crate) fn parse_db_enum<T>(raw: &str, what: &str) -> Result<T, StorageError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    raw.parse().map_err(|e: T::Err| StorageError::Integrity(format!("invalid {what} `{raw}` in db: {e}")))
}
```

`storage/mod.rs` 冒頭に `pub mod entities; pub mod organizations; pub mod people;` を追加（affiliations / audit は Task 8 で追加済み）。

- [ ] **Step 2: organizations.rs をテスト込みで実装する**

```rust
//! 組織（名寄せ層・共有）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::contracts::types::{OrganizationPatch, OrganizationSummary};

use super::{StorageError, like_pattern, required};

pub fn insert(conn: &Connection, patch: &OrganizationPatch) -> Result<i64, StorageError> {
    let name = required(patch.name.as_deref(), "organization.name")?;
    conn.execute("INSERT INTO organizations(name, kind) VALUES (?1, ?2)", params![name, patch.kind])?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: &OrganizationPatch) -> Result<(), StorageError> {
    ensure(conn, id)?;
    conn.execute(
        "UPDATE organizations SET name = COALESCE(?2, name), kind = COALESCE(?3, kind), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.name, patch.kind],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<OrganizationSummary>, StorageError> {
    Ok(conn
        .query_row("SELECT id, name, kind FROM organizations WHERE id = ?1", params![id], row)
        .optional()?)
}

pub fn ensure(conn: &Connection, id: i64) -> Result<(), StorageError> {
    if get(conn, id)?.is_none() {
        return Err(StorageError::NotFound(format!("organization {id}")));
    }
    Ok(())
}

pub fn find_by_name(conn: &Connection, name: &str) -> Result<Vec<OrganizationSummary>, StorageError> {
    let mut stmt = conn.prepare("SELECT id, name, kind FROM organizations WHERE name = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![name], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn search_like(conn: &Connection, needle: &str, limit: usize) -> Result<Vec<OrganizationSummary>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT id, name, kind FROM organizations WHERE name LIKE ?1 ESCAPE '\\' ORDER BY name LIMIT ?2")?;
    let rows = stmt.query_map(params![like_pattern(needle), limit as i64], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row(r: &Row<'_>) -> rusqlite::Result<OrganizationSummary> {
    Ok(OrganizationSummary { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;
    use serde_json::json;

    fn patch(v: serde_json::Value) -> OrganizationPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn crud_and_search() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(c, &patch(json!({"name": "CloudNative", "kind": "affiliation"})))?;
            assert!(matches!(insert(c, &patch(json!({}))), Err(StorageError::Integrity(_))));
            update(c, id, &patch(json!({"kind": "customer"})))?;
            let got = get(c, id)?.unwrap();
            assert_eq!(got.name, "CloudNative");
            assert_eq!(got.kind.as_deref(), Some("customer"));
            assert_eq!(find_by_name(c, "CloudNative")?.len(), 1);
            assert_eq!(search_like(c, "loud", 10)?.len(), 1);
            assert!(matches!(update(c, 999, &patch(json!({}))), Err(StorageError::NotFound(_))));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 3: people.rs をテスト込みで実装する**

```rust
//! 人物（名寄せ層・共有）。name と alias の正規化形を kind='normalized' の行として自動登録する（仕様書 §5.3）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{Alias, PersonPatch, PersonSummary},
    domain::normalize::normalize_name,
};

use super::{StorageError, like_pattern, organizations, required};

pub fn insert(conn: &Connection, patch: &PersonPatch) -> Result<i64, StorageError> {
    let name = required(patch.name.as_deref(), "person.name")?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "INSERT INTO people(name, org_id, role, first_met, last_seen) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![name, patch.org_id, patch.role, patch.first_met, patch.last_seen],
    )?;
    let id = conn.last_insert_rowid();
    add_alias(conn, id, name, Some("name"))?;
    for a in &patch.aliases {
        add_alias(conn, id, &a.alias, a.kind.as_deref())?;
    }
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, patch: &PersonPatch) -> Result<(), StorageError> {
    ensure(conn, id)?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "UPDATE people SET name = COALESCE(?2, name), org_id = COALESCE(?3, org_id), role = COALESCE(?4, role), \
         first_met = COALESCE(?5, first_met), last_seen = COALESCE(?6, last_seen), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.name, patch.org_id, patch.role, patch.first_met, patch.last_seen],
    )?;
    if let Some(name) = &patch.name {
        add_alias(conn, id, name, Some("name"))?;
    }
    for a in &patch.aliases {
        add_alias(conn, id, &a.alias, a.kind.as_deref())?;
    }
    Ok(())
}

/// 生の alias と、その正規化形（kind='normalized'）を登録する。既存行は上書きしない。
pub fn add_alias(conn: &Connection, person_id: i64, alias: &str, kind: Option<&str>) -> Result<(), StorageError> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Err(StorageError::Integrity("alias is required".into()));
    }
    conn.execute(
        "INSERT INTO person_aliases(person_id, alias, kind) VALUES (?1, ?2, ?3) ON CONFLICT(person_id, alias) DO NOTHING",
        params![person_id, alias, kind],
    )?;
    let normalized = normalize_name(alias);
    if !normalized.is_empty() && normalized != alias {
        conn.execute(
            "INSERT INTO person_aliases(person_id, alias, kind) VALUES (?1, ?2, 'normalized') ON CONFLICT(person_id, alias) DO NOTHING",
            params![person_id, normalized],
        )?;
    }
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<PersonSummary>, StorageError> {
    let base = conn
        .query_row(
            "SELECT p.id, p.name, p.org_id, o.name, p.role, p.first_met, p.last_seen \
             FROM people p LEFT JOIN organizations o ON o.id = p.org_id WHERE p.id = ?1",
            params![id],
            row_to_summary,
        )
        .optional()?;
    match base {
        None => Ok(None),
        Some(mut p) => {
            p.aliases = aliases(conn, p.id)?;
            Ok(Some(p))
        }
    }
}

pub fn ensure(conn: &Connection, id: i64) -> Result<(), StorageError> {
    if get(conn, id)?.is_none() {
        return Err(StorageError::NotFound(format!("person {id}")));
    }
    Ok(())
}

/// 表示用 alias（kind='normalized' の行は隠す）。
pub fn aliases(conn: &Connection, person_id: i64) -> Result<Vec<Alias>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT alias, kind FROM person_aliases WHERE person_id = ?1 AND (kind IS NULL OR kind <> 'normalized') ORDER BY alias",
    )?;
    let rows = stmt.query_map(params![person_id], |r| Ok(Alias { alias: r.get(0)?, kind: r.get(1)? }))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 氏名・alias・正規化形いずれかの完全一致。
pub fn find_by_name(conn: &Connection, name: &str) -> Result<Vec<PersonSummary>, StorageError> {
    let normalized = normalize_name(name);
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id FROM people p LEFT JOIN person_aliases a ON a.person_id = p.id \
         WHERE p.name = ?1 OR a.alias = ?1 OR a.alias = ?2 ORDER BY p.id",
    )?;
    let ids: Vec<i64> = stmt.query_map(params![name, normalized], |r| r.get(0))?.collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

/// 正規化済み表示名の完全一致（resolve_speakers の第一経路）。
pub fn find_by_alias_normalized(conn: &Connection, normalized: &str) -> Result<Vec<PersonSummary>, StorageError> {
    let mut stmt = conn.prepare("SELECT DISTINCT person_id FROM person_aliases WHERE alias = ?1 ORDER BY person_id")?;
    let ids: Vec<i64> = stmt.query_map(params![normalized], |r| r.get(0))?.collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

pub fn search_like(conn: &Connection, needle: &str, limit: usize) -> Result<Vec<PersonSummary>, StorageError> {
    let raw = like_pattern(needle);
    let norm = like_pattern(&normalize_name(needle));
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id FROM people p LEFT JOIN person_aliases a ON a.person_id = p.id \
         WHERE p.name LIKE ?1 ESCAPE '\\' OR a.alias LIKE ?1 ESCAPE '\\' OR a.alias LIKE ?2 ESCAPE '\\' \
         ORDER BY p.id LIMIT ?3",
    )?;
    let ids: Vec<i64> = stmt.query_map(params![raw, norm, limit as i64], |r| r.get(0))?.collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

pub fn list_by_org(conn: &Connection, org_id: i64) -> Result<Vec<PersonSummary>, StorageError> {
    let mut stmt = conn.prepare("SELECT id FROM people WHERE org_id = ?1 ORDER BY name")?;
    let ids: Vec<i64> = stmt.query_map(params![org_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

pub fn load_many(conn: &Connection, ids: &[i64]) -> Result<Vec<PersonSummary>, StorageError> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(p) = get(conn, *id)? {
            out.push(p);
        }
    }
    Ok(out)
}

fn row_to_summary(r: &Row<'_>) -> rusqlite::Result<PersonSummary> {
    Ok(PersonSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        org_id: r.get(2)?,
        org_name: r.get(3)?,
        role: r.get(4)?,
        first_met: r.get(5)?,
        last_seen: r.get(6)?,
        aliases: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::OrganizationPatch, storage::Db};
    use serde_json::json;

    #[test]
    fn insert_registers_normalized_aliases_and_lookup_works() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let org = organizations::insert(c, &serde_json::from_value::<OrganizationPatch>(json!({"name": "CloudNative"})).unwrap())?;
            let p: PersonPatch = serde_json::from_value(json!({
                "name": "岡村 慎太郎", "org_id": org, "role": "CTO",
                "aliases": [{"alias": "Okamura Shintaro", "kind": "romaji"}]
            }))
            .unwrap();
            let id = insert(c, &p)?;
            // 表示 alias に normalized 行は含まれない
            let names: Vec<String> = aliases(c, id)?.into_iter().map(|a| a.alias).collect();
            assert_eq!(names, vec!["Okamura Shintaro", "岡村 慎太郎"]);
            // 正規化完全一致
            assert_eq!(find_by_alias_normalized(c, "岡村慎太郎")?.len(), 1);
            assert_eq!(find_by_alias_normalized(c, "okamurashintaro")?.len(), 1);
            // 表示名（括弧付き）でも find_by_name が正規化経由でヒットし、org 名が載る
            let found = find_by_name(c, "岡村 慎太郎 (CloudNative)")?;
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].org_name.as_deref(), Some("CloudNative"));
            // 部分一致
            assert_eq!(search_like(c, "岡村", 10)?.len(), 1);
            // name 無しの insert は拒否
            assert!(matches!(insert(c, &serde_json::from_value::<PersonPatch>(json!({})).unwrap()), Err(StorageError::Integrity(_))));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn update_coalesces_and_adds_aliases() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "田中 太郎"})).unwrap())?;
            update(c, id, &serde_json::from_value::<PersonPatch>(json!({"role": "PM", "aliases": [{"alias": "tanaka"}]})).unwrap())?;
            let got = get(c, id)?.unwrap();
            assert_eq!(got.name, "田中 太郎");
            assert_eq!(got.role.as_deref(), Some("PM"));
            assert_eq!(find_by_alias_normalized(c, "tanaka")?.len(), 1);
            assert!(matches!(update(c, 999, &serde_json::from_value::<PersonPatch>(json!({})).unwrap()), Err(StorageError::NotFound(_))));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 4: entities.rs をテスト込みで実装する**

```rust
//! 汎用エンティティ（名寄せ層・共有）。attrs は JSON オブジェクトを TEXT で持つ。
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Map, Value};

use crate::contracts::types::{EntityPatch, EntitySummary};

use super::{StorageError, like_pattern, required};

pub fn insert(conn: &Connection, patch: &EntityPatch) -> Result<i64, StorageError> {
    let type_ = required(patch.type_.as_deref(), "entity.type")?;
    let name = required(patch.name.as_deref(), "entity.name")?;
    let attrs = Value::Object(patch.attrs.clone()).to_string();
    conn.execute("INSERT INTO entities(type, name, attrs) VALUES (?1, ?2, ?3)", params![type_, name, attrs])?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: &EntityPatch) -> Result<(), StorageError> {
    ensure(conn, id)?;
    // attrs は「空なら変更しない、非空なら全置換」
    let attrs = if patch.attrs.is_empty() { None } else { Some(Value::Object(patch.attrs.clone()).to_string()) };
    conn.execute(
        "UPDATE entities SET type = COALESCE(?2, type), name = COALESCE(?3, name), attrs = COALESCE(?4, attrs), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.type_, patch.name, attrs],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<EntitySummary>, StorageError> {
    let raw = conn
        .query_row("SELECT id, type, name, attrs FROM entities WHERE id = ?1", params![id], raw_row)
        .optional()?;
    raw.map(convert).transpose()
}

pub fn ensure(conn: &Connection, id: i64) -> Result<(), StorageError> {
    if get(conn, id)?.is_none() {
        return Err(StorageError::NotFound(format!("entity {id}")));
    }
    Ok(())
}

pub fn search_like(conn: &Connection, needle: &str, limit: usize) -> Result<Vec<EntitySummary>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, type, name, attrs FROM entities WHERE name LIKE ?1 ESCAPE '\\' OR type LIKE ?1 ESCAPE '\\' ORDER BY name LIMIT ?2",
    )?;
    let raws: Vec<RawEntity> = stmt.query_map(params![like_pattern(needle), limit as i64], raw_row)?.collect::<Result<_, _>>()?;
    raws.into_iter().map(convert).collect()
}

type RawEntity = (i64, String, String, String);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawEntity> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
}

fn convert((id, type_, name, attrs): RawEntity) -> Result<EntitySummary, StorageError> {
    let attrs: Map<String, Value> = serde_json::from_str(&attrs)?;
    Ok(EntitySummary { id, type_, name, attrs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;
    use serde_json::json;

    #[test]
    fn attrs_round_trip_and_search() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(c, &serde_json::from_value::<EntityPatch>(json!({"type": "product", "name": "gaia-library", "attrs": {"lang": "rust"}})).unwrap())?;
            let got = get(c, id)?.unwrap();
            assert_eq!(got.type_, "product");
            assert_eq!(got.attrs["lang"], "rust");
            update(c, id, &serde_json::from_value::<EntityPatch>(json!({"attrs": {"lang": "rust", "db": "sqlite"}})).unwrap())?;
            assert_eq!(get(c, id)?.unwrap().attrs["db"], "sqlite");
            assert_eq!(search_like(c, "prod", 10)?.len(), 1);
            assert!(matches!(insert(c, &serde_json::from_value::<EntityPatch>(json!({"name": "x"})).unwrap()), Err(StorageError::Integrity(_))));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 5: テストを実行する**

Run: `cargo test -p gaia-core storage`
Expected: 既存＋新規（organizations 1 / people 2 / entities 1）がすべて成功

- [ ] **Step 6: コミット**

```bash
git add crates/gaia-core
git commit -m "feat(core): add identity-layer repositories (organizations, people, entities)" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 10: 内容層リポジトリ（targets / engagements / interactions / facts / refs / glossary）

**Files:**
- Create: `crates/gaia-core/src/storage/targets.rs`, `crates/gaia-core/src/storage/engagements.rs`, `crates/gaia-core/src/storage/interactions.rs`, `crates/gaia-core/src/storage/facts.rs`, `crates/gaia-core/src/storage/refs.rs`, `crates/gaia-core/src/storage/glossary.rs`
- Modify: `crates/gaia-core/src/storage/mod.rs`（`pub mod` 追加）

**Interfaces:**
- Consumes: Task 8 の `ScopeSet`、Task 9 のリポジトリとヘルパ
- Produces（すべて `Result<_, StorageError>`。内容層の SELECT は必ず scope フィルタ付き）:
  - `targets::{exists(conn, target_type: &str, id) -> bool, ensure(conn, target_type: &str, id)}`
  - `engagements::{insert(conn, &EngagementPatch, scope: &str) -> i64, update(conn, id, &EngagementPatch, scope: &str), add_person(conn, engagement_id, person_id, role: Option<&str>), get(conn, id, &ScopeSet) -> Option<EngagementSummary>, find_by_name(conn, name, &ScopeSet) -> Vec<_>, members(conn, engagement_id) -> Vec<EngagementPerson>, member_ids(conn, engagement_id) -> Vec<i64>, for_person(conn, person_id, &ScopeSet) -> Vec<_>, for_org(conn, org_id, &ScopeSet) -> Vec<_>, search_like(conn, needle, &ScopeSet, limit) -> Vec<_>}`
  - `interactions::{insert(conn, &InteractionPatch, scope) -> i64, update(conn, id, &InteractionPatch, scope), get(conn, id, &ScopeSet) -> Option<InteractionSummary>, recent_for_person(conn, person_id, &ScopeSet, limit) -> Vec<_>, recent_for_engagement(conn, engagement_id, &ScopeSet, limit) -> Vec<_>, search_like(conn, needle, &ScopeSet, limit) -> Vec<_>}`
  - `facts::{insert(conn, &FactPatch, kind: Kind, scope) -> i64, supersede(conn, old_id, &FactPatch, kind, scope) -> i64, update(conn, id, &FactPatch, scope), get(conn, id, &ScopeSet) -> Option<Fact>, for_entity(conn, entity_type: &str, entity_id, &ScopeSet, limit) -> Vec<Fact>, search(conn, query, &ScopeSet, limit) -> Vec<Fact>}`
  - `refs::{insert(conn, &RefPatch, scope) -> i64, update(conn, id, &RefPatch, scope), get(conn, id, &ScopeSet) -> Option<Reference>, for_target(conn, target_type: &str, target_id, &ScopeSet) -> Vec<Reference>}`
  - `glossary::{insert(conn, &GlossaryPatch, scope) -> i64, update(conn, id, &GlossaryPatch, scope), get(conn, id, &ScopeSet) -> Option<GlossaryTerm>, list(conn, engagement_id: Option<i64>, &ScopeSet) -> Vec<GlossaryTerm>, search_like(conn, needle, &ScopeSet, limit) -> Vec<GlossaryTerm>}`

- [ ] **Step 1: targets.rs をテスト込みで実装する**

```rust
//! polymorphic 参照先（entity_type + entity_id / target_type + target_id）の存在検証。
use rusqlite::{Connection, OptionalExtension, params};

use super::StorageError;

fn table_for(target_type: &str) -> Result<&'static str, StorageError> {
    Ok(match target_type {
        "person" => "people",
        "organization" => "organizations",
        "engagement" => "engagements",
        "interaction" => "interactions",
        "entity" => "entities",
        "fact" => "facts",
        other => return Err(StorageError::Integrity(format!("unknown target type `{other}`"))),
    })
}

pub fn exists(conn: &Connection, target_type: &str, id: i64) -> Result<bool, StorageError> {
    let table = table_for(target_type)?;
    let found: Option<i64> = conn
        .query_row(&format!("SELECT 1 FROM {table} WHERE id = ?1"), params![id], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

pub fn ensure(conn: &Connection, target_type: &str, id: i64) -> Result<(), StorageError> {
    if exists(conn, target_type, id)? {
        Ok(())
    } else {
        Err(StorageError::NotFound(format!("{target_type} {id}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::PersonPatch, storage::{Db, people}};
    use serde_json::json;

    #[test]
    fn exists_maps_types_to_tables_and_rejects_unknown() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "p"})).unwrap())?;
            assert!(exists(c, "person", id)?);
            assert!(!exists(c, "organization", 999)?);
            assert!(matches!(exists(c, "widget", 1), Err(StorageError::Integrity(_))));
            assert!(matches!(ensure(c, "fact", 999), Err(StorageError::NotFound(_))));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 2: engagements.rs をテスト込みで実装する**

```rust
//! 案件（内容層・scope 必須）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{EngagementPatch, EngagementPerson, EngagementSummary},
    scope::ScopeSet,
};

use super::{StorageError, like_pattern, organizations, people, required};

const COLS: &str = "e.id, e.name, e.org_id, o.name, e.scope, e.status, e.started_at, e.ended_at";
const FROM: &str = "FROM engagements e LEFT JOIN organizations o ON o.id = e.org_id";

pub fn insert(conn: &Connection, patch: &EngagementPatch, scope: &str) -> Result<i64, StorageError> {
    let name = required(patch.name.as_deref(), "engagement.name")?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "INSERT INTO engagements(name, org_id, scope, status, started_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![name, patch.org_id, scope, patch.status, patch.started_at, patch.ended_at],
    )?;
    let id = conn.last_insert_rowid();
    for p in &patch.people {
        add_person(conn, id, p.person_id, p.role.as_deref())?;
    }
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, patch: &EngagementPatch, scope: &str) -> Result<(), StorageError> {
    ensure_in_scope(conn, id, scope)?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "UPDATE engagements SET name = COALESCE(?2, name), org_id = COALESCE(?3, org_id), status = COALESCE(?4, status), \
         started_at = COALESCE(?5, started_at), ended_at = COALESCE(?6, ended_at), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.name, patch.org_id, patch.status, patch.started_at, patch.ended_at],
    )?;
    for p in &patch.people {
        add_person(conn, id, p.person_id, p.role.as_deref())?;
    }
    Ok(())
}

fn ensure_in_scope(conn: &Connection, id: i64, scope: &str) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!("engagement {id} (in scope `{scope}`)")));
    }
    Ok(())
}

pub fn add_person(conn: &Connection, engagement_id: i64, person_id: i64, role: Option<&str>) -> Result<(), StorageError> {
    people::ensure(conn, person_id)?;
    conn.execute(
        "INSERT INTO engagement_people(engagement_id, person_id, role) VALUES (?1, ?2, ?3) \
         ON CONFLICT(engagement_id, person_id) DO UPDATE SET role = COALESCE(excluded.role, role)",
        params![engagement_id, person_id, role],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64, scopes: &ScopeSet) -> Result<Option<EngagementSummary>, StorageError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} {FROM} WHERE e.id = ?1 AND e.scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            row,
        )
        .optional()?)
}

pub fn find_by_name(conn: &Connection, name: &str, scopes: &ScopeSet) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} WHERE e.name = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) ORDER BY e.id"
    ))?;
    let rows = stmt.query_map(params![name, scopes.as_json()], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 案件の関係者（PersonSummary ＋ 役割）。
pub fn members(conn: &Connection, engagement_id: i64) -> Result<Vec<EngagementPerson>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT person_id, role FROM engagement_people WHERE engagement_id = ?1 ORDER BY person_id")?;
    let pairs: Vec<(i64, Option<String>)> =
        stmt.query_map(params![engagement_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(pairs.len());
    for (pid, role) in pairs {
        if let Some(person) = people::get(conn, pid)? {
            out.push(EngagementPerson { person, role });
        }
    }
    Ok(out)
}

pub fn member_ids(conn: &Connection, engagement_id: i64) -> Result<Vec<i64>, StorageError> {
    let mut stmt = conn.prepare("SELECT person_id FROM engagement_people WHERE engagement_id = ?1 ORDER BY person_id")?;
    Ok(stmt.query_map(params![engagement_id], |r| r.get(0))?.collect::<Result<_, _>>()?)
}

pub fn for_person(conn: &Connection, person_id: i64, scopes: &ScopeSet) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} JOIN engagement_people ep ON ep.engagement_id = e.id \
         WHERE ep.person_id = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) ORDER BY e.id"
    ))?;
    let rows = stmt.query_map(params![person_id, scopes.as_json()], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn for_org(conn: &Connection, org_id: i64, scopes: &ScopeSet) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} WHERE e.org_id = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) ORDER BY e.id"
    ))?;
    let rows = stmt.query_map(params![org_id, scopes.as_json()], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn search_like(conn: &Connection, needle: &str, scopes: &ScopeSet, limit: usize) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} WHERE e.name LIKE ?1 ESCAPE '\\' AND e.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY e.name LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![like_pattern(needle), scopes.as_json(), limit as i64], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row(r: &Row<'_>) -> rusqlite::Result<EngagementSummary> {
    Ok(EngagementSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        org_id: r.get(2)?,
        org_name: r.get(3)?,
        scope: r.get(4)?,
        status: r.get(5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::PersonPatch, storage::{Db, affiliations}};
    use serde_json::json;

    #[test]
    fn scope_filters_and_members_work() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            let pid = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap())?;
            let patch: EngagementPatch = serde_json::from_value(json!({
                "name": "RELATIONS支援", "status": "active", "people": [{"person_id": pid, "role": "key_person"}]
            }))
            .unwrap();
            let id = insert(c, &patch, "cn")?;
            assert!(get(c, id, &ScopeSet::single("cn"))?.is_some());
            assert!(get(c, id, &ScopeSet::single("other"))?.is_none(), "scope 外からは見えない");
            assert_eq!(members(c, id)?.len(), 1);
            assert_eq!(member_ids(c, id)?, vec![pid]);
            assert_eq!(for_person(c, pid, &ScopeSet::single("cn"))?.len(), 1);
            assert_eq!(search_like(c, "RELATIONS", &ScopeSet::single("cn"), 10)?.len(), 1);
            assert!(matches!(
                update(c, id, &serde_json::from_value::<EngagementPatch>(json!({"status": "done"})).unwrap(), "other"),
                Err(StorageError::NotFound(_))
            ));
            update(c, id, &serde_json::from_value::<EngagementPatch>(json!({"status": "done"})).unwrap(), "cn")?;
            assert_eq!(get(c, id, &ScopeSet::single("cn"))?.unwrap().status.as_deref(), Some("done"));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 3: interactions.rs をテスト込みで実装する**

```rust
//! 会議・面談ログ（内容層・scope 必須）。全文は持たず要点のみ（正本は参照先）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{InteractionPatch, InteractionSummary},
    scope::ScopeSet,
};

use super::{StorageError, engagements, like_pattern, people, required};

const COLS: &str = "i.id, i.kind, i.occurred_at, i.summary, i.engagement_id, i.scope";

pub fn insert(conn: &Connection, patch: &InteractionPatch, scope: &str) -> Result<i64, StorageError> {
    let kind = required(patch.kind.as_deref(), "interaction.kind")?;
    let occurred_at = required(patch.occurred_at.as_deref(), "interaction.occurred_at")?;
    let summary = required(patch.summary.as_deref(), "interaction.summary")?;
    ensure_engagement_in_scope(conn, patch.engagement_id, scope)?;
    for pid in &patch.person_ids {
        people::ensure(conn, *pid)?;
    }
    conn.execute(
        "INSERT INTO interactions(kind, occurred_at, summary, engagement_id, scope) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind, occurred_at, summary, patch.engagement_id, scope],
    )?;
    let id = conn.last_insert_rowid();
    link_people(conn, id, &patch.person_ids)?;
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, patch: &InteractionPatch, scope: &str) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!("interaction {id} (in scope `{scope}`)")));
    }
    ensure_engagement_in_scope(conn, patch.engagement_id, scope)?;
    for pid in &patch.person_ids {
        people::ensure(conn, *pid)?;
    }
    conn.execute(
        "UPDATE interactions SET kind = COALESCE(?2, kind), occurred_at = COALESCE(?3, occurred_at), \
         summary = COALESCE(?4, summary), engagement_id = COALESCE(?5, engagement_id) WHERE id = ?1",
        params![id, patch.kind, patch.occurred_at, patch.summary, patch.engagement_id],
    )?;
    link_people(conn, id, &patch.person_ids)?;
    Ok(())
}

fn ensure_engagement_in_scope(conn: &Connection, engagement_id: Option<i64>, scope: &str) -> Result<(), StorageError> {
    if let Some(eid) = engagement_id {
        if engagements::get(conn, eid, &ScopeSet::single(scope))?.is_none() {
            return Err(StorageError::NotFound(format!("engagement {eid} (in scope `{scope}`)")));
        }
    }
    Ok(())
}

fn link_people(conn: &Connection, interaction_id: i64, person_ids: &[i64]) -> Result<(), StorageError> {
    for pid in person_ids {
        conn.execute(
            "INSERT INTO interaction_people(interaction_id, person_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            params![interaction_id, pid],
        )?;
    }
    Ok(())
}

pub fn get(conn: &Connection, id: i64, scopes: &ScopeSet) -> Result<Option<InteractionSummary>, StorageError> {
    let base = conn
        .query_row(
            &format!("SELECT {COLS} FROM interactions i WHERE i.id = ?1 AND i.scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            row,
        )
        .optional()?;
    fill_people(conn, base.into_iter().collect()).map(|mut v| v.pop())
}

pub fn recent_for_person(conn: &Connection, person_id: i64, scopes: &ScopeSet, limit: usize) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM interactions i JOIN interaction_people ip ON ip.interaction_id = i.id \
         WHERE ip.person_id = ?1 AND i.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY i.occurred_at DESC LIMIT ?3"
    ))?;
    let rows: Vec<InteractionSummary> = stmt.query_map(params![person_id, scopes.as_json(), limit as i64], row)?.collect::<Result<_, _>>()?;
    fill_people(conn, rows)
}

pub fn recent_for_engagement(conn: &Connection, engagement_id: i64, scopes: &ScopeSet, limit: usize) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM interactions i WHERE i.engagement_id = ?1 AND i.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY i.occurred_at DESC LIMIT ?3"
    ))?;
    let rows: Vec<InteractionSummary> = stmt.query_map(params![engagement_id, scopes.as_json(), limit as i64], row)?.collect::<Result<_, _>>()?;
    fill_people(conn, rows)
}

pub fn search_like(conn: &Connection, needle: &str, scopes: &ScopeSet, limit: usize) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM interactions i WHERE i.summary LIKE ?1 ESCAPE '\\' AND i.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY i.occurred_at DESC LIMIT ?3"
    ))?;
    let rows: Vec<InteractionSummary> = stmt.query_map(params![like_pattern(needle), scopes.as_json(), limit as i64], row)?.collect::<Result<_, _>>()?;
    fill_people(conn, rows)
}

fn fill_people(conn: &Connection, mut rows: Vec<InteractionSummary>) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare("SELECT person_id FROM interaction_people WHERE interaction_id = ?1 ORDER BY person_id")?;
    for r in &mut rows {
        r.person_ids = stmt.query_map(params![r.id], |x| x.get(0))?.collect::<Result<_, _>>()?;
    }
    Ok(rows)
}

fn row(r: &Row<'_>) -> rusqlite::Result<InteractionSummary> {
    Ok(InteractionSummary {
        id: r.get(0)?,
        kind: r.get(1)?,
        occurred_at: r.get(2)?,
        summary: r.get(3)?,
        engagement_id: r.get(4)?,
        scope: r.get(5)?,
        person_ids: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::PersonPatch, storage::{Db, affiliations}};
    use serde_json::json;

    #[test]
    fn insert_links_people_and_scope_filters_reads() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let pid = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap())?;
            let patch: InteractionPatch = serde_json::from_value(json!({
                "kind": "meeting", "occurred_at": "2026-08-27T10:00:00Z", "summary": "定例。次回は 9/3", "person_ids": [pid]
            }))
            .unwrap();
            let id = insert(c, &patch, "cn")?;
            let got = get(c, id, &ScopeSet::single("cn"))?.unwrap();
            assert_eq!(got.person_ids, vec![pid]);
            assert_eq!(recent_for_person(c, pid, &ScopeSet::single("cn"), 10)?.len(), 1);
            assert_eq!(search_like(c, "定例", &ScopeSet::single("cn"), 10)?.len(), 1);
            // summary 無しは拒否
            assert!(matches!(
                insert(c, &serde_json::from_value::<InteractionPatch>(json!({"kind": "call", "occurred_at": "2026-08-27"})).unwrap(), "cn"),
                Err(StorageError::Integrity(_))
            ));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 4: facts.rs をテスト込みで実装する**

```rust
//! facts（内容層・scope 必須）。「現在の fact」= superseded_by IS NULL。検索は trigram FTS（3 文字未満は LIKE）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{Fact, FactPatch, Kind},
    scope::ScopeSet,
};

use super::{StorageError, like_pattern, parse_db_enum, required, targets};

const COLS: &str = "f.id, f.entity_type, f.entity_id, f.statement, f.predicate, f.value, f.kind, f.scope, f.valid_from, f.superseded_by, f.created_at";

pub fn insert(conn: &Connection, patch: &FactPatch, kind: Kind, scope: &str) -> Result<i64, StorageError> {
    let entity_type = patch.entity_type.ok_or_else(|| StorageError::Integrity("fact.entity_type is required".into()))?;
    let entity_id = patch.entity_id.ok_or_else(|| StorageError::Integrity("fact.entity_id is required".into()))?;
    let statement = required(patch.statement.as_deref(), "fact.statement")?;
    targets::ensure(conn, &entity_type.to_string(), entity_id)?;
    conn.execute(
        "INSERT INTO facts(entity_type, entity_id, statement, predicate, value, kind, scope, valid_from) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entity_type.to_string(),
            entity_id,
            statement,
            patch.predicate,
            patch.value,
            kind.to_string(),
            scope,
            patch.valid_from
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 旧 fact を新 fact で置き換える（superseded_by リンク）。旧 fact は同じ scope 内・未置換であること。
pub fn supersede(conn: &Connection, old_id: i64, patch: &FactPatch, kind: Kind, scope: &str) -> Result<i64, StorageError> {
    let old = get(conn, old_id, &ScopeSet::single(scope))?
        .ok_or_else(|| StorageError::NotFound(format!("fact {old_id} (in scope `{scope}`)")))?;
    if let Some(by) = old.superseded_by {
        return Err(StorageError::Integrity(format!("fact {old_id} is already superseded by {by}")));
    }
    let new_id = insert(conn, patch, kind, scope)?;
    conn.execute("UPDATE facts SET superseded_by = ?2 WHERE id = ?1", params![old_id, new_id])?;
    Ok(new_id)
}

pub fn update(conn: &Connection, id: i64, patch: &FactPatch, scope: &str) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!("fact {id} (in scope `{scope}`)")));
    }
    conn.execute(
        "UPDATE facts SET statement = COALESCE(?2, statement), predicate = COALESCE(?3, predicate), \
         value = COALESCE(?4, value), valid_from = COALESCE(?5, valid_from) WHERE id = ?1",
        params![id, patch.statement, patch.predicate, patch.value, patch.valid_from],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64, scopes: &ScopeSet) -> Result<Option<Fact>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM facts f WHERE f.id = ?1 AND f.scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
}

/// エンティティに付く現在の facts（新しい順）。
pub fn for_entity(conn: &Connection, entity_type: &str, entity_id: i64, scopes: &ScopeSet, limit: usize) -> Result<Vec<Fact>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM facts f WHERE f.entity_type = ?1 AND f.entity_id = ?2 AND f.superseded_by IS NULL \
         AND f.scope IN (SELECT value FROM json_each(?3)) ORDER BY f.id DESC LIMIT ?4"
    ))?;
    collect(stmt.query_map(params![entity_type, entity_id, scopes.as_json(), limit as i64], raw_row)?)
}

/// 全文検索。3 文字（Unicode 文字数）以上は trigram FTS を bm25 順で、未満は LIKE。
pub fn search(conn: &Connection, query: &str, scopes: &ScopeSet, limit: usize) -> Result<Vec<Fact>, StorageError> {
    if query.chars().count() >= 3 {
        let match_expr = format!("\"{}\"", query.replace('"', "\"\""));
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM (SELECT rowid AS fid, rank FROM facts_fts WHERE facts_fts MATCH ?1) m \
             JOIN facts f ON f.id = m.fid \
             WHERE f.superseded_by IS NULL AND f.scope IN (SELECT value FROM json_each(?2)) \
             ORDER BY m.rank LIMIT ?3"
        ))?;
        collect(stmt.query_map(params![match_expr, scopes.as_json(), limit as i64], raw_row)?)
    } else {
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM facts f WHERE f.statement LIKE ?1 ESCAPE '\\' AND f.superseded_by IS NULL \
             AND f.scope IN (SELECT value FROM json_each(?2)) ORDER BY f.id DESC LIMIT ?3"
        ))?;
        collect(stmt.query_map(params![like_pattern(query), scopes.as_json(), limit as i64], raw_row)?)
    }
}

type RawFact = (i64, String, i64, String, Option<String>, Option<String>, String, String, Option<String>, Option<i64>, String);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawFact> {
    Ok((
        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?,
    ))
}

fn convert(raw: RawFact) -> Result<Fact, StorageError> {
    let (id, entity_type, entity_id, statement, predicate, value, kind, scope, valid_from, superseded_by, created_at) = raw;
    Ok(Fact {
        id,
        entity_type: parse_db_enum(&entity_type, "fact entity_type")?,
        entity_id,
        statement,
        predicate,
        value,
        kind: parse_db_enum(&kind, "fact kind")?,
        scope,
        valid_from,
        superseded_by,
        created_at,
    })
}

fn collect(rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<RawFact>>) -> Result<Vec<Fact>, StorageError> {
    rows.collect::<Result<Vec<RawFact>, _>>()?.into_iter().map(convert).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::PersonPatch, storage::{Db, affiliations, people}};
    use serde_json::json;

    fn fixture(db: &Db) -> i64 {
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap())
        })
        .unwrap()
    }

    fn fp(v: serde_json::Value) -> FactPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn insert_requires_existing_target_and_search_respects_scope() {
        let db = Db::open_in_memory().unwrap();
        let pid = fixture(&db);
        db.with_conn::<_, StorageError>(|c| {
            assert!(matches!(
                insert(c, &fp(json!({"entity_type": "person", "entity_id": 999, "statement": "x"})), Kind::Fact, "cn"),
                Err(StorageError::NotFound(_))
            ));
            insert(c, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "Okta の移行を支援している"})), Kind::Fact, "cn")?;
            insert(c, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "極秘の件"})), Kind::Inference, "other")?;
            // FTS（3 文字以上）は scope 内だけ
            assert_eq!(search(c, "Okta", &ScopeSet::single("cn"), 10)?.len(), 1);
            assert_eq!(search(c, "極秘の件", &ScopeSet::single("cn"), 10)?.len(), 0);
            // 2 文字は LIKE フォールバック
            assert_eq!(search(c, "移行", &ScopeSet::single("cn"), 10)?.len(), 1);
            assert_eq!(for_entity(c, "person", pid, &ScopeSet::single("cn"), 20)?.len(), 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn supersede_links_history_and_hides_old_from_search() {
        let db = Db::open_in_memory().unwrap();
        let pid = fixture(&db);
        db.with_conn::<_, StorageError>(|c| {
            let old = insert(c, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "役職はマネージャー", "predicate": "role", "value": "manager"})), Kind::Fact, "cn")?;
            let new = supersede(c, old, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "役職は部長", "predicate": "role", "value": "director"})), Kind::Fact, "cn")?;
            assert_eq!(get(c, old, &ScopeSet::single("cn"))?.unwrap().superseded_by, Some(new));
            let current = for_entity(c, "person", pid, &ScopeSet::single("cn"), 20)?;
            assert_eq!(current.len(), 1);
            assert_eq!(current[0].id, new);
            assert_eq!(search(c, "マネージャー", &ScopeSet::single("cn"), 10)?.len(), 0, "置換済みは検索に出ない");
            assert!(matches!(supersede(c, old, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "x"})), Kind::Fact, "cn"), Err(StorageError::Integrity(_))));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 5: refs.rs をテスト込みで実装する**

```rust
//! refs = 「思い出し方の索引」の実体（内容層・scope 必須）。URI だけの参照（note 無し）は登録禁止。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{RefPatch, Reference},
    scope::ScopeSet,
};

use super::{StorageError, parse_db_enum, required, targets};

const COLS: &str = "id, target_type, target_id, system, uri, title, note, snapshot, scope, last_verified, created_at";

pub fn insert(conn: &Connection, patch: &RefPatch, scope: &str) -> Result<i64, StorageError> {
    let target_type = patch.target_type.ok_or_else(|| StorageError::Integrity("ref.target_type is required".into()))?;
    let target_id = patch.target_id.ok_or_else(|| StorageError::Integrity("ref.target_id is required".into()))?;
    let system = required(patch.system.as_deref(), "ref.system")?;
    let uri = required(patch.uri.as_deref(), "ref.uri")?;
    let note = required(patch.note.as_deref(), "ref.note")?;
    targets::ensure(conn, &target_type.to_string(), target_id)?;
    conn.execute(
        "INSERT INTO refs(target_type, target_id, system, uri, title, note, snapshot, scope, last_verified) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            target_type.to_string(),
            target_id,
            system,
            uri,
            patch.title,
            note,
            patch.snapshot,
            scope,
            patch.last_verified
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 紐付け先（target_type / target_id）は変更不可。内容だけ更新する。
pub fn update(conn: &Connection, id: i64, patch: &RefPatch, scope: &str) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!("ref {id} (in scope `{scope}`)")));
    }
    conn.execute(
        "UPDATE refs SET system = COALESCE(?2, system), uri = COALESCE(?3, uri), title = COALESCE(?4, title), \
         note = COALESCE(?5, note), snapshot = COALESCE(?6, snapshot), last_verified = COALESCE(?7, last_verified) WHERE id = ?1",
        params![id, patch.system, patch.uri, patch.title, patch.note, patch.snapshot, patch.last_verified],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64, scopes: &ScopeSet) -> Result<Option<Reference>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM refs WHERE id = ?1 AND scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
}

pub fn for_target(conn: &Connection, target_type: &str, target_id: i64, scopes: &ScopeSet) -> Result<Vec<Reference>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM refs WHERE target_type = ?1 AND target_id = ?2 AND scope IN (SELECT value FROM json_each(?3)) ORDER BY id"
    ))?;
    let raws: Vec<RawRef> = stmt.query_map(params![target_type, target_id, scopes.as_json()], raw_row)?.collect::<Result<_, _>>()?;
    raws.into_iter().map(convert).collect()
}

type RawRef = (i64, String, i64, String, String, Option<String>, String, Option<String>, String, Option<String>, String);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawRef> {
    Ok((
        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?,
    ))
}

fn convert(raw: RawRef) -> Result<Reference, StorageError> {
    let (id, target_type, target_id, system, uri, title, note, snapshot, scope, last_verified, created_at) = raw;
    Ok(Reference {
        id,
        target_type: parse_db_enum(&target_type, "ref target_type")?,
        target_id,
        system,
        uri,
        title,
        note,
        snapshot,
        scope,
        last_verified,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::{FactPatch, Kind, PersonPatch}, storage::{Db, affiliations, facts, people}};
    use serde_json::json;

    #[test]
    fn note_is_mandatory_and_fact_targets_are_allowed() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let pid = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap())?;
            let fid = facts::insert(
                c,
                &serde_json::from_value::<FactPatch>(json!({"entity_type": "person", "entity_id": pid, "statement": "決定: SSO を導入する"})).unwrap(),
                Kind::Fact,
                "cn",
            )?;
            // URI だけ（note 無し）は禁止
            assert!(matches!(
                insert(c, &serde_json::from_value::<RefPatch>(json!({"target_type": "person", "target_id": pid, "system": "notion", "uri": "https://x"})).unwrap(), "cn"),
                Err(StorageError::Integrity(_))
            ));
            // fact への根拠参照
            let rid = insert(
                c,
                &serde_json::from_value::<RefPatch>(json!({
                    "target_type": "fact", "target_id": fid, "system": "minutes",
                    "uri": "minutes://meeting/42#t=1200", "note": "決定箇所の議事録参照", "snapshot": "SSO 導入を決定"
                }))
                .unwrap(),
                "cn",
            )?;
            let refs = for_target(c, "fact", fid, &ScopeSet::single("cn"))?;
            assert_eq!(refs.len(), 1);
            assert_eq!(refs[0].id, rid);
            assert_eq!(refs[0].snapshot.as_deref(), Some("SSO 導入を決定"));
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 6: glossary.rs をテスト込みで実装する**

```rust
//! 案件別用語集（内容層・scope 必須）。Whisper の initial_prompt に注入する語彙ヒントの供給源。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{GlossaryPatch, GlossaryTerm},
    scope::ScopeSet,
};

use super::{StorageError, engagements, like_pattern, required};

const COLS: &str = "id, term, reading, definition, engagement_id, scope";

pub fn insert(conn: &Connection, patch: &GlossaryPatch, scope: &str) -> Result<i64, StorageError> {
    let term = required(patch.term.as_deref(), "glossary.term")?;
    if let Some(eid) = patch.engagement_id {
        if engagements::get(conn, eid, &ScopeSet::single(scope))?.is_none() {
            return Err(StorageError::NotFound(format!("engagement {eid} (in scope `{scope}`)")));
        }
    }
    conn.execute(
        "INSERT INTO glossary(term, reading, definition, engagement_id, scope) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![term, patch.reading, patch.definition, patch.engagement_id, scope],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: &GlossaryPatch, scope: &str) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!("glossary term {id} (in scope `{scope}`)")));
    }
    conn.execute(
        "UPDATE glossary SET term = COALESCE(?2, term), reading = COALESCE(?3, reading), \
         definition = COALESCE(?4, definition), engagement_id = COALESCE(?5, engagement_id), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.term, patch.reading, patch.definition, patch.engagement_id],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64, scopes: &ScopeSet) -> Result<Option<GlossaryTerm>, StorageError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM glossary WHERE id = ?1 AND scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            row,
        )
        .optional()?)
}

/// engagement_id 指定ならその案件の用語、None なら scope 内の全用語。
pub fn list(conn: &Connection, engagement_id: Option<i64>, scopes: &ScopeSet) -> Result<Vec<GlossaryTerm>, StorageError> {
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match engagement_id {
        Some(eid) => (
            format!("SELECT {COLS} FROM glossary WHERE engagement_id = ?1 AND scope IN (SELECT value FROM json_each(?2)) ORDER BY term"),
            vec![Box::new(eid), Box::new(scopes.as_json())],
        ),
        None => (
            format!("SELECT {COLS} FROM glossary WHERE scope IN (SELECT value FROM json_each(?1)) ORDER BY term"),
            vec![Box::new(scopes.as_json())],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params_vec.iter().map(|p| p.as_ref())), row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn search_like(conn: &Connection, needle: &str, scopes: &ScopeSet, limit: usize) -> Result<Vec<GlossaryTerm>, StorageError> {
    let pat = like_pattern(needle);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM glossary WHERE (term LIKE ?1 ESCAPE '\\' OR reading LIKE ?1 ESCAPE '\\' OR definition LIKE ?1 ESCAPE '\\') \
         AND scope IN (SELECT value FROM json_each(?2)) ORDER BY term LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![pat, scopes.as_json(), limit as i64], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row(r: &Row<'_>) -> rusqlite::Result<GlossaryTerm> {
    Ok(GlossaryTerm {
        id: r.get(0)?,
        term: r.get(1)?,
        reading: r.get(2)?,
        definition: r.get(3)?,
        engagement_id: r.get(4)?,
        scope: r.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::EngagementPatch, storage::{Db, affiliations}};
    use serde_json::json;

    #[test]
    fn list_by_engagement_or_all_in_scope() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let eid = engagements::insert(c, &serde_json::from_value::<EngagementPatch>(json!({"name": "案件A"})).unwrap(), "cn")?;
            insert(c, &serde_json::from_value::<GlossaryPatch>(json!({"term": "SCIM", "reading": "スキム", "engagement_id": eid})).unwrap(), "cn")?;
            insert(c, &serde_json::from_value::<GlossaryPatch>(json!({"term": "IdP"})).unwrap(), "cn")?;
            assert_eq!(list(c, Some(eid), &ScopeSet::single("cn"))?.len(), 1);
            assert_eq!(list(c, None, &ScopeSet::single("cn"))?.len(), 2);
            assert_eq!(search_like(c, "スキム", &ScopeSet::single("cn"), 10)?.len(), 1);
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 7: mod.rs に `pub mod` を追加し、テストを実行する**

`storage/mod.rs` に `pub mod engagements; pub mod facts; pub mod glossary; pub mod interactions; pub mod refs; pub mod targets;` を追加。

Run: `cargo test -p gaia-core storage`
Expected: すべて成功。`facts::collect` の型が合わない場合は `MappedRows` を使わず `let raws: Vec<RawFact> = rows.collect::<Result<_, _>>()?;` を各呼び出し側に展開してよい（挙動は同じ）

- [ ] **Step 8: コミット**

```bash
git add crates/gaia-core
git commit -m "feat(core): add scoped content repositories (engagements, interactions, facts, refs, glossary)" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 11: 提案キュー（storage/proposals ＋ domain/proposals）

**Files:**
- Create: `crates/gaia-core/src/storage/proposals.rs`, `crates/gaia-core/src/domain/proposals.rs`
- Modify: `crates/gaia-core/src/storage/mod.rs`, `crates/gaia-core/src/domain/mod.rs`

**Interfaces:**
- Consumes: Task 9 / 10 の全リポジトリ、`domain::predicates`、`contracts::types::{Proposal, Provenance, 各 Patch}`
- Produces:
  - `storage::proposals::{NewProposal { action, target_type, target_id, patch: Map<String,Value>, kind, scope: String, provenance: Option<Provenance>, provenance_id: Option<i64>, proposed_by: String, request_id: String }, insert(conn, &NewProposal) -> i64, get(conn, id) -> Option<Proposal>, find_by_request_id(conn, &str) -> Option<Proposal>, list(conn, ProposalStatus, &ScopeSet, limit) -> Vec<Proposal>, Decision<'a> { status, decided_by: &'a str, result_id: Option<i64>, provenance_id: Option<i64>, note: Option<&'a str> }, decide(conn, id, &Decision) -> Result<(), StorageError>}`（`decide` は pending 以外に対しては `NotFound`）
  - `domain::proposals::{ApplyOutcome { target_type: ProposalTargetType, id: i64 }, validate(target_type, action, target_id, patch) -> Result<(), ToolError>, apply(conn, &Proposal) -> Result<ApplyOutcome, ToolError>, materialize_provenance(conn, &Proposal, &ApplyOutcome) -> Result<Option<i64>, ToolError>}`

- [ ] **Step 1: storage/proposals.rs を実装する**

```rust
//! 提案キューの永続化。全書き込みの唯一の入口（適用ロジックは domain::proposals）。
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Map, Value};

use crate::{
    contracts::types::{Kind, Proposal, ProposalAction, ProposalStatus, ProposalTargetType, Provenance},
    scope::ScopeSet,
};

use super::{StorageError, parse_db_enum};

#[derive(Debug, Clone)]
pub struct NewProposal {
    pub action: ProposalAction,
    pub target_type: ProposalTargetType,
    pub target_id: Option<i64>,
    pub patch: Map<String, Value>,
    pub kind: Kind,
    pub scope: String,
    pub provenance: Option<Provenance>,
    pub provenance_id: Option<i64>,
    pub proposed_by: String,
    pub request_id: String,
}

pub fn insert(conn: &Connection, p: &NewProposal) -> Result<i64, StorageError> {
    let provenance_json = match &p.provenance {
        Some(v) => Some(serde_json::to_string(v)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO proposals(action, target_type, target_id, patch, kind, scope, provenance, provenance_id, proposed_by, request_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            p.action.to_string(),
            p.target_type.to_string(),
            p.target_id,
            Value::Object(p.patch.clone()).to_string(),
            p.kind.to_string(),
            p.scope,
            provenance_json,
            p.provenance_id,
            p.proposed_by,
            p.request_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

const COLS: &str = "id, action, target_type, target_id, patch, kind, scope, provenance, provenance_id, proposed_by, request_id, status, result_id, decision_note, created_at, decided_at, decided_by";

type RawProposal = (
    i64, String, String, Option<i64>, String, String, String, Option<String>, Option<i64>, String, String, String,
    Option<i64>, Option<String>, String, Option<String>, Option<String>,
);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawProposal> {
    Ok((
        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
        r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?, r.get(16)?,
    ))
}

fn convert(raw: RawProposal) -> Result<Proposal, StorageError> {
    let (
        id, action, target_type, target_id, patch, kind, scope, provenance, provenance_id, proposed_by, request_id,
        status, result_id, decision_note, created_at, decided_at, decided_by,
    ) = raw;
    let patch_value: Value = serde_json::from_str(&patch)?;
    let Value::Object(patch) = patch_value else {
        return Err(StorageError::Integrity(format!("proposal {id} patch is not a JSON object")));
    };
    let provenance: Option<Provenance> = match provenance {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    };
    Ok(Proposal {
        id,
        action: parse_db_enum(&action, "proposal action")?,
        target_type: parse_db_enum(&target_type, "proposal target_type")?,
        target_id,
        patch,
        kind: parse_db_enum(&kind, "proposal kind")?,
        scope,
        provenance,
        provenance_id,
        proposed_by,
        request_id,
        status: parse_db_enum(&status, "proposal status")?,
        result_id,
        decision_note,
        created_at,
        decided_at,
        decided_by,
    })
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Proposal>, StorageError> {
    let raw = conn
        .query_row(&format!("SELECT {COLS} FROM proposals WHERE id = ?1"), params![id], raw_row)
        .optional()?;
    raw.map(convert).transpose()
}

pub fn find_by_request_id(conn: &Connection, request_id: &str) -> Result<Option<Proposal>, StorageError> {
    let raw = conn
        .query_row(&format!("SELECT {COLS} FROM proposals WHERE request_id = ?1"), params![request_id], raw_row)
        .optional()?;
    raw.map(convert).transpose()
}

pub fn list(conn: &Connection, status: ProposalStatus, scopes: &ScopeSet, limit: usize) -> Result<Vec<Proposal>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM proposals WHERE status = ?1 AND scope IN (SELECT value FROM json_each(?2)) ORDER BY id DESC LIMIT ?3"
    ))?;
    let raws: Vec<RawProposal> =
        stmt.query_map(params![status.to_string(), scopes.as_json(), limit as i64], raw_row)?.collect::<Result<_, _>>()?;
    raws.into_iter().map(convert).collect()
}

pub struct Decision<'a> {
    pub status: ProposalStatus,
    pub decided_by: &'a str,
    pub result_id: Option<i64>,
    pub provenance_id: Option<i64>,
    pub note: Option<&'a str>,
}

/// pending の提案だけを決定できる（それ以外は NotFound）。
pub fn decide(conn: &Connection, id: i64, d: &Decision<'_>) -> Result<(), StorageError> {
    let n = conn.execute(
        "UPDATE proposals SET status = ?2, decided_by = ?3, decided_at = datetime('now'), result_id = ?4, \
         provenance_id = COALESCE(?5, provenance_id), decision_note = ?6 WHERE id = ?1 AND status = 'pending'",
        params![id, d.status.to_string(), d.decided_by, d.result_id, d.provenance_id, d.note],
    )?;
    if n == 0 {
        return Err(StorageError::NotFound(format!("pending proposal {id}")));
    }
    Ok(())
}
```

- [ ] **Step 2: domain/proposals.rs を実装する**

```rust
//! 提案の検証（propose 時）と適用（approve 時）。仕様書 §8.4。
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{
    contracts::types::{
        EngagementPatch, EntityPatch, FactPatch, GlossaryPatch, InteractionPatch, OrganizationPatch, PersonPatch,
        Proposal, ProposalAction, ProposalTargetType, RefPatch, RefTargetType,
    },
    domain::predicates,
    error::ToolError,
    storage::{engagements, entities, facts, glossary, interactions, organizations, people, refs},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub target_type: ProposalTargetType,
    pub id: i64,
}

fn parse<T: DeserializeOwned>(patch: &Map<String, Value>, target: ProposalTargetType) -> Result<T, ToolError> {
    serde_json::from_value(Value::Object(patch.clone()))
        .map_err(|e| ToolError::invalid_params(format!("patch does not match the {target} patch shape: {e}")))
}

fn blank(v: &Option<String>) -> bool {
    v.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true)
}

/// propose 時の事前検証。DB は見ない（存在検証は承認時の apply で行う）。
pub fn validate(
    target_type: ProposalTargetType,
    action: ProposalAction,
    target_id: Option<i64>,
    patch: &Map<String, Value>,
) -> Result<(), ToolError> {
    match action {
        ProposalAction::Insert => {
            if target_id.is_some() {
                return Err(ToolError::invalid_params("insert must not have target_id"));
            }
        }
        ProposalAction::Update | ProposalAction::Supersede => {
            if target_id.is_none() {
                return Err(ToolError::invalid_params(format!("{action} requires target_id")));
            }
        }
    }
    if action == ProposalAction::Supersede && target_type != ProposalTargetType::Fact {
        return Err(ToolError::invalid_params("supersede is only valid for facts"));
    }
    let insert_like = action != ProposalAction::Update;
    match target_type {
        ProposalTargetType::Person => {
            let p: PersonPatch = parse(patch, target_type)?;
            if insert_like && blank(&p.name) {
                return Err(ToolError::invalid_params("person insert requires name"));
            }
        }
        ProposalTargetType::Organization => {
            let p: OrganizationPatch = parse(patch, target_type)?;
            if insert_like && blank(&p.name) {
                return Err(ToolError::invalid_params("organization insert requires name"));
            }
        }
        ProposalTargetType::Engagement => {
            let p: EngagementPatch = parse(patch, target_type)?;
            if insert_like && blank(&p.name) {
                return Err(ToolError::invalid_params("engagement insert requires name"));
            }
        }
        ProposalTargetType::Interaction => {
            let p: InteractionPatch = parse(patch, target_type)?;
            if insert_like && (blank(&p.kind) || blank(&p.occurred_at) || blank(&p.summary)) {
                return Err(ToolError::invalid_params("interaction insert requires kind, occurred_at and summary"));
            }
        }
        ProposalTargetType::Entity => {
            let p: EntityPatch = parse(patch, target_type)?;
            if insert_like && (blank(&p.type_) || blank(&p.name)) {
                return Err(ToolError::invalid_params("entity insert requires type and name"));
            }
        }
        ProposalTargetType::Fact => {
            let p: FactPatch = parse(patch, target_type)?;
            if insert_like && (p.entity_type.is_none() || p.entity_id.is_none() || blank(&p.statement)) {
                return Err(ToolError::invalid_params("fact insert/supersede requires entity_type, entity_id and statement"));
            }
            predicates::check(p.predicate.as_deref(), p.value.as_deref())?;
        }
        ProposalTargetType::Ref => {
            let p: RefPatch = parse(patch, target_type)?;
            if insert_like
                && (p.target_type.is_none() || p.target_id.is_none() || blank(&p.system) || blank(&p.uri) || blank(&p.note))
            {
                return Err(ToolError::invalid_params(
                    "ref insert requires target_type, target_id, system, uri and note (URI-only refs are forbidden)",
                ));
            }
        }
        ProposalTargetType::Glossary => {
            let p: GlossaryPatch = parse(patch, target_type)?;
            if insert_like && blank(&p.term) {
                return Err(ToolError::invalid_params("glossary insert requires term"));
            }
        }
    }
    Ok(())
}

/// 承認時の適用。トランザクション内で呼ぶこと。scope は proposal.scope。
pub fn apply(conn: &Connection, proposal: &Proposal) -> Result<ApplyOutcome, ToolError> {
    validate(proposal.target_type, proposal.action, proposal.target_id, &proposal.patch)?;
    let t = proposal.target_type;
    let scope = proposal.scope.as_str();
    let target_id = proposal.target_id;
    let id = match (t, proposal.action) {
        (ProposalTargetType::Person, ProposalAction::Insert) => people::insert(conn, &parse(&proposal.patch, t)?)?,
        (ProposalTargetType::Person, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            people::update(conn, id, &parse(&proposal.patch, t)?)?;
            id
        }
        (ProposalTargetType::Organization, ProposalAction::Insert) => organizations::insert(conn, &parse(&proposal.patch, t)?)?,
        (ProposalTargetType::Organization, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            organizations::update(conn, id, &parse(&proposal.patch, t)?)?;
            id
        }
        (ProposalTargetType::Engagement, ProposalAction::Insert) => engagements::insert(conn, &parse(&proposal.patch, t)?, scope)?,
        (ProposalTargetType::Engagement, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            engagements::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (ProposalTargetType::Interaction, ProposalAction::Insert) => interactions::insert(conn, &parse(&proposal.patch, t)?, scope)?,
        (ProposalTargetType::Interaction, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            interactions::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (ProposalTargetType::Entity, ProposalAction::Insert) => entities::insert(conn, &parse(&proposal.patch, t)?)?,
        (ProposalTargetType::Entity, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            entities::update(conn, id, &parse(&proposal.patch, t)?)?;
            id
        }
        (ProposalTargetType::Fact, ProposalAction::Insert) => facts::insert(conn, &parse(&proposal.patch, t)?, proposal.kind, scope)?,
        (ProposalTargetType::Fact, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            facts::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (ProposalTargetType::Fact, ProposalAction::Supersede) => {
            facts::supersede(conn, target_id.expect("validated"), &parse(&proposal.patch, t)?, proposal.kind, scope)?
        }
        (ProposalTargetType::Ref, ProposalAction::Insert) => refs::insert(conn, &parse(&proposal.patch, t)?, scope)?,
        (ProposalTargetType::Ref, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            refs::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (ProposalTargetType::Glossary, ProposalAction::Insert) => glossary::insert(conn, &parse(&proposal.patch, t)?, scope)?,
        (ProposalTargetType::Glossary, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            glossary::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (_, ProposalAction::Supersede) => unreachable!("validate: supersede only for facts"),
    };
    Ok(ApplyOutcome { target_type: t, id })
}

/// 出所の実体化。inline 指定（system/uri/note）は適用結果のレコードに紐付く ref として登録する。
/// `ref` / `glossary` を対象とする提案は RefTargetType に変換できないため ref_id 形式のみ許す。
pub fn materialize_provenance(conn: &Connection, proposal: &Proposal, outcome: &ApplyOutcome) -> Result<Option<i64>, ToolError> {
    let Some(p) = &proposal.provenance else {
        return Ok(proposal.provenance_id);
    };
    if let Some(ref_id) = p.ref_id {
        return Ok(Some(ref_id));
    }
    let target_type: RefTargetType = outcome.target_type.to_string().parse().map_err(|_| {
        ToolError::invalid_params(format!("inline provenance cannot attach to {}; use {{\"ref_id\": ...}}", outcome.target_type))
    })?;
    let patch: RefPatch = serde_json::from_value(serde_json::json!({
        "target_type": target_type,
        "target_id": outcome.id,
        "system": p.system,
        "uri": p.uri,
        "title": p.title,
        "note": p.note,
        "snapshot": p.snapshot,
    }))?;
    Ok(Some(refs::insert(conn, &patch, &proposal.scope)?))
}
```

`storage/mod.rs` に `pub mod proposals;`、`domain/mod.rs` に `pub mod proposals;` を追加。

- [ ] **Step 3: domain/proposals.rs のテストを書く（同ファイル末尾）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::Kind,
        error::ErrorCode,
        storage::{Db, StorageError, affiliations, facts, people, proposals, refs},
        scope::ScopeSet,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn setup() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn make_proposal(
        db: &Db,
        target_type: &str,
        action: &str,
        target_id: Option<i64>,
        patch: serde_json::Value,
        provenance: Option<serde_json::Value>,
    ) -> crate::contracts::types::Proposal {
        db.with_conn::<_, StorageError>(|c| {
            let new = proposals::NewProposal {
                action: action.parse().unwrap(),
                target_type: target_type.parse().unwrap(),
                target_id,
                patch: patch.as_object().unwrap().clone(),
                kind: Kind::Fact,
                scope: "cn".into(),
                provenance: provenance.map(|v| serde_json::from_value(v).unwrap()),
                provenance_id: None,
                proposed_by: "bot".into(),
                request_id: format!("req-{}", SEQ.fetch_add(1, Ordering::SeqCst)),
            };
            let id = proposals::insert(c, &new)?;
            Ok(proposals::get(c, id)?.unwrap())
        })
        .unwrap()
    }

    #[test]
    fn validate_rejects_malformed_proposals() {
        let patch = json!({"name": "x"}).as_object().unwrap().clone();
        let empty = json!({}).as_object().unwrap().clone();
        let bogus = json!({"name": "x", "bogus": 1}).as_object().unwrap().clone();
        let ok = validate("person".parse().unwrap(), "insert".parse().unwrap(), None, &patch);
        assert!(ok.is_ok());
        for (t, a, id, p) in [
            ("person", "insert", Some(1), &patch),          // insert に target_id
            ("person", "update", None, &patch),             // update に target_id 無し
            ("person", "supersede", Some(1), &patch),       // supersede は fact のみ
            ("person", "insert", None, &empty),             // name 必須
            ("person", "insert", None, &bogus),             // 未知フィールド
            ("fact", "insert", None, &patch),               // entity_type 等必須
        ] {
            let err = validate(t.parse().unwrap(), a.parse().unwrap(), id, p).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams, "{t}/{a}");
        }
        // レジストリ外 predicate
        let bad_pred = json!({"entity_type": "person", "entity_id": 1, "statement": "s", "predicate": "mood", "value": "x"})
            .as_object().unwrap().clone();
        assert_eq!(
            validate("fact".parse().unwrap(), "insert".parse().unwrap(), None, &bad_pred).unwrap_err().code,
            ErrorCode::InvalidParams
        );
    }

    #[test]
    fn person_insert_apply_then_inline_provenance_ref() {
        let db = setup();
        let proposal = make_proposal(
            &db, "person", "insert", None,
            json!({"name": "岡村 慎太郎", "aliases": [{"alias": "okash1n"}]}),
            Some(json!({"system": "minutes", "uri": "minutes://meeting/1", "note": "初回打合せの議事録"})),
        );
        db.with_conn::<_, ToolError>(|c| {
            let outcome = apply(c, &proposal)?;
            assert_eq!(outcome.target_type.to_string(), "person");
            assert!(people::get(c, outcome.id).map_err(ToolError::from)?.is_some());
            let ref_id = materialize_provenance(c, &proposal, &outcome)?.unwrap();
            let r = refs::get(c, ref_id, &ScopeSet::single("cn")).map_err(ToolError::from)?.unwrap();
            assert_eq!(r.target_id, outcome.id);
            assert_eq!(r.system, "minutes");
            proposals::decide(c, proposal.id, &proposals::Decision {
                status: "approved".parse().unwrap(),
                decided_by: "me",
                result_id: Some(outcome.id),
                provenance_id: Some(ref_id),
                note: None,
            }).map_err(ToolError::from)?;
            // 二重承認は不可
            assert!(proposals::decide(c, proposal.id, &proposals::Decision {
                status: "approved".parse().unwrap(), decided_by: "me", result_id: None, provenance_id: None, note: None,
            }).is_err());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn fact_supersede_and_update_not_found() {
        let db = setup();
        let pid = db
            .with_conn::<_, StorageError>(|c| people::insert(c, &serde_json::from_value(json!({"name": "田中"})).unwrap()))
            .unwrap();
        let ins = make_proposal(&db, "fact", "insert", None, json!({"entity_type": "person", "entity_id": pid, "statement": "旧情報"}), None);
        let fid = db.with_conn::<_, ToolError>(|c| Ok(apply(c, &ins)?.id)).unwrap();
        let sup = make_proposal(&db, "fact", "supersede", Some(fid), json!({"entity_type": "person", "entity_id": pid, "statement": "新情報"}), None);
        let new_id = db.with_conn::<_, ToolError>(|c| Ok(apply(c, &sup)?.id)).unwrap();
        db.with_conn::<_, StorageError>(|c| {
            assert_eq!(facts::get(c, fid, &ScopeSet::single("cn"))?.unwrap().superseded_by, Some(new_id));
            Ok(())
        })
        .unwrap();
        let upd = make_proposal(&db, "person", "update", Some(9999), json!({"role": "PM"}), None);
        let err = db.with_conn::<_, ToolError>(|c| apply(c, &upd).map(|_| ())).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn inline_provenance_rejected_for_glossary_target() {
        let db = setup();
        let proposal = make_proposal(
            &db, "glossary", "insert", None,
            json!({"term": "SCIM"}),
            Some(json!({"system": "notion", "uri": "https://x", "note": "n"})),
        );
        let err = db
            .with_conn::<_, ToolError>(|c| {
                let outcome = apply(c, &proposal)?;
                materialize_provenance(c, &proposal, &outcome).map(|_| ())
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
    }

    #[test]
    fn request_id_lookup_and_scoped_list() {
        let db = setup();
        let p = make_proposal(&db, "organization", "insert", None, json!({"name": "ACME"}), None);
        db.with_conn::<_, StorageError>(|c| {
            assert_eq!(proposals::find_by_request_id(c, &p.request_id)?.unwrap().id, p.id);
            assert!(proposals::find_by_request_id(c, "missing")?.is_none());
            let pending = proposals::list(c, "pending".parse().unwrap(), &ScopeSet::single("cn"), 10)?;
            assert!(pending.iter().any(|x| x.id == p.id));
            assert!(proposals::list(c, "approved".parse().unwrap(), &ScopeSet::single("cn"), 10)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
```

- [ ] **Step 4: テストを実行する**

Run: `cargo test -p gaia-core proposals`
Expected: 5 tests passed（`"insert".parse::<ProposalAction>()` などの enum FromStr は typify 生成。名前が違う場合は生成物を確認して合わせる）

- [ ] **Step 5: lint を含めて全体を確認しコミット**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功

```bash
git add crates/gaia-core
git commit -m "feat(core): add proposal queue with validation, apply and provenance" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 12: ToolService 本体 ＋ admin ＋ get_server_info / get_job_status

**Files:**
- Create: `crates/gaia-core/src/admin.rs`, `crates/gaia-core/src/tools/mod.rs`, `crates/gaia-core/src/tools/server_info.rs`, `crates/gaia-core/src/tools/job_status.rs`
- Modify: `crates/gaia-core/src/lib.rs`（`pub mod admin; pub mod tools;`）

**Interfaces:**
- Consumes: これまでの全モジュール
- Produces:
  - `gaia_core::tools::{ToolService, CallContext, HANDLED_TOOLS}`
    - `ToolService::new(db: Db, catalog: Catalog) -> Self`、`call(&self, client: &ClientIdentity, tool: &str, args: Value) -> Result<Value, ToolError>`、`visible_tools(&self, Role) -> Vec<&ToolSpec>`、`catalog(&self) -> &Catalog`、`db(&self) -> &Db`
    - `CallContext<'a> { pub client: &'a ClientIdentity, pub db: &'a Db, pub catalog: &'a Catalog }`
    - 各ツールモジュールは `pub fn handle(ctx: &CallContext<'_>, input: XInput) -> Result<XOutput, ToolError>` の形（提案系は `propose_update` 等の関数名）
  - `gaia_core::admin::{add_affiliation(db, actor, name, identity) -> Result<i64, ToolError>, list_affiliations(db) -> Result<Vec<Affiliation>, ToolError>}`（提案キュー原則の唯一の例外。audit_log(admin_write) に記録）
  - テスト補助 `tools::test_support`（`#[cfg(test)]`）: `service() -> ToolService`（in-memory DB ＋ affiliation `cn`）、`human()` / `agent() -> ClientIdentity`

- [ ] **Step 1: admin.rs を実装する**

```rust
//! affiliations の管理。提案キュー原則の唯一の例外（機密境界そのものの定義のため）。
//! 必ず audit_log(admin_write) に残す。CLI / デスクトップの管理操作からのみ呼ぶ。
use serde_json::json;

use crate::{
    error::ToolError,
    storage::{Db, affiliations, audit},
};

pub fn add_affiliation(db: &Db, actor: &str, name: &str, identity: Option<&str>) -> Result<i64, ToolError> {
    db.with_tx(|tx| {
        let id = affiliations::insert(tx, name, identity)?;
        audit::record(tx, actor, "admin_write", &json!({"op": "add_affiliation", "name": name}))?;
        Ok::<_, ToolError>(id)
    })
}

pub fn list_affiliations(db: &Db) -> Result<Vec<affiliations::Affiliation>, ToolError> {
    db.with_conn(|c| Ok::<_, ToolError>(affiliations::list(c)?))
}
```

- [ ] **Step 2: tools/mod.rs を実装する（この時点のハンドラは 2 つ。後続タスクで mod・dispatch・HANDLED_TOOLS に追記していく）**

```rust
//! ToolService: CLI と MCP の唯一の入口。仕様書 §8.1。
//! 手順: ツール解決 → role 認可 → 契約スキーマで入力検証 → 型付きハンドラ → （debug/test）出力検証。
mod job_status;
mod server_info;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    contracts::{Catalog, ToolSpec},
    error::ToolError,
    identity::{ClientIdentity, Role},
    storage::Db,
};

/// dispatch 済みツール。Task 16 完了時に「enabled な契約 = この一覧」となる（テストで固定）。
pub const HANDLED_TOOLS: &[&str] = &["get_server_info", "get_job_status"];

pub struct ToolService {
    db: Db,
    catalog: Catalog,
}

pub struct CallContext<'a> {
    pub client: &'a ClientIdentity,
    pub db: &'a Db,
    pub catalog: &'a Catalog,
}

impl ToolService {
    pub fn new(db: Db, catalog: Catalog) -> Self {
        Self { db, catalog }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn visible_tools(&self, role: Role) -> Vec<&ToolSpec> {
        self.catalog.visible(role)
    }

    pub fn call(&self, client: &ClientIdentity, tool: &str, args: Value) -> Result<Value, ToolError> {
        let spec = self
            .catalog
            .get(tool)
            .filter(|s| s.enabled)
            .ok_or_else(|| ToolError::not_found(format!("unknown tool `{tool}`")))?;
        if !spec.allows(client.role) {
            return Err(ToolError::unauthorized(format!(
                "tool `{tool}` is not allowed for role `{}` (client `{}`)",
                client.role, client.name
            )));
        }
        let args = if args.is_null() { json!({}) } else { args };
        spec.validate_input(&args)?;
        let ctx = CallContext { client, db: &self.db, catalog: &self.catalog };
        let out = dispatch(&ctx, tool, args)?;
        if cfg!(any(test, debug_assertions)) {
            spec.validate_output(&out)?;
        }
        Ok(out)
    }
}

fn dispatch(ctx: &CallContext<'_>, tool: &str, args: Value) -> Result<Value, ToolError> {
    match tool {
        "get_server_info" => run(ctx, args, server_info::handle),
        "get_job_status" => run(ctx, args, job_status::handle),
        other => Err(ToolError::not_implemented(format!("tool `{other}` has no handler yet"))),
    }
}

fn run<I, O>(
    ctx: &CallContext<'_>,
    args: Value,
    f: impl FnOnce(&CallContext<'_>, I) -> Result<O, ToolError>,
) -> Result<Value, ToolError>
where
    I: DeserializeOwned,
    O: Serialize,
{
    let input: I = serde_json::from_value(args)
        .map_err(|e| ToolError::internal(format!("validated arguments failed to deserialize into contract types: {e}")))?;
    let out = f(ctx, input)?;
    Ok(serde_json::to_value(out)?)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::ToolService;
    use crate::{
        contracts::Catalog,
        identity::{ClientIdentity, Role},
        storage::{Db, StorageError, affiliations},
    };

    pub(crate) fn service() -> ToolService {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            Ok(())
        })
        .unwrap();
        ToolService::new(db, Catalog::embedded().unwrap())
    }

    pub(crate) fn human() -> ClientIdentity {
        ClientIdentity { name: "me".into(), role: Role::Human, default_scope: Some("cn".into()) }
    }

    pub(crate) fn agent() -> ClientIdentity {
        ClientIdentity { name: "bot".into(), role: Role::Agent, default_scope: Some("cn".into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{agent, human, service};
    use super::*;
    use crate::error::ErrorCode;
    use serde_json::json;

    #[test]
    fn get_server_info_reports_identity_and_visible_tools() {
        let s = service();
        let out = s.call(&agent(), "get_server_info", json!({})).unwrap();
        assert_eq!(out["name"], "gaia_library");
        assert_eq!(out["contract_version"], "1.0.0");
        assert_eq!(out["client"]["role"], "agent");
        assert_eq!(out["client"]["default_scope"], "cn");
        let tools: Vec<&str> = out["capabilities"]["tools"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(!tools.contains(&"approve_proposal"));
        let human_out = s.call(&human(), "get_server_info", json!({})).unwrap();
        let human_tools: Vec<&str> =
            human_out["capabilities"]["tools"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(human_tools.contains(&"approve_proposal"));
    }

    #[test]
    fn call_enforces_existence_role_and_input_schema() {
        let s = service();
        assert_eq!(s.call(&agent(), "nope", json!({})).unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(s.call(&agent(), "resolve_source", json!({})).unwrap_err().code, ErrorCode::NotFound, "disabled = 存在しない扱い");
        assert_eq!(s.call(&agent(), "approve_proposal", json!({"proposal_id": 1})).unwrap_err().code, ErrorCode::Unauthorized);
        assert_eq!(s.call(&agent(), "get_job_status", json!({"job_id": 1})).unwrap_err().code, ErrorCode::InvalidParams);
        assert_eq!(s.call(&agent(), "get_job_status", json!({"job_id": "j1"})).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn handled_tools_are_enabled_contract_tools() {
        let s = service();
        for name in HANDLED_TOOLS {
            let spec = s.catalog().get(name).unwrap_or_else(|| panic!("{name} missing from contracts"));
            assert!(spec.enabled, "{name} must be enabled");
        }
    }

    #[test]
    fn admin_add_affiliation_is_audited() {
        let s = service();
        crate::admin::add_affiliation(s.db(), "me", "assoc", Some("理事")).unwrap();
        let entries = s
            .db()
            .with_conn::<_, crate::storage::StorageError>(|c| crate::storage::audit::recent(c, 5))
            .unwrap();
        assert_eq!(entries[0].action, "admin_write");
        assert_eq!(entries[0].detail["name"], "assoc");
    }
}
```

- [ ] **Step 3: server_info.rs と job_status.rs を実装する**

`tools/server_info.rs`:

```rust
//! get_server_info。契約版と能力、接続クライアントの識別を返す。
use crate::{
    contracts::types::{
        ClientInfo, GetServerInfoInput, GetServerInfoOutput, SearchCapabilities, ServerCapabilitiesInfo,
        ServerProtocolInfo,
    },
    error::ToolError,
};

use super::CallContext;

pub fn handle(ctx: &CallContext<'_>, _input: GetServerInfoInput) -> Result<GetServerInfoOutput, ToolError> {
    let tools = ctx.catalog.visible(ctx.client.role).iter().map(|t| t.name.clone()).collect();
    Ok(GetServerInfoOutput {
        name: ctx.catalog.server_name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        contract_version: ctx.catalog.contract_version.clone(),
        protocol: ServerProtocolInfo { transports: vec!["stdio".to_string()] },
        capabilities: ServerCapabilitiesInfo {
            tools,
            resolvers: Vec::new(),
            search: SearchCapabilities { fts: "trigram".to_string() },
        },
        client: ClientInfo {
            name: ctx.client.name.clone(),
            role: ctx.client.role.to_string(),
            default_scope: ctx.client.default_scope.clone(),
        },
    })
}
```

`tools/job_status.rs`:

```rust
//! get_job_status。v1 にジョブは無い（narumi と契約規約を揃えるためのツール）。
use serde_json::Value;

use crate::{contracts::types::GetJobStatusInput, error::ToolError};

use super::CallContext;

pub fn handle(_ctx: &CallContext<'_>, input: GetJobStatusInput) -> Result<Value, ToolError> {
    Err(ToolError::not_found(format!("job `{}` not found (gaia_library v1 has no jobs)", input.job_id)))
}
```

`lib.rs` に `pub mod admin; pub mod tools;` を追加。

- [ ] **Step 4: テストを実行しコミット**

Run: `cargo test -p gaia-core tools`
Expected: 4 tests passed

```bash
git add crates/gaia-core
git commit -m "feat(core): add ToolService with role/schema enforcement and info tools" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 13: 提案系ツール（propose_update / list_proposals / approve_proposal / reject_proposal）

**Files:**
- Create: `crates/gaia-core/src/tools/propose.rs`
- Modify: `crates/gaia-core/src/tools/mod.rs`（`mod propose;`・dispatch 4 本・`HANDLED_TOOLS` へ追加・`test_support::seed_basic` 追加）

**Interfaces:**
- Produces: `tools::propose::{propose_update, list_proposals, approve_proposal, reject_proposal}`（すべて `(ctx, input) -> Result<Output, ToolError>`）
- `test_support::{seed_basic(s: &ToolService) -> SeedIds, SeedIds { org, person, engagement, fact, reference, glossary, interaction: i64 }}` — human クライアントで propose → approve を回してデータ投入する（書き込み経路のドッグフーディング）

- [ ] **Step 1: propose.rs を実装する**

```rust
//! 提案系ツール。書き込みは全てここを通る。仕様書 §8.4。
use serde_json::json;

use crate::{
    contracts::types::{
        ApplyResult, ApproveProposalInput, ApproveProposalOutput, ListProposalsInput, ListProposalsOutput,
        ProposalStatus, ProposalTargetType, ProposeUpdateInput, ProposeUpdateOutput, RejectProposalInput,
        RejectProposalOutput,
    },
    domain,
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{affiliations, audit, proposals, refs},
};

use super::CallContext;

pub fn propose_update(ctx: &CallContext<'_>, input: ProposeUpdateInput) -> Result<ProposeUpdateOutput, ToolError> {
    if input.request_id.trim().len() < 8 {
        return Err(ToolError::invalid_params("request_id must be at least 8 characters"));
    }
    domain::proposals::validate(input.target_type, input.action, input.target_id, &input.patch)?;
    ctx.db.with_tx(|tx| {
        let scope = match input.scope.clone().or_else(|| ctx.client.default_scope.clone()) {
            Some(s) => s,
            None => {
                return Err(ToolError::scope_denied(format!(
                    "scope is required: pass `scope` or set default_scope for client `{}`",
                    ctx.client.name
                )));
            }
        };
        if !affiliations::exists(tx, &scope)? {
            return Err(ToolError::not_found(format!("scope `{scope}` (affiliation) not found")));
        }
        // request_id による冪等化
        if let Some(existing) = proposals::find_by_request_id(tx, &input.request_id)? {
            if existing.proposed_by == ctx.client.name {
                return Ok(ProposeUpdateOutput { proposal_id: existing.id, status: existing.status, duplicate: true });
            }
            return Err(ToolError::conflict(format!(
                "request_id `{}` was already used by another client",
                input.request_id
            )));
        }
        // provenance の事前検証（ref_id は存在確認、inline は必須項目と紐付け先の型を確認）
        let (provenance, provenance_id) = match &input.provenance {
            None => (None, None),
            Some(p) if p.ref_id.is_some() => {
                let rid = p.ref_id.expect("checked");
                if refs::get(tx, rid, &ScopeSet::single(&scope))?.is_none() {
                    return Err(ToolError::not_found(format!("provenance ref {rid} (in scope `{scope}`)")));
                }
                (None, Some(rid))
            }
            Some(p) => {
                let blankish = |v: &Option<String>| v.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
                if blankish(&p.system) || blankish(&p.uri) || blankish(&p.note) {
                    return Err(ToolError::invalid_params("inline provenance requires system, uri and note (or pass ref_id)"));
                }
                if matches!(input.target_type, ProposalTargetType::Ref | ProposalTargetType::Glossary) {
                    return Err(ToolError::invalid_params(format!(
                        "inline provenance cannot attach to {}; use ref_id",
                        input.target_type
                    )));
                }
                (Some(p.clone()), None)
            }
        };
        let id = proposals::insert(
            tx,
            &proposals::NewProposal {
                action: input.action,
                target_type: input.target_type,
                target_id: input.target_id,
                patch: input.patch.clone(),
                kind: input.kind,
                scope: scope.clone(),
                provenance,
                provenance_id,
                proposed_by: ctx.client.name.clone(),
                request_id: input.request_id.clone(),
            },
        )?;
        audit::record(
            tx,
            &ctx.client.name,
            "propose",
            &json!({"proposal_id": id, "target_type": input.target_type, "action": input.action, "scope": scope, "request_id": input.request_id}),
        )?;
        Ok(ProposeUpdateOutput { proposal_id: id, status: ProposalStatus::Pending, duplicate: false })
    })
}

pub fn list_proposals(ctx: &CallContext<'_>, input: ListProposalsInput) -> Result<ListProposalsOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "list_proposals")?;
        let status = input.status.unwrap_or(ProposalStatus::Pending);
        let limit = input.limit.clamp(1, 200) as usize;
        Ok(ListProposalsOutput { proposals: proposals::list(c, status, &scopes, limit)? })
    })
}

pub fn approve_proposal(ctx: &CallContext<'_>, input: ApproveProposalInput) -> Result<ApproveProposalOutput, ToolError> {
    ctx.db.with_tx(|tx| {
        let proposal = proposals::get(tx, input.proposal_id)?
            .ok_or_else(|| ToolError::not_found(format!("proposal {}", input.proposal_id)))?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ToolError::conflict(format!("proposal {} is already {}", proposal.id, proposal.status)));
        }
        // 適用に失敗したら with_tx が rollback し、提案は pending のまま残る（仕様書 §8.4）
        let outcome = domain::proposals::apply(tx, &proposal)?;
        let provenance_id = domain::proposals::materialize_provenance(tx, &proposal, &outcome)?;
        proposals::decide(
            tx,
            proposal.id,
            &proposals::Decision {
                status: ProposalStatus::Approved,
                decided_by: &ctx.client.name,
                result_id: Some(outcome.id),
                provenance_id,
                note: None,
            },
        )?;
        audit::record(
            tx,
            &ctx.client.name,
            "approve",
            &json!({"proposal_id": proposal.id, "result": {"target_type": outcome.target_type, "id": outcome.id}}),
        )?;
        Ok(ApproveProposalOutput {
            proposal_id: proposal.id,
            status: ProposalStatus::Approved,
            result: ApplyResult { target_type: outcome.target_type, id: outcome.id },
        })
    })
}

pub fn reject_proposal(ctx: &CallContext<'_>, input: RejectProposalInput) -> Result<RejectProposalOutput, ToolError> {
    ctx.db.with_tx(|tx| {
        let proposal = proposals::get(tx, input.proposal_id)?
            .ok_or_else(|| ToolError::not_found(format!("proposal {}", input.proposal_id)))?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ToolError::conflict(format!("proposal {} is already {}", proposal.id, proposal.status)));
        }
        proposals::decide(
            tx,
            proposal.id,
            &proposals::Decision {
                status: ProposalStatus::Rejected,
                decided_by: &ctx.client.name,
                result_id: None,
                provenance_id: None,
                note: input.reason.as_deref(),
            },
        )?;
        audit::record(tx, &ctx.client.name, "reject", &json!({"proposal_id": proposal.id, "reason": input.reason}))?;
        Ok(RejectProposalOutput { proposal_id: proposal.id, status: ProposalStatus::Rejected })
    })
}
```

- [ ] **Step 2: mod.rs に配線する**

`mod propose;` を追加し、dispatch に 4 本追加:

```rust
        "propose_update" => run(ctx, args, propose::propose_update),
        "list_proposals" => run(ctx, args, propose::list_proposals),
        "approve_proposal" => run(ctx, args, propose::approve_proposal),
        "reject_proposal" => run(ctx, args, propose::reject_proposal),
```

`HANDLED_TOOLS` を `&["get_server_info", "get_job_status", "propose_update", "list_proposals", "approve_proposal", "reject_proposal"]` に更新。

- [ ] **Step 3: test_support::seed_basic を追加する（tools/mod.rs の test_support 内）**

```rust
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    pub(crate) struct SeedIds {
        pub org: i64,
        pub person: i64,
        pub engagement: i64,
        pub fact: i64,
        pub reference: i64,
        pub glossary: i64,
        pub interaction: i64,
    }

    /// human クライアントで propose → approve を回す（書き込み経路そのものをテストデータ投入に使う）。
    pub(crate) fn write(s: &ToolService, target_type: &str, patch: serde_json::Value) -> i64 {
        let h = human();
        let out = s
            .call(&h, "propose_update", json!({
                "target_type": target_type, "action": "insert", "patch": patch, "kind": "fact",
                "request_id": format!("seed-{target_type}-{:04}", SEQ.fetch_add(1, Ordering::SeqCst)),
            }))
            .unwrap();
        let pid = out["proposal_id"].as_i64().unwrap();
        let approved = s.call(&h, "approve_proposal", json!({"proposal_id": pid})).unwrap();
        approved["result"]["id"].as_i64().unwrap()
    }

    pub(crate) fn seed_basic(s: &ToolService) -> SeedIds {
        let org = write(s, "organization", json!({"name": "RELATIONS", "kind": "customer"}));
        let person = write(s, "person", json!({
            "name": "岡村 慎太郎", "org_id": org, "role": "情シス",
            "aliases": [{"alias": "Okamura Shintaro", "kind": "romaji"}, {"alias": "okash1n", "kind": "nickname"}]
        }));
        let engagement = write(s, "engagement", json!({
            "name": "Okta導入支援", "org_id": org, "status": "active",
            "people": [{"person_id": person, "role": "key_person"}]
        }));
        let fact = write(s, "fact", json!({
            "entity_type": "engagement", "entity_id": engagement,
            "statement": "決定: SCIM プロビジョニングは Phase 2 で対応する", "predicate": "decision", "value": "scim-phase2"
        }));
        let reference = write(s, "ref", json!({
            "target_type": "fact", "target_id": fact, "system": "minutes",
            "uri": "minutes://meeting/42#t=1200", "note": "決定箇所の議事録参照", "snapshot": "SCIM は Phase 2"
        }));
        let glossary = write(s, "glossary", json!({"term": "SCIM", "reading": "スキム", "definition": "プロビジョニング標準", "engagement_id": engagement}));
        let interaction = write(s, "interaction", json!({
            "kind": "meeting", "occurred_at": "2026-08-20T10:00:00Z",
            "summary": "定例。SCIM の段階対応を決定", "engagement_id": engagement, "person_ids": [person]
        }));
        SeedIds { org, person, engagement, fact, reference, glossary, interaction }
    }
```

- [ ] **Step 4: テストを書く（propose.rs 末尾）**

```rust
#[cfg(test)]
mod tests {
    use crate::error::ErrorCode;
    use crate::tools::test_support::{agent, human, seed_basic, service};
    use serde_json::json;

    #[test]
    fn agent_proposes_human_approves_and_duplicate_is_idempotent() {
        let s = service();
        let propose = |rid: &str| {
            s.call(&agent(), "propose_update", json!({
                "target_type": "person", "action": "insert",
                "patch": {"name": "田中 太郎"}, "kind": "fact", "request_id": rid
            }))
        };
        let out = propose("req-tanaka-1").unwrap();
        assert_eq!(out["status"], "pending");
        assert_eq!(out["duplicate"], false);
        let pid = out["proposal_id"].as_i64().unwrap();
        // 同じ request_id の再送は duplicate
        let dup = propose("req-tanaka-1").unwrap();
        assert_eq!(dup["proposal_id"].as_i64().unwrap(), pid);
        assert_eq!(dup["duplicate"], true);
        // 一覧（pending 既定）
        let listed = s.call(&agent(), "list_proposals", json!({})).unwrap();
        assert!(listed["proposals"].as_array().unwrap().iter().any(|p| p["id"].as_i64() == Some(pid)));
        // human が承認 → 適用結果が返る
        let approved = s.call(&human(), "approve_proposal", json!({"proposal_id": pid})).unwrap();
        assert_eq!(approved["status"], "approved");
        assert_eq!(approved["result"]["target_type"], "person");
        // 二重承認は conflict
        assert_eq!(
            s.call(&human(), "approve_proposal", json!({"proposal_id": pid})).unwrap_err().code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn short_request_id_and_unknown_scope_are_rejected() {
        let s = service();
        let err = s
            .call(&agent(), "propose_update", json!({
                "target_type": "person", "action": "insert", "patch": {"name": "x"}, "kind": "fact", "request_id": "short"
            }))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        let err = s
            .call(&agent(), "propose_update", json!({
                "target_type": "person", "action": "insert", "patch": {"name": "x"}, "kind": "fact",
                "scope": "zzz", "request_id": "req-00000001"
            }))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn failed_apply_keeps_proposal_pending() {
        let s = service();
        // 存在しない entity への fact 提案は propose では通り、approve で失敗して pending のまま
        let out = s
            .call(&agent(), "propose_update", json!({
                "target_type": "fact", "action": "insert",
                "patch": {"entity_type": "person", "entity_id": 9999, "statement": "孤児 fact"},
                "kind": "inference", "request_id": "req-orphan-1"
            }))
            .unwrap();
        let pid = out["proposal_id"].as_i64().unwrap();
        let err = s.call(&human(), "approve_proposal", json!({"proposal_id": pid})).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let listed = s.call(&human(), "list_proposals", json!({"status": "pending"})).unwrap();
        assert!(listed["proposals"].as_array().unwrap().iter().any(|p| p["id"].as_i64() == Some(pid)));
        // 却下できる
        let rejected = s.call(&human(), "reject_proposal", json!({"proposal_id": pid, "reason": "対象が存在しない"})).unwrap();
        assert_eq!(rejected["status"], "rejected");
    }

    #[test]
    fn seed_basic_builds_a_connected_dataset() {
        let s = service();
        let ids = seed_basic(&s);
        assert!(ids.org > 0 && ids.person > 0 && ids.engagement > 0);
        assert!(ids.fact > 0 && ids.reference > 0 && ids.glossary > 0 && ids.interaction > 0);
    }
}
```

- [ ] **Step 5: テストを実行しコミット**

Run: `cargo test -p gaia-core`
Expected: すべて成功

```bash
git add crates/gaia-core
git commit -m "feat(core): add proposal tools (propose/list/approve/reject)" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 14: 参照系ツール（get_person / get_organization / get_engagement）

**Files:**
- Create: `crates/gaia-core/src/tools/get_person.rs`, `crates/gaia-core/src/tools/get_organization.rs`, `crates/gaia-core/src/tools/get_engagement.rs`
- Modify: `crates/gaia-core/src/tools/mod.rs`（`mod` 3 本・dispatch 3 本・`HANDLED_TOOLS` 追加）

**Interfaces:**
- Produces: `get_person::handle` / `get_organization::handle` / `get_engagement::handle`
- 共通規則: id または name のどちらかで特定（両方無ければ `invalid_params`、name 複数一致は `conflict`＋候補 details）。scope は `ScopeSet::resolve` → `audit_cross_read`。facts は現在のもの最大 50、interactions は直近 20。refs はエンティティ直付け＋facts の根拠参照

- [ ] **Step 1: get_person.rs を実装する**

```rust
//! get_person。人物の詳細＋facts＋refs＋直近の interactions。
use serde_json::json;

use crate::{
    contracts::types::{GetPersonInput, GetPersonOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, facts, interactions, organizations, people, refs},
};

use super::CallContext;

pub fn handle(ctx: &CallContext<'_>, input: GetPersonInput) -> Result<GetPersonOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let person = match (input.person_id, input.name.as_deref()) {
            (Some(id), _) => people::get(c, id)?.ok_or_else(|| ToolError::not_found(format!("person {id}")))?,
            (None, Some(name)) => {
                let found = people::find_by_name(c, name)?;
                match found.len() {
                    0 => return Err(ToolError::not_found(format!("person `{name}`"))),
                    1 => found.into_iter().next().expect("len checked"),
                    _ => {
                        let candidates: Vec<_> = found
                            .iter()
                            .map(|p| json!({"person_id": p.id, "name": p.name, "org_name": p.org_name}))
                            .collect();
                        return Err(ToolError::conflict(format!("multiple people match `{name}`; pass person_id"))
                            .with_details(json!({"candidates": candidates})));
                    }
                }
            }
            (None, None) => return Err(ToolError::invalid_params("pass person_id or name")),
        };
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_person")?;
        let organization = match person.org_id {
            Some(oid) => organizations::get(c, oid)?,
            None => None,
        };
        let engagement_list = engagements::for_person(c, person.id, &scopes)?;
        let fact_list = facts::for_entity(c, "person", person.id, &scopes, 50)?;
        let mut ref_list = refs::for_target(c, "person", person.id, &scopes)?;
        for f in &fact_list {
            ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
        }
        let interaction_list = interactions::recent_for_person(c, person.id, &scopes, 20)?;
        Ok(GetPersonOutput {
            person,
            organization,
            engagements: engagement_list,
            facts: fact_list,
            refs: ref_list,
            interactions: interaction_list,
        })
    })
}
```

- [ ] **Step 2: get_organization.rs を実装する**

```rust
//! get_organization。組織の詳細＋所属人物＋案件（scope 内）＋facts＋refs。
use serde_json::json;

use crate::{
    contracts::types::{GetOrganizationInput, GetOrganizationOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, facts, organizations, people, refs},
};

use super::CallContext;

pub fn handle(ctx: &CallContext<'_>, input: GetOrganizationInput) -> Result<GetOrganizationOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let organization = match (input.organization_id, input.name.as_deref()) {
            (Some(id), _) => organizations::get(c, id)?.ok_or_else(|| ToolError::not_found(format!("organization {id}")))?,
            (None, Some(name)) => {
                let found = organizations::find_by_name(c, name)?;
                match found.len() {
                    0 => return Err(ToolError::not_found(format!("organization `{name}`"))),
                    1 => found.into_iter().next().expect("len checked"),
                    _ => {
                        let candidates: Vec<_> =
                            found.iter().map(|o| json!({"organization_id": o.id, "name": o.name, "kind": o.kind})).collect();
                        return Err(ToolError::conflict(format!("multiple organizations match `{name}`; pass organization_id"))
                            .with_details(json!({"candidates": candidates})));
                    }
                }
            }
            (None, None) => return Err(ToolError::invalid_params("pass organization_id or name")),
        };
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_organization")?;
        let people_list = people::list_by_org(c, organization.id)?;
        let engagement_list = engagements::for_org(c, organization.id, &scopes)?;
        let fact_list = facts::for_entity(c, "organization", organization.id, &scopes, 50)?;
        let mut ref_list = refs::for_target(c, "organization", organization.id, &scopes)?;
        for f in &fact_list {
            ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
        }
        Ok(GetOrganizationOutput {
            organization,
            people: people_list,
            engagements: engagement_list,
            facts: fact_list,
            refs: ref_list,
        })
    })
}
```

- [ ] **Step 3: get_engagement.rs を実装する**

```rust
//! get_engagement。案件の詳細＋関係者（alias 込み）＋facts＋refs＋用語集＋直近 interactions。
//! 案件自体が scope 外なら not_found（存在を漏らさない）。
use serde_json::json;

use crate::{
    contracts::types::{GetEngagementInput, GetEngagementOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, facts, glossary, interactions, organizations, refs},
};

use super::CallContext;

pub fn handle(ctx: &CallContext<'_>, input: GetEngagementInput) -> Result<GetEngagementOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_engagement")?;
        let engagement = match (input.engagement_id, input.name.as_deref()) {
            (Some(id), _) => engagements::get(c, id, &scopes)?.ok_or_else(|| ToolError::not_found(format!("engagement {id}")))?,
            (None, Some(name)) => {
                let found = engagements::find_by_name(c, name, &scopes)?;
                match found.len() {
                    0 => return Err(ToolError::not_found(format!("engagement `{name}`"))),
                    1 => found.into_iter().next().expect("len checked"),
                    _ => {
                        let candidates: Vec<_> =
                            found.iter().map(|e| json!({"engagement_id": e.id, "name": e.name, "scope": e.scope})).collect();
                        return Err(ToolError::conflict(format!("multiple engagements match `{name}`; pass engagement_id"))
                            .with_details(json!({"candidates": candidates})));
                    }
                }
            }
            (None, None) => return Err(ToolError::invalid_params("pass engagement_id or name")),
        };
        let organization = match engagement.org_id {
            Some(oid) => organizations::get(c, oid)?,
            None => None,
        };
        let member_list = engagements::members(c, engagement.id)?;
        let fact_list = facts::for_entity(c, "engagement", engagement.id, &scopes, 50)?;
        let mut ref_list = refs::for_target(c, "engagement", engagement.id, &scopes)?;
        for f in &fact_list {
            ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
        }
        let glossary_list = glossary::list(c, Some(engagement.id), &scopes)?;
        let interaction_list = interactions::recent_for_engagement(c, engagement.id, &scopes, 20)?;
        Ok(GetEngagementOutput {
            engagement,
            organization,
            people: member_list,
            facts: fact_list,
            refs: ref_list,
            glossary: glossary_list,
            interactions: interaction_list,
        })
    })
}
```

- [ ] **Step 4: mod.rs に配線し、テストを書く（tools/mod.rs の tests に追記）**

`mod get_engagement; mod get_organization; mod get_person;` と dispatch 3 本、`HANDLED_TOOLS` へ `"get_person", "get_organization", "get_engagement"` を追加。

```rust
    #[test]
    fn get_person_by_name_returns_connected_context() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s.call(&agent(), "get_person", json!({"name": "okash1n"})).unwrap();
        assert_eq!(out["person"]["id"].as_i64().unwrap(), ids.person);
        assert_eq!(out["organization"]["name"], "RELATIONS");
        assert_eq!(out["engagements"][0]["name"], "Okta導入支援");
        assert_eq!(out["interactions"].as_array().unwrap().len(), 1);
        // 引数無しは invalid_params、未知 id は not_found
        assert_eq!(s.call(&agent(), "get_person", json!({})).unwrap_err().code, ErrorCode::InvalidParams);
        assert_eq!(s.call(&agent(), "get_person", json!({"person_id": 9999})).unwrap_err().code, ErrorCode::NotFound);
    }

    #[test]
    fn get_engagement_hides_out_of_scope_and_returns_members() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s.call(&agent(), "get_engagement", json!({"engagement_id": ids.engagement})).unwrap();
        assert_eq!(out["people"][0]["person"]["id"].as_i64().unwrap(), ids.person);
        assert_eq!(out["people"][0]["role"], "key_person");
        assert_eq!(out["glossary"][0]["term"], "SCIM");
        assert_eq!(out["facts"][0]["statement"].as_str().unwrap().contains("SCIM"), true);
        // fact の根拠参照（minutes）が refs に載る
        assert!(out["refs"].as_array().unwrap().iter().any(|r| r["system"] == "minutes"));
        // 別 scope からは not_found
        let err = s
            .call(&agent(), "get_engagement", json!({"engagement_id": ids.engagement, "scope": "other"}))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn get_organization_lists_people_and_engagements() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s.call(&agent(), "get_organization", json!({"name": "RELATIONS"})).unwrap();
        assert_eq!(out["organization"]["id"].as_i64().unwrap(), ids.org);
        assert_eq!(out["people"].as_array().unwrap().len(), 1);
        assert_eq!(out["engagements"].as_array().unwrap().len(), 1);
    }
```

（`use super::test_support;` を tests 冒頭に追加。）

- [ ] **Step 5: テストを実行しコミット**

Run: `cargo test -p gaia-core`
Expected: すべて成功

```bash
git add crates/gaia-core
git commit -m "feat(core): add get_person/get_organization/get_engagement tools" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 15: get_glossary ＋ resolve_speakers

**Files:**
- Create: `crates/gaia-core/src/tools/get_glossary.rs`, `crates/gaia-core/src/tools/resolve_speakers.rs`
- Modify: `crates/gaia-core/src/tools/mod.rs`（配線・`HANDLED_TOOLS` 追加）

**Interfaces:**
- Produces: `get_glossary::handle`（`vocabulary_hints` = 用語・読み・案件関係者の名前と alias を重複除去した配列）、`resolve_speakers::handle`（正規化 → 完全一致 → matched(1.0) / ambiguous(0.5・engagement で 1 人に絞れたら matched 0.9) / unmatched(部分一致候補 0.4〜0.6)）
- resolve_speakers の scope 解決は `engagement_id` 指定時のみ行う（人物は名寄せ層＝共有のため）

- [ ] **Step 1: get_glossary.rs を実装する**

```rust
//! get_glossary。用語集と、Whisper initial_prompt 用の語彙ヒント。
use std::collections::HashSet;

use crate::{
    contracts::types::{GetGlossaryInput, GetGlossaryOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, glossary},
};

use super::CallContext;

pub fn handle(ctx: &CallContext<'_>, input: GetGlossaryInput) -> Result<GetGlossaryOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_glossary")?;
        if let Some(eid) = input.engagement_id {
            if engagements::get(c, eid, &scopes)?.is_none() {
                return Err(ToolError::not_found(format!("engagement {eid}")));
            }
        }
        let terms = glossary::list(c, input.engagement_id, &scopes)?;
        let mut hints: Vec<String> = Vec::new();
        for t in &terms {
            hints.push(t.term.clone());
            if let Some(r) = &t.reading {
                hints.push(r.clone());
            }
        }
        if let Some(eid) = input.engagement_id {
            for m in engagements::members(c, eid)? {
                hints.push(m.person.name.clone());
                for a in &m.person.aliases {
                    hints.push(a.alias.clone());
                }
            }
        }
        let mut seen = HashSet::new();
        hints.retain(|h| seen.insert(h.clone()));
        Ok(GetGlossaryOutput { terms, vocabulary_hints: hints })
    })
}
```

- [ ] **Step 2: resolve_speakers.rs を実装する**

```rust
//! resolve_speakers。会議ツールの表示名 → people の突合（話者実名化用）。仕様書 §8.3。
use rusqlite::Connection;

use crate::{
    contracts::types::{ResolveSpeakersInput, ResolveSpeakersOutput, SpeakerCandidate, SpeakerResult, SpeakerStatus},
    domain::normalize::normalize_name,
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, people},
};

use super::CallContext;

pub fn handle(ctx: &CallContext<'_>, input: ResolveSpeakersInput) -> Result<ResolveSpeakersOutput, ToolError> {
    ctx.db.with_conn(|c| {
        // 人物は名寄せ層（共有）なので、scope が要るのは engagement の関係者を引くときだけ。
        let preferred: Vec<i64> = match input.engagement_id {
            Some(eid) => {
                let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
                scopes.audit_cross_read(c, &ctx.client.name, "resolve_speakers")?;
                if engagements::get(c, eid, &scopes)?.is_none() {
                    return Err(ToolError::not_found(format!("engagement {eid}")));
                }
                engagements::member_ids(c, eid)?
            }
            None => Vec::new(),
        };
        let mut results = Vec::with_capacity(input.display_names.len());
        for raw in &input.display_names {
            results.push(resolve_one(c, raw, &preferred)?);
        }
        Ok(ResolveSpeakersOutput { results })
    })
}

fn resolve_one(c: &Connection, raw: &str, preferred: &[i64]) -> Result<SpeakerResult, ToolError> {
    let normalized = normalize_name(raw);
    if normalized.is_empty() {
        return Ok(result(raw, normalized, SpeakerStatus::Unmatched, 0.0, None, Vec::new()));
    }
    let matches = people::find_by_alias_normalized(c, &normalized)?;
    match matches.len() {
        1 => {
            let person = matches.into_iter().next().expect("len checked");
            Ok(result(raw, normalized, SpeakerStatus::Matched, 1.0, Some(person), Vec::new()))
        }
        0 => {
            let candidates: Vec<SpeakerCandidate> = people::search_like(c, raw, 5)?
                .into_iter()
                .map(|p| SpeakerCandidate {
                    confidence: if preferred.contains(&p.id) { 0.6 } else { 0.4 },
                    reason: "partial match".to_string(),
                    person_id: p.id,
                    name: p.name,
                })
                .collect();
            Ok(result(raw, normalized, SpeakerStatus::Unmatched, 0.0, None, candidates))
        }
        _ => {
            // 完全一致が複数。engagement の関係者で 1 人に絞れれば matched(0.9)。
            let narrowed: Vec<_> = matches.iter().filter(|p| preferred.contains(&p.id)).cloned().collect();
            if narrowed.len() == 1 {
                let person = narrowed.into_iter().next().expect("len checked");
                return Ok(result(raw, normalized, SpeakerStatus::Matched, 0.9, Some(person), Vec::new()));
            }
            let candidates: Vec<SpeakerCandidate> = matches
                .iter()
                .map(|p| SpeakerCandidate {
                    person_id: p.id,
                    name: p.name.clone(),
                    confidence: 0.5,
                    reason: "exact alias match".to_string(),
                })
                .collect();
            Ok(result(raw, normalized, SpeakerStatus::Ambiguous, 0.5, None, candidates))
        }
    }
}

fn result(
    raw: &str,
    normalized: String,
    status: SpeakerStatus,
    confidence: f64,
    person: Option<crate::contracts::types::PersonSummary>,
    candidates: Vec<SpeakerCandidate>,
) -> SpeakerResult {
    SpeakerResult { input: raw.to_string(), normalized, status, confidence, person, candidates }
}
```

- [ ] **Step 3: mod.rs に配線し、テストを書く（tools/mod.rs の tests に追記）**

`mod get_glossary; mod resolve_speakers;` と dispatch 2 本、`HANDLED_TOOLS` へ `"get_glossary", "resolve_speakers"` を追加。

```rust
    #[test]
    fn glossary_hints_include_terms_readings_and_member_aliases() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s.call(&agent(), "get_glossary", json!({"engagement_id": ids.engagement})).unwrap();
        let hints: Vec<&str> = out["vocabulary_hints"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        for expected in ["SCIM", "スキム", "岡村 慎太郎", "okash1n", "Okamura Shintaro"] {
            assert!(hints.contains(&expected), "missing {expected}: {hints:?}");
        }
        // engagement 省略で scope 内全用語
        let all = s.call(&agent(), "get_glossary", json!({})).unwrap();
        assert_eq!(all["terms"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn resolve_speakers_matches_zoom_style_display_names() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(&agent(), "resolve_speakers", json!({
                "display_names": ["岡村 慎太郎 (RELATIONS)", "OKAMURA SHINTARO", "見知らぬ 人"],
                "engagement_id": ids.engagement
            }))
            .unwrap();
        let results = out["results"].as_array().unwrap();
        assert_eq!(results[0]["status"], "matched");
        assert_eq!(results[0]["person"]["id"].as_i64().unwrap(), ids.person);
        assert_eq!(results[1]["status"], "matched", "ローマ字大文字も正規化で一致する");
        assert_eq!(results[2]["status"], "unmatched");
    }

    #[test]
    fn resolve_speakers_reports_ambiguity_and_narrows_by_engagement() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        // 同じ「田中」を 2 人つくる（片方だけ案件の関係者）
        let t1 = test_support::write(&s, "person", json!({"name": "田中 太郎", "aliases": [{"alias": "田中"}]}));
        let _t2 = test_support::write(&s, "person", json!({"name": "田中 次郎", "aliases": [{"alias": "田中"}]}));
        s.call(&human(), "propose_update", json!({
            "target_type": "engagement", "action": "update", "target_id": ids.engagement,
            "patch": {"people": [{"person_id": t1, "role": "member"}]}, "kind": "fact", "request_id": "req-add-tanaka-1"
        }))
        .and_then(|out| s.call(&human(), "approve_proposal", json!({"proposal_id": out["proposal_id"]})))
        .unwrap();
        // engagement 無し → ambiguous
        let out = s.call(&agent(), "resolve_speakers", json!({"display_names": ["田中"]})).unwrap();
        assert_eq!(out["results"][0]["status"], "ambiguous");
        assert_eq!(out["results"][0]["candidates"].as_array().unwrap().len(), 2);
        // engagement 指定 → 関係者の田中太郎に絞られて matched(0.9)
        let out = s
            .call(&agent(), "resolve_speakers", json!({"display_names": ["田中"], "engagement_id": ids.engagement}))
            .unwrap();
        assert_eq!(out["results"][0]["status"], "matched");
        assert_eq!(out["results"][0]["person"]["id"].as_i64().unwrap(), t1);
        assert!((out["results"][0]["confidence"].as_f64().unwrap() - 0.9).abs() < 1e-9);
    }
```

- [ ] **Step 4: テストを実行しコミット**

Run: `cargo test -p gaia-core`
Expected: すべて成功

```bash
git add crates/gaia-core
git commit -m "feat(core): add get_glossary and resolve_speakers tools" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 16: search_context

**Files:**
- Create: `crates/gaia-core/src/tools/search_context.rs`
- Modify: `crates/gaia-core/src/tools/mod.rs`（配線・`HANDLED_TOOLS` 完成・完全性テスト）

**Interfaces:**
- Produces: `search_context::handle`。スコア: 名前一致 3.0 / alias 一致 2.0 / fact ヒット 1.0（加算）。fact ヒットはそのエンティティに折りたたむ。各エンティティには現在の facts（最大 20）と refs（直付け＋fact の根拠参照）を同梱

- [ ] **Step 1: search_context.rs を実装する**

```rust
//! search_context =「回答の設計図」。仕様書 §8.3。
use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::{
    contracts::types::{
        Fact, SearchContextInput, SearchContextOutput, SearchEntity, SearchEntityType, SearchType,
    },
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, entities, facts, glossary, interactions, organizations, people, refs},
};

use super::CallContext;

struct Hit {
    type_: SearchEntityType,
    name: String,
    summary: String,
    score: f64,
    matched_on: Vec<String>,
}

pub fn handle(ctx: &CallContext<'_>, input: SearchContextInput) -> Result<SearchContextOutput, ToolError> {
    let query = input.query.trim().to_string();
    if query.is_empty() {
        return Err(ToolError::invalid_params("query must not be blank"));
    }
    let limit = input.limit.clamp(1, 50) as usize;
    let wants = |t: SearchType| input.types.is_empty() || input.types.contains(&t);
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "search_context")?;
        let mut hints: Vec<String> = Vec::new();
        if query.chars().count() < 3 {
            hints.push("query is shorter than 3 characters; substring match was used instead of full-text search".into());
        }
        if scopes.is_cross() {
            hints.push("cross-scope read (recorded in the audit log)".into());
        }

        let mut hits: BTreeMap<(String, i64), Hit> = BTreeMap::new();
        if wants(SearchType::Person) {
            for p in people::search_like(c, &query, limit)? {
                let by_name = p.name.to_lowercase().contains(&query.to_lowercase());
                let summary = describe_person(&p);
                add(&mut hits, SearchEntityType::Person, p.id, p.name.clone(), summary,
                    if by_name { 3.0 } else { 2.0 }, if by_name { "name" } else { "alias" });
            }
        }
        if wants(SearchType::Organization) {
            for o in organizations::search_like(c, &query, limit)? {
                add(&mut hits, SearchEntityType::Organization, o.id, o.name.clone(), o.kind.clone().unwrap_or_default(), 3.0, "name");
            }
        }
        if wants(SearchType::Engagement) {
            for e in engagements::search_like(c, &query, &scopes, limit)? {
                let summary = format!(
                    "{}{}",
                    e.org_name.clone().map(|o| format!("{o} / ")).unwrap_or_default(),
                    e.status.clone().unwrap_or_default()
                );
                add(&mut hits, SearchEntityType::Engagement, e.id, e.name.clone(), summary, 3.0, "name");
            }
        }
        if wants(SearchType::Entity) {
            for e in entities::search_like(c, &query, limit)? {
                add(&mut hits, SearchEntityType::Entity, e.id, e.name.clone(), e.type_.clone(), 3.0, "name");
            }
        }
        // facts 全文ヒット → 親エンティティに折りたたむ
        for f in facts::search(c, &query, &scopes, limit * 2)? {
            let et: SearchEntityType = f
                .entity_type
                .to_string()
                .parse()
                .map_err(|_| ToolError::internal("EntityType must map to SearchEntityType"))?;
            let wanted = match et {
                SearchEntityType::Person => wants(SearchType::Person),
                SearchEntityType::Organization => wants(SearchType::Organization),
                SearchEntityType::Engagement => wants(SearchType::Engagement),
                SearchEntityType::Entity => wants(SearchType::Entity),
                SearchEntityType::Interaction => wants(SearchType::Interaction),
            };
            if !wanted {
                continue;
            }
            if let Some((name, summary)) = entity_headline(c, &f, &scopes)? {
                add(&mut hits, et, f.entity_id, name, summary, 1.0, &format!("fact:{}", f.id));
            }
        }

        let mut entity_list: Vec<SearchEntity> = Vec::with_capacity(hits.len());
        for ((type_str, id), hit) in hits {
            let fact_list = facts::for_entity(c, &type_str, id, &scopes, 20)?;
            let mut ref_list = refs::for_target(c, &type_str, id, &scopes)?;
            for f in &fact_list {
                ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
            }
            entity_list.push(SearchEntity {
                type_: hit.type_,
                id,
                name: hit.name,
                summary: hit.summary,
                score: hit.score,
                matched_on: hit.matched_on,
                facts: fact_list,
                refs: ref_list,
            });
        }
        entity_list.sort_by(|a, b| {
            b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.name.cmp(&b.name))
        });
        entity_list.truncate(limit);

        let glossary_list = if wants(SearchType::Glossary) { glossary::search_like(c, &query, &scopes, limit)? } else { Vec::new() };
        let interaction_list =
            if wants(SearchType::Interaction) { interactions::search_like(c, &query, &scopes, limit)? } else { Vec::new() };
        Ok(SearchContextOutput {
            query: query.clone(),
            scopes: scopes.names().to_vec(),
            cross_scope: scopes.is_cross(),
            entities: entity_list,
            glossary: glossary_list,
            interactions: interaction_list,
            hints,
        })
    })
}

fn describe_person(p: &crate::contracts::types::PersonSummary) -> String {
    match (&p.role, &p.org_name) {
        (Some(role), Some(org)) => format!("{role} @ {org}"),
        (Some(role), None) => role.clone(),
        (None, Some(org)) => format!("@ {org}"),
        (None, None) => String::new(),
    }
}

fn entity_headline(c: &Connection, f: &Fact, scopes: &ScopeSet) -> Result<Option<(String, String)>, ToolError> {
    Ok(match f.entity_type.to_string().as_str() {
        "person" => people::get(c, f.entity_id)?.map(|p| { let s = describe_person(&p); (p.name, s) }),
        "organization" => organizations::get(c, f.entity_id)?.map(|o| (o.name, o.kind.unwrap_or_default())),
        "engagement" => engagements::get(c, f.entity_id, scopes)?.map(|e| (e.name, e.status.unwrap_or_default())),
        "interaction" => interactions::get(c, f.entity_id, scopes)?.map(|i| (format!("{} {}", i.occurred_at, i.kind), i.summary)),
        "entity" => entities::get(c, f.entity_id)?.map(|e| (e.name, e.type_)),
        _ => None,
    })
}

fn add(
    hits: &mut BTreeMap<(String, i64), Hit>,
    t: SearchEntityType,
    id: i64,
    name: String,
    summary: String,
    score: f64,
    matched: &str,
) {
    let entry = hits
        .entry((t.to_string(), id))
        .or_insert_with(|| Hit { type_: t, name, summary, score: 0.0, matched_on: Vec::new() });
    entry.score += score;
    entry.matched_on.push(matched.to_string());
}
```

- [ ] **Step 2: mod.rs に配線し、完全性テストを追加する**

`mod search_context;`、dispatch に `"search_context" => run(ctx, args, search_context::handle),`、`HANDLED_TOOLS` を最終形（12 ツール）にする。tests に追記:

```rust
    #[test]
    fn handled_tools_equal_enabled_contract_tools() {
        let s = service();
        let mut enabled: Vec<&str> = s.catalog().tools().iter().filter(|t| t.enabled).map(|t| t.name.as_str()).collect();
        let mut handled: Vec<&str> = HANDLED_TOOLS.to_vec();
        enabled.sort();
        handled.sort();
        assert_eq!(enabled, handled, "契約の enabled ツールと dispatch が 1:1 であること");
    }

    #[test]
    fn search_context_returns_answer_blueprint() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s.call(&agent(), "search_context", json!({"query": "SCIM"})).unwrap();
        // fact ヒットが案件に折りたたまれ、facts と minutes への参照が同梱される
        let e = &out["entities"][0];
        assert_eq!(e["type"], "engagement");
        assert_eq!(e["id"].as_i64().unwrap(), ids.engagement);
        assert!(e["matched_on"].as_array().unwrap().iter().any(|m| m.as_str().unwrap().starts_with("fact:")));
        assert!(e["refs"].as_array().unwrap().iter().any(|r| r["system"] == "minutes"));
        assert_eq!(out["glossary"][0]["term"], "SCIM");
        assert_eq!(out["interactions"].as_array().unwrap().len(), 1);
        assert_eq!(out["cross_scope"], false);
    }

    #[test]
    fn search_context_finds_people_by_alias_and_flags_short_queries() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s.call(&agent(), "search_context", json!({"query": "okash1n"})).unwrap();
        assert_eq!(out["entities"][0]["type"], "person");
        assert_eq!(out["entities"][0]["id"].as_i64().unwrap(), ids.person);
        assert!(out["entities"][0]["matched_on"].as_array().unwrap().iter().any(|m| m == "alias"));
        // 2 文字クエリはヒント付き
        let short = s.call(&agent(), "search_context", json!({"query": "決定", "types": ["engagement"]})).unwrap();
        assert!(!short["hints"].as_array().unwrap().is_empty());
        // scope 外のデータは出ない
        let other = s.call(&agent(), "search_context", json!({"query": "SCIM", "scope": "other"})).unwrap();
        assert_eq!(other["entities"].as_array().unwrap().len(), 0);
    }
```

- [ ] **Step 3: テスト・lint を実行しコミット**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功

```bash
git add crates/gaia-core
git commit -m "feat(core): add search_context returning answer blueprints" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 17: gaia-mcp（rmcp ServerHandler ＋ stdio）

**Files:**
- Create: `crates/gaia-mcp/src/server.rs`, `crates/gaia-mcp/src/stdio.rs`
- Modify: `crates/gaia-mcp/src/lib.rs`

**Interfaces:**
- Consumes: `gaia_core::tools::ToolService`、`gaia_core::contracts::ToolSpec`、`gaia_core::identity::ClientIdentity`
- Produces: `gaia_mcp::{GaiaServer, ServeError, serve_stdio}`
  - `GaiaServer::new(service: Arc<ToolService>, identity: ClientIdentity) -> GaiaServer`
  - `serve_stdio(server: GaiaServer) -> impl Future<Output = Result<(), ServeError>>`
- エラー写像（仕様書 §8.2）: 未知ツール → JSON-RPC `-32602`／`unauthorized` `invalid_params` `contract_mismatch` → JSON-RPC（unauthorized は `-32001`）、`data` に `ErrorObject`／その他（not_found / scope_denied / conflict / busy …）→ `CallToolResult` の `isError: true` ＋ `structuredContent.error`
- 注意（調査で確認済みの rmcp 3.1.4 の事実）: `ServerHandler` の `list_tools` / `call_tool` は `async fn` で実装できる。`call_tool` の戻りは `CallToolResponse`（`CallToolResult` から `.into()`）。`Tool::new_with_raw(name, description, Arc<JsonObject>)`。stdio ではログを stderr に限る

- [ ] **Step 1: server.rs を実装する**

```rust
//! rmcp ServerHandler。gaia_core::tools::ToolService への薄いアダプタ（ツールの解釈はしない）。
use std::{borrow::Cow, sync::Arc};

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode as RpcErrorCode, Implementation,
        InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
};
use serde_json::{Value, json};

use gaia_core::{contracts::ToolSpec, error::ToolError, identity::ClientIdentity, tools::ToolService};

pub struct GaiaServer {
    service: Arc<ToolService>,
    identity: ClientIdentity,
}

impl GaiaServer {
    pub fn new(service: Arc<ToolService>, identity: ClientIdentity) -> Self {
        Self { service, identity }
    }

    fn tools(&self) -> Vec<Tool> {
        self.service.visible_tools(self.identity.role).into_iter().map(to_tool).collect()
    }
}

pub(crate) fn to_tool(spec: &ToolSpec) -> Tool {
    let schema = match &spec.input_schema {
        Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    let mut tool = Tool::new_with_raw(spec.name.clone(), Some(Cow::Owned(spec.description.clone())), Arc::new(schema))
        .with_annotations(
            ToolAnnotations::new()
                .read_only(spec.annotations.read_only_hint)
                .destructive(spec.annotations.destructive_hint)
                .idempotent(spec.annotations.idempotent_hint)
                .open_world(spec.annotations.open_world_hint),
        );
    if let Some(title) = &spec.title {
        tool = tool.with_title(title.clone());
    }
    if let Some(Value::Object(out)) = &spec.output_schema {
        tool = tool.with_raw_output_schema(Arc::new(out.clone()));
    }
    tool
}

fn to_rpc_error(e: &ToolError) -> ErrorData {
    use gaia_core::error::ErrorCode;
    let code = match e.code {
        ErrorCode::Unauthorized => RpcErrorCode(-32001),
        _ => RpcErrorCode::INVALID_PARAMS,
    };
    ErrorData::new(code, e.message.clone(), Some(e.to_json()))
}

impl ServerHandler for GaiaServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("gaia_library", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "仕事の記憶の索引。search_context が要点と注記付き参照（回答の設計図）を返すので、返った refs は \
                 クライアント側のコネクタ（Notion / Box / ファイル等）で辿ること。書き込みは propose_update で提案し、\
                 人間の承認（approve_proposal）を待つ。",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // 未知ツールはプロトコルエラー（業務データの not_found と区別する）
        if self.service.catalog().get(request.name.as_ref()).filter(|s| s.enabled).is_none() {
            return Err(ErrorData::invalid_params(format!("unknown tool `{}`", request.name), None));
        }
        let args = request.arguments.clone().map(Value::Object).unwrap_or(json!({}));
        match self.service.call(&self.identity, request.name.as_ref(), args) {
            Ok(v) => Ok(CallToolResult::structured(v).into()),
            Err(e) if e.code.is_protocol_error() => Err(to_rpc_error(&e)),
            Err(e) => Ok(CallToolResult::structured_error(json!({"error": e.to_json()})).into()),
        }
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.service.catalog().get(name).filter(|s| s.allows(self.identity.role)).map(to_tool)
    }
}

#[cfg(test)]
mod tests {
    use super::to_tool;
    use gaia_core::contracts::Catalog;

    #[test]
    fn to_tool_carries_schema_title_and_annotations() {
        let catalog = Catalog::embedded().unwrap();
        let spec = catalog.get("search_context").unwrap();
        let tool = to_tool(spec);
        assert_eq!(tool.name, "search_context");
        assert!(tool.title.is_some());
        assert_eq!(tool.input_schema.get("type").and_then(|v| v.as_str()), Some("object"));
        assert!(tool.output_schema.is_some());
        let ann = tool.annotations.expect("annotations");
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.open_world_hint, Some(false));
        // 自己完結スキーマ（外部 $ref なし）で公開される
        let text = serde_json::to_string(&*tool.input_schema).unwrap();
        assert!(!text.contains("common.json"));
    }
}
```

- [ ] **Step 2: stdio.rs と lib.rs を実装する**

`stdio.rs`:

```rust
//! stdio トランスポート。stdout は JSON-RPC 専用（ログは stderr のみに出すこと）。
use rmcp::ServiceExt;

use crate::server::GaiaServer;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("initialize failed: {0}")]
    Init(String),
    #[error("server task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub async fn serve_stdio(server: GaiaServer) -> Result<(), ServeError> {
    let running = server.serve(rmcp::transport::stdio()).await.map_err(|e| ServeError::Init(e.to_string()))?;
    running.waiting().await?;
    Ok(())
}
```

`lib.rs`:

```rust
//! rmcp の ServerHandler を gaia_core::tools::ToolService に接続する薄い層。
pub mod server;
pub mod stdio;

pub use server::GaiaServer;
pub use stdio::{ServeError, serve_stdio};
```

- [ ] **Step 3: テストを実行しコミット**

Run: `cargo test -p gaia-mcp`
Expected: 1 test passed（`ToolAnnotations` のフィールドが `Option<bool>` でない場合は生成物・rmcp の定義に合わせてテスト側を調整）

```bash
git add crates/gaia-mcp
git commit -m "feat(mcp): add rmcp ServerHandler adapter and stdio transport" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 18: CLI 前半（init / client / affiliation / serve / info / call）

**Files:**
- Create: `crates/gaia/src/cli/mod.rs`, `crates/gaia/src/cli/app.rs`, `crates/gaia/src/cli/admin_cmd.rs`, `crates/gaia/src/cli/serve.rs`
- Modify: `crates/gaia/src/main.rs`

**Interfaces:**
- Consumes: `gaia_core::{config, admin, identity, storage::Db, contracts::Catalog, tools::ToolService}`、`gaia_mcp::{GaiaServer, serve_stdio}`
- Produces:
  - `cli::Cli`（clap。グローバル: `--config <path>` / `--client <name>` / `--json`）と `cli::run(Cli) -> anyhow::Result<()>`
  - `cli::app::App { config_path, config, service }`、`App::open(config_override) -> anyhow::Result<App>`、`App::identity(name: Option<&str>) -> anyhow::Result<ClientIdentity>`、`App::call(&self, &ClientIdentity, tool, args) -> anyhow::Result<Value>`、`app::print_json(&Value, compact: bool)`、`app::init(&InitArgs, config_override) -> anyhow::Result<()>`
  - コマンド: `init --affiliation <name> [--identity] [--client-name] [--db]`／`client add <name> --role <human|agent> [--default-scope]`・`client list`／`affiliation add <name> [--identity]`・`affiliation list`／`serve --stdio`／`info`／`call <tool> --args <json>`
- Task 19 が `cli::query` / `cli::write` と残りの Command variant・dispatch を追加する。このタスクの `Command` enum は Init / Serve / Affiliation / Client / Info / Call の 6 variant のみ（Step 5 のコードのとおり）

- [ ] **Step 1: main.rs を書き換える**

```rust
use clap::Parser;

mod cli;

fn main() {
    // ログは stderr へ。stdout は serve の JSON-RPC と JSON 出力専用。
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = cli::Cli::parse();
    if let Err(e) = cli::run(cli) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 2: cli/app.rs を実装する**

```rust
//! 起動処理: 設定ロード → DB オープン → ToolService 構築 → 識別解決。
use std::path::PathBuf;

use anyhow::{Context, bail};
use serde_json::Value;

use gaia_core::{
    config::{self, Config},
    contracts::Catalog,
    identity::{ClientIdentity, Role},
    storage::Db,
    tools::ToolService,
};

pub struct App {
    pub config_path: PathBuf,
    pub config: Config,
    pub service: ToolService,
}

impl App {
    pub fn open(config_override: Option<&PathBuf>) -> anyhow::Result<Self> {
        let config_path = resolve_config_path(config_override)?;
        if !config_path.exists() {
            bail!(
                "設定がありません: {} — まず `gaia init --affiliation <name>` を実行してください",
                config_path.display()
            );
        }
        let config = Config::load(&config_path)?;
        let db = Db::open(&config::db_path(&config)?)?;
        let catalog = Catalog::embedded().context("contracts のロードに失敗")?;
        Ok(Self { config_path, config, service: ToolService::new(db, catalog) })
    }

    pub fn identity(&self, name: Option<&str>) -> anyhow::Result<ClientIdentity> {
        Ok(self.config.resolve_client(name)?.clone())
    }

    pub fn call(&self, client: &ClientIdentity, tool: &str, args: Value) -> anyhow::Result<Value> {
        self.service.call(client, tool, args).map_err(|e| {
            let details = e.details.clone().map(|d| format!("\n{d:#}")).unwrap_or_default();
            anyhow::anyhow!("{e}{details}")
        })
    }
}

pub fn resolve_config_path(config_override: Option<&PathBuf>) -> anyhow::Result<PathBuf> {
    Ok(match config_override {
        Some(p) => p.clone(),
        None => config::config_path()?,
    })
}

pub fn print_json(value: &Value, compact: bool) {
    if compact {
        println!("{value}");
    } else {
        println!("{value:#}");
    }
}

pub fn init(args: &super::InitArgs, config_override: Option<&PathBuf>) -> anyhow::Result<()> {
    let config_path = resolve_config_path(config_override)?;
    if config_path.exists() {
        bail!(
            "設定が既にあります: {} — affiliation は `gaia affiliation add`、クライアントは `gaia client add` を使ってください",
            config_path.display()
        );
    }
    let client_name = args
        .client_name
        .clone()
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "me".to_string());
    let mut config = Config::default();
    config.db_path = args.db.clone();
    config.cli.default_client = Some(client_name.clone());
    config.add_client(ClientIdentity {
        name: client_name.clone(),
        role: Role::Human,
        default_scope: Some(args.affiliation.clone()),
    })?;
    config.save(&config_path)?;
    let db = Db::open(&config::db_path(&config)?)?;
    gaia_core::admin::add_affiliation(&db, &client_name, &args.affiliation, args.identity.as_deref())?;
    eprintln!("初期化しました:");
    eprintln!("  config: {}", config_path.display());
    eprintln!("  db:     {}", config::db_path(&config)?.display());
    eprintln!("  human client: {client_name} (default_scope={})", args.affiliation);
    Ok(())
}
```

- [ ] **Step 3: cli/admin_cmd.rs を実装する**

```rust
//! 管理系: affiliation（DB・audit 付き）と client（設定ファイル）。提案キュー原則の例外はここだけ。
use std::path::PathBuf;

use anyhow::bail;
use clap::Subcommand;
use serde_json::json;

use gaia_core::{config::Config, identity::{ClientIdentity, Role}};

use super::app::{App, print_json};

#[derive(Subcommand)]
pub enum AffiliationCmd {
    /// 機密境界を追加する（human。audit_log(admin_write) に記録）
    Add { name: String, #[arg(long)] identity: Option<String> },
    /// 一覧
    List,
}

#[derive(Subcommand)]
pub enum ClientCmd {
    /// クライアント（識別）を設定ファイルへ追加する
    Add {
        name: String,
        #[arg(long)]
        role: Role,
        #[arg(long)]
        default_scope: Option<String>,
    },
    /// 一覧
    List,
}

pub fn affiliation(app: &App, cli_client: Option<&str>, cmd: &AffiliationCmd, compact: bool) -> anyhow::Result<()> {
    match cmd {
        AffiliationCmd::Add { name, identity } => {
            let actor = app.identity(cli_client)?;
            if actor.role != Role::Human {
                bail!("affiliation の管理は human クライアントのみ（--client を確認）");
            }
            let id = gaia_core::admin::add_affiliation(app.service.db(), &actor.name, name, identity.as_deref())?;
            print_json(&json!({"id": id, "name": name}), compact);
        }
        AffiliationCmd::List => {
            let list = gaia_core::admin::list_affiliations(app.service.db())?;
            let rows: Vec<_> =
                list.iter().map(|a| json!({"id": a.id, "name": a.name, "identity": a.identity})).collect();
            print_json(&serde_json::Value::Array(rows), compact);
        }
    }
    Ok(())
}

pub fn client(config_path: &PathBuf, cmd: &ClientCmd, compact: bool) -> anyhow::Result<()> {
    let mut config = Config::load(config_path)?;
    match cmd {
        ClientCmd::Add { name, role, default_scope } => {
            config.add_client(ClientIdentity { name: name.clone(), role: *role, default_scope: default_scope.clone() })?;
            config.save(config_path)?;
            eprintln!("クライアント `{name}` を追加しました（role={role}）");
        }
        ClientCmd::List => print_json(&serde_json::to_value(&config.clients)?, compact),
    }
    Ok(())
}
```

（`Role` を clap の値に使うには `FromStr` があればよい。`Role::Err = String` は `Into<Box<dyn Error>>` を満たすため `#[arg(long)]` のままで動く。動かない場合は `#[arg(long, value_parser = clap::builder::ValueParser::new(|s: &str| s.parse::<Role>()))]` にする。）

- [ ] **Step 4: cli/serve.rs を実装する**

```rust
//! gaia serve --stdio --client <name>。識別は起動時に固定される（仕様書 §7.1）。
use clap::Args;

use super::app::App;

#[derive(Args)]
pub struct ServeArgs {
    /// stdio トランスポートで起動する（v0.1 はこれのみ）
    #[arg(long)]
    pub stdio: bool,
}

pub fn serve(app: App, cli_client: Option<&str>, args: &ServeArgs) -> anyhow::Result<()> {
    if !args.stdio {
        anyhow::bail!("v0.1 は --stdio のみ対応です（HTTP は次のサブプロジェクトで追加）");
    }
    let identity = app.identity(cli_client)?;
    tracing::info!(client = %identity.name, role = %identity.role, "starting gaia_library over stdio");
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let server = gaia_mcp::GaiaServer::new(std::sync::Arc::new(app.service), identity);
        gaia_mcp::serve_stdio(server).await
    })?;
    Ok(())
}
```

- [ ] **Step 5: cli/mod.rs を実装する（Command 全 variant ＋ run。query / write の arm は一時 bail）**

```rust
//! CLI。全コマンドが ToolService::call を経由する（例外は init / affiliation / client の管理系のみ）。
mod admin_cmd;
mod app;
mod serve;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(name = "gaia", version, about = "gaia-library: 仕事の記憶の索引 MCP サーバー")]
pub struct Cli {
    /// 設定ファイルのパス（既定: $GAIA_CONFIG → ~/.config/gaia-library/config.toml）
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    /// 操作するクライアント名（既定: [cli].default_client）
    #[arg(long, global = true)]
    pub client: Option<String>,
    /// 1 行 JSON で出力（既定は整形済み JSON）
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct InitArgs {
    /// 最初の機密境界名（例: cloudnative）
    #[arg(long)]
    pub affiliation: String,
    #[arg(long)]
    pub identity: Option<String>,
    /// human クライアント名（既定: $USER）
    #[arg(long)]
    pub client_name: Option<String>,
    /// DB パス（既定: ~/.local/share/gaia-library/gaia.db）
    #[arg(long)]
    pub db: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// 設定と DB を初期化する
    Init(InitArgs),
    /// MCP サーバーを起動する
    Serve(serve::ServeArgs),
    /// 機密境界（affiliation）の管理
    Affiliation {
        #[command(subcommand)]
        cmd: admin_cmd::AffiliationCmd,
    },
    /// クライアント（識別）の管理
    Client {
        #[command(subcommand)]
        cmd: admin_cmd::ClientCmd,
    },
    /// サーバー情報（get_server_info）
    Info,
    /// 任意ツールの汎用呼び出し
    Call {
        tool: String,
        #[arg(long)]
        args: String,
    },
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let compact = cli.json;
    match &cli.command {
        Command::Init(args) => app::init(args, cli.config.as_ref()),
        Command::Client { cmd } => {
            let path = app::resolve_config_path(cli.config.as_ref())?;
            admin_cmd::client(&path, cmd, compact)
        }
        Command::Serve(args) => {
            let app = app::App::open(cli.config.as_ref())?;
            serve::serve(app, cli.client.as_deref(), args)
        }
        Command::Affiliation { cmd } => {
            let app = app::App::open(cli.config.as_ref())?;
            admin_cmd::affiliation(&app, cli.client.as_deref(), cmd, compact)
        }
        Command::Info => {
            let app = app::App::open(cli.config.as_ref())?;
            let client = app.identity(cli.client.as_deref())?;
            let out = app.call(&client, "get_server_info", json!({}))?;
            app::print_json(&out, compact);
            Ok(())
        }
        Command::Call { tool, args } => {
            let app = app::App::open(cli.config.as_ref())?;
            let client = app.identity(cli.client.as_deref())?;
            let value: serde_json::Value = serde_json::from_str(args).map_err(|e| anyhow::anyhow!("--args は JSON: {e}"))?;
            let out = app.call(&client, tool, value)?;
            app::print_json(&out, compact);
            Ok(())
        }
    }
}
```

- [ ] **Step 6: 手動スモークを実行する**

Run:

```bash
D="$(mktemp -d)"
export GAIA_CONFIG="$D/config.toml" GAIA_DB="$D/gaia.db"
cargo run -p gaia -- init --affiliation cloudnative --client-name tester
cargo run -p gaia -- client add bot --role agent --default-scope cloudnative
cargo run -p gaia -- affiliation list
cargo run -p gaia -- info
cargo run -p gaia -- --client bot --json call get_server_info --args '{}'
unset GAIA_CONFIG GAIA_DB
```

Expected: init がパスを表示、affiliation list に cloudnative、info の `client.role` が human、最後の call の `client.role` が agent

- [ ] **Step 7: lint・テストを実行しコミット**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功

```bash
git add crates/gaia
git commit -m "feat(cli): add init/client/affiliation/serve/info/call commands" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 19: CLI 後半（search / person / org / engagement / glossary / speakers / propose / proposals / approve / reject / add）

**Files:**
- Create: `crates/gaia/src/cli/query.rs`, `crates/gaia/src/cli/write.rs`
- Modify: `crates/gaia/src/cli/mod.rs`（`mod query; mod write;`、Command variant 追加、run の dispatch 追加）

**Interfaces:**
- Produces: 仕様書 §10 のコマンド一覧。すべて `ToolService::call` 経由。`add *` は「propose → approve」を 1 コマンド化（request_id は `cli-<uuid v4>` 自動発番）

- [ ] **Step 1: query.rs を実装する**

```rust
//! 参照系コマンド。引数を JSON に組んで ToolService::call に渡すだけ。
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use gaia_core::identity::ClientIdentity;

use super::app::{App, print_json};

#[derive(Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long)]
    pub scope: Vec<String>,
    /// 検索対象の種別（person / organization / engagement / entity / interaction / glossary）
    #[arg(long = "type")]
    pub types: Vec<String>,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Subcommand)]
pub enum GetCmd {
    /// id または name で 1 件取得
    Get {
        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        scope: Vec<String>,
    },
}

#[derive(Args)]
pub struct GlossaryArgs {
    #[arg(long)]
    pub engagement_id: Option<i64>,
    #[arg(long)]
    pub scope: Vec<String>,
}

#[derive(Args)]
pub struct SpeakersArgs {
    /// 会議ツールの表示名（複数可）
    pub names: Vec<String>,
    #[arg(long)]
    pub engagement_id: Option<i64>,
    #[arg(long)]
    pub scope: Vec<String>,
}

fn put_scope(payload: &mut Value, scope: &[String]) {
    if !scope.is_empty() {
        payload["scope"] = json!(scope);
    }
}

pub fn search(app: &App, client: &ClientIdentity, args: &SearchArgs, compact: bool) -> anyhow::Result<()> {
    let mut payload = json!({"query": args.query});
    put_scope(&mut payload, &args.scope);
    if !args.types.is_empty() {
        payload["types"] = json!(args.types);
    }
    if let Some(l) = args.limit {
        payload["limit"] = json!(l);
    }
    print_json(&app.call(client, "search_context", payload)?, compact);
    Ok(())
}

pub fn get_entity(
    app: &App,
    client: &ClientIdentity,
    tool: &str,
    id_key: &str,
    cmd: &GetCmd,
    compact: bool,
) -> anyhow::Result<()> {
    let GetCmd::Get { id, name, scope } = cmd;
    let mut payload = json!({});
    if let Some(id) = id {
        payload[id_key] = json!(id);
    }
    if let Some(name) = name {
        payload["name"] = json!(name);
    }
    put_scope(&mut payload, scope);
    print_json(&app.call(client, tool, payload)?, compact);
    Ok(())
}

pub fn glossary(app: &App, client: &ClientIdentity, args: &GlossaryArgs, compact: bool) -> anyhow::Result<()> {
    let mut payload = json!({});
    if let Some(eid) = args.engagement_id {
        payload["engagement_id"] = json!(eid);
    }
    put_scope(&mut payload, &args.scope);
    print_json(&app.call(client, "get_glossary", payload)?, compact);
    Ok(())
}

pub fn speakers(app: &App, client: &ClientIdentity, args: &SpeakersArgs, compact: bool) -> anyhow::Result<()> {
    let mut payload = json!({"display_names": args.names});
    if let Some(eid) = args.engagement_id {
        payload["engagement_id"] = json!(eid);
    }
    put_scope(&mut payload, &args.scope);
    print_json(&app.call(client, "resolve_speakers", payload)?, compact);
    Ok(())
}
```

- [ ] **Step 2: write.rs を実装する**

```rust
//! 提案系コマンド。`add *` は human 向けに「提案＋即時承認」を 1 コマンド化したもの。
use anyhow::Context;
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use gaia_core::identity::{ClientIdentity, Role};

use super::app::{App, print_json};

#[derive(Args)]
pub struct ProposeArgs {
    /// person / organization / engagement / interaction / entity / fact / ref / glossary
    pub target_type: String,
    /// insert / update / supersede
    pub action: String,
    /// Patch JSON（target_type ごとの形。契約 defs/common.json 参照）
    #[arg(long)]
    pub patch: String,
    #[arg(long)]
    pub target_id: Option<i64>,
    #[arg(long, default_value = "fact")]
    pub kind: String,
    #[arg(long)]
    pub scope: Option<String>,
    /// 出所 JSON（{"ref_id": N} か {"system", "uri", "note", ...}）
    #[arg(long)]
    pub provenance: Option<String>,
    /// 冪等化キー（省略時は cli-<uuid> を自動発番）
    #[arg(long)]
    pub request_id: Option<String>,
}

#[derive(Args)]
pub struct ProposalsArgs {
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub scope: Vec<String>,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Subcommand)]
pub enum AddCmd {
    Person {
        #[arg(long)] name: String,
        #[arg(long)] org_id: Option<i64>,
        #[arg(long)] role: Option<String>,
        #[arg(long)] alias: Vec<String>,
        #[arg(long)] scope: Option<String>,
    },
    Org {
        #[arg(long)] name: String,
        #[arg(long)] kind: Option<String>,
        #[arg(long)] scope: Option<String>,
    },
    Engagement {
        #[arg(long)] name: String,
        #[arg(long)] org_id: Option<i64>,
        #[arg(long)] status: Option<String>,
        #[arg(long)] person_id: Vec<i64>,
        #[arg(long)] scope: Option<String>,
    },
    Fact {
        #[arg(long)] entity_type: String,
        #[arg(long)] entity_id: i64,
        #[arg(long)] statement: String,
        #[arg(long)] predicate: Option<String>,
        #[arg(long)] value: Option<String>,
        #[arg(long, default_value = "fact")] kind: String,
        #[arg(long)] scope: Option<String>,
    },
    Ref {
        #[arg(long)] target_type: String,
        #[arg(long)] target_id: i64,
        #[arg(long)] system: String,
        #[arg(long)] uri: String,
        #[arg(long)] title: Option<String>,
        #[arg(long)] note: String,
        #[arg(long)] snapshot: Option<String>,
        #[arg(long)] scope: Option<String>,
    },
    Glossary {
        #[arg(long)] term: String,
        #[arg(long)] reading: Option<String>,
        #[arg(long)] definition: Option<String>,
        #[arg(long)] engagement_id: Option<i64>,
        #[arg(long)] scope: Option<String>,
    },
    Interaction {
        #[arg(long)] kind: String,
        #[arg(long)] occurred_at: String,
        #[arg(long)] summary: String,
        #[arg(long)] engagement_id: Option<i64>,
        #[arg(long)] person_id: Vec<i64>,
        #[arg(long)] scope: Option<String>,
    },
}

fn new_request_id() -> String {
    format!("cli-{}", uuid::Uuid::new_v4())
}

/// null を取り除いた JSON オブジェクトを作る（COALESCE 更新と噛み合わせるため）。
fn compact_object(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(map.into_iter().filter(|(_, v)| !v.is_null()).collect()),
        other => other,
    }
}

pub fn propose(app: &App, client: &ClientIdentity, args: &ProposeArgs, compact: bool) -> anyhow::Result<()> {
    let patch: Value = serde_json::from_str(&args.patch).context("--patch は JSON で指定する")?;
    let mut payload = json!({
        "target_type": args.target_type,
        "action": args.action,
        "patch": patch,
        "kind": args.kind,
        "request_id": args.request_id.clone().unwrap_or_else(new_request_id),
    });
    if let Some(id) = args.target_id {
        payload["target_id"] = json!(id);
    }
    if let Some(s) = &args.scope {
        payload["scope"] = json!(s);
    }
    if let Some(p) = &args.provenance {
        payload["provenance"] = serde_json::from_str(p).context("--provenance は JSON で指定する")?;
    }
    print_json(&app.call(client, "propose_update", payload)?, compact);
    Ok(())
}

pub fn proposals(app: &App, client: &ClientIdentity, args: &ProposalsArgs, compact: bool) -> anyhow::Result<()> {
    let mut payload = json!({});
    if let Some(s) = &args.status {
        payload["status"] = json!(s);
    }
    if !args.scope.is_empty() {
        payload["scope"] = json!(args.scope);
    }
    if let Some(l) = args.limit {
        payload["limit"] = json!(l);
    }
    print_json(&app.call(client, "list_proposals", payload)?, compact);
    Ok(())
}

pub fn add(app: &App, client: &ClientIdentity, cmd: &AddCmd, compact: bool) -> anyhow::Result<()> {
    if client.role != Role::Human {
        anyhow::bail!("`gaia add` は human クライアント専用（agent は `gaia propose` で提案する）");
    }
    let (target_type, patch, kind, scope) = match cmd {
        AddCmd::Person { name, org_id, role, alias, scope } => (
            "person",
            compact_object(json!({
                "name": name, "org_id": org_id, "role": role,
                "aliases": alias.iter().map(|a| json!({"alias": a})).collect::<Vec<_>>(),
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Org { name, kind, scope } => (
            "organization",
            compact_object(json!({"name": name, "kind": kind})),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Engagement { name, org_id, status, person_id, scope } => (
            "engagement",
            compact_object(json!({
                "name": name, "org_id": org_id, "status": status,
                "people": person_id.iter().map(|p| json!({"person_id": p})).collect::<Vec<_>>(),
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Fact { entity_type, entity_id, statement, predicate, value, kind, scope } => (
            "fact",
            compact_object(json!({
                "entity_type": entity_type, "entity_id": entity_id, "statement": statement,
                "predicate": predicate, "value": value,
            })),
            kind.clone(),
            scope.clone(),
        ),
        AddCmd::Ref { target_type, target_id, system, uri, title, note, snapshot, scope } => (
            "ref",
            compact_object(json!({
                "target_type": target_type, "target_id": target_id, "system": system, "uri": uri,
                "title": title, "note": note, "snapshot": snapshot,
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Glossary { term, reading, definition, engagement_id, scope } => (
            "glossary",
            compact_object(json!({
                "term": term, "reading": reading, "definition": definition, "engagement_id": engagement_id,
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Interaction { kind, occurred_at, summary, engagement_id, person_id, scope } => (
            "interaction",
            compact_object(json!({
                "kind": kind, "occurred_at": occurred_at, "summary": summary,
                "engagement_id": engagement_id, "person_ids": person_id,
            })),
            "fact".to_string(),
            scope.clone(),
        ),
    };
    let mut payload = json!({
        "target_type": target_type, "action": "insert", "patch": patch, "kind": kind,
        "request_id": new_request_id(),
    });
    if let Some(s) = scope {
        payload["scope"] = json!(s);
    }
    let proposed = app.call(client, "propose_update", payload)?;
    let approved = app.call(client, "approve_proposal", json!({"proposal_id": proposed["proposal_id"]}))?;
    print_json(&approved, compact);
    Ok(())
}
```

- [ ] **Step 3: mod.rs に Command variant と dispatch を追加する**

`mod query; mod write;` を追加し、`Command` に:

```rust
    /// 横断検索（search_context）
    Search(query::SearchArgs),
    /// 人物の詳細（get_person）
    Person {
        #[command(subcommand)]
        cmd: query::GetCmd,
    },
    /// 組織の詳細（get_organization）
    Org {
        #[command(subcommand)]
        cmd: query::GetCmd,
    },
    /// 案件の詳細（get_engagement）
    Engagement {
        #[command(subcommand)]
        cmd: query::GetCmd,
    },
    /// 用語集と語彙ヒント（get_glossary）
    Glossary(query::GlossaryArgs),
    /// 表示名の人物突合（resolve_speakers）
    Speakers(query::SpeakersArgs),
    /// 更新の提案（propose_update）
    Propose(write::ProposeArgs),
    /// 提案の一覧（list_proposals）
    Proposals(write::ProposalsArgs),
    /// 提案の承認（human）
    Approve { proposal_id: i64 },
    /// 提案の却下（human）
    Reject {
        proposal_id: i64,
        #[arg(long)]
        reason: Option<String>,
    },
    /// 提案＋即時承認（human）
    Add {
        #[command(subcommand)]
        cmd: write::AddCmd,
    },
```

`run()` に dispatch を追加（`App::open` → `identity` は Info / Call と同じ形）:

```rust
        Command::Search(a) => with_app(&cli, |app, client| query::search(app, client, a, compact)),
        Command::Person { cmd } => with_app(&cli, |app, client| query::get_entity(app, client, "get_person", "person_id", cmd, compact)),
        Command::Org { cmd } => with_app(&cli, |app, client| query::get_entity(app, client, "get_organization", "organization_id", cmd, compact)),
        Command::Engagement { cmd } => with_app(&cli, |app, client| query::get_entity(app, client, "get_engagement", "engagement_id", cmd, compact)),
        Command::Glossary(a) => with_app(&cli, |app, client| query::glossary(app, client, a, compact)),
        Command::Speakers(a) => with_app(&cli, |app, client| query::speakers(app, client, a, compact)),
        Command::Propose(a) => with_app(&cli, |app, client| write::propose(app, client, a, compact)),
        Command::Proposals(a) => with_app(&cli, |app, client| write::proposals(app, client, a, compact)),
        Command::Approve { proposal_id } => with_app(&cli, |app, client| {
            let out = app.call(client, "approve_proposal", json!({"proposal_id": proposal_id}))?;
            app::print_json(&out, compact);
            Ok(())
        }),
        Command::Reject { proposal_id, reason } => with_app(&cli, |app, client| {
            let out = app.call(client, "reject_proposal", json!({"proposal_id": proposal_id, "reason": reason}))?;
            app::print_json(&out, compact);
            Ok(())
        }),
        Command::Add { cmd } => with_app(&cli, |app, client| write::add(app, client, cmd, compact)),
```

補助関数（mod.rs 内）:

```rust
fn with_app(
    cli: &Cli,
    f: impl FnOnce(&app::App, &gaia_core::identity::ClientIdentity) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let app = app::App::open(cli.config.as_ref())?;
    let client = app.identity(cli.client.as_deref())?;
    f(&app, &client)
}
```

- [ ] **Step 4: 手動スモークを実行する**

Run:

```bash
D="$(mktemp -d)"
export GAIA_CONFIG="$D/config.toml" GAIA_DB="$D/gaia.db"
cargo run -p gaia -- init --affiliation cloudnative --client-name tester
cargo run -p gaia -- add org --name RELATIONS --kind customer
cargo run -p gaia -- add person --name "岡村 慎太郎" --org-id 1 --alias okash1n
cargo run -p gaia -- --json search okash1n
cargo run -p gaia -- --json speakers "岡村 慎太郎 (RELATIONS)"
cargo run -p gaia -- proposals --status approved
unset GAIA_CONFIG GAIA_DB
```

Expected: search の先頭が person、speakers が matched、approved の提案が 2 件

- [ ] **Step 5: lint・テストを実行しコミット**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: すべて成功

```bash
git add crates/gaia
git commit -m "feat(cli): add query and proposal commands with add shortcuts" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 20: 統合テスト（CLI 一気通貫 ＋ stdio MCP スモーク）

**Files:**
- Create: `crates/gaia/tests/cli_flow.rs`, `crates/gaia/tests/stdio.rs`

**Interfaces:**
- Consumes: `CARGO_BIN_EXE_gaia`（cargo が統合テストに渡すバイナリパス）、`GAIA_CONFIG` / `GAIA_DB` 環境変数（Task 6）
- 検証項目: 仕様書 §11.2 のとおり。narumi 無しで全テストが通ること

- [ ] **Step 1: cli_flow.rs を書く**

```rust
//! CLI の一気通貫: init → client add → add → search → speakers → 認可。
use std::process::Command;

fn gaia(dir: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gaia"));
    c.env("GAIA_CONFIG", dir.join("config.toml"));
    c.env("GAIA_DB", dir.join("gaia.db"));
    c
}

fn run_ok(c: &mut Command) -> serde_json::Value {
    let out = c.output().expect("spawn gaia");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() { serde_json::Value::Null } else { serde_json::from_str(trimmed).expect("json stdout") }
}

#[test]
fn init_add_search_speakers_and_authorization() {
    let dir = tempfile::tempdir().unwrap();
    run_ok(gaia(dir.path()).args(["init", "--affiliation", "cloudnative", "--client-name", "tester"]));
    run_ok(gaia(dir.path()).args(["client", "add", "bot", "--role", "agent", "--default-scope", "cloudnative"]));
    let added = run_ok(gaia(dir.path()).args(["--json", "add", "person", "--name", "岡村 慎太郎", "--alias", "okash1n"]));
    assert_eq!(added["status"], "approved");
    let person_id = added["result"]["id"].as_i64().unwrap();

    let found = run_ok(gaia(dir.path()).args(["--json", "search", "okash1n"]));
    assert_eq!(found["entities"][0]["type"], "person");
    assert_eq!(found["entities"][0]["id"].as_i64().unwrap(), person_id);

    let speakers = run_ok(gaia(dir.path()).args(["--json", "speakers", "岡村 慎太郎 (CloudNative)"]));
    assert_eq!(speakers["results"][0]["status"], "matched");

    // agent は add（承認込み）を実行できない
    let denied = gaia(dir.path())
        .args(["--client", "bot", "add", "person", "--name", "x"])
        .output()
        .unwrap();
    assert!(!denied.status.success());

    // agent の propose は通り、pending に載る
    let proposed = run_ok(gaia(dir.path()).args([
        "--client", "bot", "--json", "propose", "person", "insert",
        "--patch", r#"{"name": "田中 太郎"}"#,
    ]));
    assert_eq!(proposed["status"], "pending");
    let pending = run_ok(gaia(dir.path()).args(["--json", "proposals"]));
    assert!(!pending["proposals"].as_array().unwrap().is_empty());
}
```

- [ ] **Step 2: stdio.rs を書く（生 JSON-RPC。rmcp クライアントに依存しない）**

```rust
//! stdio MCP スモーク: initialize → tools/list → tools/call。role でツール可視性が変わること。
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    reader: BufReader<ChildStdout>,
}

impl Server {
    fn start(dir: &std::path::Path, client: &str) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gaia"))
            .args(["serve", "--stdio", "--client", client])
            .env("GAIA_CONFIG", dir.join("config.toml"))
            .env("GAIA_DB", dir.join("gaia.db"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn gaia serve");
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server { child, reader };
        s.send(serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "it", "version": "0"}}}));
        let init = s.recv();
        assert_eq!(init["result"]["serverInfo"]["name"], "gaia_library");
        s.send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        s
    }

    fn send(&mut self, v: serde_json::Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{v}").unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> serde_json::Value {
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).unwrap();
            assert!(n > 0, "server closed stdout");
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            return serde_json::from_str(t).unwrap_or_else(|e| panic!("not json: {e}: {t}"));
        }
    }

    fn request(&mut self, id: i64, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let msg = self.recv();
            if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                return msg;
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn setup(dir: &std::path::Path) {
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_gaia"))
            .args(args)
            .env("GAIA_CONFIG", dir.join("config.toml"))
            .env("GAIA_DB", dir.join("gaia.db"))
            .output()
            .unwrap();
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    };
    run(&["init", "--affiliation", "cloudnative", "--client-name", "tester"]);
    run(&["client", "add", "bot", "--role", "agent", "--default-scope", "cloudnative"]);
    run(&["add", "person", "--name", "岡村 慎太郎", "--alias", "okash1n"]);
}

#[test]
fn agent_sees_filtered_tools_and_calls_search() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let mut s = Server::start(dir.path(), "bot");

    let listed = s.request(1, "tools/list", serde_json::json!({}));
    let names: Vec<&str> =
        listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"search_context"));
    assert!(!names.contains(&"approve_proposal"), "agent には承認系が見えない: {names:?}");
    assert!(!names.contains(&"resolve_source"), "未登録ツールは見えない");

    let called = s.request(2, "tools/call", serde_json::json!({"name": "search_context", "arguments": {"query": "okash1n"}}));
    assert_eq!(called["result"]["isError"], serde_json::json!(false));
    assert_eq!(called["result"]["structuredContent"]["entities"][0]["type"], "person");

    // 業務エラー（not_found）は isError の結果
    let nf = s.request(3, "tools/call", serde_json::json!({"name": "get_person", "arguments": {"person_id": 9999}}));
    assert_eq!(nf["result"]["isError"], serde_json::json!(true));
    assert_eq!(nf["result"]["structuredContent"]["error"]["code"], "not_found");

    // 認可エラーは JSON-RPC エラー（-32001）
    let denied = s.request(4, "tools/call", serde_json::json!({"name": "approve_proposal", "arguments": {"proposal_id": 1}}));
    assert_eq!(denied["error"]["code"], serde_json::json!(-32001));

    // 引数のスキーマ違反は JSON-RPC エラー（-32602）
    let bad = s.request(5, "tools/call", serde_json::json!({"name": "search_context", "arguments": {"query": 1}}));
    assert_eq!(bad["error"]["code"], serde_json::json!(-32602));
}

#[test]
fn human_sees_approval_tools() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let mut s = Server::start(dir.path(), "tester");
    let listed = s.request(1, "tools/list", serde_json::json!({}));
    let names: Vec<&str> =
        listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"approve_proposal"));
    assert!(names.contains(&"reject_proposal"));
}
```

- [ ] **Step 3: テストを実行しコミット**

Run: `cargo test -p gaia --tests`
Expected: 3 tests passed（stdio の応答形式が想定と違う場合は、実際の 1 行 JSON をログで確認してアサートを合わせる。契約や core は変えない）

```bash
git add crates/gaia
git commit -m "test: add CLI flow and stdio MCP integration tests" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task 21: 仕上げ（ドキュメント整合・最終検証）

**Files:**
- Modify: `README.md`, `AGENTS.md`（実装とズレた箇所のみ）, `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`（§13 に実績を追記）

**Interfaces:** なし（ドキュメントと検証のみ）

- [ ] **Step 1: README.md に使い方を追記する**

「ビルドとテスト」の下に追記:

````markdown
## セットアップ

```sh
cargo install --path crates/gaia   # または cargo run -p gaia --
gaia init --affiliation <所属元名>              # 設定と DB を作成
gaia client add claude-code --role agent --default-scope <所属元名>
```

## MCP クライアントからの接続（stdio）

`.mcp.json` などに登録する:

```json
{
  "mcpServers": {
    "gaia_library": { "command": "gaia", "args": ["serve", "--stdio", "--client", "claude-code"] }
  }
}
```

## 日常の使い方

```sh
gaia add person --name "岡村 慎太郎" --alias okash1n   # 手入力（提案＋即時承認）
gaia search "Okta"                                     # 回答の設計図を得る
gaia proposals && gaia approve <id>                    # エージェントの提案を承認
```
````

- [ ] **Step 2: AGENTS.md を実装と突き合わせる**

Task 1 で書いた AGENTS.md を読み直し、実装済みの内容（コマンド名・モジュール名・テストコマンド）とズレがあれば直す。特に「未実装の機能を実装済みのように書いていないか」を確認する。

- [ ] **Step 3: 仕様書 §13 の末尾に実績を 2〜3 行で追記する**

計画どおり実装できなかった点・生成型の名前が想定と違った点など、次のサブプロジェクト（B: HTTP＋認証、C: デスクトップ）に効く事実だけを記録する（無ければ「差分なし」と書く）。

- [ ] **Step 4: 最終検証を実行する**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p gaia && ./target/release/gaia --help
./scripts/dev.sh --help >/dev/null 2>&1 || true   # narumi 無しでも動作（スキップメッセージ）を確認
```

Expected: すべて成功。`gaia --help` に全サブコマンドが並ぶ

- [ ] **Step 5: コミット**

```bash
git add README.md AGENTS.md docs
git commit -m "docs: align README/AGENTS.md with implemented foundation" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

## 実行後の続き（このリポジトリの v0.1.0 に向けて）

この計画（サブプロジェクト A）完了後、別設計書・別計画で続ける:

1. **B: HTTP ＋ 認証** — Streamable HTTP トランスポート、bearer キー、human キーのキーチェーン保持、接続設定の生成
2. **C: デスクトップアプリ（Tauri）** — サーバー・DB の起動、検索／閲覧／手入力／承認 UI、tauri-plugin-updater による自動更新、署名・公証・リリーススクリプト（solo-eikaiwa の資産を流用）

v0.1.0 のリリースは C まで揃ってから（デスクトップの updater を初回リリースに含めるため）。

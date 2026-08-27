# gaia-library 基盤 設計書（2026-08-27）

## 1. 概要

gaia-library（ガイアライブラリー）は、仕事の記憶の「思い出し方」を索引として保存し、問い合わせに対して要点と解決可能な参照からなる「回答の設計図」を返すローカル MCP サーバーである。相棒は議事録生成システム narumi だが排他ではなく、Claude Code / Codex など任意の MCP クライアントから使われる。

本書は初回実装（基盤）の設計を定める。上流の構想・確定事項は Notion の次のページにあり、本書はそれらを実装可能な粒度に落としたものである。

- コンテキストサーバー（Context MCP）: `https://app.notion.com/p/cloudnativeinc/Context-MCP-33902d68e260410d875398f8b38aebfb`
- gaia-library AGENTS.md ドラフト: `https://app.notion.com/p/04de76ae2d2e469f94b92f1ed9182d0f`
- 議事録生成システム（MCP ツール契約 v1 を含む）: `https://app.notion.com/p/3963617f2dbe80a9b88bd73153e80ae8`

### 1.1 今回の範囲

- Cargo workspace の雛形、AGENTS.md（CLAUDE.md はシンボリックリンク）、CI
- contracts/（v1 契約 13 ツール分の JSON Schema）と build 時の型生成・スキーマ同梱
- SQLite スキーマ（DDL v1 = Notion の DDL v0 ＋ 本書 §5 の差分）とマイグレーション
- scope 強制・提案キュー・監査ログを含むドメイン層と `ToolService`
- MCP サーバー（stdio トランスポート、ロール別ツール可視性と認可）
- CLI `gaia`（設定・DB 初期化、手入力、承認、サーバー起動）
- テスト（単体・統合）

### 1.2 本書の範囲外（別サブプロジェクトで扱う）

2026-08-27 の追加決定（Notion 未記載）: gaia-library は **Tauri デスクトップアプリを主 UI** とし、アプリ起動だけで DB とサーバーが立ち上がり、検索・閲覧・手入力・承認ができる。CLI / MCP（stdio・HTTP）でも同じ操作ができる。自動更新は tauri-plugin-updater（solo-eikaiwa と同方式）で、`gaia` CLI はアプリに同梱して一緒に更新する。v0.1.0 はこれらを揃えて公開する。本書（サブプロジェクト A: 基盤）はその土台であり、次は別の設計書で扱う。

- サブプロジェクト B: Streamable HTTP トランスポート、API キー（bearer）認証、human キーの OS キーチェーン保持、エージェント向け接続設定の生成
- サブプロジェクト C: デスクトップアプリ（Tauri）。サーバー・DB の起動、検索／人物・案件閲覧／手入力／提案承認の画面、updater、署名・公証・リリーススクリプト、E2E
- `resolve_source`（narumi など外部 MCP の参照解決）。契約ファイルは置くが登録しない
- 形態素解析（lindera-sqlite）、ベクトル検索（sqlite-vec）
- narumi 連携の実機確認（narumi が未実装のため）

CLI 単体の self-update は作らない（アプリの updater に一本化する）。

## 2. 決定事項

### 2.1 Notion で確定済み（本書はそのまま採用）

- 名称: リポジトリ `gaia-library`、MCP サーバー名 `gaia_library`、CLI `gaia`
- 言語: Rust。MCP は rmcp、DB は rusqlite（bundled、FTS5 trigram）、契約からの型生成は typify
- 契約: `contracts/` に 1 ツール 1 JSON Schema ＋共通 defs、`contract_version` は semver。変更は契約 → 実装の順。生成物はコミットしない
- データモデル: ハイブリッド構成。名寄せ層（people / person_aliases / organizations / entities / affiliations）は共有、内容層（engagements / interactions / facts / refs / glossary / proposals）は scope 必須
- facts は statement 必須＋任意の構造化カラム、kind = fact | inference、履歴は superseded_by
- refs はエンティティにもファクトにも紐付く。source / provenance は refs に一本化。URI だけの参照は禁止
- 書き込みは提案キュー経由のみ。承認は human ロールのみ（tool list から隠し、かつサーバー側でも認可）
- scope は default deny / explicit allow。複数 scope 明示時のみ横断し、監査ログに残す
- RAG・埋め込みは入れない
- 公開ツール v1: 参照系 7・提案系 4・共通 2

### 2.2 本セッションで決定

| 項目 | 決定 | 理由 |
| --- | --- | --- |
| エージェント向け指示ファイル名 | `AGENTS.md`（`CLAUDE.md` はシンボリックリンク） | Notion ドラフトどおり。agents.md 規約の標準名 |
| 今回の実装範囲 | 基盤＋全ツールを stdio で提供 | 絶対原則を初回から満たしつつ、HTTP 認証設計を切り離す |
| Cargo 構成 | workspace 3 crate（`gaia-core` / `gaia-mcp` / `gaia`） | 依存方向をコンパイラで強制する |
| 設定・DB の配置 | XDG 配置（macOS でも `~/.config` / `~/.local/share`） | 予測可能で環境変数で上書きしやすい |
| stdio の識別 | 起動引数 `--client` で固定。キー検証は HTTP 実装時に追加 | stdio は接続ごとの情報を持たない（§7.1） |
| DDL 差分 | §5.2 の 6 点 | 構想に書かれていて DDL v0 に欠けていたもの、および FTS 整合 |
| 構造化 predicate の初期レジストリ | `role` / `status` / `interest` / `decision` | 頻出が見えたものだけ後で昇格する方針 |

## 3. 前提: 使用ライブラリと確認済みの事実

調査は crate ソース（`~/.cargo/registry`）と検証用プロジェクトのビルド・実行で確認した。実装はこれらの事実に基づく。

| ライブラリ | 版 | 実装に効く事実 |
| --- | --- | --- |
| rmcp | 3.1.4 | `ServerHandler` を手動実装できる（`list_tools` / `call_tool` / `get_info` / `get_tool`）。`Tool::new_with_raw` で任意の JSON Schema を inputSchema にできる。**引数のスキーマ検証はしない**。stdio では `RequestContext.extensions` が空で接続情報が無い。features は `server` と `transport-io` |
| rusqlite | 0.40.2 | `bundled` で SQLite 3.53.2（FTS5 有効）。`Connection` は `Send` だが `Sync` ではない。`foreign_keys` は接続ごとに有効化が必要 |
| rusqlite_migration | 2.6.0 | `PRAGMA user_version` で管理。MSRV 1.95 |
| jsonschema | 0.51.0 | `validator_for` / `iter_errors`。外部 `$ref` を持たない自己完結スキーマなら `default-features = false` で足りる |
| typify | 0.7.0 | schemars 0.8.22 ベース。外部 `$ref` 非対応（panic）。`$defs` は最上位に集約して `add_ref_types` を 1 回だけ呼ぶ。`if/then/else` / `prefixItems` / `unevaluatedProperties` / `dependentSchemas` は非対応または無視 |
| SQLite FTS5 trigram | 3.53.2 | 3 文字未満の検索語は **エラーにならず空集合**を返す。外部コンテンツ表は同期トリガが必要。`INSERT OR REPLACE` は削除トリガが発火せず索引が壊れる |

## 4. リポジトリ構成

```
gaia-library/
├── AGENTS.md
├── CLAUDE.md -> AGENTS.md
├── README.md
├── LICENSE
├── Cargo.toml                  # [workspace] members = ["crates/*"]
├── rust-toolchain.toml         # channel = "stable"
├── .gitignore
├── .github/workflows/ci.yml
├── contracts/
│   ├── manifest.json
│   ├── defs/common.json
│   └── tools/<tool>.json       # 13 ファイル
├── crates/
│   ├── gaia-core/
│   │   ├── Cargo.toml
│   │   ├── build.rs            # contracts → OUT_DIR（自己完結スキーマ＋typify 型）
│   │   ├── migrations/0001_init.sql
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs        # ErrorCode / ToolError
│   │       ├── config.rs       # Config・パス解決
│   │       ├── identity.rs     # ClientIdentity / Role
│   │       ├── scope.rs        # ScopeSet 解決
│   │       ├── contracts/      # Catalog（ToolSpec・Validator）、生成型の include
│   │       ├── storage/        # Db（open・PRAGMA・migration）、リポジトリ群
│   │       ├── domain/         # normalize / predicates / proposals（適用ロジック）
│   │       └── tools/          # ToolService と 1 ツール 1 モジュール
│   ├── gaia-mcp/
│   │   └── src/{lib.rs, server.rs, stdio.rs}
│   └── gaia/
│       ├── src/main.rs
│       ├── src/cli/            # サブコマンド
│       └── tests/stdio.rs      # バイナリを起動する統合テスト
├── scripts/dev.sh
└── docs/superpowers/specs/
```

依存方向は `gaia`（bin）→ `gaia-mcp` → `gaia-core` の一方向。`gaia-core` は rmcp を知らない。

### 4.1 workspace 設定

- edition 2024、`rust-version = "1.95"`
- `[workspace.dependencies]` で版を一元管理: rmcp 3.1.4（`server`, `transport-io`）、rusqlite 0.40.2（`bundled`, `serde_json`）、rusqlite_migration 2.6、jsonschema 0.51（`default-features = false`）、serde / serde_json、clap 4（derive）、tokio 1（`rt-multi-thread`, `macros`, `io-std`）、tracing / tracing-subscriber、thiserror（lib）、anyhow（bin）、unicode-normalization
- build-dependencies（gaia-core）: typify 0.7、schemars 0.8.22、prettyplease、syn、serde_json
- dev-dependencies: rmcp（`client`, `transport-child-process`）、tempfile、assert_cmd（任意）

### 4.2 .gitignore

`target/`、`*.db`、`*.db-wal`、`*.db-shm`、`.env`。生成物は `OUT_DIR` に出るので対象外。

## 5. データモデル

### 5.1 DDL v1（`migrations/0001_init.sql`）

```sql
-- 名寄せ層（共有・scope なし）
CREATE TABLE affiliations (          -- scope の値域＝機密境界の定義
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,   -- 例: cloudnative
  identity   TEXT,                   -- 使うアイデンティティ・立場メモ
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE organizations (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  kind       TEXT,                   -- customer / partner / affiliation …
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
CREATE TABLE person_aliases (        -- resolve_speakers の突合対象
  person_id  INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  alias      TEXT NOT NULL,          -- 表示名・ローマ字・ニックネーム。正規化済みは kind='normalized' の別行
  kind       TEXT,                   -- display_name / romaji / nickname / normalized …
  PRIMARY KEY (person_id, alias)
);
CREATE TABLE entities (              -- 汎用受け皿
  id         INTEGER PRIMARY KEY,
  type       TEXT NOT NULL,
  name       TEXT NOT NULL,
  attrs      TEXT NOT NULL DEFAULT '{}',   -- JSON
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
CREATE TABLE engagement_people (     -- 差分①: 案件 ↔ 人物（キーパーソン・参加者）
  engagement_id INTEGER NOT NULL REFERENCES engagements(id) ON DELETE CASCADE,
  person_id     INTEGER NOT NULL REFERENCES people(id) ON DELETE CASCADE,
  role          TEXT,                -- key_person / member / contact …
  PRIMARY KEY (engagement_id, person_id)
);
CREATE TABLE interactions (
  id            INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,       -- meeting / call / chat / mail …
  occurred_at   TEXT NOT NULL,
  summary       TEXT NOT NULL,       -- 要点（全文は参照先の正本）
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
  entity_type   TEXT NOT NULL CHECK (entity_type IN ('person','organization','engagement','interaction','entity')),  -- 差分②
  entity_id     INTEGER NOT NULL,    -- polymorphic（整合はアプリ層）
  statement     TEXT NOT NULL,
  predicate     TEXT,
  value         TEXT,
  kind          TEXT NOT NULL CHECK (kind IN ('fact','inference')),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  valid_from    TEXT,
  superseded_by INTEGER REFERENCES facts(id),
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE TABLE refs (                  -- 「思い出し方の索引」の実体
  id            INTEGER PRIMARY KEY,
  target_type   TEXT NOT NULL CHECK (target_type IN ('person','organization','engagement','interaction','entity','fact')),  -- 差分②
  target_id     INTEGER NOT NULL,
  system        TEXT NOT NULL,       -- notion / box / minutes / mail / file / url …
  uri           TEXT NOT NULL,
  title         TEXT,
  note          TEXT NOT NULL,       -- 何が・どの粒度で・いつ時点か（URI だけ禁止）
  snapshot      TEXT,                -- 登録時点の要点
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
CREATE TABLE proposals (             -- 全書き込みの唯一の入口
  id            INTEGER PRIMARY KEY,
  action        TEXT NOT NULL CHECK (action IN ('insert','update','supersede')),
  target_type   TEXT NOT NULL,
  target_id     INTEGER,             -- insert 時は NULL
  patch         TEXT NOT NULL,       -- JSON
  kind          TEXT NOT NULL CHECK (kind IN ('fact','inference')),
  scope         TEXT NOT NULL REFERENCES affiliations(name),
  provenance    TEXT,                -- 差分③: 出所の指定（JSON。既存 ref の id または新規 ref の内容）
  provenance_id INTEGER REFERENCES refs(id),  -- 承認時に確定した ref の id
  proposed_by   TEXT NOT NULL,       -- クライアント名
  request_id    TEXT NOT NULL UNIQUE,
  status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
  result_id     INTEGER,             -- 差分③: 承認で生成・更新した行の id
  decision_note TEXT,                -- 差分③: 却下理由など
  created_at    TEXT NOT NULL DEFAULT (datetime('now')),
  decided_at    TEXT,
  decided_by    TEXT
);
CREATE TABLE audit_log (
  id     INTEGER PRIMARY KEY,
  actor  TEXT NOT NULL,
  action TEXT NOT NULL,              -- propose / approve / reject / cross_scope_read / admin_write
  detail TEXT NOT NULL,              -- JSON
  at     TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_facts_target ON facts(entity_type, entity_id);
CREATE INDEX idx_refs_target  ON refs(target_type, target_id);
CREATE INDEX idx_facts_scope  ON facts(scope);
CREATE INDEX idx_refs_scope   ON refs(scope);
CREATE INDEX idx_alias_lookup ON person_aliases(alias);
CREATE INDEX idx_proposals_status ON proposals(status, scope);

-- 差分⑤: 外部コンテンツ FTS ＋同期トリガ
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

### 5.2 DDL v0 からの差分

1. `engagement_people` を追加。構想の「people.engagements」「engagements.キーパーソン」に対応する結合表が DDL v0 に無い
2. `facts.entity_type` / `refs.target_type` に CHECK 制約
3. `proposals.result_id` / `proposals.decision_note` / `proposals.provenance` を追加。新規 ref を伴う出所指定は承認まで `provenance`（JSON）に保持し、承認時に refs へ書いて `provenance_id` を確定する（承認前に内容層へ書かないため）
4. `organizations` / `entities` / `engagements` / `glossary` / `affiliations` に `created_at` / `updated_at`
5. `facts_fts` の同期トリガ。`INSERT OR REPLACE` は禁止し `ON CONFLICT DO UPDATE` を使う
6. 接続時 PRAGMA: `journal_mode=WAL`、`synchronous=NORMAL`、`foreign_keys=ON`、`busy_timeout=5000`。書き込みトランザクションは `BEGIN IMMEDIATE`

### 5.3 アプリ層の規約

- polymorphic（`entity_type + entity_id`、`target_type + target_id`）の整合は書き込み時に検証し、違反は `invalid_params` で拒否する
- 内容層への全クエリは `ScopeSet` を必ず受け取り、`scope IN (...)` を付ける。scope なしのクエリ関数を作らない
- 「現在の fact」は `superseded_by IS NULL`
- 人物の `name` と各 alias について、§8.3 の正規化を施した文字列を `kind='normalized'` の行として自動登録する（`resolve_speakers` は「正規化した入力 = alias」の完全一致で突合する。`kind='normalized'` の行は出力には含めない）
- `affiliations` は機密境界の定義そのものなので、提案キューではなく CLI の管理コマンド（human）で直接書き込み、`audit_log(admin_write)` に残す。これが提案キュー原則の唯一の例外である

## 6. 契約（contracts/）

### 6.1 ファイル

`contracts/manifest.json`

```json
{
  "contract_version": "1.0.0",
  "server_name": "gaia_library",
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

`contracts/tools/<name>.json` は MCP の Tool オブジェクト 1 つ。

```json
{
  "name": "get_person",
  "title": "人物の詳細取得",
  "description": "...",
  "annotations": { "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false },
  "inputSchema": { "type": "object", "additionalProperties": false, "properties": { "scope": { "$ref": "../defs/common.json#/$defs/ScopeInput" } } },
  "outputSchema": { "type": "object", "properties": { "...": {} } }
}
```

`contracts/defs/common.json` は最上位 `$defs` にのみ型を置く。主な型: `ScopeInput`（string または string[]）、`ScopeName`、`Kind`、`EntityType`、`RefTargetType`、`ProposalAction`、`ProposalStatus`、`ErrorCode`、`ErrorObject`、`Reference`、`Fact`、`PersonSummary`、`OrganizationSummary`、`EngagementSummary`、`GlossaryTerm`、各 target_type の `Patch*`。

### 6.2 スキーマの書き方（typify 制約）

- 使ってよいキーワード: `type` / `properties` / `required` / `additionalProperties` / `enum` / `oneOf` / `items` / `minItems` / `default`（整数のみ）/ `description` / `$ref`（`../defs/common.json#/$defs/X` 形式のみ）
- 使わない: `if/then/else`、`prefixItems`、`unevaluatedProperties`、`dependentSchemas`、`$anchor`、`pattern`、`format`、ツールファイル内の `$defs`。また `minLength` / `minimum` / `maximum` は typify が newtype や `NonZeroU64` を生成して扱いにくくなるため使わず、長さ・範囲の検証はハンドラで行う
- enum はプロパティ内に直書きせず、必ず `$defs` に名前付きで定義して `$ref` する（生成される Rust 型名を安定させるため）
- 入力は `additionalProperties: false`。出力は将来の追加に備えて省略可
- `$defs` の名前は全体で一意にする（1 つのプールに集約するため）

### 6.3 build.rs の処理

1. `contracts/manifest.json` と各ツールファイル、`defs/common.json` を読む（変更検知のため `cargo:rerun-if-changed` を出す）
2. 各ツールの `$ref` を `#/$defs/X` に書き換え、`common.json` の `$defs` を inputSchema / outputSchema に同梱した自己完結スキーマを作る
3. `OUT_DIR/contracts.json` に manifest とツール一覧（自己完結スキーマ入り）を書く
4. typify で `add_ref_types(common.$defs)` を 1 回呼び、各ツールの inputSchema / outputSchema を `add_type_with_name`（`SearchContextInput` / `SearchContextOutput` のように命名）で登録し、`OUT_DIR/contract_types.rs` に出力する
5. スキーマの読み込み失敗や typify の panic はビルドエラーとして扱う（契約の誤りを早く見つける）

### 6.4 実行時

- `gaia_core::contracts::Catalog::embedded()` が `include_str!` した `contracts.json` を読み、ツールごとに `ToolSpec { name, title, description, annotations, roles, enabled, input_schema, output_schema, validator }` を構築する
- `validator` は `jsonschema::validator_for(&input_schema)`。入力は必ずここで検証する
- MCP の `tools/list` に返す inputSchema は自己完結スキーマ（外部 `$ref` を含まない）

## 7. 識別・scope・認可

### 7.1 設定と識別

設定ファイル `~/.config/gaia-library/config.toml`（`GAIA_CONFIG` で上書き）。DB は `~/.local/share/gaia-library/gaia.db`（`GAIA_DB`、または設定 `db_path` で上書き）。

```toml
[cli]
default_client = "okash1n"          # CLI コマンドが使う既定の識別

[[clients]]
name = "okash1n"
role = "human"
default_scope = "cloudnative"

[[clients]]
name = "claude-code"
role = "agent"
default_scope = "cloudnative"
```

- `ClientIdentity { name, role: Human | Agent, default_scope: Option<String> }`
- stdio: `gaia serve --stdio --client <name>` で起動時に固定する。MCP クライアント側の設定（`.mcp.json` 等）には agent クライアント名だけを書く
- CLI の他コマンドは `--client` 省略時に `[cli].default_client` を使う。未設定なら human クライアントが 1 つだけのときそれを使い、それ以外はエラー
- 限界: stdio の役割分離は「エージェントが MCP 経由で誤って承認する」ことを防ぐ仕組みであり、同一 OS ユーザーのシェルから human 識別でプロセスを起動することは防げない。API キー検証は HTTP 実装時に追加する

### 7.2 scope 解決

1. ツール引数 `scope`（string または string[]）が与えられればそれを使う
2. 無ければクライアントの `default_scope`
3. どちらも無ければ `scope_denied`
4. 各 scope 名が `affiliations` に存在しなければ `not_found`
5. 2 つ以上の scope を指定したときだけ横断し、`audit_log(cross_scope_read)` に `{ actor, tool, scopes }` を記録する

内容層を読むすべてのツールがこの手順を通る。

### 7.3 認可

- `list_tools` は `manifest.tools[].roles` にクライアントの role が含まれ、かつ `enabled` のものだけ返す
- `call_tool` でも同じ判定を行い、権限が無ければ `unauthorized`（隠すだけにしない）
- 全書き込み（propose / approve / reject / admin_write）と横断読み取りは actor 付きで `audit_log` に残す

## 8. ToolService と公開ツール

### 8.1 ToolService

`gaia_core::tools::ToolService` が唯一の入口である。CLI も MCP ハンドラもこれだけを呼ぶ。

```rust
pub fn call(&self, client: &ClientIdentity, tool: &str, args: serde_json::Value)
    -> Result<serde_json::Value, ToolError>;
pub fn visible_tools(&self, role: Role) -> Vec<&ToolSpec>;
```

処理順: ツール解決（無ければ `not_found`）→ role 判定（`unauthorized`）→ 入力スキーマ検証（`invalid_params`。違反箇所のパスと理由を `details` に入れる）→ 型付き入力へデシリアライズ → ハンドラ → 型付き出力を JSON に変換（テストビルドでは outputSchema でも検証）。

DB は `Mutex<Connection>` で保持する（`Connection` は `Sync` ではない）。個人 CRM 規模では単一接続で足りる。

### 8.2 エラー

`ErrorCode`: `not_found` / `scope_denied` / `unauthorized` / `invalid_params` / `contract_mismatch` / `conflict` / `busy` / `not_implemented` / `internal`。

MCP への写像:

- プロトコル違反（未知のツール、権限なし、引数のスキーマ違反）: JSON-RPC エラー。code は `-32602`（invalid params）、権限なしは `-32001`。`data` に `{ "code": "<ErrorCode>", "details": ... }`
- 業務エラー（`not_found` / `scope_denied` / `conflict` / `busy` / `not_implemented`）: `CallToolResult` の `isError: true` と `structuredContent: { "error": { "code", "message", "details" } }`

### 8.3 参照系ツール

**search_context**
入力: `query`（string, minLength 1）、`scope?`、`types?`（`person` / `organization` / `engagement` / `entity` / `interaction` / `glossary` の配列。省略時は全種別）、`limit?`（既定 10、最大 50）。
処理:
1. 名寄せ層の名前と alias を `LIKE '%q%'`（正規化した q でも）で照合
2. `facts_fts MATCH`（Unicode 文字数で 3 以上）または `statement LIKE '%q%'`（3 未満）で facts を検索し、scope と `superseded_by IS NULL` で絞る。ヒットした fact の entity をまとめる
3. `interactions.summary` と `glossary.term / definition` を `LIKE` で照合（scope 内）
4. エンティティごとに facts（scope 内、現在のもの）と refs（エンティティ直付け＋その facts に付いたもの）を集める
5. スコア: 名前一致 3、alias 一致 2、fact ヒット 1（bm25 順）。上位 `limit` 件
出力: `{ query, scopes, cross_scope, entities: [{ type, id, name, summary, score, matched_on, facts: [Fact], refs: [Reference] }], glossary: [GlossaryTerm], interactions: [InteractionSummary], hints: [string] }`。`hints` には「3 文字未満のため部分一致で検索した」などの注記を入れる。

**get_person** — 入力 `person_id?` または `name?`（どちらか必須。両方無ければ `invalid_params`）、`scope?`。出力: 人物、aliases、所属組織、関わる案件（`engagement_people`）、facts、refs、直近の interactions（scope 内、最大 20）。名前が複数該当なら `conflict` で候補一覧を返す。

**get_organization** — 入力 `organization_id?` / `name?`、`scope?`。出力: 組織、所属する people、案件（scope 内）、facts、refs。

**get_engagement** — 入力 `engagement_id?` / `name?`、`scope?`。出力: 案件、相手組織、関係者（役割付き、aliases 込み）、facts、refs、用語集、直近の interactions。案件自体が scope 外なら `not_found`（存在を漏らさない）。

**get_glossary** — 入力 `engagement_id?`、`scope?`。`engagement_id` 省略時は scope 内の全用語。出力 `{ terms: [GlossaryTerm], vocabulary_hints: [string] }`。`vocabulary_hints` は用語＋案件関係者の名前と alias を平坦化した配列で、Whisper の `initial_prompt` にそのまま使える。

**resolve_speakers** — 入力 `display_names`（string[], minItems 1）、`scope?`、`engagement_id?`。
正規化: NFKC → 前後空白除去 → 小文字化 → 括弧とその中身を除去（`(CloudNative)` など）→ 敬称（さん / 様 / 氏 / san）除去 → 空白除去。
突合: 正規化した名前で `person_aliases.alias` を完全一致（`kind='normalized'` 行）。1 件なら `matched`（confidence 1.0）、複数なら `ambiguous`（候補列挙）、0 件なら前方一致・部分一致で候補を探し `unmatched`（候補があれば confidence 0.6 以下で列挙）。`engagement_id` があれば `engagement_people` に含まれる人物を優先する。
出力 `{ results: [{ input, normalized, status, person?, confidence, candidates: [{ person_id, name, confidence, reason }] }] }`。

**resolve_source** — 契約のみ。`enabled: false` のため登録しない。

### 8.4 提案系ツール

**propose_update** — 入力: `target_type`（`person` / `organization` / `engagement` / `interaction` / `entity` / `fact` / `ref` / `glossary`）、`action`（`insert` / `update` / `supersede`）、`target_id?`、`patch`（target_type ごとの `Patch*`。契約上は自由なオブジェクトで、ハンドラが target_type に応じて型付きで検証する）、`kind`、`scope?`（省略時はクライアントの既定 scope。単一の scope 名）、`provenance?`（`{ ref_id }` で既存 ref を指すか、`{ system, uri, title?, note, snapshot? }` で新規 ref を承認時に同時登録。新規 ref の紐付け先は承認で生成・更新されたレコードなので、`ref` / `glossary` を対象とする提案では `ref_id` 形式のみ受け付ける）、`request_id`（string。8 文字以上かつ UTF-8 で 256 bytes 以下をハンドラで検証）。
処理: `request_id` が既存なら、`proposed_by` と送信内容（`target_type` / `action` / `target_id` / `patch` / `kind` / `scope` / `provenance`）がすべて一致するときだけ既存の提案を `duplicate: true` で返す。所有クライアントまたは送信内容が異なる場合は `conflict` とし、`audit_log(propose_conflict)` に残す。それ以外は `pending` で登録し、`audit_log(propose)`。`patch` と `provenance` の JSON 直列化後の合計は 1 MiB 以下、同一クライアント・scope の未決提案は 1,000 件未満とし、完全一致の再送判定はこの上限判定より先に行う。`provenance` は既存 ref の id ならその存在を確認して `provenance_id` に入れ、新規 ref の内容なら `provenance`（JSON）に保持して承認時に refs へ書く。
出力 `{ proposal_id, status, duplicate }`。

`Patch*` の形:

| target_type | insert 時の patch | update 時の patch |
| --- | --- | --- |
| person | `name`, `org_id?`, `role?`, `aliases?: [{ alias, kind? }]`, `first_met?`, `last_seen?` | 左の任意部分集合（`aliases` は追加のみ） |
| organization | `name`, `kind?` | 任意部分集合 |
| engagement | `name`, `org_id?`, `status?`, `started_at?`, `ended_at?`, `people?: [{ person_id, role? }]` | 任意部分集合（`people` は追加のみ） |
| interaction | `kind`, `occurred_at`, `summary`, `engagement_id?`, `person_ids?` | 任意部分集合 |
| entity | `type`, `name`, `attrs?` | 任意部分集合 |
| fact | `entity_type`, `entity_id`, `statement`, `predicate?`, `value?`, `valid_from?` | `statement?`, `predicate?`, `value?`, `valid_from?`（`supersede` は insert と同じ形で、`target_id` が旧 fact） |
| ref | `target_type`, `target_id`, `system`, `uri`, `title?`, `note`, `snapshot?`, `last_verified?` | `system?`, `uri?`, `title?`, `note?`, `snapshot?`, `last_verified?`（`target_type` / `target_id` は不変） |
| glossary | `term`, `reading?`, `definition?`, `engagement_id?` | 任意部分集合 |

`supersede` は `fact` のみ。`person` / `organization` / `entity` は名寄せ層なので `scope` は提案の文脈（監査用）としてのみ使い、レコードには保存しない。

**list_proposals** — 入力 `status?`（既定 `pending`）、`scope?`、`limit?`。出力 `{ proposals: [{ id, action, target_type, target_id, patch, kind, scope, provenance, proposed_by, request_id, status, result_id, created_at, decided_at, decided_by, decision_note }] }`。

**approve_proposal**（human のみ）— 入力 `proposal_id`、`scope?`。scope は明示指定 → クライアント既定値の順で解決し、どちらも無ければ `scope_denied`。scope 外の提案は `not_found` とする。1 トランザクションで: 提案が `pending` であること → `patch` を型付きで再検証 → polymorphic 整合（参照先の存在）→ predicate 規則（§8.6）→ 適用（insert / update / supersede）→ `result_id`、`status='approved'`、`decided_by`、`decided_at` を記録 → `audit_log(approve)`。検証に失敗した場合は提案を `pending` のまま残し、エラーを返す。複数 scope の明示指定は `audit_log(cross_scope_read)` に残し、この読み取り監査は適用失敗時にも保持する。
出力 `{ proposal_id, status, result: { target_type, id } }`。

**reject_proposal**（human のみ）— 入力 `proposal_id`、`scope?`、`reason?`。scope の解決・遮断・横断監査は承認と同じ。出力 `{ proposal_id, status }`。

### 8.5 共通ツール

**get_server_info** — 入力なし。出力 `{ name: "gaia_library", version, contract_version, protocol: { transports: ["stdio", "http"] }, capabilities: { tools: [可視ツール名], resolvers: [], search: { fts: "trigram" } }, client: { name, role, default_scope } }`。

**get_job_status** — 入力 `job_id`。v1 にはジョブが無いので常に `not_found`。契約上は narumi 側と共通規約を揃えるために置く。

### 8.6 predicate 規則

`domain::predicates` に初期レジストリ `role` / `status` / `interest` / `decision` を定義する。承認時:

- `predicate` がレジストリにあれば `value` を必須にする
- `predicate` がレジストリに無ければ `invalid_params`（自由文の `statement` のみで登録し直す）
- `predicate` が無い提案は `statement` のみで受け入れる

## 9. MCP 層（gaia-mcp）

- `GaiaServer { service: Arc<ToolService>, identity: ClientIdentity }` が `rmcp::ServerHandler` を手動実装する
  - `get_info`: `InitializeResult::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(Implementation::new("gaia_library", CARGO_PKG_VERSION)).with_instructions(使い方の要約)`
  - `list_tools`: `service.visible_tools(identity.role)` を `Tool::new_with_raw(...)` に変換し `ListToolsResult::with_all_items`
  - `call_tool`: `service.call(&identity, name, args)` の結果を §8.2 の写像で `CallToolResponse` に変換
  - `get_tool`: 可視ツールを返す
- `serve_stdio(server)`: `server.serve(rmcp::transport::stdio()).await?.waiting().await`。ログは stderr（stdout は JSON-RPC 専用）
- ツール名や引数の解釈を gaia-mcp に置かない。JSON の受け渡しと結果の写像だけを担当する

## 10. CLI `gaia`（gaia crate）

すべてのコマンドは `ToolService::call` を経由する。例外は設定ファイルと `affiliations` を扱う管理コマンドのみ。

| コマンド | 内容 |
| --- | --- |
| `gaia init --affiliation <name> [--client <name>] [--db <path>]` | 設定ファイルと DB を作成し、マイグレーション適用、最初の affiliation と human クライアントを登録 |
| `gaia serve --stdio --client <name>` | MCP サーバーを stdio で起動 |
| `gaia affiliation add <name> [--identity <text>]` / `list` | 機密境界の管理（human、`audit_log(admin_write)`） |
| `gaia client add <name> --role <human\|agent> [--default-scope <name>]` / `list` | 設定ファイルのクライアント管理 |
| `gaia search <query> [--scope ...] [--type ...]` | `search_context` |
| `gaia person get [--id <id>\|--name <name>]` / `org get` / `engagement get` / `glossary [--engagement-id <id>]` | 参照系 |
| `gaia speakers <name>...` | `resolve_speakers` |
| `gaia propose <target_type> <insert\|update\|supersede> --patch <json> [--target-id] [--kind] [--scope] [--provenance <json>]` | `propose_update`（`request_id` は自動発番） |
| `gaia proposals [--status ...] [--scope ...]` / `gaia approve <id> [--scope ...]` / `gaia reject <id> [--scope ...] [--reason ...]` | 提案の一覧・承認・却下 |
| `gaia add person --name ... [--alias ...]...` / `add org` / `add engagement` / `add fact` / `add ref` / `add glossary` / `add interaction` | human 限定の「提案＋即時承認」を 1 コマンド化 |
| `gaia call <tool> --args <json>` | 任意ツールの汎用呼び出し |
| `gaia info` | `get_server_info` |

出力は既定で整形済み JSON（人が読む用）、`--json` で 1 行の JSON（機械処理用）。ログは `tracing` で stderr、`RUST_LOG` で制御。

`init` は同じ設定への初期化を排他制御し、既存設定を上書きしない。設定保存に失敗した後の再試行では、同じ affiliation と identity の登録を無変更で再利用する。`add --scope` は提案と即時承認の両方へ同じ scope を渡す。

## 11. テスト

### 11.1 単体（gaia-core）

- contracts: manifest の全ツールファイルが読めて Validator を構築できる。生成型で代表的な入出力が往復できる。`enabled` なツールにはすべてハンドラがあり、ハンドラにはすべて契約がある
- storage: `Migrations::validate()`。PRAGMA が期待どおり。FTS の rank=1 `integrity-check` が挿入・更新・削除後に通る
- domain: 正規化（NFKC・括弧・敬称・空白）。predicate 規則。提案の適用（insert / update / supersede、polymorphic 整合違反の拒否）
- scope: 省略時の既定 scope、未知 scope、複数 scope での監査ログ記録
- 認可: agent には承認系が見えず、直接呼んでも `unauthorized`
- 各ツール: fixture を投入した in-memory DB でハンドラを直接呼ぶ。`search_context` は 3 文字未満の LIKE フォールバックを含む

### 11.2 統合（gaia crate）

- `gaia init` → `gaia add person` → `gaia search` が一時ディレクトリで通る
- `gaia serve --stdio --client agent` を子プロセスで起動し、rmcp クライアント（`transport-child-process`）から `initialize` / `tools/list` / `tools/call search_context` を実行する。human クライアントで起動すると `approve_proposal` が見える
- narumi 無しで全テストが通る

### 11.3 CI

GitHub Actions（ubuntu-latest / macos-latest）: `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。

## 12. AGENTS.md の内容

Notion ドラフトを基に、本セッションの決定（workspace 構成、依存方向、識別モデル、DDL 差分、契約の書き方、テストとコミット規約）を追記する。次を必ず含める。

- 3 crate の役割と依存方向。`gaia-mcp` と `gaia` は `ToolService` 以外の core API を直接使わない
- 契約変更の手順: `contracts/` を直す → `cargo build` で型とスキーマが更新される → 実装を直す。破壊的変更は `contract_version` を上げる
- 書き込みは提案キュー経由のみ。`affiliations` の管理コマンドが唯一の例外
- scope なしの内容層クエリを書かない
- FTS の規則（`INSERT OR REPLACE` 禁止）
- テストコマンドと「narumi 無しで全テストが通る」こと
- コミット規約: Conventional Commits、`Co-Authored-By` は付けない

## 13. リスクと未検証事項

- typify の制約により契約の表現力が draft-07 相当に限定される。narumi 側の datamodel-code-generator とも整合する見込みだが、narumi 実装時に再確認が必要
- `resolve_source` が未登録のため、v1 契約 13 ツールのうち 1 つは今回動かない
- stdio の識別固定の限界（§7.1）
- FTS trigram は日本語の同義・活用に弱い。検索が実際に失敗し始めたら lindera-sqlite への格上げを検討する
- `Mutex<Connection>` の単一接続は、HTTP で同時接続が増えた場合に見直す可能性がある
- typify の生成コードが `regress` などの追加依存を要求しないよう、契約に `pattern` を使わない運用を守る必要がある

### 実装実績（2026-08-27、サブプロジェクト A 完了時点）

計画からの逸脱は軽微で、次の 3 点のみ: (1) `build.rs` ほか計 5 箇所で clippy 起因の let-chain / `assert!` / ptr_arg 修正、(2) `const MIGRATIONS` を named-const-slice 経由に変更、(3) CLI `reject` で `--reason` 省略時に null を送らずキー自体を省略する対応。typify が契約から生成した型は想定どおりで、テスト側の追加調整は不要だった。B（HTTP＋認証）・C（デスクトップ）に影響する設計上の齟齬はなし。

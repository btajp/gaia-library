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
- 依存方向は gaia → gaia-mcp → gaia-core の一方向。gaia-mcp と gaia は `ToolService` / `contracts` / `config` / `identity` / `admin` / `storage::Db` / `error` 以外の core API を使わない

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
- 参照系（readOnlyHint）: search_context / get_person / get_organization / get_engagement / get_glossary / resolve_speakers（実装済み・登録済み）/ resolve_source（契約のみ。未登録）
- 提案系: propose_update / list_proposals / approve_proposal（human）/ reject_proposal（human）
- 共通: get_server_info / get_job_status（v1 は常に not_found）
- 書き込みはクライアント発番の request_id（8 文字以上・256 bytes 以下）と送信内容の完全一致で冪等化（不一致の再利用は conflict）。提案 JSON は 1 MiB 以下、同一クライアント・scope の未決提案は 1,000 件未満。エラーは構造化コード（not_found / scope_denied / unauthorized / invalid_params / contract_mismatch / conflict / busy / not_implemented / internal）

## 開発ルール
- テスト: `cargo test --workspace`。lint: `cargo fmt --all --check` と `cargo clippy --workspace --all-targets -- -D warnings`
- `scripts/dev.sh` は narumi の HTTP server をサブプロセス起動する（`NARUMI_BIN` 未設定ならスキップ、port は `NARUMI_PORT`、既定 8765）。narumi 無しでも全テストが通ること（任意依存）
- 契約の書き方: `contracts/tools/<name>.json` は MCP の Tool オブジェクト 1 つ。共通型は `../defs/common.json#/$defs/X` で参照。`minLength` / `minimum` / `maximum` / `pattern` / `format` / `if` / `prefixItems` は使わない（typify の制約）。enum は必ず `$defs` に定義する
- FTS: `INSERT OR REPLACE` 禁止（`ON CONFLICT DO UPDATE` を使う）。同期はトリガで行う
- 検索は person_aliases の完全一致（正規化済み別行）＋ facts_fts（trigram。3 文字未満は LIKE）の併用
- ログは stderr のみ（stdout は JSON-RPC 専用）
- コミット: Conventional Commits。`Co-Authored-By` は付けない

# gaia-library（ガイアライブラリー）

仕事の記憶の索引 MCP サーバー。実記憶の全文ではなく「思い出し方」を保存し、問い合わせには要点＋解決可能な参照からなる「回答の設計図」を返す。相棒は narumi（議事録生成）だが排他ではなく、Claude / Codex など任意のクライアントから使われる。

設計の正本: `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`

## 技術スタック
- Rust（edition 2024、MSRV 1.95）。MCP は公式 rust-sdk（rmcp 3.x）、DB は rusqlite（bundled。FTS5 trigram）。検索の格上げ経路は lindera-sqlite（形態素解析。実際に検索が失敗し始めるまで入れない）
- contracts/: JSON Schema（1 ツール 1 ファイル＋共通 defs、contract_version=semver）。契約が正本。変更は契約 → `cargo build` → 実装の順。型生成は typify（build.rs。生成物はコミットしない）
- トランスポート: ローカル常駐 Streamable HTTP（`gaia serve --http`、127.0.0.1 限定）と stdio（`gaia serve --stdio --client <name>`）
- デスクトップ: Tauri 2 / React / TypeScript / Vite / Tailwind。Bun と Tauri CLI の版は `toolchain.json`。v0.1 は Apple Silicon macOS のみ

## リポ構成（workspace）
- crates/gaia-core: 契約ロード（`contracts::Catalog`）・SQLite（`storage`）・ドメイン（`domain`）・scope（`scope::ScopeSet`）・`tools::ToolService`。rmcp を知らない
- crates/gaia-mcp: rmcp の `ServerHandler` 手動実装、stdio / HTTP 起動、Bearer 認証とセッション所有者の検査、narumi 解決器（`sources::narumi`。rmcp の client + transport-child-process）と `sources::registry`（file / url / narumi を組み立てる唯一の場所）。プロジェクト内依存は gaia-core のみ
- crates/gaia: CLI `gaia`。全コマンドが `ToolService::call` を呼ぶ（設定ファイルと `affiliations` の管理コマンドだけが例外）
- 依存方向は gaia → gaia-mcp → gaia-core の一方向。gaia-mcp と gaia は `ToolService` / `contracts` / `config`（`SourcesConfig` を含む）/ `identity` / `auth` / `admin` / `storage::Db` / `error` / `sources`（`SourceResolver` / `SourceRegistry` / `SourceSettings` / `ProtectedPaths` / `Reason` / `Note`）以外の core API を使わない。CLI / desktop は解決器を `gaia_mcp::sources::registry` 経由でのみ注入する（`ToolService::with_sources`）
- gaia-core はネットワークを ureq（`=3.4.0` 固定、rustls）、URL を url で扱い、rmcp と tokio を知らない
- desktop/src-tauri: workspace 外の独立 Cargo プロジェクト。gaia-core / gaia-mcp を path 依存で使う。core の利用境界は CLI と同じ。UI は desktop/ui にあり、データ操作は Tauri commands 経由のみ

## データモデル（DDL v1）
- 名寄せ層（共有・scope なし）: people / person_aliases / organizations / entities / affiliations（scope の値域定義）
- 内容層（scope NOT NULL）: engagements / engagement_people / interactions / facts / refs / glossary / proposals。規則は「名寄せは共有、内容は境界内」
- facts: statement（自由文）必須＋predicate / value は任意構造化。kind = fact | inference を必ず区別。上書き・履歴は superseded_by。構造化 predicate の初期レジストリは role / status / interest / decision（`domain::predicates`）。レジストリ外の predicate は拒否し、自由文のみで登録する
- refs: エンティティにもファクトにも紐付け可（polymorphic）。source / provenance は refs に 1 系統化。参照は自己記述的に（system / uri / title / note / snapshot / last_verified）。URI だけの参照は登録禁止
- polymorphic（entity_type＋entity_id / target_type＋target_id）の整合はアプリ層で書き込み時に検証する
- 人物の name と alias は正規化した文字列（`domain::normalize::normalize_name`）を kind='normalized' の行として自動登録する

## 絶対原則
1. 書き込みは提案キュー（propose_update）経由のみ。承認（approve / reject）は human ロールのみ — manifest の roles で tool list から隠し、かつ `ToolService::call` でも認可チェックする。唯一の例外は affiliations の管理コマンド（機密境界の定義そのものなので `admin` 経由で直接書き、audit_log(admin_write) に残す）。`resolve_source` は DB に書かない（`last_verified` も更新しない）。唯一の書き込みは複数 scope 明示時の audit_log(cross_scope_read)
2. scope は default deny / explicit allow。内容層の SELECT には必ず `scope IN (SELECT value FROM json_each(?))` を付ける。複数 scope の明示指定時のみ横断し、横断は audit_log(cross_scope_read) に記録。省略時はクライアントの default_scope を使う
3. 参照は必ず解決可能に: 辿れない参照だけを返すサーバーにしない。到達不能時は要点スナップショットをフォールバックに。到達可能性 > 正本一元化（二重管理は許容）
4. RAG・埋め込みは入れない（FTS で実際に困るまで）
5. stdio の役割分離は「エージェントが MCP 経由で誤って承認する」ことを防ぐ仕組みであり、同一 OS ユーザーのシェルから human 識別で起動することは防げない。HTTP は全リクエストで Bearer を検証し、セッションをクライアント名に結び付ける。キーはハッシュだけを設定に保存し、再発行はサーバー再起動なしで反映する

## 公開ツール（v1 契約、contracts/manifest.json が正本）
- 参照系（readOnlyHint）: search_context / get_person / get_organization / get_engagement / get_glossary / resolve_speakers / resolve_source（すべて実装済み・登録済み。resolve_source は v0.2.0・契約 1.1.0）
- resolve_source: `ref_id` または `uri` で登録済みの参照を特定し（`uri` は検索キーで、実効 scope 内の最新 1 件。実体化は常に DB 行の `reference.uri`）、`system` 別の解決器（core: `file` / `url`、mcp: `narumi`）で本文を `content` に返す。既定は全解決器無効（`[sources]`）。失敗は `resolved=false` ＋ 固定文言の `reason`（上流のメッセージ・パス・IP・コマンドを含めない）で、参照と `snapshot` は必ず返す。参照不在は scope 外と同一文言の not_found。同時実行上限は busy。`get_server_info.capabilities.resolvers` は設定済み解決器名
- 契約版の規則: ツールの enabled 化・description 変更は minor（1.0.0 → 1.1.0）。入出力の非互換変更は major
- 提案系: propose_update / list_proposals / approve_proposal（human）/ reject_proposal（human）。承認・却下も scope 指定またはクライアント既定値が必須
- 共通: get_server_info / get_job_status（v1 は常に not_found）
- 書き込みはクライアント発番の request_id（8 文字以上・256 bytes 以下）と送信内容の完全一致で冪等化（不一致の再利用は conflict）。提案 JSON は 1 MiB 以下、同一クライアント・scope の未決提案は 1,000 件未満。エラーは構造化コード（not_found / scope_denied / unauthorized / invalid_params / contract_mismatch / conflict / busy / not_implemented / internal）

## HTTP 接続とキー管理

- キー発行: `gaia client add <name> --role agent --default-scope <scope> --generate-key` または `gaia client keygen <name>`。平文は stdout に 1 行、config の `[keys]` にはハッシュのみ保存する
- 起動: `gaia serve --http --port <N>`。未指定時は `[server].port`、それもなければ 4111〜4114 を試す。`--port 0` は空きポートを選ぶ
- 改名: `gaia client rename <old> <new>`（desktop は設定画面の「名前を変更…」）。`Config::rename_client` が `Config::update` の lock 内で `[[clients]].name` / `[cli].default_client` / `[keys]` の参照だけを付け替える（キーのハッシュは不変で HTTP のキーは有効なまま。stdio の接続設定は `--client <new>` で出し直す）。DB の履歴（proposals の proposed_by / decided_by、audit_log の actor）は書き換えない。desktop は Keychain / 退避ファイルの保管キー（クライアント名が鍵）も新名へ移し（CLI の rename は移さない）、旧名の保管項目を削除できなかった場合は警告を出す。human は設定を都度読み直すため改名後の承認は新名で記録される。接続中の HTTP セッションは改名で一度 404 になり、同じキーの initialize で新名に復帰する。クライアント名の制御文字は `add_client` / `rename_client` が拒否する（load は拒否しない）
- キー平文は `gaia_<接頭辞>_<32 桁 hex>`。接頭辞はクライアント名から Bearer token に使える ASCII（英数字と `-._~+/`）だけを残した最大 64 文字（空なら `client`）で、識別には使わずハッシュ照合だけで行う。元のクライアント名は変更しない
- 設定出力: `gaia client mcp-config <name> --transport http --port <N> --key-stdin`（キーは標準入力で渡す。互換用の `--key <key>` は履歴・プロセス引数へ露出するため非推奨）。現在のキーと固定・非ゼロポートが必要で、起動側と同じポートを使う。stdio の設定出力は使用中の config / DB の絶対パスを含む
- CLI の HTTP サーバーは `AuthTable::from_path` を使い、設定をリクエストごとに読み直す。再発行後は次のリクエストから旧キーを拒否し、読み込み失敗時も認証を拒否する。受理済みリクエストは強制終了しない
- 設定検証は fail-closed。`[keys]` に 1 件でも不正ハッシュ（64 桁 hex 以外）・未登録クライアント・重複ハッシュがあると `Config::load` が失敗し、HTTP 認証は全クライアントで停止、`gaia client keygen` / `client add` も失敗する。復旧は `[keys]` の手編集: 不正ハッシュ・未登録名の行は削除する（未登録名を使い続けるなら `gaia client add` で登録してから発行する）。重複ハッシュは同じ平文キーの共有なので、エラーに出た両方の行を削除し、両方のクライアントで `gaia client keygen <name>` を実行して別々のキーにする。編集後は設定ファイルが `0600` のままか確認する
- human キーは既定で発行しない。接続設定に含まれる平文キーはコミットしない
- `[sources]` も呼び出しごとに読み直す（`ConfigFileSettings`）。agent キーで `resolve_source` を呼ぶと human の機械で設定済みのファイル読取・外向き HTTP・narumi 子プロセス起動が起きるので、`[sources]` は狭く保つ（`allow_hosts = ["*"]` は持ち出し経路になり得る）

## 参照解決（resolve_source）

- 解決器は `refs.system`（trim ＋ ASCII 小文字化）で選ぶ登録簿方式。`file` / `url` / `narumi` 以外（`minutes` / `notion` / `box` など）は解決器なしで `resolved=false`
- `url`: http / https の GET のみ。`[sources.url].allow_hosts`（`*` か FQDN）の明示 allow、userinfo 付き・`localhost`・単一ラベル・末尾ドットを拒否、ループバック・プライベート・リンクローカル・メタデータ・予約アドレスを DNS 解決後の全アドレスでも拒否（`sources::net::ip_is_public`）、リダイレクトは `max_redirects` 上限で各段を再検査、プロキシ環境変数と圧縮伸長を使わず Cookie / Authorization を送らない。text 系 Content-Type（charset は utf-8 のみ）と `max_bytes` に限る。HTML は変換せず注記
- `file`: `[sources.file].roots` 配下の通常ファイルのみ。symlink を辿った実体パスで判定し、`O_NOFOLLOW` で開いたハンドルを検査、NUL / 非 UTF-8 はバイナリとして拒否。設定ディレクトリ・DB ディレクトリ・デスクトップのキー退避ディレクトリ（`ProtectedPaths`）は root に指定しても常時拒否。不在・root 外・種別不可・権限不足は同一文言
- `narumi`: 起動コマンドは `[sources.narumi]`（command は絶対パス / args / timeout_secs / stderr / env）からのみ読み、ツール引数では指定できない。1 呼び出し = 1 子プロセス（起動 → initialize → get_minutes → cancel → プロセスグループ kill）。子の stdout は MCP 専用、stderr は既定で破棄。initialize 応答の `serverInfo.name` が `narumi` でなければ呼ばない。narumi へは参照行の scope 1 つだけを渡す。URI 規約は `narumi://meeting/<meeting_id>[?version=<n>][#fragment]`（設計書 `2026-08-29-gaia-library-resolve-source-design.md` §10 が正本）
- `ToolService::call` は同期。MCP サーバー（`GaiaServer::call_tool`）とデスクトップ（`commands::call_tool`）は全ツール一律 `spawn_blocking` で呼び、解決器ごとの同時実行上限（file 4 / url 2 / narumi 1）で占有を抑える。参照の特定後に DB ロックを手放してから解決する
- content は `[sources].max_content_chars` で切り詰め、BOM と C0 制御文字（`\t` `\n` `\r` を除く）を除去する。注記（切り詰め・版・話者不明など）は `reason` に `; ` 連結。URI と content はログに出さない

## デスクトップとリリース

- `desktop/build-app.sh` は UI と host target の CLI をビルドして同梱する。初回はこの処理の後で `desktop/src-tauri` の build / fmt / clippy / test を実行する
- データ変更は `ToolService` の提案・承認経由。設定と affiliations 管理だけを専用 commands に置く。設定ファイルを都度読み直し、アプリ内の設定更新・初回設定・終了処理を直列化する
- 設定ファイルの新規公開は `Config::create_with`、更新は `Config::update` を使う（設定ファイルの隣の `.lock` で CLI の `gaia init` / `gaia client` とも直列化する）。`Config::load` → 変更 → `save` の直書きはしない。ロックを通らない直接編集は直列化の対象外
- 設定ファイルが symlink の場合はリンクを残してリンク先（最大 40 段）を置換し、`.lock` と一時ファイルはリンク先の隣に作る。他ユーザー所有の symlink は辿らない。`.lock` が symlink なら開かない。拒否する操作（既存パスへの `create_with`、到達不能な設定への `update`）はリンク先のディレクトリや `.lock` を作らない。設定ファイル（リンク先を含む）は本人だけが書けるディレクトリに置く
- 平文 API キーは Keychain を優先し、失敗時のみ 0700 ディレクトリ内の 0600 ファイルへ保存する。現在の設定ハッシュと一致しない保管キーは接続設定へ出さない。キー・スニペットをログや localStorage に保存しない
- CLI リンクは設定画面からの明示操作のみ。新設と確認済みリンクの置換を区別し、確認したリンク先との一致を実行時・退避後にも検査する。通常ファイルを上書きせず、競合時は元の項目または復旧用の退避物を残す
- updater 秘密鍵 `~/.tauri/gaia-library-updater.key` は上書き禁止・バックアップ必須。公開鍵だけを Tauri 設定に含める
- `verify-updater-signature` は `updater-verifier` feature でのみ有効にする。検査時は feature を有効にし、`cargo tauri build` には渡さず配布アプリから除外する
- リリースは push 済みの clean な `main` から `scripts/release-desktop.sh <version>`。タグと GitHub Release はスクリプトが作成する。公開実行はユーザーの指示を確認する
- 鍵ローテーションは `--allow-pubkey-rotation` の橋渡しリリースのみ。生成物は旧鍵で署名し、アプリ内に次の公開鍵を入れる
- updater の実機 E2E は `desktop/e2e-updater/README.md`。ローカル `.app` の生成・署名テストと、Developer ID 公証・実更新・配布確認は区別して報告する

## 開発ルール
- テスト: `cargo test --workspace`。lint: `cargo fmt --all --check` と `cargo clippy --workspace --all-targets -- -D warnings`
- desktop のゲート: `desktop/build-app.sh` → `desktop/src-tauri` の `cargo build --all-features --locked` / `cargo fmt --check` / `cargo clippy --all-targets --all-features --locked -- -D warnings` / `cargo test --all-features --locked`。UI は `desktop/ui` で `bun test`、リリース補助はリポジトリ直下で `bun test scripts`
- `scripts/dev.sh` は narumi の HTTP server をサブプロセス起動する（`NARUMI_BIN` 未設定ならスキップ、port は `NARUMI_PORT`、既定 8765）。narumi 無しでも全テストが通ること（任意依存）。narumi 解決器のテストは偽 narumi `crates/gaia-mcp/src/bin/fake_narumi.rs`（`FAKE_NARUMI_MODE` で挙動切替。配布物には含めない）で行い、実 narumi の確認は手動。url 解決器のテストはループバックの固定応答サーバーで行い、外向きネットワークは使わない
- ureq は `unversioned::resolver`（semver 対象外）で DNS 解決後の検査を差し込むため `=3.4.0` に固定する。更新時は `GuardedResolver` のテストで壊れを検知する
- 契約の書き方: `contracts/tools/<name>.json` は MCP の Tool オブジェクト 1 つ。共通型は `../defs/common.json#/$defs/X` で参照。`minLength` / `minimum` / `maximum` / `pattern` / `format` / `if` / `prefixItems` は使わない（typify の制約）。enum は必ず `$defs` に定義する
- FTS: `INSERT OR REPLACE` 禁止（`ON CONFLICT DO UPDATE` を使う）。同期はトリガで行う
- 検索は person_aliases の完全一致（正規化済み別行）＋ facts_fts（trigram。3 文字未満は LIKE）の併用
- CLI / MCP のログは stderr のみ。stdio serve の stdout は JSON-RPC 専用、CLI の stdout はコマンド結果（JSON・発行キー）専用。desktop は stderr とアプリのログディレクトリを使用し、秘密情報を記録しない
- コミット: Conventional Commits。`Co-Authored-By` は付けない

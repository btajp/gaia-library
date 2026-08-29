# gaia-library サブプロジェクト D: resolve_source（参照のサーバー側解決）設計書（2026-08-29）

## 1. 概要

v0.1 で契約だけを置いていた `resolve_source` を v0.2.0 で登録し、`search_context` などが返す参照（refs）の実体をサーバー側で取得して返す。解決器は参照の `system` 値で選ぶレジストリ方式とし、gaia-core が `file` / `url`、gaia-mcp が `narumi`（設定したコマンドを子プロセスとして起動し MCP の `get_minutes` を呼ぶ）を持つ。

- 前提: `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`（A。§1.2 / §8.3 で「契約のみ・未登録」としていた部分を本書が引き受ける）、同 `-http-auth-design.md`（B）、同 `-desktop-design.md`（C）
- 本書の範囲: 契約の有効化、解決器レジストリ、`file` / `url` / `narumi` 解決器、設定 `[sources]`、同期 `ToolService::call` の非同期サーバーへの載せ方、CLI `gaia resolve`、デスクトップの参照カードからの取得、narumi 向け参照 URI 規約、テスト、文書、版上げ
- 範囲外: HTML の本文抽出、文字コード変換、Notion / Box など SaaS への解決器、デスクトップの `[sources]` 設定画面と narumi 接続テスト（v0.2.x 以降で検討）、narumi 側のエクスポータ変更（narumi リポジトリへの依頼事項として §10 に記す）

本書は 3 つの設計案（minimal / risk / user）の判定結果を統合したもの。骨格は risk 案（`SourceRegistry` と呼び出しごとの設定再読込、固定文言の `Reason`、常駐 runtime、`spawn_blocking`、同時実行 permit、既定は全解決器無効）を採り、user 案から narumi の現行エクスポータの実態と移行路・content verbatim・`Availability`・`gaia resolve --content`・narumi 応答の版注記を、minimal 案から builder 注入・依存無しの `ip_is_public` 純関数・loopback 固定応答サーバーによる実 HTTP テスト・偽 narumi の stdout 汚染問題の認識を取り込んだ。判定の must_fix はすべて反映している（§2 と §16 に対応を記す）。

## 2. 決定事項

| 項目 | 決定 | 理由 |
| --- | --- | --- |
| 契約 | `contracts/manifest.json` の `resolve_source` を `enabled: true`。入出力（`ref_id` / `uri` / `scope` → `reference` / `resolved` / `content?` / `reason?`）は不変。`description` を更新。`contract_version` を `1.0.0` → `1.1.0` | ツールの有効化と説明変更は後方互換の追加。narumi の gaia クライアント（`pipeline/src/narumi/gaia/client.py` の `_check_capabilities`）はツール名の有無だけを見て版に依存しない |
| 解決器の選択 | `refs.system` を trim ＋ ASCII 小文字化して完全一致で引く登録簿。登録名は `file` / `url` / `narumi` | 既存の `system` は自由文字列。`minutes` / `notion` / `box` などは「解決器なし」で `resolved=false` |
| 注入 | `ToolService::new(db, catalog)` の署名は不変。`ToolService::with_sources(SourceRegistry)` の builder で注入。既定は `SourceRegistry::empty()` | 既存 6 箇所の `ToolService::new` 呼び出しとテストを壊さない |
| 設定の読み方 | `SourceSettings` トレイトで呼び出しごとに `Config::load` し `[sources]` を取り出す（`AuthTable::from_path` と同じ思想）。読めなければ fail-closed（全解決器が `resolved=false`） | roots / command の取り消しが再起動なしで効く。デスクトップでも同じ |
| 既定値 | `file.roots = []`、`url.allow_hosts = []`、`[sources.narumi]` 無し、すなわち **全解決器が既定で無効**。`enabled` フラグは置かない | 絶対原則 2 の default deny / explicit allow に揃える。agent キーで human の機械の外向き GET やファイル読取を既定で起こさせない |
| url の許可 | `allow_hosts` の明示 allow のみ。`"*"`（全公開ホスト）か FQDN（完全一致または `.<host>` 接尾辞一致）。ポート制限は設けない（ホスト allow が制御点） | ホスト規則を複雑にしない（判定 1 の縮退指示）と既定無効（判定 2 の must_fix）を両立 |
| DB への書き込み | 解決器に `Connection` を渡さない（型で無書き込みを担保）。`last_verified` も更新しない。唯一の例外は複数 scope 明示時の `audit_log(cross_scope_read)`（他の参照系ツールと同じ） | 絶対原則 1 と 2 の両立。AGENTS.md と契約 description に明文化 |
| 参照の特定 | `ref_id` / `uri`（実効 scope 内、`uri` は最新 1 件 = id 最大）。両方指定は一致必須。どちらも無ければ `invalid_params`。不在・別 scope・不一致はすべて同一文言の `not_found` | 存在オラクルを作らない。`uri` は登録済み参照の検索キーであり取得先の指定ではない |
| 実体化する URI | 常に DB 行の `reference.uri`。入力の `uri` は特定にしか使わない | 人間が承認した参照だけを実体化する。SSRF の第一の壁 |
| reason | `Reason` enum ＋ `Display` の固定文言（英語。既存 `ToolError` の文言と同じ言語）。上流の message / OS エラー文字列 / パス / IP / コマンド文字列は含めない。narumi のエラー `code` と HTTP `status` は残す。上流の詳細は stderr の warn にのみ出す | テストは variant で断定。エージェントへ設定情報を流さない。運用者は stderr で原因を追える |
| content | verbatim（先頭 BOM の除去と C0 制御文字（`\t` `\n` `\r` を除く）・DEL の除去のみ）。切り詰めは Unicode スカラー数 `max_content_chars`。注記（切り詰め・版・話者不明など）は `reason` に `; ` 連結 | gaia 生成行を本文に混ぜない。端末エスケープ注入を防ぐ |
| 失敗の分類 | 入力・認可・scope・参照特定・busy だけが `ToolError`。参照が特定できた後の失敗はすべて `resolved=false` ＋ `reason`（`reference` と `snapshot` は必ず返す） | 契約の目的が「到達不能でも思い出し方を返す」こと（絶対原則 3） |
| 同時実行 | 解決器ごとの permit（file 4 / url 2 / narumi 1）。超過は `ToolError::busy` | HTTP 経由の agent が子プロセスや外向き HTTP を並列に大量起動できないようにする |
| 同期境界 | `GaiaServer::call_tool` と desktop `commands::call_tool`（`async fn` 化）は全ツール一律 `spawn_blocking` で `ToolService::call` を呼ぶ。`JoinError` は `internal` | 最長 30 秒のブロッキングで JSON-RPC の受信ループや他セッション・UI を止めない |
| narumi の runtime | `NarumiResolver` が `OnceLock<tokio::runtime::Runtime>`（`new_multi_thread().worker_threads(1)`）を常駐させ、`runtime.spawn` ＋ `mpsc::recv_timeout(timeout + 5s)` で待つ。呼び出しごとの runtime 生成・破棄はしない。解決器の `Drop` は runtime を `shutdown_background()` で待たずに停止する | rmcp 3.1.4 の `ChildWithCleanup::drop` は `tokio::spawn` で kill するため、runtime を直後に drop すると kill タスクが実行されず子が残る。呼び出し元が tokio ワーカー・`spawn_blocking`・runtime 無し（CLI）のいずれでも panic しない。`gaia serve` は `Runtime::block_on` の内側で `ToolService` を drop するので、通常の `Runtime` drop（待つ）だと tokio が panic する（stdio は EOF 時に exit 101、HTTP は Ctrl-C 時にワーカーが panic） |
| narumi の子プロセス | 1 呼び出し = 1 子プロセス（起動 → initialize → `get_minutes` → `RunningService::cancel`）。成功・失敗・タイムアウトの全経路で `cancel()`（→ `graceful_shutdown`: close → 3 秒待って kill）を通す。`kill_on_drop(true)` とプロセスグループ kill を併用 | 常駐子プロセスの状態共有・ゾンビを避ける。`uv run` の孫（python）を残さない |
| narumi への scope | **参照行の `reference.scope` を単一文字列で渡す**。呼び出し側の実効 scope 集合は渡さない | narumi の `scope` 省略は unscoped のみで実用にならない。集合を渡すと「scope A の参照から scope B の会議を引く」経路と narumi 側の横断監査が生じる。単一なので narumi の `cross_scope_read` は発火せず、gaia 側の横断は gaia の audit_log に残る |
| narumi のエラー | `not_found` と `scope_denied` は畳まず `NarumiError { code }` として区別する | 参照の scope は呼び出し側に既知で漏えい効果が薄く、affiliation 名 ≠ narumi scope 名の設定ミスを診断できる必要がある |
| narumi の環境 | 親の環境を継承し、`GAIA_` 接頭辞の変数だけ除去。`[sources.narumi].env` で追加・上書き。`command` は絶対パス必須。cwd は継承しない前提で `args` の `--directory` で指定 | `env_clear` ＋ allowlist は `uv run` の要件（`UV_*` / `VIRTUAL_ENV` / `SSL_CERT_FILE` 等）を落として失敗原因を追いにくい。デスクトップ（launchd 起動）は PATH が CLI と異なる |
| narumi の stderr | `stderr = "discard"`（既定）/ `"inherit"`。gaia 自身は起動失敗の `ErrorKind` を必ず stderr に warn する | 既定で静かに、設定ミスは追える |
| file の判定 | 字句検査 → canonicalize した実体パスと canonicalize した roots の `starts_with`（root 内 → root 内の symlink は許容）→ `O_NOFOLLOW \| O_NONBLOCK \| O_CLOEXEC` で open → 開いたハンドルの metadata で通常ファイル確認 → サイズ → NUL / 非 UTF-8 はバイナリとして拒否。設定ディレクトリ・DB ディレクトリ・デスクトップの鍵退避ディレクトリは常時拒否 | roots 内の実ファイルだけを返す。`gaia.db` や 0600 キーファイルを「テキスト」として返さない。不在・root 外・種別不可・権限不足は同一文言 |
| url の判定 | `url` crate で WHATWG 正規化 → scheme / userinfo / host 検査 → ureq の `Resolver` 差し替えで DNS 解決後の全アドレスを検査 → `max_redirects(0)` で自前追従し各ホップ再検査 → `proxy(None)` → 圧縮機能なし（バイト上限は wire バイト基準）→ text 系 Content-Type のみ | 表 §7.3 参照 |
| HTML | 変換せず verbatim ＋ 注記 | 依存とバグ面を増やさない。エージェントは自前の fetch で HTML を読める |
| 偽 narumi | `crates/gaia-mcp/src/bin/fake_narumi.rs`（rmcp の server 側で `get_minutes` を実装、モードは環境変数 `FAKE_NARUMI_MODE`）。統合テスト `crates/gaia-mcp/tests/narumi_resolver.rs` から `env!("CARGO_BIN_EXE_fake_narumi")` で起動 | libtest harness は stdout に `running N tests` を出し test スレッドの stdout を捕捉するため、テストバイナリ自身の再実行は stdio MCP を壊す。examples のパス推定は target 配置依存。`crates/gaia/tests` の `CARGO_BIN_EXE_gaia` と同じ慣習 |
| CLI | `gaia resolve (--ref-id <N> \| --uri <U>) [--scope <S>]... [--content]`。`--content` は本文だけを stdout、ヘッダと reason は stderr、`resolved=false` は終了コード 2。JSON 出力は常に終了コード 0 | `less` やパイプで読める。JSON では `resolved=false` も正常応答 |
| デスクトップ | 参照カードに「内容を取得」ボタンと結果表示（テキスト描画、localStorage 不保存）のみ。`[sources]` は TOML 手編集を README に案内 | v0.2.0 の工数を抑える |
| 版 | `0.2.0`。`[sources]` を含む設定は 0.1.x で読めない（`deny_unknown_fields`）。既定値のままなら `[sources]` を書き出さない | CHANGELOG と README に「戻す場合は `[sources]` を削除」を記載 |

## 3. 確認済みの事実（設計の根拠）

gaia-library（main 1f5560a）:

- `ToolService::call` は同期。`GaiaServer::call_tool`（`crates/gaia-mcp/src/server.rs:130`）は async fn の中で `self.service.call(...)` を直接呼ぶ。desktop の `commands::call_tool`（`desktop/src-tauri/src/commands.rs:39`）は同期 `#[tauri::command]`
- `ToolService::new(db, catalog)` の呼び出しは 6 箇所: core の `test_support::service`、gaia-mcp の server / http テスト、`crates/gaia/src/cli/app.rs:38`、`desktop/src-tauri/src/state.rs:128`、`desktop/src-tauri/src/first_run.rs:97`
- `Db` は `Mutex<Connection>` の単一接続。`with_conn` の閉包内で外部 I/O をすると他ツールを塞ぐ
- `get_server_info` は `capabilities.resolvers: Vec<String>` を空で返している（`crates/gaia-core/src/tools/server_info.rs`）
- `refs::get(conn, id, &ScopeSet)` はあるが uri で引く関数は無い。テスト用シード（`test_support::seed_basic`）の ref は `system = "minutes"`, `uri = "minutes://meeting/42#t=1200"`
- `Config` は `deny_unknown_fields`。`validate()` は純粋（ファイルシステムを見ない）。`ConfigError` に設定値の範囲エラー用の variant は無い
- ureq 3.4.0（workspace: `default-features = false, features = ["json"]`、TLS 無し）: `unversioned::resolver::Resolver` トレイト（`resolve(&self, uri, config, timeout) -> Result<ResolvedSocketAddrs, Error>`、`ArrayVec<SocketAddr, 16>`）、`Agent::with_parts(config, connector, resolver)`、`Config` 既定は `proxy: Proxy::try_from_env()`、`max_redirects` / `timeout_global` / `http_status_as_error` / `redirect_auth_headers` あり、`Body::with_config().limit(n)`。`rustls` feature は `rustls-no-provider` ＋ ring ＋ `rustls-webpki-roots`。`gzip` は既定 feature だが無効化済み
- `url` 2.5.8 は Cargo.lock に推移依存として存在する
- rmcp 3.1.4: `client` feature（`tokio-stream`）と `transport-child-process` feature（`process-wrap 9.0` の `tokio1`）。`TokioChildProcess::builder(cmd).stderr(Stdio).spawn() -> (TokioChildProcess, Option<ChildStderr>)`。`ChildWithCleanup::drop` は `tokio::spawn` で `kill()`。`graceful_shutdown` は close → 3 秒待って kill。`RunningService::cancel()` は transport の `close`（= `graceful_shutdown`）を経由する。`peer_info()` の `server_info: Implementation { name, version, .. }`。`CallToolRequestParams { name, arguments: Option<JsonObject>, .. }`、`CallToolResult { structured_content: Option<Value>, is_error: Option<bool>, content, .. }`。`impl ClientHandler for ()` があるので `().serve(transport)` で最小クライアントになる
- process-wrap 9.0 はローカル registry に無い（実装時に `cargo fetch`。`ProcessGroup` / `KillOnDrop` ラッパーの API 名は §16 の未検証事項）
- 既存テストの `resolve_source` 断定: `crates/gaia-core/src/tools/mod.rs`（`call_enforces_existence_role_and_input_schema` の「disabled = 存在しない扱い」と `contract_version == "1.0.0"`）、`crates/gaia-mcp/src/server.rs:264`（`get_tool("resolve_source").is_none()`）、`crates/gaia-mcp/src/http/tests/stateless.rs:48`（未知ツール列に `resolve_source` を含めて 400 / not_found を期待）、`crates/gaia/tests/stdio.rs:126`、`crates/gaia/tests/http.rs:265`
- `scripts/lib/release-metadata.mjs` は workspace `Cargo.toml`・`desktop/src-tauri/Cargo.toml`・`tauri.conf.json` の版一致と CHANGELOG の対象節を検査する

narumi（narumi のローカルチェックアウト。読み取り専用で参照した）:

- 現行の gaia エクスポータ `pipeline/src/narumi/export/gaia.py` は provenance を `system = "file"`, `uri = <minutes/v<N>/minutes.md の Path.resolve().as_uri()>`, `title = "<meeting_name> 議事録 v<N>"`, `note = "narumi meeting <id>; minutes version <N>; ..."` で送り、`snapshot` を入れていない。`narumi://` 規約は narumi 側にまだ存在しない
- 議事録の実体は `<NARUMI_HOME>/meetings/<meeting_id>/minutes/v<N>/minutes.md`。`NARUMI_HOME` の既定は `~/Library/Application Support/narumi`（`as_uri()` は空白を `%20` に符号化する）
- `get_minutes`（`contracts/tools/get_minutes.json`）: 入力 `meeting_id`（`^[0-9]{8}T[0-9]{6}Z-[0-9a-f]{8}$`）、`version?`（1 以上）、`scope?`（省略 = unscoped のみ / 単一名 = その scope ＋ unscoped / 配列 = 横断で監査）。出力 `{ meeting_id, version, markdown, generated_at, provider, unresolved_speakers, available_versions }`。失敗は `isError=true` ＋ `{"error": {code, message}}`、code は `not_found` / `scope_denied` / `invalid_argument` / `internal`
- MCP サーバー名は `narumi`（`server/src/narumi_server/app.py` の `SERVER_NAME`）。MCP クライアント向け起動形は `uv --directory <repo> run narumi-server --stdio-bridge`（常駐サーバーへの橋渡し。常駐が無ければエラー終了し自動起動しない）。ログは stderr
- gaia クライアント `_check_capabilities` はツール名の有無しか検査しない

## 4. モジュール構成と型

### 4.1 gaia-core `sources`（rmcp と tokio を知らない）

```
crates/gaia-core/src/
  sources/mod.rs      SourceResolver / SourceRegistry / SourceSettings / ResolveRequest / Resolved / Unresolved
                      Reason / Note / Availability / ProtectedPaths / shape_content
  sources/file.rs     FileResolver
  sources/url.rs      UrlResolver、GuardedResolver（ureq Resolver）、check_url
  sources/net.rs      ip_is_public / host_is_allowed（純関数。表テスト）
  tools/resolve_source.rs
  storage/refs.rs     latest_by_uri を追加
  config.rs           SourcesConfig / FileSourceConfig / UrlSourceConfig / NarumiSourceConfig / NarumiStderr と validate
```

```rust
// sources/mod.rs
pub struct ResolveRequest<'a> {
    pub reference: &'a Reference,     // 契約型（DB 行そのもの）。Connection は渡さない
    pub settings: &'a SourcesConfig,  // 呼び出し時点の [sources]
}

pub struct Resolved {
    pub content: String,
    pub notes: Vec<Note>,             // resolved=true でも reason に載せる注記
}

pub enum Unresolved {
    Unavailable(Reason),              // resolved=false + reason
    Internal(String),                 // stderr にのみ詳細。クライアントには Reason::ResolverFailed
}

/// 固定文言カタログ。Display が reason を生成する。上流の文字列を埋め込むフィールドは持たない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    NoResolver { system: String, available: Vec<String> },   // system は呼び出し側自身の ref データ
    NotConfigured { system: &'static str, setting: &'static str }, // setting = "[sources.file].roots" など
    SettingsUnavailable,
    InvalidUri { system: &'static str, rule: &'static str }, // rule = "scheme" / "host" / "path" / "meeting_id" / "query"
    FileUnavailable,                  // 不在・root 外・通常ファイル以外・権限不足を畳む
    BinaryContent,
    TooLarge,
    UrlNotAllowed { rule: UrlRule },  // Scheme / Credentials / Host / Address / Redirects
    UpstreamStatus { status: u16 },
    UnsupportedContentType,
    TimedOut { secs: u64 },
    ConnectionFailed,
    ReadFailed,
    NarumiStartFailed,
    NarumiHandshakeFailed,
    NarumiNotNarumi,
    NarumiError { code: String },     // [a-z_]{1,32} に正規化した code のみ
    NarumiInvalidResponse,
    ResolverFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlRule { Scheme, Credentials, Host, Address, Redirects }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    ContentTruncated { chars: usize, original: usize },
    BodyTruncated { bytes: u64 },
    ControlCharsRemoved,
    NonUtf8Replaced,
    HtmlAsIs,
    NarumiVersion { version: u32, available: Vec<u32>, generated_at: String, provider: String },
    UnresolvedSpeakers { count: usize },
    SnapshotFallback,                 // resolved=false かつ reference.snapshot がある
}

pub enum Availability { Ready, Unconfigured { setting: &'static str } }

pub trait SourceResolver: Send + Sync + 'static {
    fn system(&self) -> &'static str;                            // "file" | "url" | "narumi"
    fn availability(&self, settings: &SourcesConfig) -> Availability;
    fn max_concurrency(&self) -> usize;                          // file 4 / url 2 / narumi 1
    /// 外部取得。panic せず、失敗は Unresolved で返す。DB には触れない。
    fn resolve(&self, request: ResolveRequest<'_>) -> Result<Resolved, Unresolved>;
}

pub trait SourceSettings: Send + Sync + 'static {
    /// 呼び出しごとに現在の設定を返す。失敗は fail-closed。
    fn current(&self) -> Result<SourcesConfig, String>;
}
pub struct StaticSettings(pub SourcesConfig);                   // テスト・組込み用
pub struct ConfigFileSettings { path: PathBuf }                  // Config::load(path)?.sources
impl ConfigFileSettings { pub fn new(path: PathBuf) -> Self; }

pub struct SourceRegistry {
    entries: BTreeMap<&'static str, Entry>,                      // Entry { resolver: Arc<dyn SourceResolver>, gate: Gate }
    settings: Arc<dyn SourceSettings>,
}
impl SourceRegistry {
    pub fn empty() -> Self;                                      // 何も解決しない。ToolService::new の既定
    pub fn new(settings: Arc<dyn SourceSettings>) -> Self;
    pub fn register(&mut self, r: Arc<dyn SourceResolver>) -> Result<(), SourceError>; // system 重複は Err
    pub fn get(&self, system: &str) -> Option<&Arc<dyn SourceResolver>>; // trim + ASCII 小文字化 + 完全一致
    pub fn acquire(&self, system: &str) -> Option<Permit<'_>>;   // None = 上限到達（busy）
    pub fn settings(&self) -> Result<SourcesConfig, String>;
    pub fn systems(&self) -> Vec<String>;                        // 登録済み system 名（NoResolver の available に使う）
    pub fn ready_systems(&self, settings: &SourcesConfig) -> Vec<String>;  // Availability::Ready のみ
}

/// file 解決器が常時拒否する領域。呼び出し側（CLI / desktop）が組み立てる。
pub struct ProtectedPaths { pub config_dir: PathBuf, pub db_dir: PathBuf, pub extra: Vec<PathBuf> }

pub fn shape_content(text: String, max_chars: usize) -> (String, Vec<Note>);
```

`Gate` は `Mutex<usize>` の単純なカウンタ（`try_acquire` が上限なら `None`）。`Permit` は drop で減算する RAII。

`ToolService` への注入と `CallContext`:

```rust
pub struct ToolService { db: Db, catalog: Catalog, sources: SourceRegistry }
impl ToolService {
    pub fn new(db: Db, catalog: Catalog) -> Self;                 // sources = SourceRegistry::empty()
    pub fn with_sources(self, sources: SourceRegistry) -> Self;
    pub fn sources(&self) -> &SourceRegistry;
}
pub struct CallContext<'a> { pub client, pub db, pub catalog, pub sources: &'a SourceRegistry }
```

`HANDLED_TOOLS` に `"resolve_source"` を追加し、`dispatch` に `"resolve_source" => run(ctx, args, resolve_source::handle)` を足す。`get_server_info` の `capabilities.resolvers` は `sources.settings()` が成功したときの `ready_systems(&settings)`、失敗時は空配列（名前のみ。コマンドやパスは出さない）。

### 4.2 ストレージ

```rust
// storage/refs.rs
/// uri 完全一致（バイト比較。LIKE は使わない）で実効 scope 内の最新 1 件（id 最大）。
pub fn latest_by_uri(conn: &Connection, uri: &str, scopes: &ScopeSet) -> Result<Option<Reference>, StorageError>;
// SELECT {COLS} FROM refs WHERE uri = ?1 AND scope IN (SELECT value FROM json_each(?2)) ORDER BY id DESC LIMIT 1
```

### 4.3 gaia-mcp `sources`

```
crates/gaia-mcp/src/
  sources/mod.rs        pub fn registry(config_path: &Path, protected: ProtectedPaths) -> SourceRegistry
  sources/narumi.rs     NarumiResolver（rmcp client / TokioChildProcess / 常駐 runtime）、map_get_minutes
  sources/narumi_uri.rs parse_narumi_uri（純関数）
  bin/fake_narumi.rs    テスト用の偽 narumi（配布物には含めない）
  server.rs             call_tool を spawn_blocking 化
```

```rust
// sources/mod.rs
pub fn registry(config_path: &Path, protected: ProtectedPaths) -> SourceRegistry {
    let mut r = SourceRegistry::new(Arc::new(ConfigFileSettings::new(config_path.to_path_buf())));
    r.register(Arc::new(FileResolver::new(protected))).expect("unique");
    r.register(Arc::new(UrlResolver::public_only())).expect("unique");
    r.register(Arc::new(NarumiResolver::new())).expect("unique");
    r
}
```

これが `file` / `url` / `narumi` を組み立てる唯一の場所。CLI と desktop はこの関数と `ToolService::with_sources` だけを使う。gaia-mcp は core の `sources` 公開 API（トレイト・レジストリ・設定型・`Reason` / `Note`）を使うので、AGENTS.md の利用境界に `sources` を追加する（§13）。

### 4.4 CLI（crates/gaia）

- `App::open`: `ToolService::new(db, catalog).with_sources(gaia_mcp::sources::registry(&config_path, ProtectedPaths { config_dir, db_dir, extra }))`。`config_dir` は `config_path.parent()`、`db_dir` は `db_path.parent()`、`extra` は `config::key_store_dir_with`（デスクトップの平文キー退避ディレクトリ `<XDG_DATA_HOME|~/.local/share>/gaia-library/keys`。`GAIA_DB` で DB を別の場所に置いても変わらない。HOME 無しなら省く）
- `cli/query.rs` に `ResolveArgs` と `resolve(app, client, args)` を追加。`Command::Resolve(query::ResolveArgs)`

```rust
#[derive(Args)]
pub struct ResolveArgs {
    #[arg(long)] pub ref_id: Option<i64>,
    #[arg(long)] pub uri: Option<String>,
    #[arg(long)] pub scope: Vec<String>,
    /// content だけを stdout に出す（ヘッダと reason は stderr）。resolved=false は終了コード 2
    #[arg(long)] pub content: bool,
}
```

引数を JSON に組んで `ToolService::call("resolve_source")` に渡すだけ。`--content` 無しは他コマンドと同じ整形 JSON。`gaia call resolve_source --args '{...}'` も既存経路で動く。

### 4.5 desktop

- `state.rs::load_initialization` と `first_run::setup`: CLI と同じ 1 行で注入。`ProtectedPaths.extra` にキーチェーン退避ディレクトリ（`keychain.rs` の `fallback_root`。位置の算出は gaia-core の `config::key_store_dir_with` を使い、desktop は書き込み先としての検査だけを重ねる）を入れる
- `commands::call_tool` を `async fn` にし `tauri::async_runtime::spawn_blocking` で `service.call` を実行（§11）
- `desktop/ui/src/contextApi.ts` に `resolveReference(reference: Reference): Promise<ResolveSourceOutput>`（`callTool("resolve_source", { ref_id: reference.id, scope: reference.scope })`。参照自身の scope を使うので横断にならない）
- `desktop/ui/src/types.ts` に `ResolveSourceOutput = { reference: Reference; resolved: boolean; content?: string; reason?: string }`
- `RefList.tsx` の `ReferenceRow` に「内容を取得」ボタン（既存の「URI をコピー」と同じ `pending` / `mounted` パターン）。取得中は「取得中…（時間がかかることがあります）」（上限は設定次第なので秒数を出さない）。結果は `<pre className="whitespace-pre-wrap break-words">` でテキストとして描画（`dangerouslySetInnerHTML` と Markdown レンダリングは使わない）。`reason` は注記行に表示。`resolved=false` なら amber の帯に reason を出し、`snapshot` の `<details>` を `open` にする。ToolError は既存の `errorMessage` 表示。結果は state に持つだけで localStorage・ログに保存しない
- `[sources]` の設定画面・narumi 接続テストは作らない（README で TOML 手編集を案内）

### 4.6 依存の変更

| 場所 | 変更 |
| --- | --- |
| workspace `Cargo.toml` | `ureq = { version = "=3.4.0", default-features = false, features = ["json"] }`（`unversioned::resolver` は安定 API の外なので版を固定。壊れたらビルドで気付く）。`url = "2"` を追加。rmcp の features に `"client"`, `"transport-child-process"` を追加 |
| `crates/gaia-core/Cargo.toml` | `ureq = { workspace = true, features = ["rustls"] }`、`url.workspace = true`。`gzip` / `brotli` / `charset` は付けない（伸長しないのでバイト上限は wire バイト基準。`Accept-Encoding` を送らないことは実装時にキャプチャで確認） |
| `crates/gaia-mcp/Cargo.toml` | `[[bin]] name = "fake_narumi" path = "src/bin/fake_narumi.rs"`（配布物に含めない。`desktop/build-app.sh` は `gaia` だけを同梱する） |
| `Cargo.lock`（workspace）と `desktop/src-tauri/Cargo.lock` | rustls / ring / webpki-roots / process-wrap / tokio-stream / url の追加。desktop の `--locked` ゲートを忘れない |

gaia の dev-dep 側の ureq 利用（`crates/gaia/tests/http.rs`）は feature 統合で `json + rustls` になるだけで壊れない。

## 5. 設定スキーマ（config.toml）

```toml
[sources]
max_content_chars = 30000            # content の上限（Unicode スカラー数）。1_000..=500_000。既定 30_000

[sources.file]
roots = []                           # 既定: 空 = 無効。絶対パスのディレクトリのみ。重複不可、"/" 不可
max_bytes = 1048576                  # 既定 1 MiB。1..=64 MiB。超えるファイルは読まずに TooLarge

[sources.url]
allow_hosts = []                     # 既定: 空 = 無効。"*" で全公開ホスト、"example.com" はそのホストとサブドメイン
timeout_secs = 15                    # 既定 15。1..=120。リダイレクトの追従を含む 1 参照あたりの合計（接続 5 秒はその内数）
max_bytes = 1048576                  # 既定 1 MiB。1..=64 MiB
max_redirects = 3                    # 既定 3。0..=10

[sources.narumi]                     # 節ごと省略可。省略 = 無効
command = "/opt/homebrew/bin/uv"     # 絶対パス必須
args = ["--directory", "/path/to/narumi", "run", "narumi-server", "--stdio-bridge"]
timeout_secs = 30                    # 既定 30。1..=300。initialize と get_minutes の上限（起動からの締切）。子プロセスの終了処理は別に最長 3 秒（呼び出し元は timeout + 5 秒まで待つ）
max_bytes = 1048576                  # 既定 1 MiB。1..=64 MiB。get_minutes 応答の markdown がこれを超えると TooLarge
stderr = "discard"                   # "discard" | "inherit"。既定 discard
[sources.narumi.env]                 # 任意。指定したキーだけ追加・上書き
NARUMI_HOME = "/Users/me/Library/Application Support/narumi"
```

Rust 型（`config.rs`）:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfig {
    #[serde(default = "SourcesConfig::default_max_content_chars")] pub max_content_chars: usize,
    #[serde(default)] pub file: FileSourceConfig,
    #[serde(default)] pub url: UrlSourceConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub narumi: Option<NarumiSourceConfig>,
}
impl Default for SourcesConfig { /* 上記の既定値 */ }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileSourceConfig { #[serde(default)] pub roots: Vec<PathBuf>, #[serde(default = "...")] pub max_bytes: u64 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UrlSourceConfig {
    #[serde(default)] pub allow_hosts: Vec<String>,
    #[serde(default = "...")] pub timeout_secs: u64,
    #[serde(default = "...")] pub max_bytes: u64,
    #[serde(default = "...")] pub max_redirects: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NarumiSourceConfig {
    pub command: PathBuf,
    #[serde(default)] pub args: Vec<String>,
    #[serde(default = "...")] pub timeout_secs: u64,
    #[serde(default = "...")] pub max_bytes: u64,
    #[serde(default)] pub stderr: NarumiStderr,          // enum { Discard(既定), Inherit }（rename_all = "lowercase"）
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")] pub env: BTreeMap<String, String>,
}
```

`Config` には `#[serde(default, skip_serializing_if = "SourcesConfig::is_default")] pub sources: SourcesConfig` を追加する。`skip_serializing_if` により、`[sources]` を触っていない設定ファイルは `gaia client add` などの保存後も 0.1.x で読める。

`Config::validate()` に追加する規則（純粋。ファイルシステムは見ない。違反は新設 `ConfigError::InvalidSource(String)`）:

- `max_content_chars` / 各 `timeout_secs` / `max_bytes` / `max_redirects` は上記範囲内
- `file.roots`: 各要素は絶対パス、`/` ではない、NUL を含まない、重複なし。存在・ディレクトリ性・symlink・保護領域との関係は解決時に検査する（validate は環境変数や DB パスを知らない）
- `url.allow_hosts`: 各要素は `*` か、ASCII 小文字化した FQDN（`url::Host::parse` で `Host::Domain` になり、`localhost` / `*.localhost` / 単一ラベル / 末尾ドットではない）。IP リテラルは不可（IP は `ip_is_public` が判定する）。重複なし
- `narumi.command`: 絶対パス、空・NUL 不可。`args`: NUL 不可、各 4096 bytes 以下、最大 64 件。`env`: キーは `[A-Za-z_][A-Za-z0-9_]*` で `GAIA_` 接頭辞は拒否、値は NUL 不可、最大 32 件

`[sources]` 節が無い設定は「全解決器無効」として動く。既存の 0.1.x の設定はそのまま読める。

## 6. 解決アルゴリズム（`tools/resolve_source.rs`）

```rust
pub fn handle(ctx: &CallContext<'_>, input: ResolveSourceInput) -> Result<ResolveSourceOutput, ToolError>
```

1. 入力検査（契約検証の後）: `ref_id` も `uri` も無ければ `invalid_params("pass ref_id or uri")`。`uri` は trim 後に空・2048 bytes 超・制御文字含みなら `invalid_params`
2. 参照特定（`with_conn` 内。閉包は `Reference` を返して**ここで DB ロックを手放す**）:
   - `ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))` → `scopes.audit_cross_read(c, &ctx.client.name, "resolve_source")`（複数 scope 明示時のみ書く。唯一の DB 書き込み）
   - `ref_id` のみ: `refs::get(c, id, &scopes)`
   - `uri` のみ: `refs::latest_by_uri(c, uri, &scopes)`
   - 両方: `refs::get` の結果の `reference.uri == uri`（バイト一致）を要求
   - 不在・scope 外・不一致はすべて `not_found("reference not found in the effective scope")` の同一文言
3. `settings = ctx.sources.settings()`。失敗 → `resolved=false, reason = SettingsUnavailable`（stderr に warn）
4. `resolver = ctx.sources.get(&reference.system)`。無ければ `resolved=false, reason = NoResolver { system, available }`（`available` は登録済み system 名。`minutes` / `notion` / `box` はここ）
5. `resolver.availability(&settings)` が `Unconfigured { setting }` なら I/O をせずに `resolved=false, reason = NotConfigured { system, setting }`
6. `permit = ctx.sources.acquire(system)`。取れなければ `ToolError::busy("resolver `<system>` is busy; retry later")`
7. `resolver.resolve(ResolveRequest { reference: &reference, settings: &settings })`（解決器は必ず **DB 行の `reference.uri`** を実体化する）:
   - `Ok(Resolved { content, notes })` → `shape_content(content, settings.max_content_chars)`（BOM 除去 → C0 / DEL 除去（`\t` `\n` `\r` は残す。除去したら `Note::ControlCharsRemoved`）→ 文字境界で切り詰め（切ったら `Note::ContentTruncated`））→ `resolved = true, content, reason = notes を "; " 連結`（空なら省略）
   - `Err(Unavailable(reason))` → `resolved = false, content 無し, reason`。`reference.snapshot` があれば `Note::SnapshotFallback` を末尾に付ける（`"...; fallback: see reference.snapshot"`）
   - `Err(Internal(detail))` → stderr に `warn!(system, ref_id, detail)`、クライアントには `resolved = false, reason = ResolverFailed`
8. 応答 `ResolveSourceOutput { reference, resolved, content, reason }`。DB へは戻らない（`last_verified` 更新なし）
9. ログ（stderr のみ）: `info!(tool = "resolve_source", client, system, ref_id, resolved, elapsed_ms)`。URI と content はログに出さない（署名付きクエリや議事録本文をデスクトップのログファイルに残さない）

### 6.1 エラー対応表

| 段階 | 事象 | 扱い |
| --- | --- | --- |
| 入力 | `ref_id` も `uri` も無い / `uri` が空・過長・制御文字 | `ToolError::invalid_params`（JSON-RPC エラー） |
| scope | 既定 scope 無し / 未知 scope | `scope_denied` / `not_found`（`ScopeSet` 既存） |
| 参照 | 不在・別 scope・`ref_id` と `uri` の不一致 | `not_found`（同一文言） |
| DB | busy / sqlite エラー | `busy` / `internal`（既存の From） |
| 同時実行 | 解決器の permit 上限 | `busy` |
| 設定 | `[sources]` の読み込み失敗（`[keys]` 不正など fail-closed） | `resolved=false` ＋ `SettingsUnavailable`（参照と snapshot は返す） |
| 解決器 | 解決器なし / 未設定 | `resolved=false` ＋ `NoResolver` / `NotConfigured` |
| 解決 | URI 規約違反・起動失敗・接続失敗・タイムアウト・HTTP 非 200・種別不可・サイズ超・SSRF 拒否・root 外・バイナリ・narumi の isError | `resolved=false` ＋ 該当 `Reason` |
| 解決 | 解決器内部の予期しない失敗 | `resolved=false` ＋ `ResolverFailed`（詳細は stderr） |
| 整形 | 文字数上限で切った / 制御文字を除去した / 版注記 | `resolved=true` ＋ `reason` に注記 |

## 7. url 解決器（`sources/url.rs` ＋ `sources/net.rs`）

`UrlResolver { policy: AddressPolicy }`。`AddressPolicy::PublicOnly` が本番。`#[cfg(test)]` 限定で `AddressPolicy::AllowLoopback` を持ち、本番コードから到達できない（テストは `fetch(uri, settings, AddressPolicy::AllowLoopback)` を直接呼ぶ。`UrlResolver::for_tests()` は置かない）。`availability`: `allow_hosts` が非空なら `Ready`、空なら `Unconfigured { setting: "[sources.url].allow_hosts" }`。`max_concurrency = 2`。

### 7.1 URL 検査 `check_url(&Url, &UrlSourceConfig) -> Result<(), Reason>`（各ホップで同じ関数）

1. `url::Url::parse` 失敗 → `InvalidUri { system: "url", rule: "parse" }`
2. scheme が `http` / `https` 以外 → `UrlNotAllowed { Scheme }`
3. userinfo（`user:pass@`）あり → `UrlNotAllowed { Credentials }`。fragment は送らない
4. `url.host()`:
   - `Host::Ipv4 / Ipv6`（WHATWG が `2130706433` / `0x7f.1` / `0177.0.0.1` を正規化済み）→ `ip_is_public` が偽なら `UrlNotAllowed { Address }`。IP リテラルは `allow_hosts = ["*"]` のときだけ許可
   - `Host::Domain`: ASCII 小文字化。`localhost` / `*.localhost` / 単一ラベル / 末尾ドット → `UrlNotAllowed { Host }`。`host_is_allowed(host, &allow_hosts)`（`*` か、`host == h || host.ends_with(&format!(".{h}"))`）が偽 → `UrlNotAllowed { Host }`
   - host 無し → `UrlNotAllowed { Host }`

### 7.2 DNS 解決後の検査 `GuardedResolver`

ureq の `unversioned::resolver::Resolver` を実装し、`Agent::with_parts(config, DefaultConnector::default(), GuardedResolver { inner: DefaultResolver::default(), policy, rejected: AtomicBool })` で差し込む。`resolve` は内側で解決した**全アドレス**（最大 16 件）を `policy` にかけ、1 つでも不許可なら `rejected` を立てて `Err(ureq::Error::HostNotFound)` を返す（接続しない）。ureq は Resolver が返したアドレス集合にそのまま接続するため、検査した IP と接続先が一致し、リクエスト単位で DNS リバインディングが閉じる。リダイレクトの各ホップも接続ごとに Resolver を通る。ハンドラは ureq のエラー時に `rejected` を見て `UrlNotAllowed { Address }` と `ConnectionFailed` を区別する。

### 7.3 SSRF 判定表 `ip_is_public(IpAddr) -> bool`（依存無しの純関数。表テスト）

| 種別 | 拒否範囲 | 備考 |
| --- | --- | --- |
| IPv4 | `0.0.0.0/8` | this network |
| IPv4 | `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16` | RFC 1918 |
| IPv4 | `100.64.0.0/10` | CGNAT |
| IPv4 | `127.0.0.0/8` | loopback |
| IPv4 | `169.254.0.0/16` | link-local。クラウドメタデータ `169.254.169.254` を含む |
| IPv4 | `192.0.0.0/24`, `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `198.18.0.0/15`, `192.88.99.0/24` | IETF 予約・文書用・ベンチマーク・6to4 中継 |
| IPv4 | `224.0.0.0/4`, `240.0.0.0/4`（`255.255.255.255` を含む） | マルチキャスト・予約・ブロードキャスト |
| IPv6 | `::`, `::1` | unspecified / loopback |
| IPv6 | `::ffff:0:0/96` | IPv4-mapped。埋め込み v4 を取り出して v4 規則で再判定 |
| IPv6 | `64:ff9b::/96`, `64:ff9b:1::/48` | NAT64。埋め込み v4 で再判定 |
| IPv6 | `2002::/16` | 6to4。埋め込み v4 で再判定 |
| IPv6 | `fc00::/7` | ULA |
| IPv6 | `fe80::/10`, `fec0::/10` | link-local / site-local |
| IPv6 | `ff00::/8` | マルチキャスト |
| IPv6 | `2001:db8::/32`, `2001::/32`, `100::/64` | 文書用 / Teredo / discard |

上記以外は公開。`std::net::Ipv4Addr::is_private` 等には頼らず範囲を列挙する。

### 7.4 送受信

- ureq `Config`: `proxy(None)`（環境変数 `HTTP_PROXY` 等を無視。プロキシ経由で判定を迂回させない）、`max_redirects(0)`（自前で追う）、`timeout_connect(5s)`、`timeout_resolve(5s)`、`timeout_global`（`timeout_secs` はリダイレクトの追従を含む 1 参照あたりの合計。ureq の `timeout_global` は call 単位なので、各ホップの request config に締切までの残り時間を渡し、残りが無ければ call せずに `TimedOut`）、`http_status_as_error(false)`、`user_agent("gaia-library/<version>")`。TLS は rustls ＋ webpki-roots（検証無効化 API は使わない）
- メソッドは GET のみ。ヘッダは `Accept: text/markdown, text/plain, application/json, text/html;q=0.8, */*;q=0.1`。Cookie / Authorization は送らない。`Accept-Encoding` は送らない
- リダイレクト: `301/302/303/307/308` のみ追従。`Location` を現在 URL 基準で解決し §7.1 を再実行、`max_redirects` 超で `UrlNotAllowed { Redirects }`。`allow_hosts` はリダイレクト先にも適用
- 応答: ステータス 200 以外 → `UpstreamStatus { status }`（本文は読まない）。`Content-Type` の media type が `text/*`, `application/json`, `application/*+json`, `application/xml`, `application/*+xml`, `application/xhtml+xml` 以外（または無し）→ `UnsupportedContentType`（本文は読まない）。`charset` が指定され `utf-8` / `utf8` 以外 → `UnsupportedContentType`（変換は入れない）
- 本文: `Content-Length` が `max_bytes` 超なら読まずに `TooLarge`。`body_mut().with_config().limit(max_bytes + 1).read_to_vec()` で読み、`max_bytes` を超えていれば `max_bytes` で切って `Note::BodyTruncated`。`String::from_utf8_lossy` で文字列化し置換が起きたら `Note::NonUtf8Replaced`。`text/html` / `application/xhtml+xml` は変換せず `Note::HtmlAsIs`
- タイムアウト・接続失敗: `TimedOut { secs }` / `ConnectionFailed`（OS エラー文字列は stderr のみ）

## 8. file 解決器（`sources/file.rs`）

`FileResolver { protected: ProtectedPaths }`。`availability`: `roots` が非空なら `Ready`、空なら `Unconfigured { setting: "[sources.file].roots" }`。`max_concurrency = 4`。

URI 規約: `file:///absolute/path`（RFC 8089。host は空か `localhost` のみ）。`url::Url::to_file_path()` でパーセントデコード（narumi の `Path.as_uri()` が出す `%20` を戻す）。`file://otherhost/...`・相対パス・NUL 含みは `InvalidUri { system: "file", rule }`。

手順:

1. 字句検査: `Path::components()` に `ParentDir` / `CurDir` / `Prefix` が含まれれば `FileUnavailable`（`..` の正規化を試みない）
2. roots の実効化（呼び出しごと）: 各 root を `fs::canonicalize`。失敗・非ディレクトリ・`/`・`ProtectedPaths`（`config_dir` / `db_dir` / `extra`）と祖先・子孫関係にある root は無視して stderr に warn。実効 roots が空なら `NotConfigured`
3. 字句包含: 要求パスが設定値の root または実効 root のいずれかに `starts_with`（コンポーネント単位）でなければ、ファイルシステムに触れず `FileUnavailable`（roots 外の存在オラクルを作らない）
4. 要求パスを `fs::canonicalize`（symlink を全部辿った実体パス）。失敗 → `FileUnavailable`
5. 実体パスが実効 roots のいずれかに `starts_with` でなければ `FileUnavailable`。root 内から root 外を指す symlink はここで落ちる。root 内 → root 内の symlink は許容。実体パスが `ProtectedPaths` 配下なら `FileUnavailable`
6. open: unix では `OpenOptions::new().read(true).custom_flags(O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)`（最終コンポーネントの symlink 差し替えに追従しない。FIFO で永久ブロックしない）。開いたハンドルの `metadata()` が `is_file()` でなければ `FileUnavailable`（ディレクトリ・FIFO・デバイス・ソケット）。パスではなくハンドルを検査する
7. サイズ: `metadata.len() > max_bytes` なら読まずに `TooLarge`。`take(max_bytes + 1)` で読み、超えていれば（追記競合）`TooLarge`
8. 内容: NUL バイトを含む、または UTF-8 として不正なら `BinaryContent`（拡張子では判定しない。`gaia.db` や画像・鍵ファイルを返さない）
9. 読み取り中の I/O エラーは `ReadFailed`（`ErrorKind` は stderr のみ）

不在・root 外・通常ファイル以外・権限不足はすべて `FileUnavailable` の同一文言。canonicalize と open の間でパス要素が差し替えられる競合は残るが、脅威モデル（同一 OS ユーザー）では許容し §16 に明記する。

## 9. narumi 解決器（`gaia-mcp/src/sources/narumi.rs`）

`NarumiResolver { runtime: OnceLock<tokio::runtime::Runtime> }`。`availability`: `settings.narumi.is_some()` なら `Ready`、無ければ `Unconfigured { setting: "[sources.narumi].command" }`。`max_concurrency = 1`（bridge の多重起動を防ぐ）。

### 9.1 URI の解析 `parse_narumi_uri(uri: &str) -> Result<NarumiTarget, Reason>`（`narumi_uri.rs`、純関数）

```rust
pub struct NarumiTarget { pub meeting_id: String, pub version: Option<u32> }
```

- `Url::parse` で scheme `narumi`、host `meeting`（小文字固定）、path `/<meeting_id>` の 1 要素（末尾スラッシュ不可）を要求
- `meeting_id` は narumi 契約の形式（8 桁数字 `T` 6 桁数字 `Z-` 16 進小文字 8 桁、計 25 文字）を長さと文字種で手書き検査する（regex 依存なし）。合わなければ `InvalidUri { system: "narumi", rule: "meeting_id" }` を返し、**子プロセスを起動しない**
- query は `version=<1 以上の整数>` のみ許可。未知のキー・userinfo・ポートは `InvalidUri { rule: "query" / "userinfo" / "host" }`。`url` crate は `..` / `.` のドットセグメントを正規化するため、`narumi://meeting/../<id>` は `<id>` として解釈される（実装で確認済み。meeting_id 検査が最終防衛線）
- fragment は許可して無視する（`#t=1200` などクライアント向けヒントを URI に残せる。既存シード `minutes://meeting/42#t=1200` と同じ流儀）

### 9.2 同期境界

`resolve` は同期。`runtime()` が `OnceLock` で `Builder::new_multi_thread().worker_threads(1).thread_name("gaia-narumi").enable_all().build()` を初回に生成する。

```rust
fn resolve(&self, req: ResolveRequest<'_>) -> Result<Resolved, Unresolved> {
    let cfg = req.settings.narumi.clone().ok_or(Unavailable(NotConfigured { .. }))?;
    let target = parse_narumi_uri(&req.reference.uri).map_err(Unavailable)?;
    let scope = req.reference.scope.clone();
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let (tx, rx) = std::sync::mpsc::channel();
    self.runtime().spawn(async move { let _ = tx.send(fetch_minutes(cfg, target, scope).await); });
    rx.recv_timeout(timeout + Duration::from_secs(5))
        .unwrap_or(Err(Unavailable(TimedOut { secs: timeout.as_secs() })))
}
```

`Handle::block_on` / `Runtime::block_on` を使わないので、呼び出し元が tokio ワーカー・`spawn_blocking`・runtime 無し（CLI）・Tauri のどれでも解決中は panic しない。呼び出し元スレッドがブロックする間、非同期処理は常駐 runtime 上で進み、呼び出し元が先に諦めても子プロセスの終了処理は完走する。

drop 時: `impl Drop for NarumiResolver` が `OnceLock::take` した runtime を `shutdown_background()` で停止する。tokio の通常の `Runtime` drop は blocking pool の終了を待つため、async コンテキスト（`gaia serve --stdio` の EOF 後・`--http` の Ctrl-C 後に `Runtime::block_on` の内側で `ToolService` が drop される経路、および tokio ワーカー上の drop）では panic する。`shutdown_background`（= `shutdown_timeout(0)`）は blocking region の判定より前に返るので、どのコンテキストでも panic しない。その時点で進行中の解決があれば完了を待たず、子プロセスは `kill_on_drop` で消える。回帰テストは `tests/narumi_resolver.rs` の `resolver_can_be_dropped_inside_an_async_context_after_use`（`spawn_blocking` 内で解決してから async 本体で drop）。

### 9.3 起動・初期化・呼び出し・終了 `async fn fetch_minutes(cfg, target, scope) -> Result<Resolved, Unresolved>`

1. `tokio::process::Command::new(&cfg.command).args(&cfg.args)`。環境は親を継承し、`GAIA_` 接頭辞の変数を `env_remove`、`cfg.env` を `envs`。`kill_on_drop(true)`。process-wrap の `CommandWrap` で包み、`ProcessGroup::leader()` 相当のラッパーを付けてプロセスグループを作る（API 名は実装時に確認。§16）
2. `TokioChildProcess::builder(wrap).stderr(match cfg.stderr { Discard => Stdio::null(), Inherit => Stdio::inherit() }).spawn()`。失敗 → stderr に `warn!(kind = ?e.kind())`、`NarumiStartFailed`。stdin / stdout は rmcp が pipe にし **stdout は MCP 専用**（gaia 自身の stdout には触れない。stdio serve 中でも混ざらない）
3. `deadline = Instant::now() + timeout`。`timeout_at(deadline, ().serve(transport))` で initialize。経過 → `TimedOut`（transport の drop → `ChildWithCleanup::drop` の kill タスクが常駐 runtime で走る）。initialize 失敗（bridge が常駐サーバーを見つけられず即終了する典型）→ `NarumiHandshakeFailed`
4. `running.peer_info().server_info.name != "narumi"` → `cancel()` してから `NarumiNotNarumi`（誤設定で別サーバーを叩かない）
5. `timeout_at(deadline, running.peer().call_tool(CallToolRequestParams { name: "get_minutes", arguments: { meeting_id, version?, scope } }))`
6. 成功・失敗・タイムアウトのすべてで `let _ = running.cancel().await`（stdin close → 3 秒待ち → kill）。その後 transport が drop される
7. `map_get_minutes(result)` で `Resolved` に写す
8. cancel 後（および initialize 失敗・タイムアウト時）に unix の `libc::killpg(pgid, SIGKILL)` を無条件に送る（`grandchild` テストで `uv run` 相当の孫が残ることを観測したため。pgid = 子の pid で、グループが空なら ESRCH を無視する。子の終了直後に同じ pid が別のプロセスグループ leader として再利用される窓は数ミリ秒で、同一 OS ユーザーの脅威モデルでは許容する）

### 9.4 応答の解釈 `map_get_minutes(CallToolResult, &NarumiTarget, max_bytes) -> Result<Resolved, Unresolved>`（純関数。JSON フィクスチャでテスト）

- `structured_content` を優先し、無ければ `content[0]` のテキストを JSON として読む。どちらも無ければ `NarumiInvalidResponse`
- `is_error == Some(true)`: `error.code` を `[a-z_]{1,32}` に正規化して `NarumiError { code }`（`message` / `details` は stderr の warn にのみ出す）。`code` が無ければ `NarumiInvalidResponse`
- 成功: `markdown` が文字列であること、`meeting_id` が要求と一致することを検査（不一致は `NarumiInvalidResponse`）。`markdown` のバイト数が `[sources.narumi].max_bytes` を超えれば `TooLarge`（本文は返さない。切り詰めない）。`content = markdown`。`version` / `available_versions` / `generated_at` / `provider` があれば `Note::NarumiVersion`、`unresolved_speakers` が非空なら `Note::UnresolvedSpeakers { count }`。版を必ず伝えることで `version` 省略の参照でもエージェントがどの版を読んだか分かる

### 9.5 scope の受け渡し

gaia は `reference.scope`（その参照行の所属元名。常に単一）を narumi の `scope` に単一文字列で渡す。narumi 側の意味は「その scope ＋ unscoped」なので、unscoped の会議も scope 付きの会議もこれで届く。呼び出し側の実効 scope 集合は渡さない（§2）。運用規約として「narumi の scope 名 = gaia の affiliation 名」を README と §10 に書く。不一致なら narumi が `scope_denied` を返し、gaia は `NarumiError { code: "scope_denied" }` をそのまま返す（黙って別 scope を試さない）。

## 10. narumi 参照の URI 規約（gaia が定義する正本）

本節は narumi リポジトリからも参照できる独立した規約とする（narumi 側の対応は本リポジトリの範囲外で、依頼事項を末尾に記す）。

### 10.1 参照の形

```
system:   "narumi"
uri:      narumi://meeting/<meeting_id>[?version=<n>][#<fragment>]
  meeting_id  narumi の meeting_id（^[0-9]{8}T[0-9]{6}Z-[0-9a-f]{8}$）
  version     省略時は最新。narumi が propose するときは生成した版を必ず付ける
              （版は append-only なので、版付き URI は将来も同じ本文を指す）
  fragment    任意。gaia は解釈しない。クライアント向けヒント（例: #t=1200 = 秒位置）
title:    "<meeting_name> 議事録 v<n>"
note:     何が・どの粒度で・いつ時点か（会議日時、版、生成 provider）。必須
snapshot: 決定事項・要点の箇条書き（3〜10 行）。到達不能時の唯一のフォールバックなので必ず入れる
scope:    会議の scope と同じ affiliation 名で propose する
          （gaia は解決時に reference.scope を単一の scope として narumi へ渡す）
```

解決の意味: gaia は `[sources.narumi]` に設定したコマンドを子プロセスとして起動し、`get_minutes { meeting_id, version?, scope: <reference.scope> }` を呼び、`markdown` を `content` として返す。`reason` に `narumi minutes v<N> (available: 1, 2; generated_at ...; provider ...)` と話者不明の件数を添える。

`propose_update` の provenance 例:

```json
{
  "system": "narumi",
  "uri": "narumi://meeting/20260827T030500Z-a1b2c3d4?version=2",
  "title": "週次定例 議事録 v2",
  "note": "narumi meeting 20260827T030500Z-a1b2c3d4; minutes version 2; meeting occurred at 2026-08-27T03:05:00Z; provider none",
  "snapshot": "- オンボーディング資料を来週までに更新する\n- SCIM は Phase 2 で対応"
}
```

### 10.2 現行 narumi 参照の移行路（v0.2.0 で実際に動く経路）

現行の narumi エクスポータは `system = "file"`, `uri = file:///…/narumi/meetings/<meeting_id>/minutes/v<N>/minutes.md`（`snapshot` 無し）を送る。v0.2.0 では次の設定でこれらの参照がそのまま `file` 解決器で読める:

```toml
[sources.file]
roots = ["/Users/<me>/Library/Application Support/narumi/meetings"]   # NARUMI_HOME を変えている場合はそのパス
```

`narumi://` と narumi 解決器は、narumi 側のエクスポータが切り替わるまでは手入力（`gaia add ref` / デスクトップ）した参照でのみ使える。

### 10.3 narumi リポジトリへの依頼事項

- gaia エクスポータの provenance を `system = "narumi"`, `uri = "narumi://meeting/<id>?version=<n>"` に切り替える（`file://` 併記は不要。移行期間は gaia 側の `[sources.file] roots` で吸収する）
- `snapshot` に議事録の決定事項・要点（3〜10 行）を入れる
- scope 名を gaia の affiliation 名と一致させる運用を narumi の README に書く
- `--stdio-bridge` が転送する initialize 応答の `serverInfo.name` が `narumi` であることを確認する（gaia はこれで相手を検証する）

## 11. 同期 `ToolService::call` と async サーバー、配線

- `GaiaServer::call_tool`: `let service = self.service.clone(); let name = request.name.to_string(); tokio::task::spawn_blocking(move || service.call(&identity, &name, args)).await` に変更（全ツール一律。分岐を作らない）。`JoinError` は `internal` に写す。`Arc<ToolService>` は `Send + Sync`（`Db` は `Mutex<Connection>`、解決器は `Arc<dyn SourceResolver>`）
- MCP 側のリクエスト取り消しはブロッキング処理を止めない。各解決器の timeout が上限。v0.2.0 では受け入れる
- desktop `commands::call_tool`: `pub async fn call_tool(state: State<'_, DesktopState>, name: String, args: Value) -> Result<Value, Value>` にし、`tauri::async_runtime::spawn_blocking(move || runtime.service.call(&runtime.human, &name, args).map_err(|e| e.to_json())).await` を `internal` に写す。UI の `invoke` は元々 Promise なので変更不要
- CLI: メインスレッドで直接呼ぶ（runtime 不在でも narumi 解決器は常駐 runtime を自前で持つ）。プロセス終了時は `main` の return で終わり、子は `cancel` / drop で消える
- DB の Mutex は §6 のとおり解決前に解放する。`spawn_blocking` プール（既定 512）の占有は解決器の permit（file 4 / url 2 / narumi 1）が抑える
- shutdown: desktop / HTTP の停止時に進行中の解決は完了を待たない。narumi 子プロセスは `cancel` / transport drop で kill される。`ToolService` の drop（`gaia serve` では `block_on` の内側）で `NarumiResolver::drop` が常駐 runtime を `shutdown_background` で待たずに止める（§9.2）

## 12. テスト一覧（narumi 無し・ネットワーク無しで全部通る）

### 12.1 gaia-core 単体

`sources/net.rs`
- `ip_is_public` 表テスト: §7.3 の各範囲の境界値（`10.255.255.255` 拒否 / `11.0.0.0` 許可、`172.15.255.255` 許可 / `172.16.0.0` 拒否 / `172.31.255.255` 拒否 / `172.32.0.0` 許可、`169.254.169.254` 拒否、`100.64.0.0` 拒否 / `100.128.0.0` 許可、`::ffff:127.0.0.1` 拒否、`::ffff:93.184.216.34` 許可、`64:ff9b::7f00:1` 拒否、`2002:7f00:1::` 拒否、`fe80::1` / `fc00::1` / `ff02::1` / `2001:db8::1` 拒否、`2606:4700::1111` 許可）
- `host_is_allowed`: `*`、完全一致、接尾辞一致（`sub.example.com` は `example.com` に一致、`notexample.com` は不一致）、大文字入力の小文字化

`sources/url.rs`
- `check_url` 表テスト: `https://example.com/a.md` 許可、`ftp://` / `file://` / `javascript:` 拒否、`http://user:pw@example.com/` 拒否、`http://localhost/` / `http://foo.localhost/` / `http://intranet/` / `http://example.com./` 拒否、`http://127.0.0.1/` / `http://127.1/` / `http://2130706433/` / `http://0x7f.1/` / `http://0177.0.0.1/` / `http://[::1]/` / `http://[::ffff:127.0.0.1]/` / `http://169.254.169.254/latest/meta-data` 拒否（`url` の正規化後に IP として判定される）、`allow_hosts` 不一致で拒否
- `GuardedResolver` に偽の内側 Resolver を注入: 全件公開なら通る、1 件でも非公開なら `rejected` が立って失敗
- 実 HTTP 経路（`std::net::TcpListener` の固定応答サーバーを 127.0.0.1 で立て、`AddressPolicy::AllowLoopback` で疎通）: 200 `text/plain`、404 → `UpstreamStatus { 404 }`、`Content-Type: image/png` → `UnsupportedContentType`、`charset=shift_jis` → `UnsupportedContentType`、`Content-Length` 超過 → 読まずに `TooLarge`、長さ不明の超過 → 切り詰めと `BodyTruncated`、`text/html` → verbatim ＋ `HtmlAsIs`、`302 Location: http://169.254.169.254/` → `UrlNotAllowed { Address }`（本番ポリシーで別テスト）、リダイレクト 4 回 → `UrlNotAllowed { Redirects }`、応答遅延（`timeout_secs = 1`）→ `TimedOut`、各ホップは 1 秒未満だが合計で超えるリダイレクト（`timeout_secs = 1`）→ `TimedOut`（1.5 秒以内に返る）、`Set-Cookie` を次ホップに送らない、リクエストに `Accept-Encoding` が無い
- 本番ポリシーで `http://127.0.0.1:<port>/` が接続前に拒否される（サーバー側の accept が呼ばれない）

`sources/file.rs`（tempdir を root に）
- 通常ファイル成功、`..` を含むパス拒否、root 外（存在しないパスでも同じ文言）、ディレクトリ拒否、root 外を指す symlink 拒否、root 内を指す symlink 許容、`mkfifo` が即時に `FileUnavailable`（unix）、NUL 入りと非 UTF-8 が `BinaryContent`、`max_bytes` 超過が `TooLarge`、`file://otherhost/` と相対パスが `InvalidUri`、`%20` と日本語ファイル名のデコード、`ProtectedPaths`（config_dir / db_dir）を root にしても無視される、root が `/` は無視される、roots 空で `Unconfigured`
- `FileUnavailable` の 4 事象（不在・root 外・種別不可・権限不足）の文言が同一

`sources/mod.rs`
- `shape_content`: BOM 除去、C0 / DEL 除去と `ControlCharsRemoved`、`\t` `\n` `\r` は残す、多バイト・結合文字を含む文字境界の切り詰めと `ContentTruncated { chars, original }`、notes 無しで reason 省略
- `SourceRegistry`: `register` の重複が `Err`、`get` の trim ＋ 小文字化、`acquire` が `max_concurrency` を超えると `None`、`ready_systems` が `Availability` を反映
- `Reason` / `Note` の `Display` が固定文言で、`NarumiError { code }` と `UpstreamStatus { status }` 以外に可変部分を持たない

`config/tests.rs`
- `[sources]` 無しの既定値、round-trip 保存、既定値のままなら `[sources]` を書き出さない、各範囲外の拒否、相対 root / `/` / 重複の拒否、`allow_hosts` の不正値（IP リテラル・`localhost`・単一ラベル）拒否、`narumi.command` の相対パス拒否、`env` の `GAIA_` 接頭辞拒否、unknown key で全体が失敗

`tools/resolve_source.rs`（`test_support::service()` に `StubResolver`（system = `stub`、挙動を切替可能）を `with_sources` で登録）
- `{}` → `invalid_params`。`ref_id` 不在 / 他 scope の ref / `uri` 他 scope / 両指定不一致 → すべて同一文言の `not_found`
- `uri` 指定で同一 uri が複数あれば id 最大の行
- シードの `system = "minutes"` → `resolved=false`、`NoResolver` 文言、`reference.snapshot` が返る
- `Unconfigured` の解決器は `resolve` が呼ばれない
- Stub の `Ok` → `resolved=true`、切り詰めと reason 連結。`Unavailable` → `resolved=false` ＋ reason ＋ `SnapshotFallback`。`Internal` → `ResolverFailed` 文言のみ
- 同時実行: `max_concurrency = 1` の Stub を `Barrier` で待たせ、2 件目が `busy`
- 解決中に DB ロックを持たない: Stub が `Barrier` で待つ間に別スレッドの `get_person` が完了する
- DB 不変: 呼び出し前後で `refs.last_verified` と `audit_log` 件数が変わらない。複数 scope 明示時のみ `cross_scope_read` が 1 件増える
- `get_server_info.capabilities.resolvers` が `ready_systems` と一致し、`contract_version == "1.1.0"`
- 既存 `call_enforces_existence_role_and_input_schema` の `resolve_source` 断定を「`{}` は `invalid_params`」に置き換える。`handled_tools_are_enabled_contract_tools` は manifest と `HANDLED_TOOLS` の両方を変えて通す

### 12.2 gaia-mcp

`sources/narumi_uri.rs`: 表テスト（正常、`version`、fragment 無視、不正 meeting_id（長さ・大文字 hex・区切り）、余分な query、userinfo、ポート、`narumi://meeting/../x`、末尾スラッシュ、`narumi://Meeting/...`）

`sources/narumi.rs`: `map_get_minutes` の JSON フィクスチャ（正常 / `structured_content` 無しでテキスト JSON / isError 封筒の各 code / `markdown` 無し / `meeting_id` 不一致 / `unresolved_speakers` の件数 / `max_bytes` 超過で `TooLarge`・ちょうどは通る・バイト数で数える）

`src/bin/fake_narumi.rs`: rmcp の `ServerHandler` を stdio で提供する偽 narumi。`get_info` の `server_info.name` は `narumi`（`FAKE_NARUMI_MODE=wrong_name` のときだけ別名）。`get_minutes` の挙動を `FAKE_NARUMI_MODE` で切替: `ok`（受け取った `meeting_id` / `version` / `scope` を markdown に JSON で埋め込んで返す）/ `not_found` / `scope_denied` / `hang`（initialize 後に応答しない）/ `exit`（stderr に 1 行書いて即終了。bridge が常駐サーバーを見つけられない状況）/ `junk_stdout`（JSON でない行を stdout に吐く）/ `huge`（`max_content_chars` 超の markdown）/ `stderr_noise`（stderr に書く）/ `grandchild`（`sleep 300` を spawn してから応答を止める）

`tests/narumi_resolver.rs`（`env!("CARGO_BIN_EXE_fake_narumi")` を `[sources.narumi].command` にした `StaticSettings`）
- `ok`: content に markdown、`scope` が `reference.scope` の単一文字列、`version` の透過、`version` 省略時に応答の版が `NarumiVersion` に入る
- `not_found` / `scope_denied` → `NarumiError { code }` で区別される
- `hang`（`timeout_secs = 2`）→ `TimedOut`、呼び出しが 8 秒以内に返り、子の pid が消えている
- `exit` → `NarumiHandshakeFailed`。存在しないコマンド → `NarumiStartFailed`
- `wrong_name` → `NarumiNotNarumi`
- `junk_stdout` → rmcp 3.1.4 は JSON でない行を読み飛ばすため成功する。テストは `Ok` か固定文言（`NarumiHandshakeFailed` / `NarumiInvalidResponse` / `TimedOut`）のいずれかであることだけを断定し、rmcp 更新で挙動が変わっても panic しないことを確認する
- `huge` → 切り詰めと `ContentTruncated`。`max_bytes` を markdown のバイト数未満にすると `TooLarge`
- `stderr_noise` → `discard` / `inherit` の両方で結果が変わらない（親の stderr の捕捉はテストハーネスからできないため、出力の有無は断定しない）
- `grandchild` → cancel 後に孫が残らない（残るなら §9.3 の killpg 補助を入れてから通す）
- URI 規約違反で子が起動しない（`exit` モードでも reason は `InvalidUri`）
- 直列化: 2 件同時要求で子が同時に 2 つ起動せず、2 件目が `busy`
- gaia 自身の stdout に何も混ざらないことは、`crates/gaia/tests/resolve.rs` の `gaia --json resolve` が stdout を JSON として読めることで担保する（narumi 解決器は偽 narumi が gaia-mcp のテストバイナリにしか無いため、CLI からは検証していない。残リスクとして §16 に記す）

`server.rs`: `http.get_tool("resolve_source").is_some()` に反転。開始を通知してから Barrier で止まる Stub 解決器を登録した service で `resolve_source` を投げ、解決器が止まっている間に `get_server_info` が返ることを順序で断定してから Barrier を解放する（`spawn_blocking` の効果。壁時計には依存しない）

`http/tests/stateless.rs`: 未知ツール列から `resolve_source` を外し、`resolve_source` に `{}` を投げると 400 / `-32602` / `data.code == "invalid_params"` でセッションを作らないことを別に断定

### 12.3 gaia（統合）

- `tests/stdio.rs`: `tools/list` に `resolve_source` が含まれる。`tools/call resolve_source {ref_id}` で `system = minutes` が `resolved=false` になり `reference.snapshot` を含む（`isError` は false）
- `tests/http.rs`: agent キーで `resolve_source` が呼べ、無認証は 401。`tools/list` の断定を反転
- 新規 `tests/resolve.rs`: `gaia add ref` → `gaia resolve --ref-id`（JSON、`resolved=false`）、`--content` の終了コード 2、`[sources.file] roots` を（設定・DB とは別の）tempdir にした設定で `file://` の ref が `--content` で本文を出力し終了コード 0、`--uri` で同じ参照が引ける、設定ディレクトリを roots に入れても `config.toml` が読めない、設定から roots を消すと再起動なしで `resolved=false`（呼び出しごとの再読込）。`gaia --json info` の `capabilities.resolvers` の変化も確認する

### 12.4 desktop

- Rust: `state` テストで registry 注入後も既存の初期化・終了テストが通る。`call_tool` の非同期化はコンパイルと desktop ゲート（`cargo build --all-features --locked` / `cargo fmt --check` / `cargo clippy --all-targets --all-features --locked -- -D warnings` / `cargo test --all-features --locked`）で担保
- `bun test`: `contextApi.test.js` に `resolveReference` が `{ ref_id, scope }` で `call_tool("resolve_source")` を呼ぶこと。`contextViews.test.js` に「内容を取得」ボタンの存在、`resolved=false` 時の reason 表示と snapshot 展開、`<script>` を含む content がテキストとして描画される（エスケープされる）こと
- 実機（narumi あり）: 自動テストは作らず、§15 の手動確認手順に従う

## 13. 文書更新箇所

`AGENTS.md`（`CLAUDE.md` は symlink）
- リポ構成: 利用境界に `sources`（`SourceResolver` / `SourceRegistry` / `SourceSettings` / `ProtectedPaths` / `Reason` / `Note`）と `config::SourcesConfig` を追加。「narumi 解決器は gaia-mcp。CLI / desktop は `gaia_mcp::sources::registry` 経由でのみ注入」「gaia-core はネットワークを ureq、URL を url で扱い rmcp と tokio を知らない」
- 絶対原則 1 の補足: `resolve_source` は DB に書かない。唯一の例外は複数 scope 明示時の `audit_log(cross_scope_read)`
- 公開ツール: `resolve_source` を「実装済み・登録済み（v0.2.0）」に移す。要点: `system` 別解決器（core: `file` / `url`、mcp: `narumi`）、既定は全解決器無効、`uri` は登録済み参照の検索キーで実体化は常に DB の `reference.uri`、失敗は `resolved=false` ＋ 固定文言 reason、参照不在は scope 外と同一文言、`get_server_info.capabilities.resolvers` は設定済み解決器名。契約版 1.1.0 と版付け規則（ツールの enabled 化・description 変更は minor）
- HTTP 接続とキー管理: 設定をリクエストごとに読み直す対象に `[sources]` を含める。agent キーで `resolve_source` を呼ぶと human の機械で設定済みのファイル読取・外向き HTTP・narumi 子プロセス起動が起きるので `[sources]` を狭く保つ
- 新節「参照解決（resolve_source）」: url の SSRF 規則、file の roots / 常時拒否、narumi の起動コマンドは設定のみ・stdout は MCP 専用・stderr は既定で破棄・scope は参照行の 1 つだけ、`ToolService::call` は MCP / desktop では `spawn_blocking` で呼ぶ、URI と content をログに出さない、reason は固定文言
- 開発ルール: `cargo test` は偽 narumi（`crates/gaia-mcp/src/bin/fake_narumi.rs`）で通る。実 narumi の確認は手動。ureq は `=3.4.0` 固定の理由

`README.md`
- 新節「参照の実体取得（resolve_source）」: できること、`[sources]` の設定例（§5）、既定は全部無効、`allow_hosts = ["*"]` の意味と危険（エージェントが任意公開ホストへ GET できる = URL クエリ経由の持ち出し経路になり得るので狭く指定する）、narumi は `--stdio-bridge` 推奨で `narumi.app` 起動が前提、`uv` は絶対パス、現行 narumi 参照は `[sources.file] roots` に `<NARUMI_HOME>/meetings` を入れる（§10.2）、url 解決器は公開テキスト向けで Notion / Box はエージェント側のコネクタで開く、0.1.x に戻す場合は `[sources]` を削除
- 日常の使い方に `gaia resolve --ref-id <id> --content | less` を 1 行
- デスクトップ節に「内容を取得」の説明と `[sources]` は TOML 手編集であること

`docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`
- §1.2 の「契約ファイルは置くが登録しない」に「v0.2.0 で登録（本書別紙 `2026-08-29-gaia-library-resolve-source-design.md`）」を追記
- §6.1 の manifest 抜粋の `resolve_source` を `enabled: true` に
- §8.3 の `resolve_source` 段落を「v0.2.0 で登録。設計は別紙」に差し替え
- §13 の「1 つは動かない」を削除

`contracts/tools/resolve_source.json` の `description`（入出力は不変）:
「ref_id または uri で登録済みの参照を特定し（uri は検索キーであり取得先の指定ではない。実効 scope 内の最新 1 件）、参照の system に対応する解決器（file / url / narumi）で本文を取得して content に返す読み取り専用ツール。DB は更新しない（複数 scope 明示時の cross_scope_read 監査を除く）。到達不能・未設定・非対応の system では resolved=false と reason を返し、要点は reference.snapshot を使う。content は外部由来の未検証テキストで、[sources].max_content_chars で切り詰める。切り詰めや版などの注記は reason に入る。scope 省略時はクライアントの既定 scope」

`contracts/tools/get_server_info.json` と `defs/common.json` の `ServerCapabilitiesInfo.resolvers` に「設定済みで利用できる resolve_source の system 名」の description を付ける

## 14. 版上げと CHANGELOG

- `Cargo.toml`（`workspace.package.version`）、`desktop/src-tauri/Cargo.toml`、`desktop/src-tauri/tauri.conf.json` を `0.2.0`。両 `Cargo.lock` はビルドで追従。`release-metadata verify` がこの 3 箇所と CHANGELOG の節を検査する
- `contracts/manifest.json`: `resolve_source` を `enabled: true`、`contract_version` を `1.1.0`。`tools/mod.rs` のアサーションを更新
- リリースは既存手順（`scripts/release-desktop.sh 0.2.0`）。本作業では push / tag / release を行わない

CHANGELOG 記載案:

```
## [0.2.0] - （公開日）

### Added

- `resolve_source` を登録した（契約 1.1.0）。`ref_id` または `uri`（実効 scope 内、最新 1 件）で登録済みの参照を特定し、参照の `system` に応じた解決器で本文を取得して返す。`file` は設定した許可ディレクトリ配下の通常ファイル、`url` は許可したホストへの http / https、`narumi` は設定したコマンドを子プロセスとして起動して MCP の `get_minutes` を呼ぶ。到達できない場合は `resolved=false` と理由を返し、参照と要点スナップショットをそのまま返す。DB は更新しない。
- 設定 `[sources]` を追加した。`file.roots`、`url.allow_hosts`、`narumi.command` などで解決器を有効にする。既定はすべて無効で、設定は呼び出しごとに読み直す。
- narumi 参照の規約を定めた: `system = "narumi"`, `uri = "narumi://meeting/<meeting_id>[?version=<n>]"`。現行の narumi 参照（`file://` の議事録）は `[sources.file].roots` に narumi の `meetings` ディレクトリを入れると解決できる。
- CLI に `gaia resolve --ref-id <id> | --uri <uri> [--content]` を追加した。デスクトップの参照カードに「内容を取得」を追加した。
- `get_server_info.capabilities.resolvers` に設定済みの解決器名を返すようにした。

### Changed

- MCP サーバーとデスクトップのツール呼び出しをブロッキング用スレッドで実行し、時間のかかる参照解決が JSON-RPC の応答や他のセッション・画面を止めないようにした。
- `[sources]` を含む設定ファイルは 0.1.x では読めない。戻す場合は該当節を削除する。既定値のままなら `[sources]` は書き出さない。

### Security

- `resolve_source` は入力の `uri` を取得先に使わず、承認済み参照の `uri` だけを実体化する。scope 外の参照と存在しない参照は同じ `not_found` を返す。
- `url` 解決は http / https のみ。userinfo 付き URL、`localhost`、ループバック・プライベート・リンクローカル・メタデータ・予約アドレスを DNS 解決後のアドレスでも拒否し、リダイレクトは上限付きで各段を再検査する。プロキシ環境変数と圧縮伸長を使わず、Cookie や認証ヘッダーを送らない。応答はテキスト系 Content-Type とサイズ上限に限る。
- `file` 解決は許可ディレクトリ配下の通常ファイルに限り、symlink を解決した実体で判定し、`O_NOFOLLOW` で開いたハンドルを検査する。設定ディレクトリ・DB ディレクトリ・キー退避ディレクトリは常に対象外。バイナリは返さない。
- `narumi` の起動コマンドは設定ファイルからのみ読み、ツール引数では指定できない。子プロセスの stdout は MCP 専用、stderr は既定で破棄し、タイムアウトで停止する。narumi へは参照行の scope 1 つだけを渡す。解決器ごとに同時実行数を制限する。
- `resolve_source` の理由文言は固定文言のみで、上流のメッセージ・パス・IP・コマンドを含めない。URI と取得内容はログに残さない。
```

## 15. 実装順序と手動確認

実装順序（AGENTS.md の「契約 → `cargo build` → 実装」）:

1. `contracts/manifest.json`（enabled / 1.1.0）と `resolve_source.json` の description → `cargo build`（typify の生成を確認）→ `HANDLED_TOOLS` と既存テストの反転（§3 の 5 箇所）
2. `config.rs` の `[sources]` と validate → `config/tests.rs`
3. `storage/refs.rs::latest_by_uri`
4. `sources/mod.rs`（トレイト・レジストリ・Reason・shape_content）→ `tools/resolve_source.rs` と Stub テスト → `get_server_info`
5. `sources/net.rs` → `sources/url.rs` → `sources/file.rs`
6. workspace / core の依存変更と `Cargo.lock`
7. gaia-mcp: rmcp features、`sources/narumi_uri.rs` → `bin/fake_narumi.rs` → `sources/narumi.rs` → `tests/narumi_resolver.rs` → `server.rs` の `spawn_blocking` → `sources/mod.rs::registry`
8. CLI: `App::open` の注入、`gaia resolve`、統合テスト
9. desktop: 注入、`call_tool` の async 化、UI、`bun test`、desktop ゲート
10. 文書（§13）、版上げと CHANGELOG（§14）

手動確認（narumi あり。CI には入れない）:

1. `narumi.app` を起動し、`uv --directory <narumi> run narumi-server --stdio-bridge` が単体で initialize できることを確認する
2. `config.toml` に `[sources.narumi]`（`command` は `which uv` の絶対パス）と `[sources.file] roots = ["<NARUMI_HOME>/meetings"]` を書く
3. `gaia info` の `capabilities.resolvers` に `file` と `narumi` が出る
4. narumi が propose した既存の `file://` 参照を `gaia resolve --ref-id <id> --content` で読める
5. `gaia add ref --system narumi --uri "narumi://meeting/<id>?version=1" ...` を登録し `gaia resolve` で markdown が返り、`reason` に版注記が出る
6. `narumi.app` を終了して同じ参照を解決すると `NarumiHandshakeFailed` の文言と snapshot が返り、`ps` に子が残らない
7. デスクトップで同じ参照の「内容を取得」を押し、取得中に検索など他の操作が固まらない

## 16. リスクと未検証事項

- process-wrap 9.0 のプロセスグループ / kill-on-drop ラッパーの API 名は未検証（ローカル registry に無い）。実装時に `cargo fetch` して確認し、`grandchild` テストで `uv run` の孫が残らないことを観測する。残る場合は unix の `libc::killpg` を gaia-mcp に足す。README では `uv run` の代わりに venv 内の `narumi-server` 実行ファイルを直接指定する選択肢も案内する（孫プロセスと uv の暗黙ネットワーク取得を避ける）
- `--stdio-bridge` が initialize 応答の `serverInfo.name` を書き換えていないことは実機で確認する（書き換えていれば `NarumiNotNarumi` の検査を bridge の名前にも広げる）
- `ureq` の `unversioned::resolver` は安定 API の外。`=3.4.0` に固定し、更新時は `GuardedResolver` のテストで気付く
- ureq が `Accept-Encoding` を送らないこと（gzip feature 無効時）は実装時にキャプチャで確認する
- `file` 解決器の canonicalize と open の間の差し替え競合（TOCTOU）は同一 OS ユーザーの脅威モデルでは許容する。`O_NOFOLLOW` とハンドル検査で最終要素の差し替えだけは防ぐ
- `url` 解決器はホスト allow と IP 範囲で守るが、`allow_hosts = ["*"]` にするとエージェントが URL クエリに情報を載せて公開ホストへ送る持ち出し経路になり得る。README で狭い指定を推奨する（残リスク）
- macOS Keychain 上の企業 CA は使わない（rustls ＋ webpki-roots）。社内 CA の HTTPS は解決できない
- MCP のリクエスト取り消しはブロッキング処理を止めない。各解決器の timeout が上限
- narumi 解決器の `[sources.narumi].max_bytes` は受信済みの `get_minutes` 応答に対する検査で、rmcp の子プロセス transport には受信バイト数の上限を設定できない。巨大な応答は timeout まではメモリに載る（子プロセスは同一 OS ユーザーが設定したコマンドなので、脅威モデル上は許容する残リスク）
- `[sources]` を含む設定は 0.1.x で読めない（既定値のままなら書き出さないので、`[sources]` を触っていなければ影響しない）
- narumi 解決器の「gaia の stdout に混ざらない」ことは stdio serve では実機で確認する（自動テストは CLI の JSON 出力と偽 narumi の統合テストに分かれており、stdio serve ＋ narumi 解決の組み合わせは未検証）
- `killpg` は pgid の再利用窓（子の終了直後、グループが空になってから別プロセスが同じ pid で leader になるまで）で無関係なプロセスに届く可能性が理論上ある。同一 OS ユーザーの脅威モデルでは許容する
- `Mutex<Connection>` 単一接続のため、解決前にロックを解放しても DB 操作自体は直列。解決器の permit で `spawn_blocking` プールの占有を抑える
- `NarumiResolver::drop` は runtime を待たずに止めるため、呼び出し元が `recv_timeout` で諦めた直後に `ToolService` が drop されると、進行中だった `cancel`（graceful shutdown）は完走しない。子は `kill_on_drop` で SIGKILL され、孫は killpg に到達しない可能性がある（通常の運用では drop はプロセス終了時のみで、その場合は子の stdin も閉じる）

# gaia-library サブプロジェクト C: デスクトップアプリ 設計書（2026-08-27）

## 1. 概要

gaia-library の**主 UI** となる Tauri 2 デスクトップアプリ。アプリを起動するだけで DB とサーバー（Streamable HTTP）が立ち上がり、検索・閲覧・手入力・提案の承認ができる。自動更新は tauri-plugin-updater（solo-eikaiwa と同方式）で行い、同梱した CLI `gaia` もアプリ更新と同時に更新される。CLI / MCP（stdio・HTTP）でも同じ操作ができ、アプリは特権経路を持たない（すべて `ToolService` 経由）。

- 前提: A（基盤）・B（HTTP＋認証）の設計書。および solo-eikaiwa の desktop/ 実装（updater・署名・リリーススクリプト・E2E 手順の移植元）
- 2026-08-27 決定: ホスティングは**アプリ内蔵（in-process）＋ CLI 同梱**、UI は **React ＋ TypeScript ＋ Vite ＋ Tailwind（bun 管理）**

## 2. 決定事項

| 項目 | 決定 | 理由 |
| --- | --- | --- |
| ホスティング | アプリの Rust 側が `gaia-core` を直接呼ぶ。HTTP サーバー（B の `serve_http`）もプロセス内で起動 | sidecar 管理（孤児回収・ポート身元確認 等）を丸ごと回避。堅牢性・性能とも最良（2026-08-27 比較検討済み） |
| UI 経路 | webview → Tauri commands → `ToolService::call`（human 識別） | UI = API パリティを「同一関数を呼ぶ」ことで担保。コマンドは薄い写しに限定 |
| CLI 同梱 | `gaia` を externalBin として `.app` に同梱し、`~/.local/bin/gaia` への symlink を初回起動時に案内 | アプリ更新で CLI も一緒に更新される（更新経路 1 本） |
| 自動更新 | tauri-plugin-updater。minisign 鍵 `~/.tauri/gaia-library-updater.key`（新規生成）、公開鍵は tauri.conf.json にコミット、endpoint は GitHub Releases `latest.json` | solo-eikaiwa で実証済みの方式・資産を流用 |
| 署名・公証 | Developer ID ＋ notarytool を release script に組込（シークレットは `~/.config/gaia-library/release.env`） | 証明書・手順とも solo-eikaiwa で確立済み |
| アプリ ID / 名称 | identifier `com.local.gaia-library.desktop`、productName `gaia-library` | solo-eikaiwa の命名前例に従う |
| workspace との関係 | `desktop/src-tauri` は cargo workspace に**入れない**（独立プロジェクト。path 依存で `gaia-core` / `gaia-mcp` を参照） | `cargo test --workspace` に Tauri ビルドを混ぜない。CI は desktop 変更時のみ検証 |
| データ・設定 | A と同じ（`~/.config/gaia-library/config.toml`・`~/.local/share/gaia-library/gaia.db`） | アプリと CLI で単一の正 |
| キー保管 | 発行した agent キーの平文を OS キーチェーンへ（`keyring` crate。失敗時は 0600 ファイルにフォールバックし設定画面に明示） | 接続スニペットの再表示のため。config はハッシュのみ（B） |

## 3. ディレクトリ構成

```
desktop/
├── ui/                          # React + TS + Vite + Tailwind（bun 管理）
│   ├── package.json / bun.lock / vite.config.ts / tsconfig.json
│   └── src/
│       ├── main.tsx / App.tsx
│       ├── api.ts               # invoke ラッパ（call_tool / admin 系）
│       ├── screens/{Search,Detail,Proposals,AddForms,Settings}.tsx
│       └── components/          # 一覧・カード・フォーム部品
├── src-tauri/
│   ├── Cargo.toml               # workspace 外。path 依存: ../../crates/gaia-core, ../../crates/gaia-mcp
│   ├── tauri.conf.json          # frontendDist: ../ui/dist, externalBin: binaries/gaia, updater pubkey/endpoint
│   ├── tauri.updater-artifacts.conf.json   # createUpdaterArtifacts overlay（リリース/E2E 時のみ）
│   ├── capabilities/default.json
│   ├── icons/                   # cargo tauri icon で生成
│   └── src/
│       ├── main.rs / lib.rs     # setup: 状態構築 → HTTP 起動 → tray/menu → updater 起動時チェック
│       ├── state.rs             # AppState { service: Arc<ToolService>, config_path, http: BoundServer }
│       ├── commands.rs          # Tauri commands（§5）
│       ├── first_run.rs         # 初回セットアップ（affiliation 作成 = gaia init 相当）
│       ├── cli_link.rs          # ~/.local/bin/gaia symlink の作成・検査
│       ├── keychain.rs          # keyring による平文キー保管
│       ├── updater.rs           # solo-eikaiwa desktop/src-tauri/src/updater.rs の移植（文言を gaia 用に）
│       └── updater_signature.rs # 同 移植（検証 bin verify-updater-signature を含む）
├── build-app.sh                 # ui build（bun）＋ cargo build --release -p gaia → binaries/gaia-<triple>
└── e2e-updater/                 # ローカル HTTP 配信での実機更新 E2E（solo-eikaiwa 手順の移植）
scripts/release-desktop.sh       # solo-eikaiwa release-desktop.sh の移植（§8）
scripts/check-updater-key-policy.sh  # 鍵継続性チェック（公開鍵の読出し先を tauri.conf.json に）
CHANGELOG.md                     # Keep a Changelog。リリースノートの正本
```

## 4. アプリのライフサイクル

1. 起動 → `config.toml` を読む。無ければ**初回セットアップ画面**（affiliation 名・ユーザー名を入力 → `gaia init` 相当を実行し、human クライアント `desktop:<user>` と既定 agent クライアント `claude-code` を作成 → agent キーを発行しキーチェーンへ）
2. `Db::open` → `Arc<ToolService>` 構築 → B の `serve_http` をプロセス内で起動（ポートは config、既定 4111。起動失敗は画面に表示して UI は継続）
3. メニューバー（tray）＋メインウィンドウ。アプリメニューに「アップデートを確認…」（solo-eikaiwa と同じ配置・状態表示）
4. 起動時に updater の非ブロッキングチェック（失敗は無言スキップ）
5. 終了 → HTTP サーバーを graceful shutdown（DB は都度クローズ不要。WAL）

- 多重起動: 2 個目のアプリはポート bind 失敗を検知して「既に起動しています」を表示して終了する（DB は WAL で壊れないが、常駐は 1 つに保つ）
- アプリ非起動時もエージェントは stdio（`gaia serve --stdio`）で従来どおり動く（任意依存の原則）

## 5. Tauri commands（すべて薄い写し。特権経路を作らない）

| command | 内容 |
| --- | --- |
| `call_tool(name, args) -> Value` | `ToolService::call(human識別, name, args)`。UI の検索・閲覧・提案・承認は全部これ |
| `admin_affiliation_add(name, identity?)` / `admin_affiliation_list()` | `gaia_core::admin`（audit 付き） |
| `admin_client_add(name, role, default_scope?, generate_key)` / `admin_client_list()` / `admin_client_keygen(name)` | config 更新＋キー発行。平文はキーチェーンへ保存し UI に返す |
| `mcp_config_snippet(name, transport) -> String` | 接続スニペット（キーはキーチェーンから復元） |
| `server_status() -> {url, ok}` | HTTP サーバーの状態 |
| `cli_link_status()` / `cli_link_create()` | `~/.local/bin/gaia` symlink の検査・作成 |
| `check_updates()` | updater の手動チェック（メニューと同じ経路） |

human 識別はアプリ保持の固定値（初回セットアップで作った human クライアント）。承認操作が human 限定であることは A の `ToolService` がそのまま強制する。

## 6. 画面（v0.1.0）

1. **検索**: クエリ＋scope 切替 → `search_context` の entities / glossary / interactions を要点＋参照（system・note・URI）付きで表示。参照はクリックでコピー／`opener` で外部を開く。エンティティから詳細へ
2. **詳細**: 人物・組織・案件（`get_person` / `get_organization` / `get_engagement`）。facts の履歴（superseded）表示、refs 一覧、案件は関係者・用語集・直近 interactions
3. **提案キュー**: `list_proposals`（pending 既定）。patch と provenance を整形表示し、承認／却下（却下理由入力）。承認結果から詳細へ
4. **手入力**: add 系フォーム（person / org / engagement / fact / ref / glossary / interaction）。`propose_update`＋`approve_proposal` を 1 操作で。scope は既定 scope をプリセット
5. **設定**: affiliation 管理／クライアント管理（キー発行・接続スニペット表示・コピー）／サーバー状態（URL・ポート）／CLI symlink 状態と作成ボタン／バージョンと「アップデートを確認」

## 7. 自動更新（solo-eikaiwa 移植）

- `updater.rs`: 起動時非ブロッキングチェック → ネイティブダイアログ（ja/en）→ DL（停滞 90 秒・上限 20 分の watchdog）→ 適用 → 再起動確認。メニュー項目の状態表示・単一実行ガード・E2E 自動承認フック（`GAIA_UPDATER_AUTO=1`）まで同型。文言のみ gaia-library 用に変更
- `updater_signature.rs` ＋ `verify-updater-signature` bin: 公開前に実行時と同じ `minisign-verify` で `.app.tar.gz` と `.sig` を照合
- 鍵: `cargo tauri signer generate -w ~/.tauri/gaia-library-updater.key`（ユーザーが 1 回実行。**秘密鍵を失うと既存ユーザーへ更新を届けられなくなるため必ずバックアップ**）。公開鍵は `tauri.conf.json` の `plugins.updater.pubkey`
- endpoint: `https://github.com/btajp/gaia-library/releases/latest/download/latest.json`（platforms: `darwin-aarch64`。url は `.app.tar.gz`、signature は `.sig` の中身）
- sidecar が無いため solo-eikaiwa の「旧 sidecar 回収（#270）」は不要。ただし**旧バージョンの常駐アプリが HTTP ポートを掴んだまま**のケースは、更新後の再起動が同一プロセスの置換なので発生しない（相当問題なし）ことを E2E で確認する

## 8. リリース（scripts/release-desktop.sh、solo-eikaiwa 移植）

順序: push 済み clean な `main` のみ → `check-updater-key-policy.sh`（設定鍵・署名鍵・直前リリース鍵の一致。ローテーションは `--allow-pubkey-rotation` の橋渡しのみ）→ バージョン整合（root `Cargo.toml` workspace.package.version ＝ desktop Cargo.toml ＝ tauri.conf.json ＝ CHANGELOG 節、タグ未使用）→ `cargo fmt/clippy/test --workspace` ＋ desktop の `cargo test` → `desktop/build-app.sh` → `cargo tauri build --config tauri.updater-artifacts.conf.json`（Developer ID 署名・公証は env から bundler が自動実行）→ 生成物検証（codesign/staple/spctl ＋ minisign 検証）→ dmg 公証＋staple → `latest.json` / `checksums.txt` 生成 → `gh release create --draft` → publish → 事後スモーク手順表示

- `release.env`（`~/.config/gaia-library/release.env`、初回にテンプレート生成）: `APPLE_SIGNING_IDENTITY` / `APPLE_API_KEY` / `APPLE_API_ISSUER` / `APPLE_API_KEY_PATH` / `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- whisper 相当のプレ署名工程は不要（Resources に Mach-O を置かない。externalBin の `gaia` は bundler が署名する）

## 9. CI

- 既存 `verify.yml` に desktop ジョブを追加: desktop/ 変更時のみ、`desktop/build-app.sh` → `cd desktop/src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- ui: `bun install --frozen-lockfile && bun run build`（build-app.sh に含む）。toolchain 版は `toolchain.json`（bun / tauri-cli）をリポジトリ直下に置き pin

## 10. テスト

- Rust 単体: updater 純関数（solo-eikaiwa のテスト群を移植）、first_run のバリデーション、cli_link のパス判定、keychain フォールバック判定
- E2E（手動・リリース前必須）: `desktop/e2e-updater/README.md` — ローカル HTTP で `latest.json` を配信し、旧 → 新（v99.0.0）の実機更新・再起動・HTTP サーバー再開までを `GAIA_UPDATER_AUTO=1` で通す
- UI: v0.1.0 は Playwright 等を導入しない（画面が安定してから）。`bun run build`（tsc --noEmit 込み）を型ゲートとする
- 注意: `tauri.conf.json` の externalBin 参照により、`desktop/build-app.sh` 実行前は src-tauri の `cargo build/test` が失敗する（solo-eikaiwa で実測済みの挙動。README に明記）

## 11. リスク・残論点

- `keyring` crate の macOS 動作（Keychain プロンプト頻度）は実装時に検証。不可ならフォールバック（0600 ファイル）を既定にし設定画面に明示
- アイコンは仮アイコン（`cargo tauri icon` で単色 PNG から生成）で開始し、後で差し替え
- Intel mac / Windows / Linux は対象外（latest.json の platforms は darwin-aarch64 のみ。将来追加可能）
- UI の webview から `http://127.0.0.1:4111` へ直接 fetch はしない（CSP は既定のまま、データ取得はすべて Tauri commands）
- Claude Code / Codex の HTTP MCP クライアント互換は B の統合テスト＋実機確認に依存

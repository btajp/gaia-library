# gaia-library サブプロジェクト C: デスクトップアプリ 設計書（2026-08-27）

## 1. 概要

gaia-library の**主 UI** となる Tauri 2 デスクトップアプリ。アプリを起動するだけで DB とサーバー（Streamable HTTP）が立ち上がり、検索・閲覧・手入力・提案の承認ができる。自動更新は tauri-plugin-updater（solo-eikaiwa と同方式）で行い、同梱した CLI `gaia` もアプリ更新と同時に更新される。内容の読み書きは CLI / MCP と同じ `ToolService` を通り、設定と affiliation 管理だけは既存の管理 API を使う。

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
| workspace との関係 | `desktop/src-tauri` は cargo workspace に**入れない**（独立プロジェクト。path 依存で `gaia-core` / `gaia-mcp` を参照） | `cargo test --workspace` に Tauri ビルドを混ぜない。CI は desktop と依存コード・設定の変更時に検証（§9） |
| データ・設定 | A と同じ（`~/.config/gaia-library/config.toml`・`~/.local/share/gaia-library/gaia.db`） | アプリと CLI で単一の正 |
| キー保管 | 発行した agent キーの平文を OS キーチェーンへ（`keyring` crate。失敗時は 0600 ファイルにフォールバックし設定画面に明示） | 接続スニペットの再表示のため。config はハッシュのみ（B） |

## 3. ディレクトリ構成

```
desktop/
├── ui/                          # React + TS + Vite + Tailwind（bun 管理）
│   ├── package.json / bun.lock / vite.config.ts / tsconfig.json
│   └── src/
│       ├── main.tsx / App.tsx
│       ├── api.ts / contextApi.ts / settingsApi.ts  # invoke ラッパ
│       └── components/          # 検索・詳細・提案・入力・設定と共通部品
├── src-tauri/
│   ├── Cargo.toml               # workspace 外。path 依存: ../../crates/gaia-core, ../../crates/gaia-mcp
│   ├── tauri.conf.json          # frontendDist: ../ui/dist, externalBin: binaries/gaia, updater pubkey/endpoint
│   ├── tauri.updater-artifacts.conf.json   # createUpdaterArtifacts overlay（リリース/E2E 時のみ）
│   ├── capabilities/default.json
│   ├── icons/                   # cargo tauri icon で生成
│   └── src/
│       ├── main.rs / lib.rs     # setup: 状態構築 → HTTP 起動 → tray/menu → updater 起動時チェック
│       ├── state.rs             # 初期化状態、human、実 config/DB パス、HTTP の所有と終了
│       ├── commands.rs / settings_commands.rs  # Tauri commands（§5）
│       ├── client_settings.rs  # クライアント設定・キー・接続設定
│       ├── lifecycle.rs        # 常駐、再表示、終了・再起動時の HTTP 停止
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
2. `Db::open` → `Arc<ToolService>` 構築 → B の `serve_http` をプロセス内で起動（ポートは config、未指定なら 4111〜4114。キー未発行や起動失敗は画面に表示して UI は継続）
3. メニューバー（tray）＋メインウィンドウ。アプリメニューに「アップデートを確認…」（solo-eikaiwa と同じ配置・状態表示）
4. 起動時に updater の非ブロッキングチェック（失敗は無言スキップ）
5. 終了 → HTTP サーバーを graceful shutdown（DB は都度クローズ不要。WAL）

- 多重起動: single-instance plugin により 2 個目は既存ウィンドウを表示する。ポートの候補が複数あるため、bind 失敗を多重起動の検知には使わない
- ウィンドウを閉じる操作は非表示にするだけ。トレイ・Dock から復帰し、アプリの終了操作で HTTP を停止する
- 初回設定と設定更新は終了処理と直列化する。IPC の待機が取り消されても、進行中の保存を完了してから終了する
- アプリ非起動時もエージェントは stdio（`gaia serve --stdio --client <name>`）で従来どおり動く（任意依存の原則）

## 5. Tauri commands（すべて薄い写し。特権経路を作らない）

| command | 内容 |
| --- | --- |
| `is_initialized()` / `first_run_setup(affiliation, user_name)` | 未初期化と起動失敗を区別。初期設定を新規公開し、`{agent_key, storage}` を返す |
| `call_tool(name, args) -> Value` | `ToolService::call(human識別, name, args)`。UI の検索・閲覧・提案・承認は全部これ |
| `admin_affiliation_add(name, identity?)` / `admin_affiliation_list()` | `gaia_core::admin`（audit 付き） |
| `admin_client_add(name, role, default_scope?, generate_key)` / `admin_client_list()` / `admin_client_keygen(name)` | config 更新＋キー発行。発行結果は `{key, storage: {location, error}}`。追加でキー未発行なら null |
| `mcp_config_snippet(name, transport) -> {text, key_storage}` | HTTP は実 URL と現在の設定に一致した保管キーを使う。stdio は実 config/DB/同梱 CLI の絶対パス |
| `server_status() -> {url, error, client, default_scope}` | HTTP サーバーと起動時に選んだ human の状態。停止・失敗時の URL は null |
| `cli_link_status()` / `cli_link_create(expected_target?)` | `~/.local/bin/gaia` symlink の検査・作成。null は新設のみ、文字列は確認した旧リンク先との一致が必須 |
| `check_updates()` | updater の手動チェック（メニューと同じ経路） |

human 識別はアプリ保持の固定値（初回セットアップで作った human クライアント）。承認操作が human 限定であることは A の `ToolService` がそのまま強制する。

`storage.location` / `key_storage` は `keychain` / `file` / null。保管先の両方が失敗した場合も、有効な発行済みキーとコピーを促す警告を返し、画面からキーを失わないようにする。
既存設定は既定 human、または唯一の human を使う。human 不在・曖昧な設定・壊れた TOML を初回設定として上書きしない。

## 6. 画面（v0.1.0）

1. **検索**: クエリ＋scope 切替 → `search_context` の entities / glossary / interactions を要点＋参照（system・note・URI）付きで表示。参照 URI はコピーのみ。人物・組織・案件から詳細へ
2. **詳細**: 人物・組織・案件（`get_person` / `get_organization` / `get_engagement`）。現行 facts、refs 一覧、案件は関係者・用語集・直近 interactions。置換済み facts の全履歴取得は現契約では未対応
3. **提案キュー**: `list_proposals`（pending 既定）。patch と provenance を整形表示し、承認／却下（却下理由入力）。承認結果から詳細へ
4. **手入力**: add 系フォーム（person / org / engagement / fact / ref / glossary / interaction）。`propose_update`＋`approve_proposal` を 1 操作で。scope は既定 scope をプリセット
5. **設定**: affiliation 管理／クライアント管理（キー発行・接続スニペット表示・コピー）／サーバー状態（実 URL・失敗理由）／CLI symlink 状態と明示操作／バージョンと「アップデートを確認」

scope・検索語・詳細対象の変更時は、要求の世代番号と表示キーの一致で旧応答を除外する。入力・承認は二重送信を抑止し、再試行でも同じ request_id / proposal_id を使う。
検索は各カテゴリ最大 50 件、検索対象ごとの facts は最大 20 件。詳細の現行 facts は最大 50 件、直近 interactions は最大 20 件で、上限と全履歴未対応を表示する。

## 7. 自動更新（solo-eikaiwa 移植）

- `updater.rs`: 起動時非ブロッキングチェック → ネイティブダイアログ（ja/en）→ DL（停滞 90 秒・上限 20 分の watchdog）→ 適用 → 再起動確認。メニュー項目の状態表示・単一実行ガード・E2E 自動承認フック（`GAIA_UPDATER_AUTO=1`）まで同型。文言のみ gaia-library 用に変更
- `updater_signature.rs` ＋ `verify-updater-signature` bin: 公開前に実行時と同じ `minisign-verify` で `.app.tar.gz` と `.sig` を照合
- 検証 bin は `updater-verifier` feature を指定した `cargo run` で使う。CI の Rust 検査では全 feature を有効にするが、アプリの bundle では無効にして検証 bin の自動同梱を防ぐ
- 鍵: `cargo tauri signer generate -w ~/.tauri/gaia-library-updater.key` で初回にだけ生成する。既存鍵は上書きしない。**秘密鍵を失うと既存ユーザーへ更新を届けられなくなるため、公開前に秘密鍵と `.pub` を必ずバックアップする**。公開鍵は `tauri.conf.json` の `plugins.updater.pubkey`
- endpoint: `https://github.com/btajp/gaia-library/releases/latest/download/latest.json`（platforms: `darwin-aarch64`。url は `.app.tar.gz`、signature は `.sig` の中身）
- sidecar が無いため solo-eikaiwa の旧 sidecar 回収は不要。再起動要求では HTTP の停止を待ってから終了する。旧プロセスが残らず新プロセスが HTTP を再開できることは、実機更新 E2E で別途確認する

## 8. リリース（scripts/release-desktop.sh、solo-eikaiwa 移植）

順序: push 済み clean な `main` のみ → 鍵継続性・タグ未使用・バージョン整合を検査 → root / desktop / UI / scripts のビルド・静的検査・テスト → updater 用設定と一時的な Developer ID 設定を重ねて `.app` をビルド・署名・公証 → app / archive / 同梱 CLI の整合と署名を検証 → DMG を署名・公証・staple → `latest.json` / `checksums.txt` を生成 → HEAD と鍵を再検査 → GitHub の draft release に 5 資産を登録して検証 → publish → 事後スモーク手順表示

`check-updater-key-policy.sh` は設定鍵・署名鍵・直前リリース鍵の継続性を検査する。ローテーションは `--allow-pubkey-rotation` を明示し、新公開鍵を含むアプリを旧秘密鍵で署名する橋渡しのみ許可する。root / desktop / Tauri の版と CHANGELOG 節を一致させる。

- `release.env`（`~/.config/gaia-library/release.env`、初回にテンプレート生成）: `APPLE_SIGNING_IDENTITY` / `APPLE_API_KEY` / `APPLE_API_ISSUER` / `APPLE_API_KEY_PATH` / `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- whisper 相当のプレ署名工程は不要（Resources に Mach-O を置かない。externalBin の `gaia` は bundler が署名する）

## 9. CI

- `.github/workflows/ci.yml` の desktop ジョブは desktop / crates / contracts / scripts / root Cargo 設定・lockfile / toolchain / workflow の変更を検知する。macOS で UI・CLI・desktop をビルドし、fmt / clippy / Rust・Bun テスト / ローカル確認用 `.app` の生成を行う
- ui: `bun install --frozen-lockfile && bun run build`（build-app.sh に含む）。toolchain 版は `toolchain.json`（bun / tauri-cli）をリポジトリ直下に置き pin

## 10. テスト

- Rust 単体: updater 純関数（solo-eikaiwa のテスト群を移植）、first_run のバリデーション、cli_link のパス判定、keychain フォールバック判定
- E2E（手動・リリース前必須）: `desktop/e2e-updater/README.md` — ローカル HTTP で `latest.json` を配信し、旧 → 新（v99.0.0）の実機更新・再起動・HTTP サーバー再開までを `GAIA_UPDATER_AUTO=1` で通す
- UI: v0.1.0 は Playwright 等を導入しない。`bun run build`（tsc --noEmit 込み）を型ゲートとし、Bun で IPC 境界・非同期状態・静的描画を検証する。実 WebView 操作の確認とは区別する
- 注意: `tauri.conf.json` の externalBin 参照により、`desktop/build-app.sh` 実行前は src-tauri の `cargo build/test` が失敗する（solo-eikaiwa で実測済みの挙動。README に明記）

### 2026-08-28 時点の検証記録

- Apple Silicon macOS 上で root の Rust 125 件、desktop の Rust 81 件（全 feature）、UI の Bun 184 件、配布補助の Bun 78 件が成功。Rust build / fmt / clippy、UI 型検証・ビルド、ShellCheck、actionlint も通過した。GitHub Actions 自体は未実行
- ローカル確認用 `.app` と updater archive / signature を生成し、実際の公開鍵・検証 CLI で署名を照合した。異なる内容のファイルは拒否された。アプリ内の実行ファイルは `gaia-desktop` と `gaia` のみで、双方 arm64。ローカル ad-hoc 署名の検証も通過したが、Developer ID 署名・Apple 公証は未実施
- 検索・詳細・scope 切替は C4 時点のアプリで隔離データを使って操作確認した。C5 以降の手入力・承認・設定・更新メニューの実画面確認は、Mac のロックにより未実施。UI の自動テストは IPC mock・状態管理・静的描画の検証であり、実 WebView の確認を代替しない
- 実 Keychain の保存・再取得、CLI リンクの実配置、旧版→新版の実更新・再起動・HTTP 再開、Claude Code / Codex からの実接続は未実施。公開前にこれらの手動確認と署名鍵のバックアップ・Apple 資格情報の設定を完了する。配布サイトからの導入確認は公開後に実施する

## 11. リスク・残論点

- config の別プロセス間の更新は計画どおり last-write-wins。同時に CLI と設定画面で管理操作を行うと、追加・キー再発行が取り消され、旧キーが再び有効になる可能性がある。跨プロセスのロックは未実装で、管理操作は同時実行しない
- 実 Keychain の保存・再取得とプロンプト頻度は未検証。隔離した fake backend では保存失敗時の 0600 ファイルへのフォールバックと旧キーの除外を検証し、保管先を画面に表示する
- アイコンはリポジトリ内の SVG から `cargo tauri icon` で生成する
- Intel mac / Windows / Linux は対象外（latest.json の platforms は darwin-aarch64 のみ。将来追加可能）
- UI の webview から `http://127.0.0.1:4111` へ直接 fetch はしない。CSP の接続先は IPC に限定し、データ取得はすべて Tauri commands
- Claude Code / Codex の HTTP MCP クライアント互換は B の統合テスト＋実機確認に依存

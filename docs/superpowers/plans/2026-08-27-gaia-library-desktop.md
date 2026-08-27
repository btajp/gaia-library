# gaia-library デスクトップアプリ 実装計画（サブプロジェクト C）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tauri 2 デスクトップアプリ（検索・閲覧・手入力・承認、HTTP サーバー内蔵、tauri-plugin-updater による自動更新、`gaia` CLI 同梱）を作り、署名・公証付きのリリースパイプラインを整える。

**Architecture:** `desktop/src-tauri` は workspace 外の独立 Cargo プロジェクトで、path 依存で `gaia-core` / `gaia-mcp` を使う。アプリは起動時に DB を開いて `Arc<ToolService>` を保持し、B の `serve_http` をプロセス内起動。UI（React ＋ Vite ＋ Tailwind、bun 管理）は Tauri commands（`call_tool` ほか薄い写し）だけを呼ぶ。updater・署名検証・鍵ポリシー・リリーススクリプトは solo-eikaiwa の実装を移植する。

**Tech Stack:** Tauri 2（tauri-cli 2.11.4）、tauri-plugin-updater / dialog / log / opener、keyring 3、React 18 ＋ TypeScript ＋ Vite 6 ＋ Tailwind 4（bun 1.3 系）、minisign（`cargo tauri signer`）

**Spec:** `docs/superpowers/specs/2026-08-27-gaia-library-desktop-design.md`（前提: A・B の実装完了）

## Global Constraints

- A の計画の Global Constraints（コミット規約・Claude-Session トレーラー・`git add` はリスト対象のみ）を引き継ぐ
- `desktop/src-tauri` は root workspace の members に**入れない**（root の `cargo test --workspace` を汚さない）。desktop のゲートは `desktop/build-app.sh` 実行後に `cd desktop/src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- UI は Tauri commands 以外の経路（直接 HTTP fetch 等）でデータを取らない
- Tauri commands は `ToolService` / `gaia_core::admin` / config / keychain / updater の薄い写しに限定し、独自のビジネスロジックを持たない
- シークレット（Apple 資格情報・updater 秘密鍵）はリポジトリ・コミット・ログに置かない。参照は `~/.config/gaia-library/release.env` と `~/.tauri/` のみ
- `~/.tauri/gaia-library-updater.key` が既に存在する場合は**絶対に上書きしない**
- 版数の正本は root `Cargo.toml` の `workspace.package.version`。desktop 側（Cargo.toml / tauri.conf.json）はリリーススクリプトが整合チェックする

---

### Task C1: UI スキャフォールド（React ＋ Vite ＋ Tailwind、bun）

**Files:**
- Create: `toolchain.json`, `desktop/ui/package.json`, `desktop/ui/vite.config.ts`, `desktop/ui/tsconfig.json`, `desktop/ui/index.html`, `desktop/ui/src/main.tsx`, `desktop/ui/src/index.css`, `desktop/ui/src/App.tsx`, `desktop/ui/.gitignore`

**Interfaces:**
- Produces: `cd desktop/ui && bun install && bun run build` が `desktop/ui/dist/` を生成する（`build` は `tsc --noEmit` 込み）
- `toolchain.json`（リポ直下）: `{ "bun": "<インストール済み bun --version>", "tauriCli": "2.11.4" }`（実測値で書く）

- [ ] **Step 1: toolchain.json と ui/ を作る**

`desktop/ui/package.json`:

```json
{
  "name": "gaia-desktop-ui",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "react": "^18.3.0",
    "react-dom": "^18.3.0"
  },
  "devDependencies": {
    "@tailwindcss/vite": "^4",
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@vitejs/plugin-react": "^4",
    "tailwindcss": "^4",
    "typescript": "^5.6.0",
    "vite": "^6"
  }
}
```

`desktop/ui/vite.config.ts`:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: "dist" },
});
```

`desktop/ui/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "skipLibCheck": true,
    "isolatedModules": true,
    "noEmit": true
  },
  "include": ["src"]
}
```

`desktop/ui/index.html`:

```html
<!doctype html>
<html lang="ja">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>gaia-library</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

`desktop/ui/src/index.css`:

```css
@import "tailwindcss";
```

`desktop/ui/src/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

`desktop/ui/src/App.tsx`（C3 で置き換える仮画面）:

```tsx
export default function App() {
  return (
    <main className="flex h-screen items-center justify-center bg-neutral-950 text-neutral-100">
      <div className="text-center">
        <h1 className="text-2xl font-semibold">gaia-library</h1>
        <p className="mt-2 text-sm text-neutral-400">仕事の記憶の索引</p>
      </div>
    </main>
  );
}
```

`desktop/ui/.gitignore`:

```
node_modules
dist
```

`toolchain.json`（リポ直下。値は `bun --version` / `cargo tauri --version` の実測で）:

```json
{
  "bun": "<実測>",
  "tauriCli": "2.11.4"
}
```

- [ ] **Step 2: ビルドを確認する**

Run: `cd desktop/ui && bun install && bun run build && ls dist/index.html`
Expected: dist が生成される。`bun.lock` が生まれるのでコミットに含める

- [ ] **Step 3: コミット**

```bash
git add toolchain.json desktop/ui
git commit -m "feat(desktop): scaffold React+Vite+Tailwind UI shell" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C2: src-tauri スキャフォールド ＋ build-app.sh（CLI 同梱）

**Files:**
- Create: `desktop/build-app.sh`, `desktop/src-tauri/Cargo.toml`, `desktop/src-tauri/build.rs`, `desktop/src-tauri/tauri.conf.json`, `desktop/src-tauri/capabilities/default.json`, `desktop/src-tauri/src/main.rs`, `desktop/src-tauri/src/lib.rs`, `desktop/src-tauri/.gitignore`, `desktop/icon-src.png`（生成）, `desktop/src-tauri/icons/*`（`cargo tauri icon` 生成）
- Modify: `.gitignore`（`desktop/src-tauri/binaries/` と `desktop/src-tauri/target/` を追記）

**Interfaces:**
- Produces: `./desktop/build-app.sh` が (1) ui を build、(2) `cargo build --release -p gaia`、(3) `desktop/src-tauri/binaries/gaia-<host triple>` を配置。その後 `cd desktop/src-tauri && cargo build` が通り、`cargo tauri build --bundles app` で `gaia-library.app` が出る（externalBin として `Contents/MacOS/gaia` が同梱される）
- updater プラグインは **C7 で追加**（このタスクの tauri.conf.json には入れない）

- [ ] **Step 1: build-app.sh を書く（`chmod +x`）**

```bash
#!/usr/bin/env bash
# desktop/build-app.sh — cargo tauri build/dev の前に UI と同梱 CLI を組み立てる。
# 生成物（ui/dist, src-tauri/binaries）はコミットしない。
set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
TRIPLE="$(rustc -vV | awk '/^host:/ { print $2 }')"

echo "== UI build（bun）"
(cd "$REPO_DIR/desktop/ui" && bun install --frozen-lockfile && bun run build)

echo "== gaia CLI（release）"
(cd "$REPO_DIR" && cargo build --release -p gaia)

echo "== externalBin 配置"
mkdir -p "$REPO_DIR/desktop/src-tauri/binaries"
cp "$REPO_DIR/target/release/gaia" "$REPO_DIR/desktop/src-tauri/binaries/gaia-${TRIPLE}"
echo "done: binaries/gaia-${TRIPLE}"
```

- [ ] **Step 2: src-tauri を作る**

`desktop/src-tauri/Cargo.toml`:

```toml
[package]
name = "gaia-desktop"
version = "0.1.0"
description = "gaia-library desktop shell"
edition = "2021"
rust-version = "1.88"
license = "MIT"

[lib]
name = "gaia_desktop_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
gaia-core = { path = "../../crates/gaia-core" }
gaia-mcp = { path = "../../crates/gaia-mcp" }
base64 = "0.22"
keyring = "3"
log = "0.4"
minisign-verify = "0.2.5"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sys-locale = "0.3"
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-dialog = "2"
tauri-plugin-log = "2"
tauri-plugin-opener = "2"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
uuid = { version = "1", features = ["v4"] }
```

（`tauri-plugin-updater` は C7 で追加。）

`desktop/src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`desktop/src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "gaia-library",
  "version": "0.1.0",
  "identifier": "com.local.gaia-library.desktop",
  "build": {
    "frontendDist": "../ui/dist"
  },
  "app": {
    "withGlobalTauri": false,
    "windows": [
      {
        "label": "main",
        "title": "gaia-library",
        "width": 1100,
        "height": 760,
        "resizable": true,
        "center": true
      }
    ],
    "security": {
      "csp": {
        "default-src": "'self'",
        "style-src": "'self' 'unsafe-inline'",
        "connect-src": "ipc: http://ipc.localhost"
      }
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "externalBin": ["binaries/gaia"],
    "macOS": {
      "signingIdentity": "-"
    }
  }
}
```

`desktop/src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "main ウィンドウ（自アプリ配信の UI）にのみ既定権限を与える",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:default", "opener:default"]
}
```

`desktop/src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gaia_desktop_lib::run()
}
```

`desktop/src-tauri/src/lib.rs`（このタスクでは最小。C3 で拡張）:

```rust
//! gaia-library デスクトップシェル。UI は Tauri commands 経由で ToolService を呼ぶ（C3 以降）。
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri::Manager;
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .targets([
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                        tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: None }),
                    ])
                    .build(),
            )?;
            let _ = app.get_webview_window("main");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`desktop/src-tauri/.gitignore`:

```
/target
/binaries
/gen/schemas
```

ルート `.gitignore` に追記:

```
desktop/src-tauri/target
desktop/src-tauri/binaries
```

- [ ] **Step 3: アイコンを生成する**

単色のソース PNG を作って `cargo tauri icon` にかける:

```bash
python3 -c "
import struct, zlib
w = h = 1024
# 深緑の単色 RGBA
row = b'\x00' + bytes([28, 78, 60, 255]) * w
raw = row * h
def chunk(t, d):
    c = struct.pack('>I', len(d)) + t + d
    return c + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n'
png += chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
png += chunk(b'IDAT', zlib.compress(raw, 9))
png += chunk(b'IEND', b'')
open('desktop/icon-src.png', 'wb').write(png)
"
cd desktop/src-tauri && cargo tauri icon ../icon-src.png
```

Expected: `desktop/src-tauri/icons/` に icns / ico / png 一式が生成される（仮アイコン。後で差し替え）

- [ ] **Step 4: ビルドを確認する**

Run:

```bash
./desktop/build-app.sh
cd desktop/src-tauri && cargo build && cargo tauri build --bundles app
ls target/release/bundle/macos/gaia-library.app/Contents/MacOS/
```

Expected: `.app` が生成され、`Contents/MacOS/` に `gaia-desktop`（本体）と `gaia`（externalBin）が並ぶ

- [ ] **Step 5: コミット**

```bash
git add .gitignore desktop/build-app.sh desktop/icon-src.png desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock desktop/src-tauri/build.rs desktop/src-tauri/tauri.conf.json desktop/src-tauri/capabilities desktop/src-tauri/src desktop/src-tauri/.gitignore desktop/src-tauri/icons
git commit -m "feat(desktop): scaffold Tauri shell with bundled gaia CLI" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C3: アプリ状態・初回セットアップ・Tauri commands・HTTP 内蔵起動

**Files:**
- Create: `desktop/src-tauri/src/state.rs`, `desktop/src-tauri/src/commands.rs`, `desktop/src-tauri/src/first_run.rs`, `desktop/ui/src/api.ts`
- Modify: `desktop/src-tauri/src/lib.rs`, `desktop/ui/src/App.tsx`

**Interfaces:**
- Produces（Rust）:
  - `state::AppState { pub service: Arc<ToolService>, pub human: ClientIdentity, pub config_path: PathBuf, pub http: Mutex<Option<gaia_mcp::BoundServer>> }`、`state::bootstrap() -> Result<Option<AppState>, String>`（config が無ければ `Ok(None)` ＝初回セットアップ待ち）、`state::start_http(app_state) -> Result<String, String>`（URL を返す。失敗は String エラー）
  - commands（すべて `#[tauri::command]`）: `is_initialized() -> bool`／`first_run_setup(affiliation: String, user_name: String) -> Result<(), String>`（`first_run::setup` を呼び、成功後に AppState を manage）／`call_tool(name: String, args: Value) -> Result<Value, Value>`（Err は ToolError の `to_json()`）／`server_status() -> Value`（`{url: Option<String>}`）
  - `first_run::setup(config_path, affiliation, user_name) -> Result<AppState, String>`: `gaia init` 相当（human クライアント `user_name`・default_scope=affiliation・affiliations 追加）＋ agent クライアント `claude-code` 追加＋キー発行（平文はまだ返すだけ。キーチェーン保管は C6）
- Produces（TS）`desktop/ui/src/api.ts`:
  - `callTool<T = unknown>(name: string, args: unknown): Promise<T>`（`invoke("call_tool")`。Err オブジェクトは `GaiaError` として throw: `{ code, message, details }`）
  - `isInitialized(): Promise<boolean>`、`firstRunSetup(affiliation: string, userName: string): Promise<void>`、`serverStatus(): Promise<{ url: string | null }>`
- `App.tsx` はこのタスクで「初回セットアップ画面 or メイン骨格（ヘッダにサーバー状態、タブ: 検索/提案/追加/設定 — 中身は C4〜C6 のプレースホルダ文言）」に置き換える

- [ ] **Step 1: state.rs / first_run.rs / commands.rs を実装し lib.rs に配線する**

実装の要点（コードは各ファイルへ。シグネチャは上記 Interfaces のとおり）:

```rust
// state.rs の中核
pub fn bootstrap() -> Result<Option<AppState>, String> {
    let config_path = gaia_core::config::config_path().map_err(|e| e.to_string())?;
    if !config_path.exists() {
        return Ok(None);
    }
    let config = gaia_core::config::Config::load(&config_path).map_err(|e| e.to_string())?;
    let human = config
        .resolve_client(None)
        .map_err(|e| format!("human クライアントを特定できません: {e}"))?
        .clone();
    let db = gaia_core::storage::Db::open(&gaia_core::config::db_path(&config).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let catalog = gaia_core::contracts::Catalog::embedded().map_err(|e| e.to_string())?;
    let service = std::sync::Arc::new(gaia_core::tools::ToolService::new(db, catalog));
    Ok(Some(AppState { service, human, config_path, http: std::sync::Mutex::new(None) }))
}
```

- `start_http`: config を読み直して `AuthTable::from_config` → 空なら「キー未発行」を URL なしで返す（エラーにしない）→ `tauri::async_runtime::spawn` 内で `gaia_mcp::serve_http(service, auth, config.server.port).await` → 成功したら URL を state に記録。lib.rs の setup で `bootstrap()?` が Some なら manage ＋ start_http、None なら UI が初回セットアップを出す
- `call_tool` command:

```rust
#[tauri::command]
fn call_tool(
    state: tauri::State<'_, crate::state::AppState>,
    name: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    state.service.call(&state.human, &name, args).map_err(|e| e.to_json())
}
```

- `first_run_setup` は成功時に `app.manage(new_state)` し `start_http` も呼ぶ（`AppHandle` を引数に取る）
- lib.rs: `.invoke_handler(tauri::generate_handler![commands::is_initialized, commands::first_run_setup, commands::call_tool, commands::server_status])`

- [ ] **Step 2: api.ts と App.tsx を実装する**

`desktop/ui/src/api.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";

export type GaiaErrorBody = { code: string; message: string; details?: unknown };

export class GaiaError extends Error {
  code: string;
  details?: unknown;
  constructor(body: GaiaErrorBody) {
    super(body.message);
    this.code = body.code;
    this.details = body.details;
  }
}

export async function callTool<T = unknown>(name: string, args: unknown): Promise<T> {
  try {
    return (await invoke("call_tool", { name, args })) as T;
  } catch (raw) {
    if (raw && typeof raw === "object" && "code" in (raw as object)) {
      throw new GaiaError(raw as GaiaErrorBody);
    }
    throw raw;
  }
}

export const isInitialized = () => invoke<boolean>("is_initialized");
export const firstRunSetup = (affiliation: string, userName: string) =>
  invoke<void>("first_run_setup", { affiliation, userName });
export const serverStatus = () => invoke<{ url: string | null }>("server_status");
```

`App.tsx`: `useEffect` で `isInitialized()` → false なら初回セットアップフォーム（所属元名・ユーザー名 → `firstRunSetup`）、true ならタブ骨格（`検索 / 提案 / 追加 / 設定` の nav ＋ ヘッダ右に `serverStatus` の URL 表示）。タブ中身は C4〜C6 のコンポーネントに差し替えるまで「準備中」の p タグ

- [ ] **Step 3: 検証（デスクトップゲート＋実起動）**

Run:

```bash
./desktop/build-app.sh
cd desktop/src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test && cargo tauri build --bundles app
GAIA_CONFIG="$(mktemp -d)/config.toml" GAIA_DB="$(mktemp -d)/gaia.db" ./target/release/bundle/macos/gaia-library.app/Contents/MacOS/gaia-desktop &
sleep 5 && pkill -f gaia-desktop
```

Expected: 起動して初回セットアップ画面が出る（目視。スクリーンショット不要）。ログに panic が無い

- [ ] **Step 4: コミット**

```bash
git add desktop/src-tauri/src desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock desktop/ui/src
git commit -m "feat(desktop): app state, first-run setup and thin tool commands" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C4: 画面 — 検索と詳細

**Files:**
- Create: `desktop/ui/src/screens/Search.tsx`, `desktop/ui/src/screens/Detail.tsx`, `desktop/ui/src/components/Badge.tsx`, `desktop/ui/src/components/FactList.tsx`, `desktop/ui/src/components/RefList.tsx`, `desktop/ui/src/types.ts`
- Modify: `desktop/ui/src/App.tsx`（タブ「検索」に Search を接続し、詳細への遷移 state を持つ）

**Interfaces:**
- `types.ts`: 契約の出力に対応する TS 型（`SearchEntity`, `Fact`, `Reference`, `GlossaryTerm`, `InteractionSummary`, `PersonSummary` など。契約 `contracts/defs/common.json` の必須/任意に合わせる。手書きで最小限）
- `App.tsx` の画面 state: `{ tab: "search" | "proposals" | "add" | "settings", detail: { type: "person" | "organization" | "engagement", id: number } | null }`。`openDetail(type, id)` を子に渡す
- Detail は `get_person` / `get_organization` / `get_engagement` を `{ person_id | organization_id | engagement_id }` で呼ぶ

- [ ] **Step 1: types.ts と共通コンポーネントを書く**

`types.ts` は契約の該当 `$defs` をそのまま TS 化する（例）:

```ts
export type Fact = {
  id: number;
  entity_type: string;
  entity_id: number;
  statement: string;
  predicate?: string;
  value?: string;
  kind: "fact" | "inference";
  scope: string;
  valid_from?: string;
  superseded_by?: number;
  created_at: string;
};

export type Reference = {
  id: number;
  target_type: string;
  target_id: number;
  system: string;
  uri: string;
  title?: string;
  note: string;
  snapshot?: string;
  scope: string;
  last_verified?: string;
  created_at: string;
};
```

（同様に `Alias` / `PersonSummary` / `OrganizationSummary` / `EngagementSummary` / `EngagementPerson` / `InteractionSummary` / `GlossaryTerm` / `SearchEntity` / `SearchContextOutput` / `GetPersonOutput` / `GetOrganizationOutput` / `GetEngagementOutput` / `Proposal` を契約どおりに定義。）

`components/Badge.tsx`:

```tsx
export default function Badge({ children, tone = "neutral" }: { children: React.ReactNode; tone?: "neutral" | "green" | "amber" }) {
  const tones = {
    neutral: "bg-neutral-800 text-neutral-300",
    green: "bg-emerald-900 text-emerald-300",
    amber: "bg-amber-900 text-amber-300",
  } as const;
  return <span className={`rounded px-1.5 py-0.5 text-xs ${tones[tone]}`}>{children}</span>;
}
```

`components/FactList.tsx`: `facts: Fact[]` を受け、各行に kind バッジ（fact=green / inference=amber）・statement・`predicate=value`（あれば）・scope を表示。空なら「facts なし」

`components/RefList.tsx`: `refs: Reference[]` を受け、各行に system バッジ・title または uri・note、snapshot（あれば `<details>` で折りたたみ）、「コピー」ボタン（`navigator.clipboard.writeText(uri)`）を表示する。参照はコピーのみで完結させる（外部で開く導線は v0.1.0 では作らない。SaaS への到達はエージェント側コネクタの責務という設計と揃える）

- [ ] **Step 2: Search.tsx を書く**

```tsx
import { useState } from "react";
import { callTool, GaiaError } from "../api";
import type { SearchContextOutput } from "../types";
import Badge from "../components/Badge";
import FactList from "../components/FactList";
import RefList from "../components/RefList";

export default function Search({ openDetail }: { openDetail: (type: string, id: number) => void }) {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SearchContextOutput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run() {
    if (!query.trim()) return;
    setBusy(true);
    setError(null);
    try {
      setResult(await callTool<SearchContextOutput>("search_context", { query }));
    } catch (e) {
      setError(e instanceof GaiaError ? `${e.code}: ${e.message}` : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <form
        className="flex gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <input
          className="flex-1 rounded border border-neutral-700 bg-neutral-900 px-3 py-2"
          placeholder="人物・案件・事実を検索（回答の設計図を返します）"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button className="rounded bg-emerald-700 px-4 py-2 disabled:opacity-50" disabled={busy}>
          検索
        </button>
      </form>
      {error && <p className="text-sm text-red-400">{error}</p>}
      {result && (
        <div className="space-y-4">
          {result.hints.map((h) => (
            <p key={h} className="text-xs text-amber-400">{h}</p>
          ))}
          {result.entities.length === 0 && <p className="text-sm text-neutral-400">該当なし</p>}
          {result.entities.map((e) => (
            <section key={`${e.type}-${e.id}`} className="rounded border border-neutral-800 p-3">
              <header className="flex items-center gap-2">
                <Badge>{e.type}</Badge>
                {["person", "organization", "engagement"].includes(e.type) ? (
                  <button className="font-semibold underline-offset-2 hover:underline" onClick={() => openDetail(e.type, e.id)}>
                    {e.name}
                  </button>
                ) : (
                  <span className="font-semibold">{e.name}</span>
                )}
                <span className="text-sm text-neutral-400">{e.summary}</span>
                <span className="ml-auto text-xs text-neutral-500">score {e.score.toFixed(1)} · {e.matched_on.join(", ")}</span>
              </header>
              <div className="mt-2 grid gap-3 md:grid-cols-2">
                <FactList facts={e.facts} />
                <RefList refs={e.refs} />
              </div>
            </section>
          ))}
          {result.glossary.length > 0 && (
            <p className="text-sm">
              用語: {result.glossary.map((g) => `${g.term}${g.reading ? `（${g.reading}）` : ""}`).join(" / ")}
            </p>
          )}
          {result.interactions.map((i) => (
            <p key={i.id} className="text-sm text-neutral-400">
              {i.occurred_at} {i.kind}: {i.summary}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 3: Detail.tsx を書く**

- props: `{ target: { type: string; id: number }; onBack: () => void; openDetail: (type: string, id: number) => void }`
- `useEffect` で type に応じ `get_person({person_id})` / `get_organization({organization_id})` / `get_engagement({engagement_id})` を呼び、出力型で分岐レンダリング:
  - person: 名前・role・org（クリックで org 詳細へ）・aliases（Badge 列）・engagements（クリックで詳細へ）・FactList・RefList・interactions
  - organization: kind・people（クリックで person 詳細へ）・engagements・FactList・RefList
  - engagement: status・期間・org・people（role 付き・クリック可）・glossary（term/読み）・FactList・RefList・interactions
- 読み込み中は「読み込み中…」、GaiaError は code:message を赤字表示。「← 検索へ戻る」ボタンで `onBack()`

- [ ] **Step 4: App.tsx にタブと遷移を配線し、検証・コミット**

Run:

```bash
cd desktop/ui && bun run build
../build-app.sh >/dev/null && cd ../src-tauri && cargo tauri build --bundles app
```

手動確認: A/B 済みの実データ（`gaia init` 済み環境）でアプリを起動し、検索 → 詳細遷移 → 戻るを一巡

```bash
git add desktop/ui/src
git commit -m "feat(desktop): search and detail screens" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C5: 画面 — 提案キューと手入力

**Files:**
- Create: `desktop/ui/src/screens/Proposals.tsx`, `desktop/ui/src/screens/AddForms.tsx`
- Modify: `desktop/ui/src/App.tsx`（タブ接続）

**Interfaces:**
- Proposals: `list_proposals` を status タブ（pending / approved / rejected）付きで表示。pending 行に「承認」「却下」ボタン（却下は理由の inline input）。承認成功で一覧を再取得し、`result.target_type` が閲覧可能なら詳細へ飛べるリンクを表示
- AddForms: 種別セレクタ＋種別ごとのフィールド定義から動的にフォームを組み、`propose_update`（`request_id` は `crypto.randomUUID()` で `ui-` プレフィックス）→ 成功後 `approve_proposal` を続けて呼ぶ（human なのでそのまま通る）。結果（承認済み ID）を表示

- [ ] **Step 1: Proposals.tsx を書く**

```tsx
import { useCallback, useEffect, useState } from "react";
import { callTool, GaiaError } from "../api";
import type { Proposal } from "../types";
import Badge from "../components/Badge";

const STATUSES = ["pending", "approved", "rejected"] as const;

export default function Proposals({ openDetail }: { openDetail: (type: string, id: number) => void }) {
  const [status, setStatus] = useState<(typeof STATUSES)[number]>("pending");
  const [items, setItems] = useState<Proposal[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [reasonFor, setReasonFor] = useState<{ id: number; reason: string } | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const out = await callTool<{ proposals: Proposal[] }>("list_proposals", { status });
      setItems(out.proposals);
    } catch (e) {
      setError(e instanceof GaiaError ? `${e.code}: ${e.message}` : String(e));
    }
  }, [status]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function decide(id: number, approve: boolean, reason?: string) {
    setError(null);
    try {
      if (approve) {
        await callTool("approve_proposal", { proposal_id: id });
      } else {
        await callTool("reject_proposal", { proposal_id: id, reason });
      }
      setReasonFor(null);
      await refresh();
    } catch (e) {
      setError(e instanceof GaiaError ? `${e.code}: ${e.message}` : String(e));
    }
  }

  return (
    <div className="space-y-3">
      <nav className="flex gap-2">
        {STATUSES.map((s) => (
          <button
            key={s}
            onClick={() => setStatus(s)}
            className={`rounded px-3 py-1 text-sm ${s === status ? "bg-emerald-700" : "bg-neutral-800"}`}
          >
            {s}
          </button>
        ))}
        <button onClick={() => void refresh()} className="ml-auto rounded bg-neutral-800 px-3 py-1 text-sm">
          再読込
        </button>
      </nav>
      {error && <p className="text-sm text-red-400">{error}</p>}
      {items.length === 0 && <p className="text-sm text-neutral-400">{status} の提案はありません</p>}
      {items.map((p) => (
        <section key={p.id} className="rounded border border-neutral-800 p-3">
          <header className="flex flex-wrap items-center gap-2 text-sm">
            <span className="font-mono text-neutral-500">#{p.id}</span>
            <Badge>{p.target_type}</Badge>
            <Badge tone="amber">{p.action}</Badge>
            <Badge tone={p.kind === "fact" ? "green" : "amber"}>{p.kind}</Badge>
            <span className="text-neutral-400">scope: {p.scope}</span>
            <span className="text-neutral-400">by {p.proposed_by}</span>
            <span className="ml-auto text-xs text-neutral-500">{p.created_at}</span>
          </header>
          <pre className="mt-2 overflow-x-auto rounded bg-neutral-900 p-2 text-xs">{JSON.stringify(p.patch, null, 2)}</pre>
          {p.provenance && (
            <p className="mt-1 text-xs text-neutral-400">出所: {JSON.stringify(p.provenance)}</p>
          )}
          {p.status === "pending" ? (
            <div className="mt-2 flex items-center gap-2">
              <button onClick={() => void decide(p.id, true)} className="rounded bg-emerald-700 px-3 py-1 text-sm">
                承認
              </button>
              {reasonFor?.id === p.id ? (
                <>
                  <input
                    autoFocus
                    className="rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-sm"
                    placeholder="却下理由（任意）"
                    value={reasonFor.reason}
                    onChange={(e) => setReasonFor({ id: p.id, reason: e.target.value })}
                  />
                  <button onClick={() => void decide(p.id, false, reasonFor.reason || undefined)} className="rounded bg-red-800 px-3 py-1 text-sm">
                    確定
                  </button>
                  <button onClick={() => setReasonFor(null)} className="text-sm text-neutral-400">
                    やめる
                  </button>
                </>
              ) : (
                <button onClick={() => setReasonFor({ id: p.id, reason: "" })} className="rounded bg-neutral-800 px-3 py-1 text-sm">
                  却下…
                </button>
              )}
            </div>
          ) : (
            <p className="mt-2 text-xs text-neutral-400">
              {p.status} by {p.decided_by} at {p.decided_at}
              {p.decision_note ? `（${p.decision_note}）` : ""}
              {p.status === "approved" && p.result_id && ["person", "organization", "engagement"].includes(p.target_type) && (
                <button className="ml-2 underline" onClick={() => openDetail(p.target_type, p.result_id!)}>
                  結果を見る
                </button>
              )}
            </p>
          )}
        </section>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: AddForms.tsx を書く（種別ごとのフィールド定義で動的フォーム）**

構成（完全実装。フィールド定義は契約の Patch 型と一致させる）:

```tsx
type Field = { key: string; label: string; required?: boolean; kind?: "text" | "number" | "textarea" | "list" };

const FORMS: Record<string, { label: string; fields: Field[] }> = {
  person: { label: "人物", fields: [
    { key: "name", label: "氏名", required: true },
    { key: "org_id", label: "組織 ID", kind: "number" },
    { key: "role", label: "役職" },
    { key: "aliases", label: "別名（カンマ区切り）", kind: "list" },
  ]},
  organization: { label: "組織", fields: [
    { key: "name", label: "名前", required: true },
    { key: "kind", label: "種別（customer / partner …）" },
  ]},
  engagement: { label: "案件", fields: [
    { key: "name", label: "案件名", required: true },
    { key: "org_id", label: "相手組織 ID", kind: "number" },
    { key: "status", label: "ステータス" },
  ]},
  fact: { label: "事実", fields: [
    { key: "entity_type", label: "対象種別（person / organization / engagement / interaction / entity）", required: true },
    { key: "entity_id", label: "対象 ID", required: true, kind: "number" },
    { key: "statement", label: "内容", required: true, kind: "textarea" },
    { key: "predicate", label: "predicate（role / status / interest / decision のみ）" },
    { key: "value", label: "value（predicate 指定時必須）" },
  ]},
  ref: { label: "参照", fields: [
    { key: "target_type", label: "対象種別（fact も可）", required: true },
    { key: "target_id", label: "対象 ID", required: true, kind: "number" },
    { key: "system", label: "システム（notion / box / minutes …）", required: true },
    { key: "uri", label: "URI", required: true },
    { key: "title", label: "タイトル" },
    { key: "note", label: "注記（何が・どの粒度で・いつ時点か）", required: true, kind: "textarea" },
    { key: "snapshot", label: "要点スナップショット", kind: "textarea" },
  ]},
  glossary: { label: "用語", fields: [
    { key: "term", label: "用語", required: true },
    { key: "reading", label: "読み" },
    { key: "definition", label: "定義", kind: "textarea" },
    { key: "engagement_id", label: "案件 ID", kind: "number" },
  ]},
  interaction: { label: "接点ログ", fields: [
    { key: "kind", label: "種別（meeting / call …）", required: true },
    { key: "occurred_at", label: "日時（ISO 8601）", required: true },
    { key: "summary", label: "要点", required: true, kind: "textarea" },
    { key: "engagement_id", label: "案件 ID", kind: "number" },
  ]},
};
```

- 送信処理: フィールド値から patch を組む（`number` は Number()、`list`（person.aliases）は `[{alias}]` 配列に、空文字は omit）。`kind` は fact フォームのみ fact / inference のラジオ、他は "fact" 固定。`callTool("propose_update", { target_type, action: "insert", patch, kind, request_id: "ui-" + crypto.randomUUID() })` → `callTool("approve_proposal", { proposal_id })` → 成功メッセージ（承認済み ID・種別）を表示しフォームをリセット。GaiaError は赤字表示（invalid_params の details も `<pre>` で出す）

- [ ] **Step 3: App.tsx に配線し、検証・コミット**

Run: `cd desktop/ui && bun run build`（tsc ゲート込み）→ アプリ起動での手動一巡（追加 → 提案タブで approved を確認 → 検索でヒット）

```bash
git add desktop/ui/src
git commit -m "feat(desktop): proposal queue and manual entry screens" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C6: 画面 — 設定（クライアント・キー・接続スニペット・CLI symlink）＋ keychain / cli_link

**Files:**
- Create: `desktop/src-tauri/src/keychain.rs`, `desktop/src-tauri/src/cli_link.rs`, `desktop/ui/src/screens/Settings.tsx`
- Modify: `desktop/src-tauri/src/commands.rs`, `desktop/src-tauri/src/lib.rs`（invoke_handler 追記）, `desktop/ui/src/api.ts`, `desktop/ui/src/App.tsx`

**Interfaces:**
- `keychain.rs`: `store_key(client: &str, plaintext: &str) -> Result<StoreLocation, String>`／`load_key(client: &str) -> Result<Option<(String, StoreLocation)>, String>`。`StoreLocation = Keychain | File`（serde 出力は "keychain" / "file"）。keyring（service `gaia-library`, account `<client>`）を先に試し、失敗時は `~/.local/share/gaia-library/keys/<client>.key`（0600）へフォールバック
- `cli_link.rs`: `link_path() -> PathBuf`（`~/.local/bin/gaia`）／`bundled_cli() -> Result<PathBuf, String>`（`std::env::current_exe()?.parent()/gaia`）／`status() -> LinkStatus`（`Ok | Missing | WrongTarget { current: String } | NotSymlink`）／`create() -> Result<(), String>`（Missing なら symlink 作成、WrongTarget かつ symlink なら張り替え、NotSymlink はエラー）
- 追加 commands: `admin_affiliation_add(name, identity) / admin_affiliation_list()`（`gaia_core::admin`）／`admin_client_list() -> Vec<{name, role, default_scope, has_key}>`／`admin_client_add(name, role, default_scope, generate_key) -> Option<String>`（キー発行時は keychain へ保存し平文を返す）／`admin_client_keygen(name) -> String`（同上）／`mcp_config_snippet(name, transport) -> Result<String, String>`（http はキーを keychain から復元。無ければ Err で「キーを発行してください」）／`cli_link_status() / cli_link_create()`
- 対応する api.ts ラッパを追加（`adminClientAdd` など同名 camelCase）

- [ ] **Step 1: keychain.rs / cli_link.rs を実装する（両方に `#[cfg(test)]` の単体テスト: フォールバックのファイルモード 0600、`status` の分岐）**

- [ ] **Step 2: commands.rs に追記し lib.rs へ配線する**

config を触る command は `state.config_path` から都度 `Config::load` → 変更 → `save`（アプリ内キャッシュを持たない。CLI と並走しても最後の保存が勝つだけで壊れない）

- [ ] **Step 3: Settings.tsx を書く**

セクション構成（各セクションは読み込み → 一覧 → 追加/操作ボタンの定型）:
1. **所属元**: 一覧（name / identity）＋追加フォーム
2. **クライアント**: 一覧（name / role / default_scope / キー有無）。「キー発行」→ 平文とスニペット（`mcp_config_snippet(name, "http")` と stdio 版の両方）を表示し「コピー」。「クライアント追加」フォーム（generate_key チェックつき）
3. **サーバー**: `serverStatus()` の URL 表示（null なら「キー未発行のため停止中」）
4. **CLI**: `cli_link_status` の表示と「~/.local/bin に gaia を作成」ボタン。作成後に `gaia --help` の案内文
5. **バージョン**: `tauri` の `getVersion()`（`@tauri-apps/api/app`）表示（アップデート確認ボタンは C7 で追加）

- [ ] **Step 4: 検証・コミット**

Run: `./desktop/build-app.sh && cd desktop/src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test && cd ../ui && bun run build`

手動確認: 設定タブでクライアント追加＋キー発行 → スニペット表示 → サーバー URL が表示される → CLI symlink 作成

```bash
git add desktop/src-tauri/src desktop/ui/src desktop/src-tauri/Cargo.toml desktop/src-tauri/Cargo.lock
git commit -m "feat(desktop): settings screen with key issuance, snippets and CLI link" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C7: 自動更新（tauri-plugin-updater、solo-eikaiwa 移植）

**Files:**
- Create: `desktop/src-tauri/src/updater.rs`, `desktop/src-tauri/src/updater_signature.rs`, `desktop/src-tauri/src/bin/verify-updater-signature.rs`, `desktop/src-tauri/tauri.updater-artifacts.conf.json`
- Modify: `desktop/src-tauri/Cargo.toml`（`tauri-plugin-updater = "2"` 追加・`default-run = "gaia-desktop"` 追加）, `desktop/src-tauri/tauri.conf.json`（plugins.updater）, `desktop/src-tauri/src/lib.rs`（メニュー＋起動時チェック）, `desktop/src-tauri/src/commands.rs`（`check_updates`）, `desktop/ui/src/screens/Settings.tsx`（「アップデートを確認」ボタン）

**Interfaces:**
- 移植元（同一マシン上の実証済み実装）: `/Users/okash1n/ghq/github.com/btajp/solo-eikaiwa/desktop/src-tauri/src/updater.rs`・`updater_signature.rs`・`src/bin/verify-updater-signature.rs`・`src/lib.rs`（メニュー配線部）
- E2E 自動承認の環境変数は `GAIA_UPDATER_AUTO=1`
- endpoint: `https://github.com/btajp/gaia-library/releases/latest/download/latest.json`

- [ ] **Step 1: updater 鍵を用意し公開鍵を設定する（既存鍵は絶対に上書きしない）**

```bash
if [ -f ~/.tauri/gaia-library-updater.key ]; then
  echo "既存の鍵を使う（上書きしない）"
else
  cd desktop/src-tauri && cargo tauri signer generate -w ~/.tauri/gaia-library-updater.key --password ""
fi
cat ~/.tauri/gaia-library-updater.key.pub
```

`tauri.conf.json` に追加（pubkey は上の `.pub` ファイル内容をそのまま貼る）:

```json
  "plugins": {
    "updater": {
      "pubkey": "<~/.tauri/gaia-library-updater.key.pub の内容>",
      "endpoints": ["https://github.com/btajp/gaia-library/releases/latest/download/latest.json"]
    }
  }
```

`tauri.updater-artifacts.conf.json`（overlay。リリース／E2E 時のみ使用）:

```json
{
  "bundle": {
    "createUpdaterArtifacts": true
  }
}
```

**秘密鍵のバックアップを利用者（人間）に依頼するメッセージを報告に含めること**（失うと既存ユーザーへ更新を届けられなくなる）。

- [ ] **Step 2: updater.rs / updater_signature.rs / verify bin を移植する**

移植元ファイルをコピーし、次の置換だけを施す（構造・ガード・watchdog・テストは変えない）:
- 文字列 `solo-eikaiwa` → `gaia-library`（表示文言・RELEASES_URL: `https://github.com/btajp/gaia-library/releases`）
- 環境変数 `SOLO_EIKAIWA_UPDATER_AUTO` → `GAIA_UPDATER_AUTO`（`should_auto_confirm` のテストも同じ意味論のまま）
- `app_lib::` → `gaia_desktop_lib::`（verify bin の import）
- solo-eikaiwa 固有の sidecar 連携コメント（#270 等）は削除し、「gaia は in-process HTTP のため sidecar 回収は不要」の 1 行に置き換える
- `updater_signature.rs` のテスト（固定ベクタでの検証・改竄拒否・非 base64 拒否）はそのまま維持

`lib.rs` への配線（solo-eikaiwa の lib.rs と同型）:
- `.plugin(tauri_plugin_updater::Builder::new().build())`
- macOS アプリメニューの About 直後に `MENU_ID_CHECK_UPDATES` 項目を挿入し `UpdateMenuState` を manage、`on_menu_event` で `updater::spawn_manual_check`
- setup 末尾で `updater::spawn_startup_check(app.handle().clone())`

`commands.rs` に `check_updates`（`updater::spawn_manual_check(app_handle)` を呼ぶだけ）を追加し、Settings のバージョン節に「アップデートを確認」ボタンを付ける。

- [ ] **Step 3: 検証・コミット**

Run:

```bash
./desktop/build-app.sh
cd desktop/src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
# updater アーティファクト生成の成立確認（署名鍵 env が要る）
TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/gaia-library-updater.key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  cargo tauri build --bundles app --config tauri.updater-artifacts.conf.json
ls target/release/bundle/macos/gaia-library.app.tar.gz target/release/bundle/macos/gaia-library.app.tar.gz.sig
cargo run --bin verify-updater-signature -- \
  target/release/bundle/macos/gaia-library.app.tar.gz \
  target/release/bundle/macos/gaia-library.app.tar.gz.sig \
  "$(cat ~/.tauri/gaia-library-updater.key.pub)"
```

Expected: `.app.tar.gz` と `.sig` が生成され、検証 bin が成功する

```bash
git add desktop/src-tauri
git commit -m "feat(desktop): port auto-updater with minisign verification" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C8: リリースパイプライン（鍵ポリシー・release-desktop.sh・E2E 手順・CHANGELOG）

**Files:**
- Create: `CHANGELOG.md`, `scripts/check-updater-key-policy.sh`, `scripts/release-desktop.sh`, `desktop/e2e-updater/README.md`, `desktop/e2e-updater/old.conf.json`, `desktop/e2e-updater/new.conf.json`

**Interfaces:**
- 移植元: `/Users/okash1n/ghq/github.com/btajp/solo-eikaiwa/scripts/{release-desktop.sh,check-updater-key-policy.sh}`・`desktop/e2e-updater/`
- gaia 向けの差分（これ以外は移植元の構造・チェックを維持する）:
  - 公開鍵の読み出し元: `desktop/src-tauri/tauri.conf.json` の `plugins.updater.pubkey`
  - バージョン整合: root `Cargo.toml` の `workspace.package.version`・`desktop/src-tauri/Cargo.toml`・`desktop/src-tauri/tauri.conf.json`・`CHANGELOG.md` の `## [X.Y.Z]` 節・タグ未使用の 5 点一致
  - 検証ゲート: `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo test --workspace` ＋ `desktop/build-app.sh` 後の `cd desktop/src-tauri && cargo test`
  - ビルド: `desktop/build-app.sh` → `cd desktop/src-tauri && CI=true cargo tauri build --config tauri.updater-artifacts.conf.json`
  - 生成物: `gaia-library_<ver>_aarch64.dmg`・`gaia-library.app.tar.gz`＋`.sig`・`latest.json`（platforms.darwin-aarch64）・`checksums.txt`（shasum -a 256）。solo の SBOM/provenance 工程と whisper プレ署名は**入れない**（v0.1 の簡略化。将来必要になったら移植元から足す）
  - `release.env`: `~/.config/gaia-library/release.env`（無ければテンプレート生成して終了。項目は APPLE_SIGNING_IDENTITY / APPLE_API_KEY / APPLE_API_ISSUER / APPLE_API_KEY_PATH / TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD）
  - GitHub Release: `gh release create vX.Y.Z --draft --target <HEAD>` に dmg / app.tar.gz / latest.json / checksums.txt を添付 → `--draft=false`。リリースノートは CHANGELOG の該当節を抽出
  - main ブランチ・push 済み・clean 強制、タグはスクリプトが作る（先に手動で打たない）

- [ ] **Step 1: CHANGELOG.md を作る**

```markdown
# Changelog

形式は [Keep a Changelog](https://keepachangelog.com/ja/1.1.0/)、バージョニングは [SemVer](https://semver.org/lang/ja/)。
リリースノートは各節から `scripts/release-desktop.sh` が抽出する。

## [Unreleased]

### Added
- 基盤: 契約駆動の MCP サーバー（stdio / Streamable HTTP、scope 強制、提案キュー、監査ログ）
- CLI `gaia`（init / serve / 検索・閲覧 / 提案・承認 / キー発行・接続スニペット）
- デスクトップアプリ（検索・閲覧・手入力・承認、HTTP サーバー内蔵、自動更新、CLI 同梱）
```

- [ ] **Step 2: check-updater-key-policy.sh を移植する**

移植元をコピーし、`extract_pubkey` の対象を `desktop/src-tauri/tauri.conf.json`（`plugins.updater.pubkey`）に変更。引数仕様（`--repo` / `--private-key` / `--allow-pubkey-rotation`）・鍵継続性のロジック（直前タグの conf と比較・初回許容・橋渡しは旧鍵署名を強制）は**そのまま**。`chmod +x`

- [ ] **Step 3: release-desktop.sh を移植する**

上記 Interfaces の差分どおりに調整した完全版を書く（移植元の段階構成: env 読込 → preflight（gh / tauri-cli / bun の存在）→ git 前提 → 鍵ポリシー → バージョン整合 → 検証ゲート → build-app → tauri build → 署名/公証検証（codesign --verify --deep --strict / stapler validate / spctl -a）→ minisign 検証（verify-updater-signature）→ dmg notarize + staple → latest.json → checksums.txt → gh release draft → publish → 事後スモーク表示）。`chmod +x`。dry-run 検証として `bash -n scripts/release-desktop.sh` と shellcheck（あれば）を通す

- [ ] **Step 4: e2e-updater/ を移植する**

- `old.conf.json`: endpoint をローカル（`http://127.0.0.1:8930/latest.json`）へ差し替える overlay（`dangerousInsecureTransportProtocol` は E2E 専用と明記。本番 conf に入れたら即失格）
- `new.conf.json`: `version: 99.0.0` ＋ `createUpdaterArtifacts: true`
- README: solo-eikaiwa の手順を gaia 用パスに置換（`GAIA_UPDATER_AUTO=1`、`/private/tmp` 直下で実行する symlink 注意も維持）。合格条件: 旧 → 99.0.0 の自動適用・再起動・更新後にアプリの HTTP サーバーが再度 listen すること

- [ ] **Step 5: 検証・コミット**

Run: `bash -n scripts/release-desktop.sh && bash -n scripts/check-updater-key-policy.sh && (command -v shellcheck >/dev/null && shellcheck scripts/release-desktop.sh scripts/check-updater-key-policy.sh || echo "shellcheck なし: スキップ")`

```bash
git add CHANGELOG.md scripts/check-updater-key-policy.sh scripts/release-desktop.sh desktop/e2e-updater
git commit -m "feat(release): port signed release pipeline and updater e2e docs" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

### Task C9: CI・ドキュメント・最終検証

**Files:**
- Modify: `.github/workflows/ci.yml`（desktop ジョブ追加）, `README.md`（デスクトップ節）, `AGENTS.md`（desktop / updater / release の規則）, `docs/superpowers/specs/2026-08-27-gaia-library-desktop-design.md`（§11 に実績追記）

- [ ] **Step 1: ci.yml に desktop ジョブを追加する**

solo-eikaiwa の verify.yml と同型の変更検知付き:

```yaml
  desktop:
    runs-on: macos-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - id: changes
        env:
          BASE_SHA: ${{ github.event.pull_request.base.sha || github.event.before }}
          HEAD_SHA: ${{ github.event.pull_request.head.sha || github.sha }}
        run: |
          if [[ "$BASE_SHA" =~ ^0+$ ]] || ! git cat-file -e "$BASE_SHA^{commit}" 2>/dev/null; then
            echo "required=true" >> "$GITHUB_OUTPUT"
          elif git diff --quiet "$BASE_SHA" "$HEAD_SHA" -- desktop crates contracts toolchain.json .github/workflows/ci.yml; then
            echo "required=false" >> "$GITHUB_OUTPUT"
          else
            echo "required=true" >> "$GITHUB_OUTPUT"
          fi
      - if: steps.changes.outputs.required == 'true'
        uses: dtolnay/rust-toolchain@stable
      - if: steps.changes.outputs.required == 'true'
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: |
            .
            desktop/src-tauri
      - if: steps.changes.outputs.required == 'true'
        id: toolchain
        run: echo "bun=$(python3 -c 'import json; print(json.load(open("toolchain.json"))["bun"])')" >> "$GITHUB_OUTPUT"
      - if: steps.changes.outputs.required == 'true'
        uses: oven-sh/setup-bun@v2
        with:
          bun-version: ${{ steps.toolchain.outputs.bun }}
      - if: steps.changes.outputs.required == 'true'
        run: cargo install tauri-cli --version "$(python3 -c 'import json; print(json.load(open("toolchain.json"))["tauriCli"])')" --locked
      - if: steps.changes.outputs.required == 'true'
        run: ./desktop/build-app.sh
      - if: steps.changes.outputs.required == 'true'
        working-directory: desktop/src-tauri
        run: cargo fmt --check && cargo clippy -- -D warnings && cargo test
      - if: steps.changes.outputs.required != 'true'
        run: echo "desktop 関連の変更がないためスキップ"
```

- [ ] **Step 2: README / AGENTS.md を更新する**

- README: 「デスクトップアプリ」節（build-app.sh → cargo tauri build、初回セットアップ、自動更新の説明、リリースは `scripts/release-desktop.sh <version>`）
- AGENTS.md: リポ構成に desktop/（workspace 外・path 依存）を追記。開発ルールに「desktop のゲートは build-app.sh 後に src-tauri で fmt/clippy/test」「updater 秘密鍵 `~/.tauri/gaia-library-updater.key` は上書き禁止・バックアップ必須」「リリースは push 済み clean main から `scripts/release-desktop.sh`（タグはスクリプトが作る）」「鍵ローテーションは `--allow-pubkey-rotation` の橋渡しリリースのみ」を追記

- [ ] **Step 3: 最終検証**

Run:

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
./desktop/build-app.sh && cd desktop/src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test && cargo tauri build --bundles app
```

手動スモーク（チェックリストとして報告に含める）:
1. アプリ初回起動 → セットアップ → 検索/追加/承認/設定の一巡
2. 設定でキー発行 → 表示されたスニペットで Claude Code から HTTP 接続（または `gaia client mcp-config` で stdio 接続）
3. `desktop/e2e-updater/README.md` の実機更新 E2E（リリース前必須）

- [ ] **Step 4: コミット**

```bash
git add .github/workflows/ci.yml README.md AGENTS.md docs
git commit -m "docs(desktop): CI job, README and AGENTS.md for the desktop app" -m "Claude-Session: https://claude.ai/code/session_01Sq22Nvar8VdcQQstWvNTLR"
```

---

## 実行後（v0.1.0 リリース）

A → B → C が揃ったら: CHANGELOG の `## [0.1.0]` 節を作り、`workspace.package.version` / desktop 版数を 0.1.0 に揃え、main へマージ・push 後に `scripts/release-desktop.sh 0.1.0` を実行する（Apple 資格情報と updater 鍵は `~/.config/gaia-library/release.env`）。公開後の実機スモーク（dmg 導入・旧→新の自動更新）まで含めて v0.1.0 完了。

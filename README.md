# gaia-library

仕事の記憶の「思い出し方」を索引として保存し、問い合わせに要点＋解決可能な参照からなる「回答の設計図」を返すローカル MCP サーバー。

- 設計: `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`
- HTTP・認証: `docs/superpowers/specs/2026-08-27-gaia-library-http-auth-design.md`
- エージェント向け指示: `AGENTS.md`

## ビルドとテスト

```sh
cargo build --workspace
cargo test --workspace
```

## セットアップ

```sh
cargo build --release -p gaia     # バイナリは target/release/gaia
gaia init --affiliation <所属元名>              # 設定と DB を作成
gaia client add claude-code --role agent --default-scope <所属元名>
```

以降の `gaia` はビルド済みバイナリを指します。PATH にない場合は `./target/release/gaia` に置き換えます。

## HTTP での接続

```sh
gaia client add narumi --role agent --default-scope <所属元名> --generate-key
gaia serve --http
```

キーは設定の保存成功後に stdout へ一度だけ表示します。narumi では「Gaia 接続」の API キー欄に入力します。設定ファイルには SHA-256 ハッシュだけを保存するため、平文キーを失った場合は `gaia client keygen narumi` で再発行します。サーバーは認証ごとに設定を読み直すので、再起動せず旧キーが失効します。`--json` で発行すると、キーを含む JSON を stdout へ返します。これらの出力をログや公開ファイルへ保存しないでください。

設定の検証は fail-closed です。`[keys]` に 1 件でも不正なハッシュ（64 桁の hex 以外）、`[[clients]]` に無いクライアント名、別クライアントと重複するハッシュがあると設定全体の読み込みが失敗し、HTTP 認証はすべてのクライアントで拒否され、`gaia client keygen` / `gaia client add` も失敗します。復旧は設定ファイルの `[keys]` を手で編集します。不正なハッシュの行と、`[[clients]]` に無い名前の行は削除します（その名前を使い続けるなら `gaia client add <name> ...` で登録してからキーを発行します）。重複するハッシュは同じ平文キーを 2 つのクライアントで共有している状態なので、エラーに表示された両方の行を削除し、両方のクライアントで `gaia client keygen <name>` を実行して別々のキーにします。片方だけ削除すると、残した側でそのキーが引き続き有効になります。編集後は設定ファイルの権限が `0600` のままか確認してください（エディタによっては別ファイルとして保存し直し、`0644` になる場合があります）。

サーバーは `127.0.0.1` のみに bind します。ポート未指定時は設定の `server.port`、それもなければ 4111〜4114 の順で使用可能なポートを探します。`--port N` で固定、`--port 0` で空きポートを選択できます。起動した URL は通常 stderr、`gaia --json serve --http --port 0` では stdout の `{"status":"listening","url":"..."}` で確認できます。

すべての HTTP リクエストで `Authorization: Bearer <key>` が必要です。MCP セッションはクライアント名に結び付け、別クライアントによる応答の再取得・操作・削除を拒否します。同じクライアントのキー再発行ではセッションを引き継げます。HTTP の `--client` 指定は不要で、各リクエストのキーから役割と scope を解決します。HTTP でも承認・却下は human のみです。

`.mcp.json` 用の設定を生成する場合は `gaia client mcp-config narumi --transport http --port 4111 --key-stdin` に有効なキーを標準入力で渡します。ポートは起動したサーバーの実ポートに合わせてください（`--port` を省略できるのは設定の `server.port` が固定値の場合だけです）。互換用の `--key` もありますが、コマンド履歴やプロセス引数への露出を避けるため `--key-stdin` を推奨します。生成結果は認証ヘッダーを含む秘密の設定として扱ってください。

## MCP クライアントからの接続（stdio）

`.mcp.json` などに登録する:

```json
{
  "mcpServers": {
    "gaia_library": { "command": "gaia", "args": ["serve", "--stdio", "--client", "claude-code"] }
  }
}
```

stdio はキー不要ですが、接続主体を固定する `--client` が必須です。`--stdio` と `--http` は同時に指定できません。

`gaia client mcp-config <name> --transport stdio` で生成すると、検証に使った設定ファイルと実効 DB の絶対パスもスニペットへ保持します。別の作業ディレクトリや環境変数から起動しても接続先が変わらないためです。DB の配置を変更した場合はスニペットを再生成してください。

## 日常の使い方

```sh
gaia add person --name "岡村 慎太郎" --alias okash1n   # 手入力（提案＋即時承認）
gaia search "Okta"                                     # 回答の設計図を得る
gaia proposals && gaia approve <id>                    # エージェントの提案を承認
```

参照の本文はサーバー側で取得できる: `gaia resolve --ref-id <id> --content | less`（設定が必要。下記「参照の実体取得」）。
承認・却下も既定 scope 内だけを対象にする。別の scope を扱う場合は `gaia approve <id> --scope <所属元名>` / `gaia reject <id> --scope <所属元名>` のように明示する。
`propose --request-id` の再送は、同じクライアント・scope・提案内容の場合だけ重複として扱い、異なる内容での再利用は `conflict` になる。

## 参照の実体取得（resolve_source）

`search_context` などが返す参照（refs）の本文を、`resolve_source`（MCP）/ `gaia resolve`（CLI）/ デスクトップの「内容を取得」でサーバー側に取得させられる。参照の `system` に応じた解決器が本文を返し、取得できない場合は `resolved=false` と理由、参照と要点スナップショットをそのまま返す。DB は更新しない。

解決器は既定ですべて無効で、設定ファイル `config.toml` の `[sources]` で個別に有効にする（設定は呼び出しごとに読み直すので再起動は不要）。

```toml
[sources]
max_content_chars = 30000            # content の上限（文字数）。1000〜500000

[sources.file]                       # file:///... の参照。許可ディレクトリ配下の通常ファイルだけを読む
roots = ["/Users/<me>/Library/Application Support/narumi/meetings"]
max_bytes = 1048576

[sources.url]                        # http / https の参照。許可したホストへの GET だけ
allow_hosts = ["docs.example.com"]   # "*" は全公開ホスト（下記の注意）。"example.com" はそのホストとサブドメイン
timeout_secs = 15                    # 1 参照あたりの合計（リダイレクトの追従を含む）
max_bytes = 1048576
max_redirects = 3

[sources.narumi]                     # narumi://meeting/<meeting_id>[?version=<n>] の参照
command = "/opt/homebrew/bin/uv"     # 絶対パス。`which uv` の結果
args = ["--directory", "/path/to/narumi", "run", "narumi-server", "--stdio-bridge"]
timeout_secs = 30
max_bytes = 1048576                  # get_minutes 応答の markdown の上限（バイト）。超過は本文を返さない
stderr = "discard"                   # "inherit" で narumi のログを gaia の stderr に流す
[sources.narumi.env]                 # 任意。追加・上書きするキーだけ
NARUMI_HOME = "/Users/<me>/Library/Application Support/narumi"
```

- `file`: `roots` は絶対パスのディレクトリ。symlink で外へ出る参照、ディレクトリ、バイナリ、`max_bytes` 超は読まない。設定ファイル・DB・アプリのキー退避ディレクトリは `roots` に入れても常に対象外。現行の narumi が登録する `file://` の議事録参照は、`roots` に narumi の `meetings` ディレクトリ（`NARUMI_HOME` 配下）を入れると読める。
- `url`: 公開テキスト向け。`localhost`・プライベート・リンクローカル・メタデータ IP は DNS 解決後でも拒否し、リダイレクトも各段で検査する。`allow_hosts = ["*"]` はエージェントが任意の公開ホストへ GET できる状態（URL クエリ経由の持ち出し経路になり得る）なので、必要なホストだけを指定する。Notion / Box などはエージェント側のコネクタで開く。
- `narumi`: `narumi.app` を起動した状態で `--stdio-bridge` を使う（常駐サーバーへの橋渡し）。`uv` の代わりに venv 内の `narumi-server` 実行ファイルを直接指定してもよい（孫プロセスと uv の暗黙取得を避けられる）。narumi の scope 名は gaia の所属元名（affiliation）と一致させる。narumi 参照の登録規約（`system = "narumi"`, `uri = "narumi://meeting/<meeting_id>?version=<n>"`, `snapshot` 必須）は設計書 `docs/superpowers/specs/2026-08-29-gaia-library-resolve-source-design.md` §10 を参照。
- `[sources]` を書いた設定ファイルは 0.1.x では読めない。戻す場合は `[sources]` の節を削除する（既定値のままなら書き出されない）。

```sh
gaia info                                  # capabilities.resolvers に設定済みの解決器名が出る
gaia resolve --ref-id 12                   # JSON（resolved / content / reason）
gaia resolve --ref-id 12 --content | less  # 本文だけを stdout へ。取得できなければ終了コード 2
gaia resolve --uri "narumi://meeting/20260827T030500Z-a1b2c3d4?version=2"
```

## デスクトップアプリ（Apple Silicon macOS）

Rust と `toolchain.json` に記載した Bun / Tauri CLI が必要。`desktop/src-tauri` は root の Cargo workspace とは別プロジェクトである。

```sh
./desktop/build-app.sh
cd desktop/src-tauri
cargo build --all-features --locked
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
cargo tauri build --bundles app --no-sign --ci
```

ローカル確認用アプリは `desktop/src-tauri/target/release/bundle/macos/gaia-library.app` に生成される。
`--no-sign` は配布用の署名・公証を行わない。同梱 CLI と UI が必要なため、初回の Rust 検証より先に `build-app.sh` を実行する。
UI の型検証は `build-app.sh` に含む。回帰テストは `cd desktop/ui && bun test`、リリース用の隔離テストはリポジトリ直下の `bun test scripts` で実行できる。
`--all-features` は検証用 CLI も検査するために指定する。アプリ生成の `cargo tauri build` には付けない。検証用 CLI は `updater-verifier` feature でのみ有効にし、配布アプリには同梱しない。

初回起動では、所属元名（情報を分ける scope）とユーザー名を入力する。
human `desktop:<ユーザー名>` と agent `claude-code` を作成し、agent のキーだけを発行する。既存設定は上書きせず読み込む。不正な設定や human を選べない状態はエラーとして表示する。

- 検索・詳細・提案・手入力は CLI と同じ `ToolService` を呼ぶ。scope は画面上部で選び、空欄なら起動した human の既定値を使う。
- 手入力も提案を作成してから承認する。承認だけが失敗した場合は、画面に残る提案 ID を使って再試行する。
- 参照 URI はコピーのみ。参照先を自動で開かない。参照カードの「内容を取得」は `resolve_source` で本文を取得してテキストとして表示し（保存はしない）、取得できない場合は理由と要点スナップショットを表示する。解決器の設定（`[sources]`）はアプリ内にはなく、設定ファイルを直接編集する。現行 facts や検索結果には取得上限があり、全履歴・総件数・ページ送りには未対応。
- HTTP は `127.0.0.1` だけで待ち受ける。キー未発行やポート競合でも画面は使える。設定画面に表示された実際の URL を接続に使う。
- ウィンドウを閉じても常駐を続ける。終了はアプリメニューまたはトレイの「終了」。二重起動は既存ウィンドウを表示する。

### 設定と接続キー

設定画面で所属元・クライアントを追加し、キーを発行・再発行できる。再発行後は旧キーが使えなくなるため、接続先クライアントの設定も更新する。
クライアント管理・キー再発行は、CLI と設定画面のどちらから行っても設定ファイルの隣に作る `.lock` ファイルで直列化する。同時に実行しても、成功と返った追加や再発行は失われない。ただし、このロックを通らない直接編集（エディタで設定ファイルを書き換えるなど）は対象外で、その間に行った変更は上書きされる場合がある。
設定ファイルが symlink の場合はリンクを残してリンク先を更新し、`.lock` と一時ファイルもリンク先の隣に作る。設定ファイル（symlink の場合はリンク先を含む）は本人だけが書けるディレクトリに置く。他のユーザーが所有する symlink は辿らずエラーで停止する。dotfiles リポジトリに置く場合は `config.toml.lock` を `.gitignore` に追加する。
平文キーは macOS Keychain に保管し、失敗した場合はデータディレクトリの `keys/` 内の 0600 ファイルへ保存する。保存場所を画面に表示する。ファイル名はクライアント名の SHA-256、ディレクトリは 0700。
両方の保管に失敗した場合も発行したキーを表示するので、画面を閉じる前に安全な場所へコピーする。ブラウザの localStorage には保存しない。

「接続設定」で HTTP / stdio の JSON を再表示・コピーできる。HTTP は現在の設定ハッシュと一致したキーだけを復元し、起動中サーバーの実 URL を使う。
CLI で再発行したキーはアプリの Keychain には保存されない。復元できない場合は、CLI で保存したキーを使うか、アプリから再発行する。
stdio の設定には同梱 CLI、設定ファイル、実際に開いた DB の絶対パスが入る。

CLI のリンクは設定画面の明示操作で `~/.local/bin/gaia` に作成する。通常ファイルは上書きしない。既存の別リンクは行き先を確認してから張り替える。表示後にリンク先が変わった場合は変更せず中止し、再読込・再確認を求める。
`~/.local/bin` が PATH に入っている場合は、新しいターミナルで `gaia --help` を確認する。PATH はアプリから変更しない。

### 自動更新とリリース

起動時と「アップデートを確認」で GitHub Releases を確認する。更新は確認後にダウンロードし、署名を検証して適用する。再起動のタイミングも選択できる。
初回リリース前やオフライン時の起動時チェック失敗は画面を妨げない。手動チェックでは確認できなかったことを表示する。

配布用リリースは、次の条件を満たした push 済み・未変更の `main` からだけ実行する。

- `desktop/e2e-updater/README.md` の隔離された旧版→新版更新テストを完了する。
- `~/.tauri/gaia-library-updater.key` と同名の `.pub` をバックアップする。秘密鍵は上書きしない。
- `~/.config/gaia-library/release.env` に Developer ID と Apple 公証用 API 資格情報を設定し、0600 にする。未作成ならスクリプトが空欄のテンプレートを作成して停止する。
- Cargo workspace、desktop、Tauri 設定、CHANGELOG の版数をそろえ、対象タグを未使用にする。

```sh
./scripts/release-desktop.sh <version>   # 例: CHANGELOG と Cargo / Tauri 設定でそろえた版数
```

スクリプトはビルド・テスト、Developer ID 署名、公証、updater 署名を確認し、5 個の配布ファイルを draft に添付してから公開する。ローカル `.app` の生成だけではリリース完了にならない。
鍵変更は `--allow-pubkey-rotation` を指定し、旧鍵で署名する橋渡しリリースに限る。新しい公開鍵だけへ直接切り替えると既存アプリが更新を受け取れなくなる。

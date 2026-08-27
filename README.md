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

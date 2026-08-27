# gaia-library

仕事の記憶の「思い出し方」を索引として保存し、問い合わせに要点＋解決可能な参照からなる「回答の設計図」を返すローカル MCP サーバー。

- 設計: `docs/superpowers/specs/2026-08-27-gaia-library-foundation-design.md`
- エージェント向け指示: `AGENTS.md`

## ビルドとテスト

```sh
cargo build --workspace
cargo test --workspace
```

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

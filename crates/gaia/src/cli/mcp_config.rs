//! MCP 設定スニペット。キーは検証後の明示出力だけに含め、診断へは出さない。
use std::{io::Read, path::Path};

use clap::Args;
use gaia_core::{
    auth::AuthTable,
    config::{self, Config},
    error::ToolError,
};
use serde_json::json;

use super::app::print_json;

#[derive(Args)]
pub struct McpConfigArgs {
    name: String,
    #[arg(long, default_value = "stdio")]
    transport: String,
    /// HTTP 用の平文キー（argv へ残さない場合は --key-stdin を使う）
    #[arg(long, conflicts_with = "key_stdin")]
    key: Option<String>,
    /// HTTP 用の平文キーを標準入力から読む（末尾の改行のみ除去する）
    #[arg(long)]
    key_stdin: bool,
    /// HTTP の固定ポート（省略時は [server].port）
    #[arg(long)]
    port: Option<u16>,
}

pub fn print(config_path: &Path, args: &McpConfigArgs, compact: bool) -> anyhow::Result<()> {
    let config_path = std::path::absolute(config_path)?;
    let config = Config::load(&config_path)?;
    let name = &args.name;
    if config.client(name).is_none() {
        return Err(ToolError::not_found(format!("クライアント `{name}` がありません")).into());
    }
    let snippet = match args.transport.as_str() {
        "stdio" => {
            if args.key.is_some() || args.key_stdin || args.port.is_some() {
                return Err(ToolError::invalid_params(
                    "stdio では --key、--key-stdin、--port は指定しません",
                )
                .into());
            }
            let db_path = std::path::absolute(config::db_path(&config)?)?;
            let config_path = utf8_path(&config_path)?;
            let db_path = utf8_path(&db_path)?;
            json!({
                "mcpServers": {
                    "gaia_library": {
                        "command": "gaia",
                        "args": ["serve", "--stdio", "--client", name, "--config", config_path],
                        "env": {"GAIA_DB": db_path}
                    }
                }
            })
        }
        "http" => {
            let key = if args.key_stdin {
                read_key(&mut std::io::stdin().lock())?
            } else {
                args.key.clone().ok_or_else(|| {
                    ToolError::invalid_params(
                        "--key または --key-stdin で平文キーを指定してください",
                    )
                })?
            };
            if !config.keys.contains_key(name) {
                return Err(ToolError::invalid_params(format!(
                    "クライアント `{name}` のキーが未発行です（gaia client keygen で発行してください）"
                ))
                .into());
            }
            if AuthTable::from_config(&config)
                .verify(&key)
                .is_none_or(|identity| identity.name != *name)
            {
                return Err(ToolError::invalid_params(
                    "指定されたキーがクライアントの現在のキーと一致しません",
                )
                .into());
            }
            let port = args.port.or(config.server.port).filter(|port| *port != 0).ok_or_else(|| {
                ToolError::invalid_params(
                    "HTTP スニペットには固定ポートが必要です（--port <1..65535> または [server].port を指定してください）",
                )
            })?;
            json!({
                "mcpServers": {
                    "gaia_library": {
                        "type": "http",
                        "url": format!("http://127.0.0.1:{port}/mcp"),
                        "headers": {"Authorization": format!("Bearer {key}")}
                    }
                }
            })
        }
        _ => {
            return Err(ToolError::invalid_params(
                "--transport は stdio または http を指定してください",
            )
            .into());
        }
    };
    print_json(&snippet, compact);
    Ok(())
}

fn utf8_path(path: &Path) -> Result<&str, ToolError> {
    path.to_str().ok_or_else(|| {
        ToolError::invalid_params("MCP スニペットの設定・DB パスは UTF-8 で指定してください")
    })
}

fn read_key(input: &mut impl Read) -> Result<String, ToolError> {
    let mut key = String::new();
    input.read_to_string(&mut key).map_err(|_| {
        ToolError::invalid_params("標準入力から UTF-8 のキーを読み取れませんでした")
    })?;
    let key = key.trim_end_matches(['\r', '\n']).to_string();
    if key.is_empty() {
        return Err(ToolError::invalid_params("標準入力のキーは空にできません"));
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::read_key;

    #[test]
    fn stdin_removes_only_trailing_newlines() {
        assert_eq!(
            read_key(&mut " secret \r\n".as_bytes()).unwrap(),
            " secret "
        );
        assert_eq!(read_key(&mut "secret".as_bytes()).unwrap(), "secret");
        assert!(read_key(&mut "\r\n".as_bytes()).is_err());
        assert!(read_key(&mut &b"private\xff"[..]).is_err());
    }
}

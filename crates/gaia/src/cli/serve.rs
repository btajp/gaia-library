//! MCP サーバー起動。stdio は起動時の明示 client、HTTP は Bearer ごとに識別する。
use std::sync::Arc;

use clap::Args;

use gaia_core::{auth::AuthTable, error::ToolError};

use super::app::{App, print_json};

#[derive(Args)]
pub struct ServeArgs {
    /// stdio トランスポートで起動する
    #[arg(long, conflicts_with = "http")]
    pub stdio: bool,
    /// Streamable HTTP（127.0.0.1）で起動する
    #[arg(long)]
    pub http: bool,
    /// HTTP のポート（省略時は config → 4111..4114。0 で空きポート）
    #[arg(long, requires = "http")]
    pub port: Option<u16>,
}

pub fn validate_args(args: &ServeArgs, cli_client: Option<&str>) -> Result<(), ToolError> {
    if args.stdio == args.http {
        return Err(ToolError::invalid_params(
            "--stdio か --http のどちらか一方を指定してください",
        ));
    }
    if args.stdio {
        explicit_stdio_client(cli_client)?;
    }
    Ok(())
}

pub fn serve(
    app: App,
    cli_client: Option<&str>,
    args: &ServeArgs,
    compact: bool,
) -> anyhow::Result<()> {
    if args.http {
        return serve_http(app, args.port, compact);
    }
    let client_name = explicit_stdio_client(cli_client)?;
    let identity = app.identity(Some(client_name))?;
    tracing::info!(client = %identity.name, role = %identity.role, "starting gaia_library over stdio");
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let server = gaia_mcp::GaiaServer::new(Arc::new(app.service), identity);
        gaia_mcp::serve_stdio(server).await
    })?;
    Ok(())
}

fn explicit_stdio_client(cli_client: Option<&str>) -> Result<&str, ToolError> {
    cli_client.ok_or_else(|| {
        ToolError::unauthorized("stdio サーバーは接続主体を固定するため --client <name> が必須です")
    })
}

fn serve_http(app: App, port_override: Option<u16>, compact: bool) -> anyhow::Result<()> {
    let auth = Arc::new(AuthTable::from_path(&app.config_path)?);
    let port = port_override.or(app.config.server.port);
    let service = Arc::new(app.service);
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let bound = gaia_mcp::serve_http(service, auth, port).await?;
        if compact {
            print_json(
                &serde_json::json!({"status": "listening", "url": bound.url()}),
                true,
            );
        } else {
            eprintln!("gaia_library listening on {}", bound.url());
        }
        tokio::signal::ctrl_c().await?;
        bound.shutdown().await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::{ServeArgs, explicit_stdio_client, validate_args};
    use gaia_core::error::ErrorCode;

    #[test]
    fn stdio_requires_an_explicit_client_but_http_does_not() {
        assert_eq!(
            explicit_stdio_client(None).unwrap_err().code,
            ErrorCode::Unauthorized
        );
        assert_eq!(explicit_stdio_client(Some("bot")).unwrap(), "bot");
        let mut args = ServeArgs {
            stdio: false,
            http: true,
            port: None,
        };
        assert!(validate_args(&args, None).is_ok());
        args.stdio = true;
        assert_eq!(
            validate_args(&args, Some("bot")).unwrap_err().code,
            ErrorCode::InvalidParams
        );
        args.http = false;
        assert_eq!(
            validate_args(&args, None).unwrap_err().code,
            ErrorCode::Unauthorized
        );
        args.stdio = false;
        assert_eq!(
            validate_args(&args, Some("bot")).unwrap_err().code,
            ErrorCode::InvalidParams
        );
    }
}

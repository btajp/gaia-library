//! gaia serve --stdio --client <name>。識別は起動時に固定される（仕様書 §7.1）。
use clap::Args;

use super::app::App;

#[derive(Args)]
pub struct ServeArgs {
    /// stdio トランスポートで起動する（v0.1 はこれのみ）
    #[arg(long)]
    pub stdio: bool,
}

pub fn serve(app: App, cli_client: Option<&str>, args: &ServeArgs) -> anyhow::Result<()> {
    if !args.stdio {
        anyhow::bail!("v0.1 は --stdio のみ対応です（HTTP は次のサブプロジェクトで追加）");
    }
    let identity = app.identity(cli_client)?;
    tracing::info!(client = %identity.name, role = %identity.role, "starting gaia_library over stdio");
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let server = gaia_mcp::GaiaServer::new(std::sync::Arc::new(app.service), identity);
        gaia_mcp::serve_stdio(server).await
    })?;
    Ok(())
}

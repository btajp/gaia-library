//! stdio トランスポート。stdout は JSON-RPC 専用（ログは stderr のみに出すこと）。
use rmcp::ServiceExt;

use crate::server::GaiaServer;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("initialize failed: {0}")]
    Init(String),
    #[error("server task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

pub async fn serve_stdio(server: GaiaServer) -> Result<(), ServeError> {
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| ServeError::Init(e.to_string()))?;
    running.waiting().await?;
    Ok(())
}

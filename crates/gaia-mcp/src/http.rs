//! Loopback Streamable HTTP。各リクエストの認証と session 所有者を検証する。
use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    http::{
        StatusCode,
        header::{AUTHORIZATION, CONNECTION},
    },
    middleware,
    response::IntoResponse,
};
use gaia_core::{auth::AuthTable, identity::ClientIdentity, tools::ToolService};
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio_util::sync::CancellationToken;

use crate::server::GaiaServer;
use sessions::OwnedSessionManager;

mod sessions;

pub const DEFAULT_PORTS: [u16; 4] = [4111, 4112, 4113, 4114];
const SESSION_HEADER: &str = "mcp-session-id";

#[derive(Debug, thiserror::Error)]
pub enum HttpServeError {
    #[error("auth table is empty: issue a key first (gaia client keygen <name>)")]
    NoKeys,
    #[error("no port available (tried {0:?})")]
    NoPort(Vec<u16>),
    #[error("bind failed on {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("server failed: {0}")]
    Serve(String),
}

pub struct BoundServer {
    local_addr: SocketAddr,
    cancellation: CancellationToken,
    sessions: Arc<OwnedSessionManager>,
    handle: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl BoundServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn url(&self) -> String {
        format!("http://{}/mcp", self.local_addr)
    }

    pub async fn shutdown(self) -> Result<(), HttpServeError> {
        self.cancellation.cancel();
        self.join().await
    }

    pub async fn wait(self) -> Result<(), HttpServeError> {
        self.join().await
    }

    async fn join(self) -> Result<(), HttpServeError> {
        let result = self.handle.await;
        self.cancellation.cancel();
        self.sessions
            .close_all()
            .await
            .map_err(|error| HttpServeError::Serve(error.to_string()))?;
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(HttpServeError::Serve(error.to_string())),
            Err(error) => Err(HttpServeError::Serve(error.to_string())),
        }
    }
}

#[derive(Clone)]
struct HttpState {
    auth: Arc<AuthTable>,
    service: Arc<ToolService>,
    sessions: Arc<OwnedSessionManager>,
    cancellation: CancellationToken,
}

async fn bearer_middleware(
    axum::extract::State(state): axum::extract::State<HttpState>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, StatusCode> {
    let mut authorization = request.headers().get_all(AUTHORIZATION).iter();
    let identity: Option<ClientIdentity> = authorization
        .next()
        .filter(|_| authorization.next().is_none())
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
        // scheme の区切りは 1*SP。キー本体の大小文字や内容は変更しない。
        .map(|(_, token)| token.trim_start_matches(' '))
        .filter(|token| !token.is_empty() && !token.bytes().any(|byte| byte.is_ascii_whitespace()))
        .and_then(|token| state.auth.verify(token));
    let Some(identity) = identity else {
        tracing::warn!("http: rejected request without a valid bearer key");
        return Err(StatusCode::UNAUTHORIZED);
    };
    if state.cancellation.is_cancelled() {
        return Ok(shutdown_response());
    }

    if request.headers().contains_key(SESSION_HEADER) {
        let mut values = request.headers().get_all(SESSION_HEADER).iter();
        let id = values
            .next()
            .filter(|_| values.next().is_none())
            .and_then(|value| value.to_str().ok())
            .ok_or(StatusCode::NOT_FOUND)?;
        if !state
            .sessions
            .is_owned_by(id, &identity.name)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    request.extensions_mut().insert(identity);
    tokio::select! {
        biased;
        _ = state.cancellation.cancelled() => Ok(shutdown_response()),
        response = next.run(request) => Ok(response),
    }
}

fn shutdown_response() -> axum::response::Response {
    (StatusCode::SERVICE_UNAVAILABLE, [(CONNECTION, "close")]).into_response()
}

async fn mcp_request(
    axum::extract::State(state): axum::extract::State<HttpState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let config =
        StreamableHttpServerConfig::default().with_cancellation_token(state.cancellation.clone());
    let sessions = Arc::new(state.sessions.for_request());
    // rmcp 3.1.4 は未知ツール名も schema cache に保持する。HTTP 要求単位で破棄し、
    // session / 認証 / 業務データだけを共有して cache の無制限蓄積を防ぐ。
    let mcp = StreamableHttpService::new(
        move || Ok(GaiaServer::new_http(state.service.clone())),
        sessions.clone(),
        config,
    );
    let response = mcp.handle(request).await.into_response();
    capacity_response(response, sessions.capacity_denied())
}

fn capacity_response(
    response: axum::response::Response,
    capacity_denied: bool,
) -> axum::response::Response {
    // SDK は SessionManager のエラーを一律 500 にする。他の障害の分類は変えない。
    if capacity_denied && response.status() == StatusCode::INTERNAL_SERVER_ERROR {
        StatusCode::TOO_MANY_REQUESTS.into_response()
    } else {
        response
    }
}

pub async fn serve_http(
    service: Arc<ToolService>,
    auth: Arc<AuthTable>,
    port: Option<u16>,
) -> Result<BoundServer, HttpServeError> {
    serve_with_sessions(
        service,
        auth,
        port,
        Arc::new(OwnedSessionManager::default()),
    )
    .await
}

async fn serve_with_sessions(
    service: Arc<ToolService>,
    auth: Arc<AuthTable>,
    port: Option<u16>,
    sessions: Arc<OwnedSessionManager>,
) -> Result<BoundServer, HttpServeError> {
    if auth.is_empty() {
        return Err(HttpServeError::NoKeys);
    }
    let candidates = port.map_or_else(|| DEFAULT_PORTS.to_vec(), |port| vec![port]);
    let mut listener = None;
    let mut last_error = None;
    for candidate in &candidates {
        let addr = SocketAddr::from(([127, 0, 0, 1], *candidate));
        match tokio::net::TcpListener::bind(addr).await {
            Ok(bound) => {
                listener = Some(bound);
                break;
            }
            Err(error) => last_error = Some((addr, error)),
        }
    }
    let Some(listener) = listener else {
        return match (port, last_error) {
            (Some(_), Some((addr, source))) => Err(HttpServeError::Bind { addr, source }),
            _ => Err(HttpServeError::NoPort(candidates)),
        };
    };
    let local_addr = listener
        .local_addr()
        .map_err(|error| HttpServeError::Serve(error.to_string()))?;
    let cancellation = CancellationToken::new();
    let state = HttpState {
        auth,
        service,
        sessions: sessions.clone(),
        cancellation: cancellation.clone(),
    };
    let app = Router::new()
        .nest_service(
            "/mcp",
            Router::new()
                .fallback(mcp_request)
                .with_state(state.clone()),
        )
        .layer(middleware::from_fn_with_state(state, bearer_middleware));
    let shutdown = cancellation.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    Ok(BoundServer {
        local_addr,
        cancellation,
        sessions,
        handle,
    })
}

#[cfg(test)]
mod tests;

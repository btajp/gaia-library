//! SDK の session と同じ寿命で所有者と容量枠を保持する。
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use futures_core::Stream;
use gaia_core::identity::ClientIdentity;
use rmcp::{
    model::{ClientJsonRpcMessage, GetExtensions, ServerJsonRpcMessage},
    transport::streamable_http_server::session::{
        ServerSseMessage, SessionId, SessionManager,
        local::{LocalSessionManager, LocalSessionManagerError, SessionConfig},
    },
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) const MAX_SESSIONS: usize = 128;

struct SessionOwner {
    client: Option<String>,
    _slot: OwnedSemaphorePermit,
}

pub(super) struct OwnedSessionManager {
    inner: Arc<LocalSessionManager>,
    owners: Arc<Mutex<HashMap<SessionId, SessionOwner>>>,
    slots: Arc<Semaphore>,
    capacity_denied: AtomicBool,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum SessionError {
    #[error(transparent)]
    Sdk(#[from] LocalSessionManagerError),
    #[error("active session limit reached")]
    Capacity,
    #[error("session owner state unavailable")]
    StateUnavailable,
    #[error("session has no authenticated owner")]
    MissingOwner,
    #[error("session owner mismatch")]
    OwnerMismatch,
}

impl Default for OwnedSessionManager {
    fn default() -> Self {
        Self::new(MAX_SESSIONS, SessionConfig::default())
    }
}

impl OwnedSessionManager {
    pub(super) fn new(limit: usize, config: SessionConfig) -> Self {
        let mut inner = LocalSessionManager::default();
        inner.session_config = config;
        Self {
            inner: Arc::new(inner),
            owners: Arc::new(Mutex::new(HashMap::new())),
            slots: Arc::new(Semaphore::new(limit)),
            capacity_denied: AtomicBool::new(false),
        }
    }

    pub(super) fn for_request(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            owners: self.owners.clone(),
            slots: self.slots.clone(),
            capacity_denied: AtomicBool::new(false),
        }
    }

    pub(super) fn capacity_denied(&self) -> bool {
        self.capacity_denied.load(Ordering::Relaxed)
    }

    pub(super) async fn is_owned_by(&self, id: &str, client: &str) -> Result<bool, SessionError> {
        let owns = self
            .owners
            .lock()
            .map_err(|_| SessionError::StateUnavailable)?
            .get(id)
            .is_some_and(|owner| owner.client.as_deref() == Some(client));
        if !owns {
            return Ok(false);
        }
        Ok(self.inner.has_session(&SessionId::from(id)).await?)
    }

    #[cfg(test)]
    pub(super) fn available_slots(&self) -> usize {
        self.slots.available_permits()
    }

    pub(super) async fn close_all(&self) -> Result<(), SessionError> {
        let ids: Vec<_> = self
            .owners
            .lock()
            .map_err(|_| SessionError::StateUnavailable)?
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.close_session(&id).await?;
        }
        Ok(())
    }

    fn bind_owner(&self, id: &SessionId, client: String) -> Result<(), SessionError> {
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| SessionError::StateUnavailable)?;
        let entry = owners.get_mut(id).ok_or(SessionError::MissingOwner)?;
        if entry.client.as_ref().is_some_and(|owner| owner != &client) {
            return Err(SessionError::OwnerMismatch);
        }
        entry.client = Some(client);
        Ok(())
    }
}

impl SessionManager for OwnedSessionManager {
    type Error = SessionError;
    type Transport = <LocalSessionManager as SessionManager>::Transport;

    async fn create_session(&self) -> Result<(SessionId, Self::Transport), Self::Error> {
        let slot = self.slots.clone().try_acquire_owned().map_err(|_| {
            // SDK が実際に生成を要求した場合だけ、その HTTP 要求へ容量不足を伝える。
            self.capacity_denied.store(true, Ordering::Relaxed);
            SessionError::Capacity
        })?;
        let (id, transport) = self.inner.create_session().await?;
        // SDK が session を生成した後は await を挟まず枠を移す。初期化中断でも枠を失わない。
        self.owners
            .lock()
            .map_err(|_| SessionError::StateUnavailable)?
            .insert(
                id.clone(),
                SessionOwner {
                    client: None,
                    _slot: slot,
                },
            );
        Ok((id, transport))
    }

    async fn initialize_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, Self::Error> {
        let client = match &message {
            ClientJsonRpcMessage::Request(request) => request
                .request
                .extensions()
                .get::<http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<ClientIdentity>())
                .map(|identity| identity.name.clone()),
            _ => None,
        };
        let Some(client) = client else {
            self.close_session(id).await?;
            return Err(SessionError::MissingOwner);
        };
        self.bind_owner(id, client)?;
        match self.inner.initialize_session(id, message).await {
            Ok(response) => Ok(response),
            Err(error) => {
                self.close_session(id).await?;
                Err(error.into())
            }
        }
    }

    async fn has_session(&self, id: &SessionId) -> Result<bool, Self::Error> {
        Ok(self.inner.has_session(id).await?)
    }

    async fn close_session(&self, id: &SessionId) -> Result<(), Self::Error> {
        // SDK の worker が idle expiry / 接続終了時にもこの関数を呼ぶ。
        // 実 session を先に除去し、キャンセルで容量枠だけが先行解放されるのを防ぐ。
        let result = self.inner.close_session(id).await;
        self.owners
            .lock()
            .map_err(|_| SessionError::StateUnavailable)?
            .remove(id);
        Ok(result?)
    }

    async fn create_stream(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        Ok(self.inner.create_stream(id, message).await?)
    }

    async fn accept_message(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<(), Self::Error> {
        Ok(self.inner.accept_message(id, message).await?)
    }

    async fn create_standalone_stream(
        &self,
        id: &SessionId,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        Ok(self.inner.create_standalone_stream(id).await?)
    }

    async fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + Sync + 'static, Self::Error> {
        Ok(self.inner.resume(id, last_event_id).await?)
    }
}

#[cfg(test)]
mod tests;

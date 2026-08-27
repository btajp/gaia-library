//! 処理中の POST 切断を決定的に再現し、SDK が保持している SSE stream を再開する。
use std::{
    net::SocketAddr,
    sync::{Arc, mpsc},
    thread,
};

use gaia_core::{storage::StorageError, tools::ToolService};
use serde_json::json;

use super::support::{headers, request, status};

pub(super) struct PendingRead {
    release: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for PendingRead {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.join().expect("DB fixture thread failed");
        }
    }
}

pub(super) async fn pending_read(
    service: Arc<ToolService>,
    addr: SocketAddr,
    key: &str,
    session: &str,
) -> PendingRead {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release, released) = mpsc::channel();
    let worker = thread::spawn(move || {
        service
            .db()
            .with_conn::<_, StorageError>(|_| {
                ready_tx.send(()).unwrap();
                let _ = released.recv();
                Ok(())
            })
            .unwrap();
    });
    let pending = PendingRead {
        release: Some(release),
        worker: Some(worker),
    };
    ready_rx.await.unwrap();
    let body = json!({"jsonrpc":"2.0", "id":2, "method":"tools/call", "params":{
        "name":"list_proposals", "arguments":{}
    }});
    let (stream, bytes) = headers(
        addr,
        request(addr, "POST", Some(key), Some(session), "", Some(&body)),
    )
    .await;
    assert_eq!(status(&String::from_utf8(bytes).unwrap()), 200);
    drop(stream);
    pending
}

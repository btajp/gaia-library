use std::{sync::Arc, time::Duration};

use rmcp::transport::streamable_http_server::session::local::SessionConfig;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::support::*;
use crate::http::{serve_with_sessions, sessions::OwnedSessionManager};

async fn wait_for_slots(sessions: &OwnedSessionManager, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while sessions.available_slots() != expected {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("SDK session cleanup did not release its slot");
}

#[tokio::test]
async fn unauthenticated_and_invalid_requests_do_not_allocate_sessions() {
    let (auth, human, _) = auth();
    let sessions = Arc::new(OwnedSessionManager::new(1, SessionConfig::default()));
    let server = serve_with_sessions(service(), auth, Some(0), sessions.clone())
        .await
        .unwrap();
    let addr = server.local_addr();
    for _ in 0..3 {
        let unauthorized = response(
            addr,
            request(addr, "POST", None, None, "", Some(&initialize_message())),
        )
        .await;
        assert_eq!(status(&unauthorized), 401);
        let unknown = response(
            addr,
            request(
                addr,
                "POST",
                Some(&human),
                Some("guessed-session"),
                "",
                Some(&initialize_message()),
            ),
        )
        .await;
        assert_eq!(status(&unknown), 404);
        let invalid = response(
            addr,
            request(
                addr,
                "POST",
                Some(&human),
                None,
                "",
                Some(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})),
            ),
        )
        .await;
        assert_eq!(status(&invalid), 422);
        assert_eq!(sessions.available_slots(), 1);
    }
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn capacity_is_reclaimed_after_delete_and_shutdown() {
    let (auth, human, _) = auth();
    let sessions = Arc::new(OwnedSessionManager::new(1, SessionConfig::default()));
    let server = serve_with_sessions(service(), auth, Some(0), sessions.clone())
        .await
        .unwrap();
    let addr = server.local_addr();
    let id = initialize(addr, &human).await;
    assert_eq!(sessions.available_slots(), 0);
    let full = response(
        addr,
        request(
            addr,
            "POST",
            Some(&human),
            None,
            "",
            Some(&initialize_message()),
        ),
    )
    .await;
    assert_eq!(status(&full), 429);
    let deleted = response(
        addr,
        request(addr, "DELETE", Some(&human), Some(&id), "", None),
    )
    .await;
    assert_eq!(status(&deleted), 202);
    wait_for_slots(&sessions, 1).await;
    assert!(!sessions.is_owned_by(&id, "me").await.unwrap());
    initialize(addr, &human).await;
    assert_eq!(sessions.available_slots(), 0);
    server.shutdown().await.unwrap();
    assert_eq!(sessions.available_slots(), 1);
}

#[tokio::test]
async fn abandoned_initialization_and_idle_sessions_expire_with_the_sdk() {
    let (auth, human, _) = auth();
    let mut config = SessionConfig::default();
    config.keep_alive = Some(Duration::from_millis(100));
    let sessions = Arc::new(OwnedSessionManager::new(1, config));
    let server = serve_with_sessions(service(), auth, Some(0), sessions.clone())
        .await
        .unwrap();
    let addr = server.local_addr();
    // initialize 応答後にクライアントが切断し、initialized 通知を送らない。
    let result = response(
        addr,
        request(
            addr,
            "POST",
            Some(&human),
            None,
            "",
            Some(&initialize_message()),
        ),
    )
    .await;
    assert_eq!(status(&result), 200);
    let abandoned = header(&result, "mcp-session-id").unwrap();
    wait_for_slots(&sessions, 1).await;
    assert!(!sessions.is_owned_by(abandoned, "me").await.unwrap());
    let expired = response(
        addr,
        request(addr, "GET", Some(&human), Some(abandoned), "", None),
    )
    .await;
    assert_eq!(status(&expired), 404);

    let initialized = initialize(addr, &human).await;
    wait_for_slots(&sessions, 1).await;
    assert!(!sessions.is_owned_by(&initialized, "me").await.unwrap());
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_cancels_authenticated_requests_with_an_incomplete_body() {
    for with_session in [false, true] {
        let (auth, human, _) = auth();
        let sessions = Arc::new(OwnedSessionManager::new(1, SessionConfig::default()));
        let server = serve_with_sessions(service(), auth, Some(0), sessions.clone())
            .await
            .unwrap();
        let addr = server.local_addr();
        let session = if with_session {
            format!("Mcp-Session-Id: {}\r\n", initialize(addr, &human).await)
        } else {
            String::new()
        };
        let partial = format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {human}\r\n\
             {session}Accept: application/json, text/event-stream\r\n\
             Content-Type: application/json\r\nMcp-Protocol-Version: 2025-06-18\r\n\
             Expect: 100-continue\r\nContent-Length: 4096\r\n\r\n"
        );
        let (mut held, interim) = headers(addr, partial).await;
        // SDK が body を読みに入ったことを 100 Continue で確認してから中断する。
        assert_eq!(status(&String::from_utf8(interim).unwrap()), 100);
        held.write_all(b"{").await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), server.shutdown())
            .await
            .expect("shutdown waited for an incomplete HTTP body")
            .unwrap();
        let mut response = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(1), held.read_to_end(&mut response))
            .await
            .expect("incomplete-body connection remained open");
        // 未受信 body を残して閉じた TCP は、OS により EOF または RST になる。
        assert!(
            closed.is_ok()
                || closed
                    .as_ref()
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionReset)
        );
        if !response.is_empty() {
            assert_eq!(status(&String::from_utf8(response).unwrap()), 503);
        }
        assert_eq!(sessions.available_slots(), 1);
    }
}

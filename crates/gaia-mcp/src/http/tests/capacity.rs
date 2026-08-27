use std::sync::Arc;

use axum::{http::StatusCode, response::IntoResponse};
use rmcp::transport::streamable_http_server::session::{SessionManager, local::SessionConfig};
use serde_json::json;
use tokio::io::AsyncWriteExt;

use super::{stateless::stateless_call, support::*};
use crate::http::{
    capacity_response, serve_with_sessions,
    sessions::{OwnedSessionManager, SessionError},
};

#[tokio::test]
async fn capacity_does_not_reject_stateless_calls_or_existing_sessions() {
    let (auth, human, agent) = auth();
    let sessions = Arc::new(OwnedSessionManager::new(1, SessionConfig::default()));
    let server = serve_with_sessions(service(), auth, Some(0), sessions.clone())
        .await
        .unwrap();
    let addr = server.local_addr();
    let id = initialize(addr, &human).await;
    assert_eq!(sessions.available_slots(), 0);
    let stateless = response(
        addr,
        stateless_call(addr, &agent, 10, "list_proposals", json!({})),
    )
    .await;
    assert_eq!(status(&stateless), 200, "{stateless}");
    assert_eq!(
        rpc(&stateless)["result"]["structuredContent"]["proposals"],
        json!([])
    );
    assert!(header(&stateless, "mcp-session-id").is_none());
    let existing = call(addr, &human, &id, 11, "get_server_info", json!({})).await;
    assert_eq!(
        existing["result"]["structuredContent"]["client"]["name"],
        "me"
    );
    assert_eq!(sessions.available_slots(), 0);
    assert!(sessions.is_owned_by(&id, "me").await.unwrap());
    server.shutdown().await.unwrap();
    assert_eq!(sessions.available_slots(), 1);
}

#[tokio::test]
async fn capacity_reached_after_header_acceptance_returns_429() {
    let (auth, human, _) = auth();
    let sessions = Arc::new(OwnedSessionManager::new(1, SessionConfig::default()));
    let server = serve_with_sessions(service(), auth, Some(0), sessions.clone())
        .await
        .unwrap();
    let addr = server.local_addr();
    let first = request(
        addr,
        "POST",
        Some(&human),
        None,
        "Expect: 100-continue\r\n",
        Some(&initialize_message()),
    );
    let (head, body) = first.split_once("\r\n\r\n").unwrap();
    let (mut waiting, interim) = headers(addr, format!("{head}\r\n\r\n")).await;
    // 先行要求が middleware を通過して body 待ちに入った後、別要求で最後の枠を取る。
    assert_eq!(status(&String::from_utf8(interim).unwrap()), 100);
    assert_eq!(sessions.available_slots(), 1);
    let id = initialize(addr, &human).await;
    assert_eq!(sessions.available_slots(), 0);
    waiting.write_all(body.as_bytes()).await.unwrap();
    let limited = finish(waiting, Vec::new()).await;
    assert_eq!(status(&limited), 429, "{limited}");
    assert!(header(&limited, "mcp-session-id").is_none());
    assert_eq!(sessions.available_slots(), 0);
    assert!(sessions.is_owned_by(&id, "me").await.unwrap());
    server.shutdown().await.unwrap();
    assert_eq!(sessions.available_slots(), 1);
}

#[tokio::test]
async fn capacity_failure_is_request_local_and_other_errors_are_unchanged() {
    let sessions = OwnedSessionManager::new(0, SessionConfig::default());
    let failed_request = sessions.for_request();
    let unrelated_request = sessions.for_request();
    assert!(matches!(
        failed_request.create_session().await,
        Err(SessionError::Capacity)
    ));
    assert!(failed_request.capacity_denied());
    assert!(!unrelated_request.capacity_denied());
    assert!(!sessions.capacity_denied());
    for (request, input, expected) in [
        (
            &failed_request,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::TOO_MANY_REQUESTS,
        ),
        (
            &unrelated_request,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            &failed_request,
            StatusCode::BAD_REQUEST,
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let original = (input, "original body").into_response();
        let result = capacity_response(original, request.capacity_denied());
        assert_eq!(result.status(), expected);
        let body = axum::body::to_bytes(result.into_body(), 1024)
            .await
            .unwrap();
        if input == expected {
            assert_eq!(body.as_ref(), b"original body");
        } else {
            assert!(body.is_empty());
        }
    }
}

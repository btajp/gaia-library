use std::{sync::Arc, time::Duration};

use gaia_core::{auth::AuthTable, config::Config};
use serde_json::json;

use super::{HttpServeError, serve_http};
use support::*;

mod authentication;
mod capacity;
mod inflight;
mod lifecycle;
mod ownership;
mod stateless;
mod support;

#[tokio::test]
async fn empty_auth_table_is_rejected() {
    let empty = Arc::new(AuthTable::from_config(&Config::default()));
    assert!(matches!(
        serve_http(service(), empty, Some(0)).await,
        Err(HttpServeError::NoKeys)
    ));
}

#[tokio::test]
async fn ephemeral_server_binds_loopback_and_shuts_down() {
    let (auth, _, _) = auth();
    let server = serve_http(service(), auth, Some(0)).await.unwrap();
    assert!(server.local_addr().ip().is_loopback());
    assert_ne!(server.local_addr().port(), 0);
    assert_eq!(server.url(), format!("http://{}/mcp", server.local_addr()));
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn authentication_precedes_session_lookup_for_every_method() {
    let (auth, human, _) = auth();
    let server = serve_http(service(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    for method in ["POST", "GET", "DELETE"] {
        for key in [None, Some("invalid-key")] {
            let result =
                response(addr, request(addr, method, key, Some("unknown"), "", None)).await;
            assert_eq!(status(&result), 401);
        }
        let result = response(
            addr,
            request(addr, method, Some(&human), Some("unknown"), "", None),
        )
        .await;
        assert_eq!(status(&result), 404);
    }
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn human_and_agent_keep_tool_visibility_and_role_authorization() {
    let (auth, human, agent) = auth();
    let server = serve_http(service(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let human_session = initialize(addr, &human).await;
    let agent_session = initialize(addr, &agent).await;
    for (key, id, sees_approval) in [
        (&human, &human_session, true),
        (&agent, &agent_session, false),
    ] {
        let body = json!({"jsonrpc":"2.0", "id":2, "method":"tools/list"});
        let result = response(
            addr,
            request(addr, "POST", Some(key), Some(id), "", Some(&body)),
        )
        .await;
        let listed = rpc(&result);
        assert_eq!(
            listed["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "approve_proposal"),
            sees_approval
        );
    }
    let proposed = call(
        addr,
        &agent,
        &agent_session,
        3,
        "propose_update",
        json!({
            "target_type":"person", "action":"insert", "patch":{"name":"承認対象"},
            "kind":"fact", "request_id":"http-role-proposal"
        }),
    )
    .await;
    let proposal_id = proposed["result"]["structuredContent"]["proposal_id"]
        .as_i64()
        .unwrap();
    let denied = call(
        addr,
        &agent,
        &agent_session,
        4,
        "approve_proposal",
        json!({"proposal_id":proposal_id}),
    )
    .await;
    assert_eq!(denied["error"]["data"]["code"], "unauthorized");
    let approved = call(
        addr,
        &human,
        &human_session,
        4,
        "approve_proposal",
        json!({"proposal_id":proposal_id}),
    )
    .await;
    assert_eq!(
        approved["result"]["structuredContent"]["status"],
        "approved"
    );
    let unknown = call(addr, &agent, &agent_session, 5, "missing-tool", json!({})).await;
    assert_eq!(unknown["error"]["data"]["code"], "not_found");
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_terminates_an_active_sse_session() {
    let (auth, human, _) = auth();
    let server = serve_http(service(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let id = initialize(addr, &human).await;
    let (_active_stream, bytes) = headers(
        addr,
        request(addr, "GET", Some(&human), Some(&id), "", None),
    )
    .await;
    assert_eq!(status(&String::from_utf8(bytes).unwrap()), 200);
    tokio::time::timeout(Duration::from_secs(2), server.shutdown())
        .await
        .expect("shutdown timed out with an active SSE session")
        .unwrap();
}

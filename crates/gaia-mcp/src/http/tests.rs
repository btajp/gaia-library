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

/// 300 ms 待つ解決器。ToolService::call が spawn_blocking で呼ばれ、他の要求を止めないことを観測する。
struct SlowResolver;

impl gaia_core::sources::SourceResolver for SlowResolver {
    fn system(&self) -> &'static str {
        "slow"
    }

    fn availability(
        &self,
        _settings: &gaia_core::config::SourcesConfig,
    ) -> gaia_core::sources::Availability {
        gaia_core::sources::Availability::Ready
    }

    fn max_concurrency(&self) -> usize {
        1
    }

    fn resolve(
        &self,
        _request: gaia_core::sources::ResolveRequest<'_>,
    ) -> Result<gaia_core::sources::Resolved, gaia_core::sources::Unresolved> {
        std::thread::sleep(Duration::from_millis(300));
        Ok(gaia_core::sources::Resolved {
            content: "slow body".into(),
            notes: vec![],
        })
    }
}

#[tokio::test]
async fn blocking_tool_calls_do_not_stall_other_requests() {
    let (auth, human, agent) = auth();
    let mut registry = gaia_core::sources::SourceRegistry::empty();
    registry.register(Arc::new(SlowResolver)).unwrap();
    let service = {
        let db = gaia_core::storage::Db::open_in_memory().unwrap();
        gaia_core::admin::add_affiliation(&db, "fixture", "cn", None).unwrap();
        Arc::new(
            gaia_core::tools::ToolService::new(
                db,
                gaia_core::contracts::Catalog::embedded().unwrap(),
            )
            .with_sources(registry),
        )
    };
    let server = serve_http(service, auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let human_session = initialize(addr, &human).await;
    let person = call(
        addr,
        &human,
        &human_session,
        2,
        "propose_update",
        json!({"target_type":"person","action":"insert","patch":{"name":"対象"},"kind":"fact","request_id":"slow-person-1"}),
    )
    .await;
    let proposal_id = person["result"]["structuredContent"]["proposal_id"]
        .as_i64()
        .unwrap();
    let approved = call(
        addr,
        &human,
        &human_session,
        3,
        "approve_proposal",
        json!({"proposal_id": proposal_id}),
    )
    .await;
    let person_id = approved["result"]["structuredContent"]["result"]["id"]
        .as_i64()
        .unwrap();
    let reference = call(
        addr,
        &human,
        &human_session,
        4,
        "propose_update",
        json!({"target_type":"ref","action":"insert","kind":"fact","request_id":"slow-ref-1",
               "patch":{"target_type":"person","target_id":person_id,"system":"slow","uri":"slow://1","note":"n"}}),
    )
    .await;
    let proposal_id = reference["result"]["structuredContent"]["proposal_id"]
        .as_i64()
        .unwrap();
    let approved = call(
        addr,
        &human,
        &human_session,
        5,
        "approve_proposal",
        json!({"proposal_id": proposal_id}),
    )
    .await;
    let ref_id = approved["result"]["structuredContent"]["result"]["id"]
        .as_i64()
        .unwrap();

    let agent_session = initialize(addr, &agent).await;
    let started = std::time::Instant::now();
    let slow = tokio::spawn({
        let agent = agent.clone();
        let agent_session = agent_session.clone();
        async move {
            call(
                addr,
                &agent,
                &agent_session,
                6,
                "resolve_source",
                json!({"ref_id": ref_id}),
            )
            .await
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let info = call(
        addr,
        &agent,
        &agent_session,
        7,
        "get_server_info",
        json!({}),
    )
    .await;
    let info_elapsed = started.elapsed();
    assert_eq!(info["result"]["structuredContent"]["name"], "gaia_library");
    assert!(
        info_elapsed < Duration::from_millis(280),
        "get_server_info waited for the blocking resolve: {info_elapsed:?}"
    );
    let slow = slow.await.unwrap();
    assert_eq!(slow["result"]["structuredContent"]["resolved"], true);
    assert_eq!(slow["result"]["structuredContent"]["content"], "slow body");
    assert!(started.elapsed() >= Duration::from_millis(300));
    server.shutdown().await.unwrap();
}

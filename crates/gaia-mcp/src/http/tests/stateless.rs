use std::{net::SocketAddr, sync::Arc};

use rmcp::transport::streamable_http_server::session::local::SessionConfig;
use serde_json::{Value, json};

use super::support::*;
use crate::http::{serve_with_sessions, sessions::OwnedSessionManager};

pub(super) fn stateless_call(
    addr: SocketAddr,
    key: &str,
    id: i64,
    name: &str,
    arguments: Value,
) -> String {
    let body = json!({"jsonrpc":"2.0", "id":id, "method":"tools/call", "params": {
        "name":name, "arguments":arguments,
        "_meta": {
            "io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientCapabilities":{}
        }
    }});
    request(
        addr,
        "POST",
        Some(key),
        None,
        &format!("Mcp-Method: tools/call\r\nMcp-Name: {name}\r\n"),
        Some(&body),
    )
    .replace(
        "Mcp-Protocol-Version: 2025-06-18",
        "Mcp-Protocol-Version: 2026-07-28",
    )
}

#[tokio::test]
async fn request_local_schema_lookup_preserves_stateless_tool_errors_and_authorization() {
    let (auth, human, agent) = auth();
    let sessions = Arc::new(OwnedSessionManager::new(1, SessionConfig::default()));
    let server = serve_with_sessions(service(), auth, Some(0), sessions.clone())
        .await
        .unwrap();
    let addr = server.local_addr();
    // 各要求は別々の SDK schema cache を持ち、未知名を後続要求へ蓄積しない。
    let names = (0..64)
        .map(|id| format!("missing-tool-{id}"))
        .chain(["resolve_source".into()]);
    for (id, name) in names.enumerate() {
        let response = response(
            addr,
            stateless_call(addr, &agent, id as i64, &name, json!({})),
        )
        .await;
        assert_eq!(status(&response), 400, "{response}");
        let error = rpc(&response);
        assert_eq!(error["id"], id);
        assert_eq!(error["error"]["code"], -32602);
        assert_eq!(error["error"]["data"]["code"], "not_found");
        assert_eq!(error["error"]["data"]["details"]["tool"], name);
        assert_eq!(sessions.available_slots(), 1);
        assert!(header(&response, "mcp-session-id").is_none());
    }
    let denied = response(
        addr,
        stateless_call(
            addr,
            &agent,
            100,
            "approve_proposal",
            json!({"proposal_id":1}),
        ),
    )
    .await;
    assert_eq!(rpc(&denied)["error"]["data"]["code"], "unauthorized");
    for key in [&human, &agent] {
        let successful = response(
            addr,
            stateless_call(addr, key, 101, "list_proposals", json!({})),
        )
        .await;
        assert_eq!(status(&successful), 200, "{successful}");
        assert_eq!(
            rpc(&successful)["result"]["structuredContent"]["proposals"],
            json!([])
        );
    }
    assert_eq!(sessions.available_slots(), 1);
    server.shutdown().await.unwrap();
}

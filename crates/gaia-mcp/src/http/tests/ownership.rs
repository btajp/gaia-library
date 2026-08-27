use std::sync::Arc;

use gaia_core::{
    auth::{AuthTable, generate_key},
    config::Config,
};
use serde_json::json;

use super::inflight::pending_read;
use super::support::*;
use crate::http::serve_http;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn another_client_cannot_replay_post_to_or_delete_a_session() {
    let (auth, human, agent) = auth();
    let service = service();
    let server = serve_http(service.clone(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let id = initialize(addr, &human).await;
    let pending = pending_read(service, addr, &human, &id).await;

    for method in ["GET", "DELETE", "POST"] {
        let body = json!({"jsonrpc":"2.0", "id":3, "method":"tools/call", "params":{
            "name":"get_server_info", "arguments":{}
        }});
        let result = response(
            addr,
            request(
                addr,
                method,
                Some(&agent),
                Some(&id),
                "Last-Event-ID: 0/0\r\n",
                (method == "POST").then_some(&body),
            ),
        )
        .await;
        assert_eq!(status(&result), 404, "{method}: {result}");
        assert!(!result.contains("structuredContent"));
    }
    // 元の所有者は同じ request stream を再取得でき、拒否された DELETE は session を壊さない。
    let (stream, bytes) = headers(
        addr,
        request(
            addr,
            "GET",
            Some(&human),
            Some(&id),
            "Last-Event-ID: 0/0\r\n",
            None,
        ),
    )
    .await;
    drop(pending);
    let replay = finish(stream, bytes).await;
    assert_eq!(status(&replay), 200, "{replay}");
    let resumed = rpc(&replay);
    assert_eq!(resumed["id"], 2);
    assert_eq!(
        resumed["result"]["structuredContent"]["proposals"],
        json!([])
    );
    let after = call(addr, &human, &id, 4, "get_server_info", json!({})).await;
    assert_eq!(after["result"]["structuredContent"]["client"]["name"], "me");
    let deleted = response(
        addr,
        request(addr, "DELETE", Some(&human), Some(&id), "", None),
    )
    .await;
    assert_eq!(status(&deleted), 202, "{deleted}");
    for method in ["POST", "GET", "DELETE"] {
        let missing = response(
            addr,
            request(addr, method, Some(&human), Some(&id), "", None),
        )
        .await;
        assert_eq!(status(&missing), 404);
    }
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_client_key_rotation_keeps_session_and_old_key_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let (config, old_key, agent) = auth_config();
    config.save(&path).unwrap();
    let auth = Arc::new(AuthTable::from_path(&path).unwrap());
    let service = service();
    let server = serve_http(service.clone(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let id = initialize(addr, &old_key).await;
    let pending = pending_read(service, addr, &old_key, &id).await;
    let (new_key, hash) = generate_key("me");
    Config::update(&path, |config| {
        config.keys.insert("me".into(), hash);
        Ok(())
    })
    .unwrap();
    for method in ["POST", "GET", "DELETE"] {
        let denied = response(
            addr,
            request(
                addr,
                method,
                Some(&old_key),
                Some(&id),
                "Last-Event-ID: 0/0\r\n",
                None,
            ),
        )
        .await;
        assert_eq!(status(&denied), 401);
        let wrong_owner = response(
            addr,
            request(addr, method, Some(&agent), Some(&id), "", None),
        )
        .await;
        assert_eq!(status(&wrong_owner), 404);
    }
    let (stream, bytes) = headers(
        addr,
        request(
            addr,
            "GET",
            Some(&new_key),
            Some(&id),
            "Last-Event-ID: 0/0\r\n",
            None,
        ),
    )
    .await;
    drop(pending);
    let replay = finish(stream, bytes).await;
    assert_eq!(status(&replay), 200);
    let resumed = rpc(&replay);
    assert_eq!(resumed["id"], 2);
    assert_eq!(
        resumed["result"]["structuredContent"]["proposals"],
        json!([])
    );
    let current = call(addr, &new_key, &id, 3, "get_server_info", json!({})).await;
    assert_eq!(
        current["result"]["structuredContent"]["client"]["name"],
        "me"
    );
    let deleted = response(
        addr,
        request(addr, "DELETE", Some(&new_key), Some(&id), "", None),
    )
    .await;
    assert_eq!(status(&deleted), 202);
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn duplicate_or_invalid_session_headers_do_not_bypass_owner_checks() {
    let (auth, human, agent) = auth();
    let server = serve_http(service(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let id = initialize(addr, &human).await;
    let body = json!({"jsonrpc":"2.0", "id":2, "method":"tools/list"});
    for extra in [
        format!("Mcp-Session-Id: {id}\r\n"),
        "Mcp-Session-Id: invalid\r\n".into(),
    ] {
        let result = response(
            addr,
            request(addr, "POST", Some(&agent), Some(&id), &extra, Some(&body)),
        )
        .await;
        assert_eq!(status(&result), 404);
    }
    let duplicate_auth = format!("Authorization: Bearer {human}\r\n");
    let result = response(
        addr,
        request(
            addr,
            "POST",
            Some(&agent),
            Some(&id),
            &duplicate_auth,
            Some(&body),
        ),
    )
    .await;
    assert_eq!(status(&result), 401);
    server.shutdown().await.unwrap();
}

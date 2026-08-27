use std::{net::SocketAddr, sync::Arc, time::Duration};

use gaia_core::{
    auth::{AuthTable, generate_key},
    config::Config,
    contracts::Catalog,
    identity::{ClientIdentity, Role},
    storage::Db,
    tools::ToolService,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub(super) fn service() -> Arc<ToolService> {
    let db = Db::open_in_memory().unwrap();
    gaia_core::admin::add_affiliation(&db, "fixture", "cn", None).unwrap();
    Arc::new(ToolService::new(db, Catalog::embedded().unwrap()))
}

pub(super) fn auth_config() -> (Config, String, String) {
    let mut config = Config::default();
    let mut keys = Vec::new();
    for (name, role) in [("me", Role::Human), ("bot", Role::Agent)] {
        config
            .add_client(ClientIdentity {
                name: name.into(),
                role,
                default_scope: Some("cn".into()),
            })
            .unwrap();
        let (key, hash) = generate_key(name);
        config.keys.insert(name.into(), hash);
        keys.push(key);
    }
    (config, keys.remove(0), keys.remove(0))
}

pub(super) fn auth() -> (Arc<AuthTable>, String, String) {
    let (config, human, agent) = auth_config();
    (Arc::new(AuthTable::from_config(&config)), human, agent)
}

pub(super) fn request(
    addr: SocketAddr,
    method: &str,
    key: Option<&str>,
    session: Option<&str>,
    extra_headers: &str,
    body: Option<&Value>,
) -> String {
    let body = body.map(Value::to_string).unwrap_or_default();
    let auth = key
        .map(|key| format!("Authorization: Bearer {key}\r\n"))
        .unwrap_or_default();
    let session = session
        .map(|id| format!("Mcp-Session-Id: {id}\r\n"))
        .unwrap_or_default();
    format!(
        "{method} /mcp HTTP/1.1\r\nHost: {addr}\r\n{auth}{session}\
         Accept: application/json, text/event-stream\r\nContent-Type: application/json\r\n\
         Mcp-Protocol-Version: 2025-06-18\r\nConnection: close\r\n\
         {extra_headers}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

pub(super) async fn headers(addr: SocketAddr, request: String) -> (TcpStream, Vec<u8>) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "response ended before headers");
            bytes.extend_from_slice(&chunk[..read]);
        }
        (stream, bytes)
    })
    .await
    .expect("HTTP response headers timed out")
}

pub(super) async fn response(addr: SocketAddr, request: String) -> String {
    let (stream, bytes) = headers(addr, request).await;
    finish(stream, bytes).await
}

pub(super) async fn finish(mut stream: TcpStream, mut bytes: Vec<u8>) -> String {
    tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut bytes))
        .await
        .expect("HTTP response body timed out")
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

pub(super) fn status(response: &str) -> u16 {
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

pub(super) fn header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    response
        .split("\r\n\r\n")
        .next()
        .unwrap()
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then_some(value.trim())
        })
}

pub(super) fn rpc(response: &str) -> Value {
    let (_, raw_body) = response.split_once("\r\n\r\n").unwrap();
    let body = if header(response, "transfer-encoding") == Some("chunked") {
        let mut rest = raw_body;
        let mut body = String::new();
        loop {
            let (size, after_size) = rest.split_once("\r\n").expect("chunk size");
            let size = usize::from_str_radix(size.split(';').next().unwrap(), 16).unwrap();
            if size == 0 {
                break;
            }
            body.push_str(&after_size[..size]);
            rest = &after_size[size + 2..];
        }
        body
    } else {
        raw_body.into()
    };
    if header(response, "content-type") == Some("application/json") {
        return serde_json::from_str(&body).expect("JSON-RPC response");
    }
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .find_map(|data| serde_json::from_str(data).ok())
        .unwrap_or_else(|| panic!("JSON-RPC SSE event missing in response: {response}"))
}

pub(super) fn initialize_message() -> Value {
    json!({"jsonrpc":"2.0", "id": 1, "method":"initialize", "params": {
        "protocolVersion":"2025-06-18", "capabilities":{},
        "clientInfo":{"name":"http-test", "version":"0"}
    }})
}

pub(super) async fn initialize(addr: SocketAddr, key: &str) -> String {
    let result = response(
        addr,
        request(
            addr,
            "POST",
            Some(key),
            None,
            "",
            Some(&initialize_message()),
        ),
    )
    .await;
    assert_eq!(status(&result), 200, "{result}");
    assert!(rpc(&result).get("result").is_some());
    let id = header(&result, "mcp-session-id").unwrap().to_string();
    let initialized = json!({"jsonrpc":"2.0", "method":"notifications/initialized"});
    let result = response(
        addr,
        request(addr, "POST", Some(key), Some(&id), "", Some(&initialized)),
    )
    .await;
    assert_eq!(status(&result), 202, "{result}");
    id
}

pub(super) async fn call(
    addr: SocketAddr,
    key: &str,
    id: &str,
    request_id: i64,
    name: &str,
    args: Value,
) -> Value {
    let body = json!({"jsonrpc":"2.0", "id":request_id, "method":"tools/call", "params":{"name":name,"arguments":args}});
    let result = response(
        addr,
        request(addr, "POST", Some(key), Some(id), "", Some(&body)),
    )
    .await;
    assert_eq!(status(&result), 200, "{result}");
    rpc(&result)
}

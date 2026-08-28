//! HTTP の実プロセス統合テスト。認証・role・キー再発行を JSON-RPC で確認する。
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use serde_json::{Value, json};

struct HttpServer {
    child: Child,
    url: String,
}

impl HttpServer {
    fn start(dir: &Path) -> Self {
        Self::start_with_json(dir, false)
    }

    fn start_with_json(dir: &Path, compact: bool) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gaia"));
        command.args(["serve", "--http", "--port", "0"]);
        if compact {
            command.arg("--json");
        }
        let mut child = command
            .env("GAIA_CONFIG", dir.join("config.toml"))
            .env("GAIA_DB", dir.join("gaia.db"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn gaia serve --http");
        let ready_output: Box<dyn Read + Send> = if compact {
            Box::new(child.stdout.take().unwrap())
        } else {
            Box::new(child.stderr.take().unwrap())
        };
        let (ready, listening) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(ready_output).lines().map_while(Result::ok) {
                if compact {
                    let output: Value = serde_json::from_str(&line).expect("JSON readiness");
                    assert_eq!(output["status"], "listening");
                    let _ = ready.send(output["url"].as_str().unwrap().to_string());
                } else if let Some(url) = line.trim().strip_prefix("gaia_library listening on ") {
                    let _ = ready.send(url.to_string());
                }
            }
        });
        let url = match listening.recv_timeout(Duration::from_secs(10)) {
            Ok(url) => url,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("server did not report its listening URL: {error}");
            }
        };
        Self { child, url }
    }

    #[cfg(unix)]
    fn shutdown(mut self) {
        let sent = Command::new("/bin/kill")
            .args(["-INT", &self.child.id().to_string()])
            .status()
            .expect("send SIGINT to the test server");
        assert!(sent.success());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "SIGINT shutdown must be graceful");
                break;
            }
            assert!(std::time::Instant::now() < deadline, "shutdown timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cli_ok(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .env("GAIA_CONFIG", dir.join("config.toml"))
        .env("GAIA_DB", dir.join("gaia.db"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn setup(dir: &Path) -> (String, String) {
    cli_ok(
        dir,
        &["init", "--affiliation", "cloudnative", "--client", "tester"],
    );
    cli_ok(
        dir,
        &[
            "add",
            "person",
            "--name",
            "岡村 慎太郎",
            "--alias",
            "okash1n",
        ],
    );
    let agent_key = cli_ok(
        dir,
        &[
            "client",
            "add",
            "bot",
            "--role",
            "agent",
            "--default-scope",
            "cloudnative",
            "--generate-key",
        ],
    );
    let human_key = cli_ok(dir, &["client", "keygen", "tester"]);
    (agent_key, human_key)
}

/// SSE の data 行、または素の JSON から JSON-RPC 応答を取り出す。
fn parse_body(text: &str) -> Value {
    for line in text.lines() {
        if let Some(data) = line.trim().strip_prefix("data:")
            && let Ok(value) = serde_json::from_str(data.trim())
        {
            return value;
        }
    }
    serde_json::from_str(text.trim())
        .unwrap_or_else(|error| panic!("unparseable body ({error}): {text}"))
}

struct Rpc {
    url: String,
    key: Option<String>,
    session: Option<String>,
    agent: ureq::Agent,
}

impl Rpc {
    fn new(url: &str, key: Option<&str>) -> Self {
        Self {
            url: url.into(),
            key: key.map(String::from),
            session: None,
            agent: ureq::Agent::config_builder()
                .proxy(None)
                .timeout_global(Some(Duration::from_secs(10)))
                .build()
                .into(),
        }
    }

    fn post(&self, body: Value) -> (u16, Value, Option<String>) {
        let mut request = self
            .agent
            .post(&self.url)
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json")
            .header("MCP-Protocol-Version", "2025-06-18");
        if let Some(key) = &self.key {
            request = request.header("Authorization", &format!("Bearer {key}"));
        }
        if let Some(session) = &self.session {
            request = request.header("Mcp-Session-Id", session);
        }
        match request.send(body.to_string()) {
            Ok(mut response) => {
                let session = response
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|value| value.to_str().ok())
                    .map(String::from);
                let status = response.status().as_u16();
                let text = response.body_mut().read_to_string().unwrap();
                let body = if text.trim().is_empty() {
                    Value::Null
                } else {
                    parse_body(&text)
                };
                (status, body, session)
            }
            Err(ureq::Error::StatusCode(status)) => (status, Value::Null, None),
            Err(error) => panic!("request failed: {error}"),
        }
    }

    fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        let (status, body, session) = self.post(json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        assert_eq!(status, 200, "{method}: {body}");
        assert_eq!(body["id"], id);
        if session.is_some() {
            self.session = session;
        }
        body
    }

    fn initialize(&mut self) {
        let initialized = self.request(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "integration-test", "version": "0"}
            }),
        );
        assert_eq!(initialized["result"]["serverInfo"]["name"], "gaia_library");
        assert!(self.session.is_some(), "initialize must create a session");
        let (status, _, _) = self.post(json!({
            "jsonrpc": "2.0", "method": "notifications/initialized"
        }));
        assert!(status == 200 || status == 202, "initialized: {status}");
    }
}

fn tool_names(listed: &Value) -> Vec<&str> {
    listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect()
}

#[test]
fn http_auth_roles_search_and_live_key_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let (agent_key, human_key) = setup(dir.path());
    let server = HttpServer::start(dir.path());

    for key in [None, Some("gaia_bot_wrong")] {
        let unauthenticated = Rpc::new(&server.url, key);
        let (status, _, _) =
            unauthenticated.post(json!({"jsonrpc": "2.0", "id": 0, "method": "ping"}));
        assert_eq!(status, 401);
    }

    let mut agent = Rpc::new(&server.url, Some(&agent_key));
    agent.initialize();
    let listed = agent.request(2, "tools/list", json!({}));
    let names = tool_names(&listed);
    assert!(names.contains(&"search_context"));
    assert!(!names.contains(&"approve_proposal"));
    assert!(!names.contains(&"reject_proposal"));
    assert!(names.contains(&"resolve_source"));

    let searched = agent.request(
        3,
        "tools/call",
        json!({"name": "search_context", "arguments": {"query": "okash1n"}}),
    );
    assert_eq!(searched["result"]["isError"], false);
    assert_eq!(
        searched["result"]["structuredContent"]["entities"][0]["type"],
        "person"
    );
    let denied = agent.request(
        4,
        "tools/call",
        json!({"name": "approve_proposal", "arguments": {"proposal_id": 1}}),
    );
    assert_eq!(denied["error"]["code"], -32001);

    let mut human = Rpc::new(&server.url, Some(&human_key));
    human.initialize();
    let listed = human.request(2, "tools/list", json!({}));
    let names = tool_names(&listed);
    assert!(names.contains(&"approve_proposal"));
    assert!(names.contains(&"reject_proposal"));

    let replacement = cli_ok(dir.path(), &["client", "keygen", "bot"]);
    assert_ne!(replacement, agent_key);
    let (status, _, _) = agent.post(json!({"jsonrpc": "2.0", "id": 5, "method": "ping"}));
    assert_eq!(status, 401, "old key must be rejected without restarting");
    agent.key = Some(replacement);
    let listed = agent.request(6, "tools/list", json!({}));
    assert!(tool_names(&listed).contains(&"search_context"));
    assert!(!tool_names(&listed).contains(&"approve_proposal"));

    #[cfg(unix)]
    server.shutdown();
}

#[test]
fn http_without_client_reports_json_readiness_on_loopback() {
    let dir = tempfile::tempdir().unwrap();
    let (agent_key, _) = setup(dir.path());
    let server = HttpServer::start_with_json(dir.path(), true);
    assert!(server.url.starts_with("http://127.0.0.1:"));
    assert!(server.url.ends_with("/mcp"));
    let mut agent = Rpc::new(&server.url, Some(&agent_key));
    agent.initialize();

    #[cfg(unix)]
    server.shutdown();
}

#[test]
fn unicode_client_names_authenticate_over_http_without_changing_identity() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let name = "日本語 クライアント";
    let key = cli_ok(
        dir.path(),
        &[
            "client",
            "add",
            name,
            "--role",
            "agent",
            "--default-scope",
            "cloudnative",
            "--generate-key",
        ],
    );
    assert!(key.is_ascii());
    assert!(!key.bytes().any(|byte| byte.is_ascii_whitespace()));
    let server = HttpServer::start(dir.path());
    let mut client = Rpc::new(&server.url, Some(&key));
    client.initialize();
    let info = client.request(
        2,
        "tools/call",
        json!({
            "name": "get_server_info", "arguments": {}
        }),
    );
    assert_eq!(info["result"]["structuredContent"]["client"]["name"], name);
    assert_eq!(
        info["result"]["structuredContent"]["client"]["role"],
        "agent"
    );
    let listed = client.request(3, "tools/list", json!({}));
    assert!(!tool_names(&listed).contains(&"approve_proposal"));
    let searched = client.request(
        4,
        "tools/call",
        json!({
            "name": "search_context", "arguments": {"query": "okash1n"}
        }),
    );
    assert_eq!(searched["result"]["isError"], false);
    assert_eq!(
        searched["result"]["structuredContent"]["entities"][0]["type"],
        "person"
    );

    #[cfg(unix)]
    server.shutdown();
}

#[test]
fn generated_key_for_japanese_client_initializes_http_as_original_client() {
    let dir = tempfile::tempdir().unwrap();
    cli_ok(
        dir.path(),
        &[
            "init",
            "--affiliation",
            "cloudnative",
            "--client-name",
            "tester",
        ],
    );
    let client_name = "議事録クライアント";
    let key = cli_ok(
        dir.path(),
        &[
            "client",
            "add",
            client_name,
            "--role",
            "agent",
            "--default-scope",
            "cloudnative",
            "--generate-key",
        ],
    );
    assert!(key.bytes().all(|byte| byte.is_ascii_graphic()));
    assert_eq!(key.lines().count(), 1);
    let server = HttpServer::start(dir.path());
    let mut client = Rpc::new(&server.url, Some(&key));
    client.initialize();
    let info = client.request(
        2,
        "tools/call",
        json!({
            "name": "get_server_info", "arguments": {}
        }),
    );
    assert_eq!(info["result"]["isError"], false);
    let identity = &info["result"]["structuredContent"]["client"];
    assert_eq!(identity["name"], client_name);
    assert_eq!(identity["role"], "agent");
    assert_eq!(identity["default_scope"], "cloudnative");
}

#[test]
fn http_agent_can_call_resolve_source_and_unauthenticated_calls_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (agent_key, _human_key) = setup(dir.path());
    let server = HttpServer::start(dir.path());
    let unauthenticated = Rpc::new(&server.url, None);
    let (status, _, _) = unauthenticated.post(json!({
        "jsonrpc": "2.0", "id": 0, "method": "tools/call",
        "params": {"name": "resolve_source", "arguments": {"ref_id": 1}}
    }));
    assert_eq!(status, 401);
    let mut agent = Rpc::new(&server.url, Some(&agent_key));
    agent.initialize();
    let missing = agent.request(
        2,
        "tools/call",
        json!({"name": "resolve_source", "arguments": {"ref_id": 9999}}),
    );
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(
        missing["result"]["structuredContent"]["error"]["code"],
        "not_found"
    );
    let invalid = agent.request(
        3,
        "tools/call",
        json!({"name": "resolve_source", "arguments": {}}),
    );
    assert_eq!(invalid["error"]["code"], -32602);
    assert_eq!(invalid["error"]["data"]["code"], "invalid_params");
}

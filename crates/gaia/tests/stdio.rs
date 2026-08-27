//! stdio MCP スモーク: initialize → tools/list → tools/call。role でツール可視性が変わること。
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdout, Command, Stdio};

struct Server {
    child: Child,
    reader: BufReader<ChildStdout>,
}

impl Server {
    fn start(dir: &std::path::Path, client: &str) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_gaia"))
            .args(["serve", "--stdio", "--client", client])
            .env("GAIA_CONFIG", dir.join("config.toml"))
            .env("GAIA_DB", dir.join("gaia.db"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn gaia serve");
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server { child, reader };
        s.send(serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "it", "version": "0"}}}));
        let init = s.recv();
        assert_eq!(init["result"]["serverInfo"]["name"], "gaia_library");
        s.send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        s
    }

    fn send(&mut self, v: serde_json::Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{v}").unwrap();
        stdin.flush().unwrap();
    }

    fn recv(&mut self) -> serde_json::Value {
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).unwrap();
            assert!(n > 0, "server closed stdout");
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            return serde_json::from_str(t).unwrap_or_else(|e| panic!("not json: {e}: {t}"));
        }
    }

    fn request(&mut self, id: i64, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(
            serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        );
        loop {
            let msg = self.recv();
            if msg.get("id").and_then(|v| v.as_i64()) == Some(id) {
                return msg;
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn setup(dir: &std::path::Path) {
    let run = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_gaia"))
            .args(args)
            .env("GAIA_CONFIG", dir.join("config.toml"))
            .env("GAIA_DB", dir.join("gaia.db"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&[
        "init",
        "--affiliation",
        "cloudnative",
        "--client-name",
        "tester",
    ]);
    run(&[
        "client",
        "add",
        "bot",
        "--role",
        "agent",
        "--default-scope",
        "cloudnative",
    ]);
    run(&[
        "add",
        "person",
        "--name",
        "岡村 慎太郎",
        "--alias",
        "okash1n",
    ]);
}

#[test]
fn agent_sees_filtered_tools_and_calls_search() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let mut s = Server::start(dir.path(), "bot");

    let listed = s.request(1, "tools/list", serde_json::json!({}));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"search_context"));
    assert!(
        !names.contains(&"approve_proposal"),
        "agent には承認系が見えない: {names:?}"
    );
    assert!(!names.contains(&"resolve_source"), "未登録ツールは見えない");

    let called = s.request(
        2,
        "tools/call",
        serde_json::json!({"name": "search_context", "arguments": {"query": "okash1n"}}),
    );
    assert_eq!(called["result"]["isError"], serde_json::json!(false));
    assert_eq!(
        called["result"]["structuredContent"]["entities"][0]["type"],
        "person"
    );

    // 業務エラー（not_found）は isError の結果
    let nf = s.request(
        3,
        "tools/call",
        serde_json::json!({"name": "get_person", "arguments": {"person_id": 9999}}),
    );
    assert_eq!(nf["result"]["isError"], serde_json::json!(true));
    assert_eq!(
        nf["result"]["structuredContent"]["error"]["code"],
        "not_found"
    );

    // 認可エラーは JSON-RPC エラー（-32001）
    let denied = s.request(
        4,
        "tools/call",
        serde_json::json!({"name": "approve_proposal", "arguments": {"proposal_id": 1}}),
    );
    assert_eq!(denied["error"]["code"], serde_json::json!(-32001));

    // 引数のスキーマ違反は JSON-RPC エラー（-32602）
    let bad = s.request(
        5,
        "tools/call",
        serde_json::json!({"name": "search_context", "arguments": {"query": 1}}),
    );
    assert_eq!(bad["error"]["code"], serde_json::json!(-32602));
}

#[test]
fn human_sees_approval_tools() {
    let dir = tempfile::tempdir().unwrap();
    setup(dir.path());
    let mut s = Server::start(dir.path(), "tester");
    let listed = s.request(1, "tools/list", serde_json::json!({}));
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"approve_proposal"));
    assert!(names.contains(&"reject_proposal"));
}

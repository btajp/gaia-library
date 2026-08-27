//! 生成スニペットは別の cwd・設定環境でも、生成元の識別と実効 DB を保持する。
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use gaia_core::config::Config;
use serde_json::{Value, json};

fn gaia(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gaia"));
    command
        .current_dir(dir)
        .env("GAIA_CONFIG", "settings/config.toml")
        .env("GAIA_DB", "data/selected.db");
    command
}

fn run_ok(command: &mut Command) -> Value {
    let output = command.output().expect("spawn gaia");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if output.stdout.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&output.stdout).expect("JSON output")
    }
}

struct SnippetServer {
    child: Child,
    messages: mpsc::Receiver<Value>,
}

impl SnippetServer {
    fn start(snippet: &Value, launch_dir: &Path) -> Self {
        let server = &snippet["mcpServers"]["gaia_library"];
        let executable = Path::new(env!("CARGO_BIN_EXE_gaia"));
        let inherited_path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(executable.parent().unwrap().to_path_buf())
                .chain(std::env::split_paths(&inherited_path)),
        )
        .unwrap();
        let mut command = Command::new(server["command"].as_str().unwrap());
        command
            .current_dir(launch_dir)
            .env("PATH", path)
            .env("GAIA_CONFIG", "settings/config.toml")
            .env("GAIA_DB", "data/selected.db")
            .args(
                server["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|arg| arg.as_str().unwrap()),
            );
        for (name, value) in server["env"].as_object().unwrap() {
            command.env(name, value.as_str().unwrap());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("launch generated MCP snippet");
        let stdout = child.stdout.take().unwrap();
        let (send, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if !line.trim().is_empty() {
                    let value = serde_json::from_str(&line).expect("JSON-RPC stdout");
                    if send.send(value).is_err() {
                        break;
                    }
                }
            }
        });
        let mut server = Self { child, messages };
        let initialized = server.request(
            0,
            "initialize",
            json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "snippet-launch-test", "version": "0"}
            }),
        );
        assert_eq!(initialized["result"]["serverInfo"]["name"], "gaia_library");
        server.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        server
    }

    fn send(&mut self, message: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(10))
                .expect("MCP response before timeout");
            if message["id"].as_i64() == Some(id) {
                return message;
            }
        }
    }
}

impl Drop for SnippetServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_snippet_keeps_config_and_db(db_from_config: bool) {
    let source = tempfile::tempdir().unwrap();
    let launch = tempfile::tempdir().unwrap();
    run_ok(gaia(source.path()).args(["init", "--affiliation", "primary", "--client", "operator"]));
    run_ok(gaia(source.path()).args([
        "client",
        "add",
        "bot",
        "--role",
        "agent",
        "--default-scope",
        "primary",
    ]));
    let added = run_ok(gaia(source.path()).args([
        "add",
        "person",
        "--name",
        "Selected database",
        "--alias",
        "snippet-source-proof",
    ]));
    // 同名 client が human の別設定と空 DB を、起動先の既定値として用意する。
    run_ok(gaia(launch.path()).args(["init", "--affiliation", "primary", "--client", "bot"]));

    let config_path = source.path().join("settings/config.toml");
    let mut generate = gaia(source.path());
    if db_from_config {
        Config::update(&config_path, |config| {
            config.db_path = Some(PathBuf::from("data/selected.db"));
            Ok(())
        })
        .unwrap();
        generate.env_remove("GAIA_DB");
    } else {
        assert!(Config::load(&config_path).unwrap().db_path.is_none());
    }
    let snippet = run_ok(generate.args([
        "--config",
        "settings/config.toml",
        "--json",
        "client",
        "mcp-config",
        "bot",
    ]));
    let server = &snippet["mcpServers"]["gaia_library"];
    let args = server["args"].as_array().unwrap();
    let config_index = args.iter().position(|arg| arg == "--config").unwrap();
    let retained_config = Path::new(args[config_index + 1].as_str().unwrap());
    assert!(retained_config.is_absolute());
    assert_eq!(
        retained_config.canonicalize().unwrap(),
        config_path.canonicalize().unwrap()
    );
    let retained_db = Path::new(server["env"]["GAIA_DB"].as_str().unwrap());
    assert!(retained_db.is_absolute());
    assert_eq!(
        retained_db.canonicalize().unwrap(),
        source
            .path()
            .join("data/selected.db")
            .canonicalize()
            .unwrap()
    );

    let mut launched = SnippetServer::start(&snippet, launch.path());
    let info = launched.request(
        1,
        "tools/call",
        json!({"name": "get_server_info", "arguments": {}}),
    );
    assert_eq!(info["result"]["structuredContent"]["client"]["name"], "bot");
    assert_eq!(
        info["result"]["structuredContent"]["client"]["role"],
        "agent"
    );
    let listed = launched.request(2, "tools/list", json!({}));
    assert!(
        listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["name"] != "approve_proposal")
    );
    let searched = launched.request(
        3,
        "tools/call",
        json!({
            "name": "search_context", "arguments": {"query": "snippet-source-proof"}
        }),
    );
    assert_eq!(searched["result"]["isError"], false);
    assert_eq!(
        searched["result"]["structuredContent"]["entities"][0]["id"],
        added["result"]["id"]
    );
    assert_eq!(
        searched["result"]["structuredContent"]["entities"][0]["name"],
        "Selected database"
    );
}

#[test]
fn generated_stdio_snippet_preserves_custom_config_and_gaia_db() {
    assert_snippet_keeps_config_and_db(false);
}

#[test]
fn generated_stdio_snippet_preserves_relative_database_from_config() {
    assert_snippet_keeps_config_and_db(true);
}

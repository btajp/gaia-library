//! MCP スニペットの平文キーは明示出力だけに含め、入力不備や診断には出さない。
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use gaia_core::config::Config;
use serde_json::{Value, json};

fn gaia(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gaia"));
    command
        .env("GAIA_CONFIG", dir.join("config.toml"))
        .env("GAIA_DB", dir.join("gaia.db"));
    command
}

fn json_output(output: Output, success: bool) -> Value {
    assert_eq!(output.status.success(), success);
    let text = if success {
        assert!(output.stderr.is_empty(), "success must not use stderr");
        String::from_utf8(output.stdout).unwrap()
    } else {
        assert!(
            output.stdout.is_empty(),
            "failed snippet must not emit a key"
        );
        String::from_utf8(output.stderr).unwrap()
    };
    assert_eq!(text.lines().count(), 1, "JSON must be one line");
    serde_json::from_str(&text).expect("JSON response")
}

fn json_ok(command: &mut Command) -> Value {
    json_output(command.arg("--json").output().unwrap(), true)
}

fn json_error(command: &mut Command) -> Value {
    json_output(command.arg("--json").output().unwrap(), false)
}

fn setup(dir: &Path) -> String {
    let initialized = gaia(dir)
        .args(["init", "--affiliation", "primary", "--client", "tester"])
        .output()
        .unwrap();
    assert!(initialized.status.success());
    let issued =
        json_ok(gaia(dir).args(["client", "add", "bot", "--role", "agent", "--generate-key"]));
    issued["key"].as_str().unwrap().to_string()
}

fn stdin_snippet(dir: &Path, input: &[u8]) -> Output {
    let mut child = gaia(dir)
        .args([
            "--json",
            "client",
            "mcp-config",
            "bot",
            "--transport",
            "http",
            "--key-stdin",
            "--port",
            "4123",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(input).unwrap();
    drop(stdin);
    child.wait_with_output().unwrap()
}

#[test]
fn snippets_require_a_current_key_and_fixed_http_port() {
    let dir = tempfile::tempdir().unwrap();
    let key = setup(dir.path());
    let snippet = json_ok(gaia(dir.path()).args([
        "client",
        "mcp-config",
        "bot",
        "--transport",
        "http",
        "--key",
        &key,
        "--port",
        "4123",
    ]));
    assert_eq!(
        snippet["mcpServers"]["gaia_library"]["url"],
        "http://127.0.0.1:4123/mcp"
    );
    assert_eq!(
        snippet["mcpServers"]["gaia_library"]["headers"]["Authorization"],
        format!("Bearer {key}")
    );

    for port in [None, Some("0")] {
        let mut command = gaia(dir.path());
        command.args([
            "client",
            "mcp-config",
            "bot",
            "--transport",
            "http",
            "--key",
            &key,
        ]);
        if let Some(port) = port {
            command.args(["--port", port]);
        }
        let error = json_error(&mut command);
        assert_eq!(error["code"], "invalid_params");
        assert!(error["message"].as_str().unwrap().contains("固定ポート"));
        assert!(!error.to_string().contains(&key));
    }

    Config::update(&dir.path().join("config.toml"), |config| {
        config.server.port = Some(4222);
        Ok(())
    })
    .unwrap();
    let configured = json_ok(gaia(dir.path()).args([
        "client",
        "mcp-config",
        "bot",
        "--transport",
        "http",
        "--key",
        &key,
    ]));
    assert_eq!(
        configured["mcpServers"]["gaia_library"]["url"],
        "http://127.0.0.1:4222/mcp"
    );

    json_ok(gaia(dir.path()).args(["client", "keygen", "bot"]));
    let stale = json_error(gaia(dir.path()).args([
        "client",
        "mcp-config",
        "bot",
        "--transport",
        "http",
        "--key",
        &key,
    ]));
    assert_eq!(stale["code"], "invalid_params");
    assert!(!stale.to_string().contains(&key));
}

#[test]
fn stdio_snippets_keep_explicit_identity_and_reject_http_options() {
    let dir = tempfile::tempdir().unwrap();
    let key = setup(dir.path());
    let snippet = json_ok(gaia(dir.path()).args(["client", "mcp-config", "bot"]));
    assert_eq!(snippet["mcpServers"]["gaia_library"]["command"], "gaia");
    assert_eq!(
        snippet["mcpServers"]["gaia_library"]["args"],
        json!([
            "serve",
            "--stdio",
            "--client",
            "bot",
            "--config",
            std::path::absolute(dir.path().join("config.toml")).unwrap()
        ])
    );
    assert_eq!(
        snippet["mcpServers"]["gaia_library"]["env"]["GAIA_DB"],
        json!(std::path::absolute(dir.path().join("gaia.db")).unwrap())
    );
    for options in [
        vec!["--key", &key],
        vec!["--key-stdin"],
        vec!["--port", "4123"],
    ] {
        let error = json_error(
            gaia(dir.path())
                .args(["client", "mcp-config", "bot"])
                .args(options),
        );
        assert_eq!(error["code"], "invalid_params");
        assert!(!error.to_string().contains(&key));
    }
    let unknown_transport = json_error(gaia(dir.path()).args([
        "client",
        "mcp-config",
        "bot",
        "--transport",
        "unknown",
        "--key",
        &key,
    ]));
    assert_eq!(unknown_transport["code"], "invalid_params");
    assert!(!unknown_transport.to_string().contains(&key));
}

#[test]
fn stdin_keys_remove_only_trailing_newlines_and_never_leak_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let key = setup(dir.path());
    for suffix in ["", "\n", "\r\n"] {
        let input = format!("{key}{suffix}");
        let snippet = json_output(stdin_snippet(dir.path(), input.as_bytes()), true);
        assert_eq!(
            snippet["mcpServers"]["gaia_library"]["headers"]["Authorization"],
            format!("Bearer {key}")
        );
    }
    for input in [
        Vec::new(),
        b"\n".to_vec(),
        b"private-wrong-key\n".to_vec(),
        format!("{key} \n").into_bytes(),
        format!("{key}\n{key}\n").into_bytes(),
        b"private-invalid-utf8\xff".to_vec(),
    ] {
        let error = json_output(stdin_snippet(dir.path(), &input), false);
        assert_eq!(error["code"], "invalid_params");
        let text = error.to_string();
        assert!(!text.contains(&key));
        assert!(!text.contains("private-wrong-key"));
        assert!(!text.contains("private-invalid-utf8"));
    }
    let conflict = json_error(gaia(dir.path()).args([
        "client",
        "mcp-config",
        "bot",
        "--transport",
        "http",
        "--key",
        &key,
        "--key-stdin",
        "--port",
        "4123",
    ]));
    assert_eq!(conflict["code"], "invalid_params");
    assert!(!conflict.to_string().contains(&key));
}

#[test]
fn missing_or_other_client_keys_cannot_be_used_in_a_snippet() {
    let dir = tempfile::tempdir().unwrap();
    let key = setup(dir.path());
    for name in ["tester", "missing"] {
        let error = json_error(gaia(dir.path()).args([
            "client",
            "mcp-config",
            name,
            "--transport",
            "http",
            "--key",
            &key,
            "--port",
            "4123",
        ]));
        assert!(!error.to_string().contains(&key));
    }
    json_ok(gaia(dir.path()).args(["client", "keygen", "tester"]));
    let wrong_client = json_error(gaia(dir.path()).args([
        "client",
        "mcp-config",
        "tester",
        "--transport",
        "http",
        "--key",
        &key,
        "--port",
        "4123",
    ]));
    assert_eq!(wrong_client["code"], "invalid_params");
    assert!(!wrong_client.to_string().contains(&key));
}

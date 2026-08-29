//! キー発行・ローテーションは設定の lock 内で更新し、保存後だけ平文を返す。
use std::{
    path::Path,
    process::{Command, Output},
    sync::{Arc, Barrier},
};

use gaia_core::{
    auth::{AuthTable, hash_key},
    config::Config,
};
use serde_json::Value;

fn gaia(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gaia"));
    command
        .env("GAIA_CONFIG", dir.join("config.toml"))
        .env("GAIA_DB", dir.join("gaia.db"));
    command
}

fn text_ok(command: &mut Command) -> String {
    let output = command.output().expect("spawn gaia");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

fn json_output(output: Output, success: bool) -> Value {
    assert_eq!(
        output.status.success(),
        success,
        "unexpected command status"
    );
    let text = if success {
        assert!(output.stderr.is_empty(), "JSON success must not use stderr");
        String::from_utf8(output.stdout).unwrap()
    } else {
        assert!(
            output.stdout.is_empty(),
            "failed issuance must not emit a key"
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

fn init(dir: &Path) {
    text_ok(gaia(dir).args(["init", "--affiliation", "primary", "--client", "tester"]));
}

#[test]
fn issuance_and_rotation_only_publish_plaintext_in_the_requested_output() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    let config_path = dir.path().join("config.toml");
    let first_output = gaia(dir.path())
        .args(["client", "add", "bot", "--role", "agent", "--generate-key"])
        .output()
        .unwrap();
    assert!(first_output.status.success());
    let first = String::from_utf8(first_output.stdout).unwrap();
    assert_eq!(first.lines().count(), 1);
    let first = first.trim_end();
    assert!(first.starts_with("gaia_bot_"));
    assert!(!String::from_utf8_lossy(&first_output.stderr).contains(first));

    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.keys["bot"], hash_key(first));
    assert!(
        !std::fs::read_to_string(&config_path)
            .unwrap()
            .contains(first)
    );

    let issued = json_ok(gaia(dir.path()).args(["client", "keygen", "bot"]));
    assert_eq!(issued["client"], "bot");
    let second = issued["key"].as_str().unwrap();
    assert_ne!(first, second);
    let config = Config::load(&config_path).unwrap();
    let auth = AuthTable::from_config(&config);
    assert!(auth.verify(first).is_none());
    assert_eq!(auth.verify(second).unwrap().name, "bot");
    assert_eq!(config.keys["bot"], hash_key(second));
    assert!(
        !std::fs::read_to_string(&config_path)
            .unwrap()
            .contains(second)
    );

    let listed = json_ok(gaia(dir.path()).args(["client", "list"])).to_string();
    assert!(!listed.contains(first));
    assert!(!listed.contains(second));
    assert!(!listed.contains(&hash_key(second)));

    let unkeyed = json_ok(gaia(dir.path()).args(["client", "add", "unkeyed", "--role", "agent"]));
    assert_eq!(unkeyed["client"], "unkeyed");
    assert!(unkeyed.get("key").is_none());
}

#[test]
fn concurrent_additions_and_rotations_preserve_clients_keys_and_server_config() {
    const PAIRS: usize = 6;
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    let tester = text_ok(gaia(dir.path()).args(["client", "keygen", "tester"]));
    for index in 0..PAIRS {
        text_ok(gaia(dir.path()).args([
            "client",
            "add",
            &format!("rotate-{index}"),
            "--role",
            "agent",
            "--generate-key",
        ]));
    }
    let config_path = dir.path().join("config.toml");
    Config::update(&config_path, |config| {
        config.server.port = Some(4123);
        Ok(())
    })
    .unwrap();

    let barrier = Arc::new(Barrier::new(PAIRS * 2));
    let workers: Vec<_> = (0..PAIRS * 2)
        .map(|index| {
            let directory = dir.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let name = if index < PAIRS {
                    format!("rotate-{index}")
                } else {
                    format!("new-{index}")
                };
                barrier.wait();
                let output = if index < PAIRS {
                    json_ok(gaia(&directory).args(["client", "keygen", &name]))
                } else {
                    json_ok(gaia(&directory).args([
                        "client",
                        "add",
                        &name,
                        "--role",
                        "agent",
                        "--generate-key",
                    ]))
                };
                (name, output["key"].as_str().unwrap().to_string())
            })
        })
        .collect();
    let issued: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.clients.len(), PAIRS * 2 + 1);
    assert_eq!(config.keys.len(), PAIRS * 2 + 1);
    assert_eq!(config.server.port, Some(4123));
    assert_eq!(config.cli.default_client.as_deref(), Some("tester"));
    assert_eq!(config.keys["tester"], hash_key(&tester));
    let auth = AuthTable::from_config(&config);
    for (name, plaintext) in issued {
        assert_eq!(config.keys[&name], hash_key(&plaintext));
        assert_eq!(auth.verify(&plaintext).unwrap().name, name);
    }
}

#[test]
fn duplicate_or_missing_clients_fail_without_a_key_or_config_change() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    let config_path = dir.path().join("config.toml");
    let before = std::fs::read(&config_path).unwrap();
    let duplicate = json_error(gaia(dir.path()).args([
        "client",
        "add",
        "tester",
        "--role",
        "agent",
        "--generate-key",
    ]));
    assert_eq!(duplicate["code"], "conflict");
    let missing = json_error(gaia(dir.path()).args(["client", "keygen", "missing"]));
    assert_eq!(missing["code"], "not_found");
    assert_eq!(std::fs::read(&config_path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn failed_persistence_never_publishes_generated_keys() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    text_ok(gaia(dir.path()).args(["client", "keygen", "tester"]));
    let config_path = dir.path().join("config.toml");
    // lock は NAME_MAX 内、保存用の一時ファイル名だけが NAME_MAX を超える。
    let invalid_config = dir.path().join(format!("{}.toml", "c".repeat(245)));
    std::fs::copy(&config_path, &invalid_config).unwrap();
    let before = std::fs::read(&invalid_config).unwrap();
    for args in [
        vec![
            "client",
            "add",
            "unpublished",
            "--role",
            "agent",
            "--generate-key",
        ],
        vec!["client", "keygen", "tester"],
    ] {
        let error = json_error(
            gaia(dir.path())
                .env("GAIA_CONFIG", &invalid_config)
                .args(args),
        );
        assert_eq!(error["code"], "internal");
        let error = error.to_string();
        assert!(!error.contains("gaia_unpublished_"));
        assert!(!error.contains("gaia_tester_"));
        assert_eq!(std::fs::read(&invalid_config).unwrap(), before);
    }
}

#[test]
fn rename_moves_config_references_and_keeps_keys_and_history() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    let config_path = dir.path().join("config.toml");
    let tester_key = text_ok(gaia(dir.path()).args(["client", "keygen", "tester"]));
    let bot_key = text_ok(gaia(dir.path()).args([
        "client",
        "add",
        "bot",
        "--role",
        "agent",
        "--default-scope",
        "primary",
        "--generate-key",
    ]));
    // 改名前に agent が提案しておく（履歴の proposed_by は旧名のまま残る）
    let proposed = json_ok(gaia(dir.path()).args([
        "--client",
        "bot",
        "propose",
        "person",
        "insert",
        "--patch",
        r#"{"name": "田中 太郎"}"#,
    ]));
    assert_eq!(proposed["status"], "pending");
    let proposal_id = proposed["proposal_id"].as_i64().unwrap();

    // 既定 client（human）と agent を改名する
    let renamed = json_ok(gaia(dir.path()).args(["client", "rename", "tester", " me "]));
    assert_eq!(renamed["client"], "me");
    assert_eq!(renamed["previous"], "tester");
    assert!(renamed["notice"].as_str().unwrap().contains("--client me"));
    let output = gaia(dir.path())
        .args(["client", "rename", "bot", "robot"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "rename prints nothing to stdout");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--client robot"));
    assert!(stderr.contains("HTTP のキーは有効なまま"));
    // デスクトップ保管キーは CLI の rename では移らないため、案内を含める
    assert!(stderr.contains("デスクトップ"));

    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.cli.default_client.as_deref(), Some("me"));
    assert!(config.client("tester").is_none());
    assert!(config.client("bot").is_none());
    assert_eq!(config.client("me").unwrap().role.to_string(), "human");
    assert_eq!(
        config.client("robot").unwrap().default_scope.as_deref(),
        Some("primary")
    );
    assert_eq!(config.keys.len(), 2);
    assert_eq!(config.keys["me"], hash_key(&tester_key));
    assert_eq!(config.keys["robot"], hash_key(&bot_key));
    let auth = AuthTable::from_config(&config);
    assert_eq!(auth.verify(&bot_key).unwrap().name, "robot");
    assert_eq!(auth.verify(&tester_key).unwrap().name, "me");

    // 旧名は使えず、新名で再発行できる
    let missing = json_error(gaia(dir.path()).args(["client", "keygen", "bot"]));
    assert_eq!(missing["code"], "not_found");
    let reissued = json_ok(gaia(dir.path()).args(["client", "keygen", "robot"]));
    assert_eq!(reissued["client"], "robot");
    // 旧名では識別できない（--client の解決は設定の UnknownClient をそのまま返す既存挙動）
    let unknown = json_error(gaia(dir.path()).args(["--client", "bot", "proposals"]));
    assert!(unknown["message"].as_str().unwrap().contains("bot"));

    // 既定 client が追従し、履歴の proposed_by は旧名のまま
    let listed = json_ok(gaia(dir.path()).args(["proposals"]));
    let proposal = listed["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == proposal_id)
        .expect("proposal listed under the renamed default client");
    assert_eq!(proposal["proposed_by"], "bot");
    let approved = json_ok(gaia(dir.path()).args(["approve", &proposal_id.to_string()]));
    assert_eq!(approved["status"], "approved");
    let decided = json_ok(gaia(dir.path()).args(["proposals", "--status", "approved"]));
    let decided = decided["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == proposal_id)
        .unwrap();
    assert_eq!(decided["proposed_by"], "bot");
    assert_eq!(decided["decided_by"], "me");

    // 不在・重複・空名は拒否し、設定は変わらない
    let before = std::fs::read(&config_path).unwrap();
    for (args, code) in [
        (["client", "rename", "bot", "other"], "not_found"),
        (["client", "rename", "robot", "me"], "conflict"),
        (["client", "rename", "robot", "  "], "invalid_params"),
        (["client", "rename", "robot", "bad\nname"], "invalid_params"),
    ] {
        let error = json_error(gaia(dir.path()).args(args));
        assert_eq!(error["code"], code, "{args:?}");
    }
    assert_eq!(std::fs::read(&config_path).unwrap(), before);
}

#[test]
fn invalid_serve_flags_and_implicit_stdio_are_structured_errors() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());
    for (args, code) in [
        (vec!["serve", "--stdio"], "unauthorized"),
        (vec!["serve", "--stdio", "--http"], "invalid_params"),
        (vec!["serve", "--port", "0"], "invalid_params"),
        (vec!["serve"], "invalid_params"),
    ] {
        let error = json_error(gaia(dir.path()).args(args));
        assert_eq!(error["code"], code);
    }
}

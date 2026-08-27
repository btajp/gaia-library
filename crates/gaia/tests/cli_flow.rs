//! CLI の一気通貫: init → client add → add → search → speakers → 認可。
use std::{
    collections::HashSet,
    process::{Command, Stdio},
};

use gaia_core::{admin, config::Config, storage::Db};

fn gaia(dir: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gaia"));
    c.env("GAIA_CONFIG", dir.join("config.toml"));
    c.env("GAIA_DB", dir.join("gaia.db"));
    c
}

fn run_ok(c: &mut Command) -> serde_json::Value {
    let out = c.output().expect("spawn gaia");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(trimmed).expect("json stdout")
    }
}

fn run_json_error(c: &mut Command) -> serde_json::Value {
    let out = c.output().expect("spawn gaia");
    assert!(!out.status.success(), "command unexpectedly succeeded");
    assert!(
        out.stdout.is_empty(),
        "error must not use stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<_> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "JSON error must be one line: {stderr}");
    serde_json::from_str(lines[0]).expect("JSON error on stderr")
}

fn init(dir: &std::path::Path, client: &str, affiliation: &str) {
    run_ok(gaia(dir).args(["init", "--affiliation", affiliation, "--client", client]));
}

fn add_agent(dir: &std::path::Path) {
    run_ok(gaia(dir).args([
        "client",
        "add",
        "bot",
        "--role",
        "agent",
        "--default-scope",
        "cloudnative",
    ]));
}

#[test]
fn init_add_search_speakers_and_authorization() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path(), "tester", "cloudnative");
    add_agent(dir.path());
    let added = run_ok(gaia(dir.path()).args([
        "--json",
        "add",
        "person",
        "--name",
        "岡村 慎太郎",
        "--alias",
        "okash1n",
    ]));
    assert_eq!(added["status"], "approved");
    let person_id = added["result"]["id"].as_i64().unwrap();

    let found = run_ok(gaia(dir.path()).args(["--json", "search", "okash1n"]));
    assert_eq!(found["entities"][0]["type"], "person");
    assert_eq!(found["entities"][0]["id"].as_i64().unwrap(), person_id);

    let speakers =
        run_ok(gaia(dir.path()).args(["--json", "speakers", "岡村 慎太郎 (CloudNative)"]));
    assert_eq!(speakers["results"][0]["status"], "matched");

    // agent は add（承認込み）を実行できない
    let denied = gaia(dir.path())
        .args(["--client", "bot", "add", "person", "--name", "x"])
        .output()
        .unwrap();
    assert!(!denied.status.success());

    // agent の propose は通り、pending に載る
    let proposed = run_ok(gaia(dir.path()).args([
        "--client",
        "bot",
        "--json",
        "propose",
        "person",
        "insert",
        "--patch",
        r#"{"name": "田中 太郎"}"#,
    ]));
    assert_eq!(proposed["status"], "pending");
    let pending = run_ok(gaia(dir.path()).args(["--json", "proposals"]));
    assert!(!pending["proposals"].as_array().unwrap().is_empty());
}

#[test]
fn init_uses_global_client_and_normalizes_affiliation() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path(), "alice", "  cloudnative  ");

    let info = run_ok(gaia(dir.path()).args(["--json", "info"]));
    assert_eq!(info["client"]["name"], "alice");
    assert_eq!(info["client"]["default_scope"], "cloudnative");

    let affiliations = run_ok(gaia(dir.path()).args(["--json", "affiliation", "list"]));
    assert_eq!(affiliations.as_array().unwrap().len(), 1);
    assert_eq!(affiliations[0]["name"], "cloudnative");

    let invalid = tempfile::tempdir().unwrap();
    let error = run_json_error(gaia(invalid.path()).args([
        "--json",
        "init",
        "--affiliation",
        "   ",
        "--client",
        "alice",
    ]));
    assert_eq!(error["code"], "invalid_params");
    assert!(!invalid.path().join("config.toml").exists());
    assert!(!invalid.path().join("gaia.db").exists());

    let invalid_db = tempfile::tempdir().unwrap();
    let parent_file = invalid_db.path().join("not-a-directory");
    std::fs::write(&parent_file, "block database creation").unwrap();
    let db_path = parent_file.join("gaia.db");
    let mut command = gaia(invalid_db.path());
    command.env_remove("GAIA_DB");
    let error = run_json_error(
        command
            .args([
                "--json",
                "init",
                "--affiliation",
                "cloudnative",
                "--client",
                "alice",
                "--db",
            ])
            .arg(&db_path),
    );
    assert_eq!(error["code"], "internal");
    assert!(!invalid_db.path().join("config.toml").exists());
}

#[cfg(unix)]
#[test]
fn init_retries_after_config_save_failure_without_changing_affiliation() {
    let dir = tempfile::tempdir().unwrap();
    // lock は NAME_MAX=255 に収まるが、保存用一時ファイルの名前だけが超過する。
    let invalid_config = dir.path().join(format!("{}.toml", "c".repeat(245)));
    let arguments = [
        "--json",
        "init",
        "--affiliation",
        "cloudnative",
        "--client",
        "alice",
        "--identity",
        "member",
    ];
    let error = run_json_error(
        gaia(dir.path())
            .env("GAIA_CONFIG", &invalid_config)
            .args(arguments),
    );
    assert_eq!(error["code"], "internal");
    assert!(!invalid_config.exists());
    assert!(!dir.path().join("config.toml").exists());
    let db = Db::open(&dir.path().join("gaia.db")).unwrap();
    let original = admin::list_affiliations(&db).unwrap();
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].identity.as_deref(), Some("member"));

    // 設定先だけを修正し、同じ DB・所属・identity で通常の init を再実行する。
    run_ok(gaia(dir.path()).args(arguments));
    assert_eq!(admin::list_affiliations(&db).unwrap(), original);
    let info = run_ok(gaia(dir.path()).args(["--json", "info"]));
    assert_eq!(info["client"]["name"], "alice");
    assert_eq!(info["client"]["default_scope"], "cloudnative");
}

#[test]
fn init_database_write_failure_leaves_no_config_and_rolls_back_affiliation() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("gaia.db")).unwrap();
    db.with_conn::<_, anyhow::Error>(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER fail_init_audit BEFORE INSERT ON audit_log
             BEGIN SELECT RAISE(ABORT, 'injected audit failure'); END;",
        )?;
        Ok(())
    })
    .unwrap();

    let error = run_json_error(gaia(dir.path()).args([
        "--json",
        "init",
        "--affiliation",
        "cloudnative",
        "--client",
        "alice",
    ]));
    assert_eq!(error["code"], "internal");
    assert!(!dir.path().join("config.toml").exists());
    assert!(admin::list_affiliations(&db).unwrap().is_empty());

    db.with_conn::<_, anyhow::Error>(|conn| {
        conn.execute_batch("DROP TRIGGER fail_init_audit")?;
        Ok(())
    })
    .unwrap();
    init(dir.path(), "alice", "cloudnative");
    assert_eq!(admin::list_affiliations(&db).unwrap().len(), 1);
}

#[test]
fn concurrent_initialization_never_overwrites_config_or_adds_losing_affiliations() {
    const INIT_COUNT: usize = 8;

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.toml");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.path().join("config.toml.lock"))
        .unwrap();
    lock.lock().unwrap();
    let children: Vec<_> = (0..INIT_COUNT)
        .map(|index| {
            let child = gaia(dir.path())
                .args([
                    "init",
                    "--affiliation",
                    &format!("scope-{index}"),
                    "--client",
                    &format!("human-{index}"),
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            (index, child)
        })
        .collect();
    assert!(!dir.path().join("gaia.db").exists());
    drop(lock);

    let mut winners = Vec::new();
    for (index, child) in children {
        let output = child.wait_with_output().unwrap();
        if output.status.success() {
            winners.push(index);
        } else {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("設定が既にあります"),
                "unexpected failure: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    assert_eq!(winners.len(), 1);
    let winner = winners[0];
    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.clients.len(), 1);
    assert_eq!(config.clients[0].name, format!("human-{winner}"));
    let affiliation = format!("scope-{winner}");
    assert_eq!(
        config.clients[0].default_scope.as_deref(),
        Some(affiliation.as_str())
    );
    let db = Db::open(&dir.path().join("gaia.db")).unwrap();
    let affiliations = admin::list_affiliations(&db).unwrap();
    assert_eq!(affiliations.len(), 1);
    assert_eq!(affiliations[0].name, affiliation);
}

#[test]
fn concurrent_client_additions_are_not_lost() {
    const CLIENT_COUNT: usize = 16;

    let dir = tempfile::tempdir().unwrap();
    init(dir.path(), "tester", "cloudnative");

    let children: Vec<_> = (0..CLIENT_COUNT)
        .map(|index| {
            gaia(dir.path())
                .args([
                    "client",
                    "add",
                    &format!("agent-{index}"),
                    "--role",
                    "agent",
                    "--default-scope",
                    "cloudnative",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let clients = run_ok(gaia(dir.path()).args(["--json", "client", "list"]));
    let names: HashSet<_> = clients
        .as_array()
        .unwrap()
        .iter()
        .map(|client| client["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), CLIENT_COUNT + 1);
    assert!(names.contains("tester"));
    for index in 0..CLIENT_COUNT {
        assert!(names.contains(format!("agent-{index}").as_str()));
    }
}

#[test]
fn affiliation_management_is_human_only() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path(), "tester", "cloudnative");
    add_agent(dir.path());

    let added = run_ok(gaia(dir.path()).args([
        "--json",
        "affiliation",
        "add",
        "partner",
        "--identity",
        "member",
    ]));
    assert_eq!(added["name"], "partner");

    for args in [
        vec!["--client", "bot", "--json", "affiliation", "list"],
        vec![
            "--client",
            "bot",
            "--json",
            "affiliation",
            "add",
            "secret",
            "--identity",
            "hidden",
        ],
    ] {
        let error = run_json_error(gaia(dir.path()).args(args));
        assert_eq!(error["code"], "unauthorized");
    }

    let visible = run_ok(gaia(dir.path()).args(["--json", "affiliation", "list"]));
    assert!(
        visible
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "partner")
    );
    assert!(
        visible
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["name"] != "secret")
    );
}

#[test]
fn json_errors_keep_tool_details_and_failed_add_proposal_id() {
    let dir = tempfile::tempdir().unwrap();

    let parse_error =
        run_json_error(gaia(dir.path()).args(["--json", "person", "get", "--id", "nope"]));
    assert_eq!(parse_error["code"], "invalid_params");
    assert!(parse_error["details"]["kind"].is_string());

    init(dir.path(), "tester", "cloudnative");

    let args_error =
        run_json_error(gaia(dir.path()).args(["--json", "call", "get_server_info", "--args", "{"]));
    assert_eq!(args_error["code"], "invalid_params");

    let not_found =
        run_json_error(gaia(dir.path()).args(["--json", "person", "get", "--id", "9999"]));
    assert_eq!(not_found["code"], "not_found");
    assert!(not_found["message"].as_str().unwrap().contains("9999"));

    let failed = run_json_error(gaia(dir.path()).args([
        "--json",
        "add",
        "fact",
        "--entity-type",
        "person",
        "--entity-id",
        "9999",
        "--statement",
        "存在しない人物へのファクト",
    ]));
    let proposal_id = failed["details"]["proposal_id"]
        .as_i64()
        .expect("failed add must expose proposal_id");
    assert_eq!(failed["details"]["phase"], "approve");
    assert_eq!(failed["details"]["proposal_created"], true);

    let pending = run_ok(gaia(dir.path()).args(["--json", "proposals"]));
    assert!(
        pending["proposals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|proposal| {
                proposal["id"].as_i64() == Some(proposal_id) && proposal["status"] == "pending"
            })
    );
}

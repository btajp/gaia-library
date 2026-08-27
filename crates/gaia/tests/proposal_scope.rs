//! CLI の承認・却下・即時追加で scope と JSON 応答を維持する。
use std::{path::Path, process::Command};

use serde_json::Value;

fn gaia(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gaia"));
    command
        .env("GAIA_CONFIG", dir.join("config.toml"))
        .env("GAIA_DB", dir.join("gaia.db"));
    command
}

fn run_ok(command: &mut Command) -> Value {
    let output = command.output().expect("spawn gaia");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(text.trim()).expect("JSON stdout")
    }
}

fn run_json(command: &mut Command) -> Result<Value, Value> {
    let output = command.arg("--json").output().expect("spawn gaia");
    let success = output.status.success();
    let text = if success {
        assert!(output.stderr.is_empty(), "success must not use stderr");
        String::from_utf8(output.stdout).unwrap()
    } else {
        assert!(output.stdout.is_empty(), "error must not use stdout");
        String::from_utf8(output.stderr).unwrap()
    };
    assert_eq!(text.lines().count(), 1, "JSON must be one line: {text}");
    let value = serde_json::from_str(&text).expect("JSON response");
    if success { Ok(value) } else { Err(value) }
}

fn init(dir: &Path) {
    run_ok(gaia(dir).args(["init", "--affiliation", "primary", "--client", "tester"]));
    run_ok(gaia(dir).args(["affiliation", "add", "secondary"]));
}

fn propose_secondary(dir: &Path) -> String {
    let proposed = run_json(gaia(dir).args([
        "propose",
        "person",
        "insert",
        "--patch",
        r#"{"name":"scope test"}"#,
        "--scope",
        "secondary",
    ]))
    .unwrap();
    assert_eq!(proposed["status"], "pending");
    proposed["proposal_id"].as_i64().unwrap().to_string()
}

#[test]
fn approvals_and_rejections_require_the_selected_scope() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());

    for (action, status) in [("approve", "approved"), ("reject", "rejected")] {
        let id = propose_secondary(dir.path());
        let implicit = gaia(dir.path()).args([action, &id]).output().unwrap();
        assert!(!implicit.status.success());
        assert!(implicit.stdout.is_empty());
        assert!(String::from_utf8_lossy(&implicit.stderr).contains("not_found"));

        let implicit_json = run_json(gaia(dir.path()).args([action, &id])).unwrap_err();
        assert_eq!(implicit_json["code"], "not_found");
        let wrong_scope =
            run_json(gaia(dir.path()).args([action, &id, "--scope", "primary"])).unwrap_err();
        assert_eq!(wrong_scope["code"], "not_found");

        let pending =
            run_json(gaia(dir.path()).args(["proposals", "--scope", "secondary"])).unwrap();
        assert_eq!(
            pending["proposals"][0]["id"].as_i64().unwrap().to_string(),
            id
        );
        assert_eq!(pending["proposals"][0]["status"], "pending");

        let mut command = gaia(dir.path());
        command.args([action, &id, "--scope", "secondary"]);
        if action == "reject" {
            command.args(["--reason", "scope regression"]);
        }
        let explicit = run_json(&mut command).unwrap();
        assert_eq!(explicit["status"], status);
        assert_eq!(explicit["proposal_id"].as_i64().unwrap().to_string(), id);

        let listed = run_json(gaia(dir.path()).args([
            "proposals",
            "--status",
            status,
            "--scope",
            "secondary",
        ]))
        .unwrap();
        assert_eq!(listed["proposals"][0]["scope"], "secondary");
        if action == "reject" {
            assert_eq!(listed["proposals"][0]["decision_note"], "scope regression");
        }
    }
}

#[test]
fn approvals_and_rejections_accept_repeated_scope_flags() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());

    for (action, status) in [("approve", "approved"), ("reject", "rejected")] {
        let id = propose_secondary(dir.path());
        let decided = run_ok(gaia(dir.path()).args([
            action,
            &id,
            "--scope",
            "primary",
            "--scope",
            "secondary",
        ]));
        assert_eq!(decided["status"], status);
        assert_eq!(decided["proposal_id"].as_i64().unwrap().to_string(), id);
    }
}

#[test]
fn immediate_add_preserves_explicit_and_default_scope() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());

    for scope in ["secondary", "primary"] {
        let mut command = gaia(dir.path());
        command.args(["add", "glossary", "--term", scope]);
        if scope == "secondary" {
            command.args(["--scope", scope]);
        }
        let added = run_json(&mut command).unwrap();
        assert_eq!(added["status"], "approved");
        let id = added["result"]["id"].as_i64().unwrap();
        for selected in ["secondary", "primary"] {
            let glossary =
                run_json(gaia(dir.path()).args(["glossary", "--scope", selected])).unwrap();
            let term = glossary["terms"]
                .as_array()
                .unwrap()
                .iter()
                .find(|term| term["id"].as_i64() == Some(id));
            assert_eq!(term.is_some(), selected == scope);
            if let Some(term) = term {
                assert_eq!(term["scope"], scope);
                assert_eq!(term["term"], scope);
            }

            let listed = run_json(gaia(dir.path()).args([
                "proposals",
                "--status",
                "approved",
                "--scope",
                selected,
            ]))
            .unwrap();
            let contains_added = listed["proposals"]
                .as_array()
                .unwrap()
                .iter()
                .any(|proposal| proposal["id"] == added["proposal_id"]);
            assert_eq!(contains_added, selected == scope);
        }
    }
}

#[test]
fn failed_scoped_add_keeps_proposal_id_and_json_error_details() {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path());

    let error = run_json(gaia(dir.path()).args([
        "add",
        "fact",
        "--entity-type",
        "person",
        "--entity-id",
        "9999",
        "--statement",
        "missing person",
        "--scope",
        "secondary",
    ]))
    .unwrap_err();
    assert_eq!(error["code"], "not_found");
    let proposal_id = error["details"]["proposal_id"].as_i64().unwrap();
    assert_eq!(error["details"]["phase"], "approve");
    assert_eq!(error["details"]["proposal_created"], true);

    let pending = run_json(gaia(dir.path()).args(["proposals", "--scope", "secondary"])).unwrap();
    assert_eq!(pending["proposals"][0]["id"], proposal_id);
    assert_eq!(pending["proposals"][0]["scope"], "secondary");
    assert_eq!(pending["proposals"][0]["status"], "pending");
    let default_pending = run_json(gaia(dir.path()).arg("proposals")).unwrap();
    assert!(default_pending["proposals"].as_array().unwrap().is_empty());
}

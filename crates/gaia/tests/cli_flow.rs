//! CLI の一気通貫: init → client add → add → search → speakers → 認可。
use std::process::Command;

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

#[test]
fn init_add_search_speakers_and_authorization() {
    let dir = tempfile::tempdir().unwrap();
    run_ok(gaia(dir.path()).args([
        "init",
        "--affiliation",
        "cloudnative",
        "--client-name",
        "tester",
    ]));
    run_ok(gaia(dir.path()).args([
        "client",
        "add",
        "bot",
        "--role",
        "agent",
        "--default-scope",
        "cloudnative",
    ]));
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

//! `gaia resolve` の一気通貫: 参照登録 → 解決器なし → [sources.file] を設定して本文取得 → 設定を消すと再起動なしで無効。
use std::{fs, path::Path, process::Command};

fn gaia(dir: &Path) -> Command {
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

fn add_ref(dir: &Path, person: i64, system: &str, uri: &str) -> i64 {
    let added = run_ok(gaia(dir).args([
        "--json",
        "add",
        "ref",
        "--target-type",
        "person",
        "--target-id",
        &person.to_string(),
        "--system",
        system,
        "--uri",
        uri,
        "--note",
        "参照の説明",
        "--snapshot",
        "要点スナップショット",
    ]));
    added["result"]["id"].as_i64().unwrap()
}

fn set_file_roots(dir: &Path, roots: Option<&Path>) {
    let path = dir.join("config.toml");
    let text = fs::read_to_string(&path).unwrap();
    let base = match text.find("\n[sources") {
        Some(index) => text[..index].to_string(),
        None => text,
    };
    let text = match roots {
        Some(root) => format!(
            "{}\n[sources.file]\nroots = [{}]\n",
            base.trim_end(),
            serde_json::to_string(&root.to_string_lossy()).unwrap()
        ),
        None => format!("{}\n", base.trim_end()),
    };
    fs::write(&path, text).unwrap();
}

#[test]
fn resolve_reads_files_under_configured_roots_and_reloads_settings_per_call() {
    let dir = tempfile::tempdir().unwrap();
    run_ok(gaia(dir.path()).args(["init", "--affiliation", "cloudnative", "--client", "tester"]));
    let person = run_ok(gaia(dir.path()).args([
        "--json",
        "add",
        "person",
        "--name",
        "岡村 慎太郎",
    ]))["result"]["id"]
        .as_i64()
        .unwrap();

    // 解決器のない system は resolved=false と snapshot
    let minutes = add_ref(dir.path(), person, "minutes", "minutes://meeting/42#t=1200");
    let out =
        run_ok(gaia(dir.path()).args(["--json", "resolve", "--ref-id", &minutes.to_string()]));
    assert_eq!(out["resolved"], false);
    assert!(out.get("content").is_none());
    assert_eq!(out["reference"]["snapshot"], "要点スナップショット");
    let reason = out["reason"].as_str().unwrap();
    assert!(
        reason.starts_with("no resolver for system `minutes`"),
        "{reason}"
    );
    assert!(
        reason.ends_with("fallback: see reference.snapshot"),
        "{reason}"
    );
    // --content は終了コード 2、stdout は空、reason は stderr
    let failed = gaia(dir.path())
        .args(["resolve", "--ref-id", &minutes.to_string(), "--content"])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2));
    assert!(failed.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(stderr.contains("no resolver for system"), "{stderr}");
    assert!(stderr.contains("要点スナップショット"), "{stderr}");
    // 未設定なので resolvers は空
    let info = run_ok(gaia(dir.path()).args(["--json", "info"]));
    assert_eq!(info["capabilities"]["resolvers"], serde_json::json!([]));

    // [sources.file].roots を設定すると、再起動なしで file:// の参照が読める
    // 設定・DB のディレクトリ配下は file 解決器が常時拒否するので、本文は別ディレクトリに置く
    let docs_dir = tempfile::tempdir().unwrap();
    let docs = docs_dir.path().join("docs");
    fs::create_dir_all(&docs).unwrap();
    let file = docs.join("議事 録.md");
    fs::write(&file, "# 議事録\n- 決定: SCIM は Phase 2\n").unwrap();
    let uri = url_for(&file);
    let file_ref = add_ref(dir.path(), person, "file", &uri);
    let not_configured =
        run_ok(gaia(dir.path()).args(["--json", "resolve", "--ref-id", &file_ref.to_string()]));
    assert_eq!(not_configured["resolved"], false);
    assert!(
        not_configured["reason"]
            .as_str()
            .unwrap()
            .starts_with("resolver `file` is not configured"),
        "{not_configured}"
    );
    set_file_roots(dir.path(), Some(&docs));
    let info = run_ok(gaia(dir.path()).args(["--json", "info"]));
    assert_eq!(
        info["capabilities"]["resolvers"],
        serde_json::json!(["file"])
    );
    let resolved = gaia(dir.path())
        .args(["resolve", "--ref-id", &file_ref.to_string(), "--content"])
        .output()
        .unwrap();
    assert_eq!(
        resolved.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&resolved.stdout),
        "# 議事録\n- 決定: SCIM は Phase 2\n"
    );
    // uri でも同じ参照が引ける（最新 1 件）
    let by_uri = run_ok(gaia(dir.path()).args(["--json", "resolve", "--uri", &uri]));
    assert_eq!(by_uri["reference"]["id"].as_i64().unwrap(), file_ref);
    assert_eq!(by_uri["resolved"], true);
    assert_eq!(by_uri["content"], "# 議事録\n- 決定: SCIM は Phase 2\n");
    // 設定ディレクトリ配下（config.toml）は roots に入れても読めない
    let config_ref = add_ref(
        dir.path(),
        person,
        "file",
        &url_for(&dir.path().join("config.toml")),
    );
    set_file_roots(dir.path(), Some(dir.path()));
    let denied =
        run_ok(gaia(dir.path()).args(["--json", "resolve", "--ref-id", &config_ref.to_string()]));
    assert_eq!(denied["resolved"], false);
    // roots を消すと次の呼び出しから無効（呼び出しごとの再読込）
    set_file_roots(dir.path(), None);
    let disabled =
        run_ok(gaia(dir.path()).args(["--json", "resolve", "--ref-id", &file_ref.to_string()]));
    assert_eq!(disabled["resolved"], false);
    assert!(
        disabled["reason"]
            .as_str()
            .unwrap()
            .starts_with("resolver `file` is not configured"),
        "{disabled}"
    );
    // 引数なしは使い方エラー（clap）
    let usage = gaia(dir.path()).args(["resolve"]).output().unwrap();
    assert!(!usage.status.success());
    assert!(usage.stdout.is_empty());
}

fn url_for(path: &Path) -> String {
    let mut encoded = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'/' | b'.' | b'-' | b'_' | b'~' => encoded.push(byte as char),
            b if b.is_ascii_alphanumeric() => encoded.push(b as char),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

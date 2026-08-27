//! 任意依存の未設定と実行不可を区別する開発起動スクリプトの回帰検証。
#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

fn run_launcher(bin: Option<&Path>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let cargo = dir.path().join("cargo");
    std::fs::write(&cargo, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o700)).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/dev.sh");
    let mut command = Command::new("/bin/bash");
    command.arg(script).arg("--help").env("PATH", dir.path());
    match bin {
        Some(path) => {
            command.env("NARUMI_BIN", path);
        }
        None => {
            command.env_remove("NARUMI_BIN");
        }
    }
    let output = command.output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn unset_optional_server_is_reported_as_unset() {
    let stderr = run_launcher(None);
    assert!(stderr.contains("NARUMI_BIN unset"));
    assert!(!stderr.contains("not executable"));
}

#[test]
fn configured_non_executable_server_is_not_reported_as_unset() {
    let dir = tempfile::tempdir().unwrap();
    let non_executable = dir.path().join("narumi-server");
    std::fs::write(&non_executable, "not executable\n").unwrap();
    std::fs::set_permissions(&non_executable, std::fs::Permissions::from_mode(0o600)).unwrap();
    for path in [non_executable, dir.path().join("missing-server")] {
        let stderr = run_launcher(Some(&path));
        assert!(stderr.contains("NARUMI_BIN is set"));
        assert!(stderr.contains("not executable"));
        assert!(!stderr.contains("unset"));
    }
}

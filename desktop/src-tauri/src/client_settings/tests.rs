use std::{
    cell::Cell,
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

use gaia_core::auth::hash_key;
use serde_json::Value;

use super::*;

fn config_at(path: &Path) -> Config {
    let mut config = Config::default();
    config
        .add_client(ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: Some("scope".into()),
        })
        .unwrap();
    config.save(path).unwrap();
    config
}

#[test]
fn add_validates_and_preserves_other_clients_and_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut original = config_at(&path);
    original.server.port = Some(4123);
    original.save(&path).unwrap();
    let untouched = std::fs::read(&path).unwrap();
    for name in [" ", "bad\nname", "bot"] {
        assert!(
            add_with(&path, name, Role::Agent, None, true, |_, _| {
                panic!("must validate before storing a key")
            })
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), untouched);
    }
    assert!(
        add_with(
            &path,
            " new ",
            Role::Human,
            Some(" scope "),
            false,
            |_, _| { panic!("no key was requested") }
        )
        .unwrap()
        .is_none()
    );
    let saved = Config::load(&path).unwrap();
    assert_eq!(saved.clients.len(), 2);
    assert_eq!(saved.server.port, Some(4123));
    assert_eq!(
        saved.client("new").unwrap().default_scope.as_deref(),
        Some("scope")
    );
    assert!(saved.keys.is_empty());
}

#[test]
fn key_storage_failure_returns_the_valid_key_with_a_redacted_warning() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    config_at(&path);
    let issued = keygen_with(&path, "bot", |_, key| Err(format!("unsafe error {key}"))).unwrap();
    let config = Config::load(&path).unwrap();
    assert_eq!(config.keys["bot"], hash_key(&issued.key));
    assert!(issued.storage.location.is_none());
    let warning = issued.storage.error.unwrap();
    assert!(!warning.contains(&issued.key));
    assert!(!warning.contains("unsafe error"));
    assert!(
        AuthTable::from_config(&config)
            .verify(&issued.key)
            .is_some()
    );
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains(&issued.key)
    );
}

#[test]
fn unknown_client_does_not_issue_or_store_a_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    config_at(&path);
    // IssuedKey は秘密を含むため Debug を持たず、unwrap_err は使えない。
    let error = keygen_with(&path, "missing", |_, _| panic!("unknown client")).err();
    assert_eq!(error.as_deref(), Some("指定されたクライアントがありません"));
    assert!(Config::load(&path).unwrap().keys.is_empty());
}

#[test]
fn invalid_config_is_reported_as_such_even_when_it_names_the_target_client() {
    // default_client が未登録の名前を指す設定は読み込み時に UnknownClient になる。
    // その名前を追加・再発行の対象にしても「クライアントが無い」ではなく設定の異常として伝える。
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let original = "[cli]\ndefault_client = \"ghost\"\n";
    std::fs::write(&path, original).unwrap();
    let expected = ConfigError::UnknownClient("ghost".into()).to_string();
    for name in ["ghost", "other"] {
        let error = add_with(&path, name, Role::Agent, None, true, |_, _| {
            panic!("invalid config must not issue a key")
        })
        .err();
        assert_eq!(error.as_deref(), Some(expected.as_str()));
        let error = keygen_with(&path, name, |_, _| {
            panic!("invalid config must not issue a key")
        })
        .err();
        assert_eq!(error.as_deref(), Some(expected.as_str()));
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn concurrent_app_and_cli_updates_keep_every_client_and_key() {
    const ROUNDS: usize = 6;
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("config.toml"));
    config_at(&path);
    let barrier = Arc::new(Barrier::new(ROUNDS * 2 + 1));
    let mut issuers = Vec::new();
    for index in 0..ROUNDS {
        // 設定画面からの追加とキー発行。
        let (app_path, app_barrier) = (Arc::clone(&path), Arc::clone(&barrier));
        issuers.push(thread::spawn(move || {
            app_barrier.wait();
            let name = format!("app-{index}");
            let issued = add_with(
                &app_path,
                &name,
                Role::Agent,
                Some("scope"),
                true,
                |_, _| Ok(StoreLocation::Keychain),
            )
            .unwrap()
            .unwrap();
            (name, issued.key)
        }));
        // CLI の `gaia client add --generate-key` と同じ経路（Config::update）。
        let (cli_path, cli_barrier) = (Arc::clone(&path), Arc::clone(&barrier));
        issuers.push(thread::spawn(move || {
            cli_barrier.wait();
            let name = format!("cli-{index}");
            let key = Config::update(&cli_path, |config| {
                thread::sleep(Duration::from_millis(2));
                config.add_client(ClientIdentity {
                    name: name.clone(),
                    role: Role::Agent,
                    default_scope: Some("scope".into()),
                })?;
                let (key, hash) = auth::generate_key(&name);
                config.keys.insert(name.clone(), hash);
                Ok(key)
            })
            .unwrap();
            (name, key)
        }));
    }
    // 既存クライアントの再発行も同時に行い、結果のキーだけが有効になる。
    let reissued = {
        let (path, barrier) = (Arc::clone(&path), Arc::clone(&barrier));
        thread::spawn(move || {
            barrier.wait();
            keygen_with(&path, "bot", |_, _| Ok(StoreLocation::File))
                .unwrap()
                .key
        })
    };
    let mut issued: Vec<_> = issuers
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    issued.push(("bot".into(), reissued.join().unwrap()));

    let config = Config::load(&path).unwrap();
    assert_eq!(config.clients.len(), ROUNDS * 2 + 1);
    assert_eq!(config.keys.len(), ROUNDS * 2 + 1);
    let table = AuthTable::from_config(&config);
    for (name, key) in &issued {
        assert_eq!(config.keys[name], hash_key(key));
        assert_eq!(table.verify(key).unwrap().name, *name);
    }
}

#[test]
fn newly_added_agent_returns_key_and_storage_location_without_exposing_them_in_list() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    config_at(&path);
    let called = Cell::new(false);
    let issued = add_with(
        &path,
        "new-bot",
        Role::Agent,
        Some("scope"),
        true,
        |name, key| {
            assert_eq!(name, "new-bot");
            assert_eq!(Config::load(&path).unwrap().keys[name], hash_key(key));
            called.set(true);
            Ok(StoreLocation::File)
        },
    )
    .unwrap()
    .unwrap();
    assert!(called.get());
    assert!(matches!(issued.storage.location, Some(StoreLocation::File)));
    let summaries = serde_json::to_string(&list(&path).unwrap()).unwrap();
    assert!(!summaries.contains(&issued.key));
    assert!(summaries.contains("\"has_key\":true"));
}

#[test]
fn rename_moves_config_references_and_the_stored_key_to_the_new_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = config_at(&path);
    config
        .add_client(ClientIdentity {
            name: "owner".into(),
            role: Role::Human,
            default_scope: Some("scope".into()),
        })
        .unwrap();
    config.cli.default_client = Some("owner".into());
    let (key, hash) = auth::generate_key("bot");
    config.keys.insert("bot".into(), hash.clone());
    config.server.port = Some(4123);
    config.save(&path).unwrap();

    let moved = Cell::new(false);
    let renamed = rename_with(&path, "bot", " robot ", |old, new, expected| {
        assert_eq!((old, new, expected), ("bot", "robot", hash.as_str()));
        // 設定の保存後に呼ばれる
        let saved = Config::load(&path).unwrap();
        assert!(saved.client("bot").is_none());
        assert_eq!(saved.keys["robot"], hash);
        moved.set(true);
        Ok(Some(StoreLocation::Keychain))
    })
    .unwrap();
    assert!(moved.get());
    assert_eq!(renamed.name, "robot");
    assert!(matches!(renamed.key_moved, Some(StoreLocation::Keychain)));
    assert!(renamed.key_error.is_none());

    let saved = Config::load(&path).unwrap();
    assert_eq!(saved.clients.len(), 2);
    assert_eq!(saved.server.port, Some(4123));
    assert_eq!(saved.cli.default_client.as_deref(), Some("owner"));
    let robot = saved.client("robot").unwrap();
    assert_eq!(robot.role, Role::Agent);
    assert_eq!(robot.default_scope.as_deref(), Some("scope"));
    assert_eq!(saved.keys.len(), 1);
    // HTTP のキーは新名で有効なまま
    assert_eq!(
        AuthTable::from_config(&saved).verify(&key).unwrap().name,
        "robot"
    );

    // default_client の human を改名すると default_client も追従する
    let renamed = rename_with(&path, "owner", "me", |_, _, _| {
        panic!("clients without a key must not touch the key store")
    })
    .unwrap();
    assert_eq!(renamed.name, "me");
    assert!(renamed.key_moved.is_none());
    assert!(renamed.key_error.is_none());
    let saved = Config::load(&path).unwrap();
    assert_eq!(saved.cli.default_client.as_deref(), Some("me"));
    assert_eq!(saved.client("me").unwrap().role, Role::Human);
}

#[test]
fn rename_rejects_blank_duplicate_and_missing_names_without_touching_config_or_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = config_at(&path);
    config
        .add_client(ClientIdentity {
            name: "other".into(),
            role: Role::Agent,
            default_scope: None,
        })
        .unwrap();
    config
        .keys
        .insert("bot".into(), auth::generate_key("bot").1);
    config.save(&path).unwrap();
    let untouched = std::fs::read(&path).unwrap();
    for (old, new, expected) in [
        (
            "bot",
            "  ",
            "クライアント名を入力してください。制御文字は使えません",
        ),
        (
            "bot",
            "bad\nname",
            "クライアント名を入力してください。制御文字は使えません",
        ),
        ("bot", "other", "同じ名前のクライアントが既にあります"),
        ("bot", " bot ", "同じ名前のクライアントが既にあります"),
        ("missing", "fresh", "指定されたクライアントがありません"),
    ] {
        let error = rename_with(&path, old, new, |_, _, _| {
            panic!("must validate before touching the key store")
        })
        .err();
        assert_eq!(error.as_deref(), Some(expected), "{old} -> {new}");
        assert_eq!(std::fs::read(&path).unwrap(), untouched);
    }
}

#[test]
fn rename_reports_a_failed_key_move_without_reverting_the_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = config_at(&path);
    let (key, hash) = auth::generate_key("bot");
    config.keys.insert("bot".into(), hash.clone());
    config.save(&path).unwrap();

    let renamed = rename_with(&path, "bot", "robot", |_, _, _| {
        Err(format!("unsafe error {key}"))
    })
    .unwrap();
    assert_eq!(renamed.name, "robot");
    assert!(renamed.key_moved.is_none());
    let warning = renamed.key_error.unwrap();
    assert!(!warning.contains(&key));
    assert!(!warning.contains("unsafe error"));
    assert!(warning.contains("再発行"));
    let saved = Config::load(&path).unwrap();
    assert_eq!(saved.keys["robot"], hash);
    assert!(saved.client("bot").is_none());

    // 保管キーが無い（CLI で発行したなど）場合は付け替え無しで成功し、警告も出さない
    let renamed = rename_with(&path, "robot", "bot", |_, _, _| Ok(None)).unwrap();
    assert_eq!(renamed.name, "bot");
    assert!(renamed.key_moved.is_none());
    assert!(renamed.key_error.is_none());
}

fn paths() -> SnippetPaths<'static> {
    SnippetPaths {
        config: Path::new("/temporary/with space/config.toml"),
        db: Path::new("/temporary/actual.db"),
        cli: Path::new("/Applications/gaia-library.app/Contents/MacOS/gaia"),
    }
}

#[test]
fn stdio_snippet_retains_actual_database_and_does_not_read_keys() {
    let dir = tempfile::tempdir().unwrap();
    let config = config_at(&dir.path().join("config.toml"));
    let result = snippet_with(&config, paths(), "bot", "stdio", None, |_, _| {
        panic!("stdio needs no key")
    })
    .unwrap();
    let value: Value = serde_json::from_str(&result.text).unwrap();
    let entry = &value["mcpServers"]["gaia_library"];
    assert_eq!(entry["command"], paths().cli.to_str().unwrap());
    assert_eq!(entry["args"][1], paths().config.to_str().unwrap());
    assert_eq!(entry["env"]["GAIA_DB"], paths().db.to_str().unwrap());
    assert_eq!(entry["args"][5], "bot");
    assert!(result.key_storage.is_none());
}

#[test]
fn http_snippet_uses_the_bound_url_and_rejects_stale_or_absent_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config_at(&dir.path().join("config.toml"));
    let (key, hash) = auth::generate_key("bot");
    config.keys.insert("bot".into(), hash.clone());
    config.server.port = Some(4111);
    let url = Some("http://127.0.0.1:4114/mcp");
    let result = snippet_with(&config, paths(), "bot", "http", url, |name, expected| {
        assert_eq!(name, "bot");
        assert_eq!(expected, hash);
        Ok(Some((key.clone(), StoreLocation::Keychain)))
    })
    .unwrap();
    let value: Value = serde_json::from_str(&result.text).unwrap();
    assert_eq!(
        value["mcpServers"]["gaia_library"]["url"],
        "http://127.0.0.1:4114/mcp"
    );
    assert!(matches!(result.key_storage, Some(StoreLocation::Keychain)));
    assert!(
        snippet_with(&config, paths(), "bot", "http", url, |_, _| {
            Ok(Some(("obsolete-key".into(), StoreLocation::File)))
        })
        .is_err()
    );
    assert!(snippet_with(&config, paths(), "bot", "http", url, |_, _| Ok(None)).is_err());
    assert!(
        snippet_with(&config, paths(), "bot", "http", None, |_, _| {
            panic!("stopped server must be rejected before key access")
        })
        .is_err()
    );
}

#[test]
fn shared_hash_across_clients_cannot_produce_an_http_snippet() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config_at(&dir.path().join("config.toml"));
    let (key, hash) = auth::generate_key("bot");
    config.keys.insert("bot".into(), hash.clone());
    config
        .add_client(ClientIdentity {
            name: "other".into(),
            role: Role::Human,
            default_scope: None,
        })
        .unwrap();
    config.keys.insert("other".into(), hash);
    assert!(
        snippet_with(
            &config,
            paths(),
            "bot",
            "http",
            Some("http://127.0.0.1:4111/mcp"),
            |_, _| { Ok(Some((key, StoreLocation::Keychain))) }
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn invalid_utf8_in_snippet_paths_is_not_silently_replaced() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let path = Path::new(OsStr::from_bytes(b"/invalid/\xff"));
    assert!(utf8_absolute(path).is_err());
}

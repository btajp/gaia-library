use std::cell::Cell;

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
    assert!(keygen_with(&path, "missing", |_, _| panic!("unknown client")).is_err());
    assert!(Config::load(&path).unwrap().keys.is_empty());
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

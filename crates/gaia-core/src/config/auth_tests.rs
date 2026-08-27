use super::*;
use crate::auth::{AuthTable, hash_key};
use std::{
    sync::{Arc, Barrier},
    thread,
};

fn valid_config() -> Config {
    let mut config = Config::default();
    for (name, role) in [("me", Role::Human), ("bot", Role::Agent)] {
        config
            .add_client(ClientIdentity {
                name: name.into(),
                role,
                default_scope: Some("cn".into()),
            })
            .unwrap();
        config
            .keys
            .insert(name.into(), hash_key(&format!("test-{name}")));
    }
    config.cli.default_client = Some("me".into());
    config
}

fn assert_rejected_on_every_load_and_save_path(invalid: &Config) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, toml::to_string_pretty(invalid).unwrap()).unwrap();
    assert!(Config::load(&path).is_err());
    assert!(AuthTable::from_config(invalid).is_empty());

    let original = valid_config();
    original.save(&path).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(invalid.save(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);

    let new_path = dir.path().join("new.toml");
    assert!(
        invalid
            .create_with::<(), ConfigError>(&new_path, || panic!(
                "invalid config must not initialize"
            ))
            .is_err()
    );
    assert!(!new_path.exists());

    assert!(
        Config::update(&path, |config| {
            *config = invalid.clone();
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(Config::load(&path).unwrap(), original);
}

#[test]
fn malformed_key_hashes_are_rejected_on_every_load_and_save_path() {
    for hash in [String::new(), "ab".into(), "zz".repeat(32), "é".repeat(32)] {
        let mut config = valid_config();
        config.keys.insert("me".into(), hash);
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidKeyHash(name)) if name == "me"
        ));
        assert_rejected_on_every_load_and_save_path(&config);
    }
}

#[test]
fn keys_for_unknown_clients_are_rejected_on_every_load_and_save_path() {
    let mut config = valid_config();
    config.keys.insert("ghost".into(), hash_key("test-ghost"));
    assert!(matches!(
        config.validate(),
        Err(ConfigError::UnknownClient(name)) if name == "ghost"
    ));
    assert_rejected_on_every_load_and_save_path(&config);
}

#[test]
fn duplicate_hashes_are_case_insensitive_on_every_load_and_save_path() {
    let mut config = valid_config();
    config.keys.insert("me".into(), "ab".repeat(32));
    config.keys.insert("bot".into(), "AB".repeat(32));
    assert!(matches!(
        config.validate(),
        Err(ConfigError::DuplicateKeyHash { .. })
    ));
    assert_rejected_on_every_load_and_save_path(&config);
}

#[test]
fn unknown_default_client_is_rejected_on_every_load_and_save_path() {
    let mut config = valid_config();
    config.cli.default_client = Some("missing".into());
    assert!(matches!(
        config.validate(),
        Err(ConfigError::UnknownClient(name)) if name == "missing"
    ));
    assert_rejected_on_every_load_and_save_path(&config);
}

#[test]
fn duplicate_identity_is_rejected_on_every_load_and_save_path() {
    let mut config = valid_config();
    config.clients.push(ClientIdentity {
        name: "me".into(),
        role: Role::Agent,
        default_scope: None,
    });
    assert!(matches!(
        config.validate(),
        Err(ConfigError::DuplicateClient(name)) if name == "me"
    ));
    assert_rejected_on_every_load_and_save_path(&config);
}

#[test]
fn concurrent_client_additions_and_rotation_preserve_every_key() {
    const ADDITIONS: usize = 12;

    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("config.toml"));
    let original = valid_config();
    original.save(&path).unwrap();
    let table = AuthTable::from_path(&*path).unwrap();
    let barrier = Arc::new(Barrier::new(ADDITIONS + 1));
    let mut handles: Vec<_> = (0..ADDITIONS)
        .map(|index| {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                Config::update(&path, |config| {
                    let name = format!("agent-{index}");
                    config.add_client(ClientIdentity {
                        name: name.clone(),
                        role: Role::Agent,
                        default_scope: Some("cn".into()),
                    })?;
                    config
                        .keys
                        .insert(name, hash_key(&format!("test-agent-{index}")));
                    Ok(())
                })
            })
        })
        .collect();
    let rotation_path = Arc::clone(&path);
    handles.push(thread::spawn(move || {
        barrier.wait();
        Config::update(&rotation_path, |config| {
            config.keys.insert("me".into(), hash_key("rotated-me"));
            Ok(())
        })
    }));
    for handle in handles {
        handle.join().unwrap().unwrap();
    }

    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded.clients.len(), ADDITIONS + original.clients.len());
    assert_eq!(loaded.keys.len(), ADDITIONS + original.keys.len());
    assert_eq!(loaded.keys["bot"], original.keys["bot"]);
    assert_eq!(loaded.cli.default_client, original.cli.default_client);
    assert!(table.verify("test-me").is_none());
    assert_eq!(table.verify("rotated-me").unwrap().name, "me");
    for index in 0..ADDITIONS {
        let name = format!("agent-{index}");
        let plaintext = format!("test-agent-{index}");
        assert_eq!(loaded.keys[&name], hash_key(&plaintext));
        assert_eq!(table.verify(&plaintext).unwrap().name, name);
    }
}

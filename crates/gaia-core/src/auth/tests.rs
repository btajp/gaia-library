use super::*;
use crate::identity::Role;

fn config_with_key() -> (Config, String) {
    let mut config = Config::default();
    config
        .add_client(ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: Some("cn".into()),
        })
        .unwrap();
    let (plaintext, hash) = generate_key("bot");
    config.keys.insert("bot".into(), hash);
    (config, plaintext)
}

#[test]
fn generated_key_verifies_and_wrong_key_does_not() {
    let (config, plaintext) = config_with_key();
    let table = AuthTable::from_config(&config);
    assert!(!table.is_empty());
    let id = table.verify(&plaintext).expect("valid key");
    assert_eq!(id.name, "bot");
    assert!(table.verify("gaia_bot_deadbeef").is_none());
    assert!(table.verify("").is_none());
}

#[test]
fn keys_without_matching_client_are_ignored() {
    let mut config = Config::default();
    config.keys.insert("ghost".into(), hash_key("gaia_ghost_x"));
    assert!(AuthTable::from_config(&config).is_empty());
}

#[test]
fn malformed_hashes_are_ignored_without_panicking() {
    let mut config = Config::default();
    config
        .add_client(ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: None,
        })
        .unwrap();
    for hash in ["aéx".into(), "zz".repeat(32), "ab".into()] {
        config.keys.insert("bot".into(), hash);
        assert!(AuthTable::from_config(&config).is_empty());
    }
}

#[test]
fn shared_hash_across_agent_and_human_is_rejected() {
    let mut config = Config::default();
    config
        .add_client(ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: Some("cn".into()),
        })
        .unwrap();
    config
        .add_client(ClientIdentity {
            name: "operator".into(),
            role: Role::Human,
            default_scope: Some("cn".into()),
        })
        .unwrap();
    let plaintext = "gaia_shared_secret";
    let hash = hash_key(plaintext);
    config.keys.insert("bot".into(), hash.clone());
    config.keys.insert("operator".into(), hash);

    let table = AuthTable::from_config(&config);
    assert!(table.is_empty());
    assert!(table.verify(plaintext).is_none());
}

#[test]
fn duplicate_client_name_rejects_its_key() {
    let mut config = Config::default();
    config.clients.push(ClientIdentity {
        name: "duplicate".into(),
        role: Role::Agent,
        default_scope: Some("cn".into()),
    });
    config.clients.push(ClientIdentity {
        name: "duplicate".into(),
        role: Role::Human,
        default_scope: Some("cn".into()),
    });
    let plaintext = "gaia_duplicate_secret";
    config.keys.insert("duplicate".into(), hash_key(plaintext));

    let table = AuthTable::from_config(&config);
    assert!(table.is_empty());
    assert!(table.verify(plaintext).is_none());
}

#[test]
fn file_backed_table_observes_rotation_and_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    config
        .add_client(ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: None,
        })
        .unwrap();
    let (first, first_hash) = generate_key("bot");
    config.keys.insert("bot".into(), first_hash);
    config.save(&path).unwrap();
    let table = AuthTable::from_path(&path).unwrap();
    assert_eq!(table.verify(&first).unwrap().name, "bot");

    let (second, second_hash) = generate_key("bot");
    Config::update(&path, |config| {
        config.keys.insert("bot".into(), second_hash);
        Ok(())
    })
    .unwrap();
    assert!(table.verify(&first).is_none());
    assert_eq!(table.verify(&second).unwrap().name, "bot");

    std::fs::write(&path, "not valid toml = [").unwrap();
    assert!(table.is_empty());
    assert!(table.verify(&second).is_none());
}

#[test]
fn key_format_and_hash_are_stable() {
    let (plaintext, hash) = generate_key("claude-code");
    assert!(plaintext.starts_with("gaia_claude-code_"));
    assert_eq!(plaintext.len(), "gaia_claude-code_".len() + 32);
    assert_eq!(hash, hash_key(&plaintext));
    assert_eq!(hash.len(), 64);
    assert_eq!(
        hash_key("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let (second, _) = generate_key("claude-code");
    assert_ne!(plaintext, second, "乱数で毎回異なる");
}

#[test]
fn entropy_failure_aborts_generation_without_exposing_input_or_source_error() {
    let failure = std::panic::catch_unwind(|| {
        generate_key_with_entropy("do-not-log-client-name", |raw| {
            raw.fill(0xab);
            Err("do-not-log-entropy-error")
        })
    })
    .expect_err("entropy failure must not return a key");
    assert_eq!(
        failure.downcast_ref::<&str>().copied(),
        Some("cannot obtain OS randomness for API key generation")
    );
}

#[test]
fn generated_keys_are_header_safe_for_unicode_spaces_and_control_characters() {
    for (name, expected_prefix) in [
        ("日本語 クライアント", "client"),
        (" two words ", "twowords"),
        ("\r\n\t\0\u{7f}", "client"),
        ("a\r\nb\t\0c", "abc"),
        ("", "client"),
    ] {
        let (plaintext, hash) = generate_key(name);
        let (prefix, random) = plaintext
            .strip_prefix("gaia_")
            .unwrap()
            .rsplit_once('_')
            .unwrap();
        assert_eq!(prefix, expected_prefix);
        assert_eq!(random.len(), 32);
        assert!(random.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(
            plaintext
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/".contains(&byte))
        );
        assert_eq!(hash, hash_key(&plaintext));

        let mut config = Config::default();
        config
            .add_client(ClientIdentity {
                name: name.into(),
                role: Role::Agent,
                default_scope: None,
            })
            .unwrap();
        config.keys.insert(name.into(), hash);
        assert_eq!(
            AuthTable::from_config(&config)
                .verify(&plaintext)
                .unwrap()
                .name,
            name
        );
    }
}

#[test]
fn existing_safe_ascii_prefix_is_preserved() {
    let name = "Agent-01_._~+/";
    let (plaintext, _) = generate_key(name);
    assert!(plaintext.starts_with(&format!("gaia_{name}_")));
    assert_eq!(plaintext.len(), "gaia_".len() + name.len() + 1 + 32);
}

#[test]
fn long_name_prefix_is_bounded_without_truncating_random_bytes() {
    let name = format!("日本語{}", "A".repeat(10_000));
    let (first, hash) = generate_key(&name);
    let expected_prefix = format!("gaia_{}_", "A".repeat(KEY_PREFIX_MAX_LENGTH));
    assert!(first.starts_with(&expected_prefix));
    assert_eq!(first.len(), expected_prefix.len() + 32);
    assert_eq!(hash, hash_key(&first));
    let (second, _) = generate_key(&name);
    assert_ne!(first, second);
}

#[test]
fn colliding_safe_prefixes_resolve_original_identities_by_hash_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    let mut keys = Vec::new();
    for (name, role) in [
        ("日本語 利用者", Role::Agent),
        ("日本語 管理者", Role::Human),
    ] {
        config
            .add_client(ClientIdentity {
                name: name.into(),
                role,
                default_scope: Some("cn".into()),
            })
            .unwrap();
        let (plaintext, hash) = generate_key(name);
        assert!(plaintext.starts_with("gaia_client_"));
        config.keys.insert(name.into(), hash);
        keys.push((plaintext, name, role));
    }
    config.save(&path).unwrap();
    let table = AuthTable::from_path(&path).unwrap();
    for (plaintext, name, role) in keys {
        let identity = table.verify(&plaintext).unwrap();
        assert_eq!(identity.name, name);
        assert_eq!(identity.role, role);
    }
}

#[test]
fn uppercase_hash_verifies_the_same_key() {
    let (mut config, plaintext) = config_with_key();
    let hash = config.keys["bot"].to_ascii_uppercase();
    config.keys.insert("bot".into(), hash);
    assert_eq!(
        AuthTable::from_config(&config)
            .verify(&plaintext)
            .unwrap()
            .name,
        "bot"
    );
}

#[test]
fn file_backed_table_reloads_identity_and_key_removal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let (config, plaintext) = config_with_key();
    config.save(&path).unwrap();
    let table = AuthTable::from_path(&path).unwrap();
    Config::update(&path, |config| {
        config.clients[0].role = Role::Human;
        config.clients[0].default_scope = Some("other".into());
        Ok(())
    })
    .unwrap();
    let identity = table.verify(&plaintext).unwrap();
    assert_eq!(identity.role, Role::Human);
    assert_eq!(identity.default_scope.as_deref(), Some("other"));

    Config::update(&path, |config| {
        config.keys.clear();
        Ok(())
    })
    .unwrap();
    assert!(table.is_empty());
    assert!(table.verify(&plaintext).is_none());
}

#[test]
fn file_backed_table_fails_closed_for_auth_invalid_or_missing_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let (config, plaintext) = config_with_key();
    config.save(&path).unwrap();
    let table = AuthTable::from_path(&path).unwrap();
    let mut invalid = Vec::new();
    let mut malformed = config.clone();
    malformed.keys.insert("bot".into(), "é".repeat(32));
    invalid.push(malformed);
    let mut unknown_client = config.clone();
    unknown_client
        .keys
        .insert("ghost".into(), hash_key("test-ghost"));
    invalid.push(unknown_client);
    let mut unknown_default = config.clone();
    unknown_default.cli.default_client = Some("ghost".into());
    invalid.push(unknown_default);
    let mut duplicate_identity = config.clone();
    duplicate_identity.clients.push(ClientIdentity {
        name: "bot".into(),
        role: Role::Human,
        default_scope: None,
    });
    invalid.push(duplicate_identity);
    let mut duplicate_hash = config.clone();
    duplicate_hash
        .add_client(ClientIdentity {
            name: "operator".into(),
            role: Role::Human,
            default_scope: None,
        })
        .unwrap();
    duplicate_hash
        .keys
        .insert("operator".into(), config.keys["bot"].to_ascii_uppercase());
    invalid.push(duplicate_hash);

    for invalid_config in invalid {
        std::fs::write(&path, toml::to_string_pretty(&invalid_config).unwrap()).unwrap();
        assert!(AuthTable::from_path(&path).is_err());
        assert!(
            AuthTable::from_config(&invalid_config)
                .verify(&plaintext)
                .is_none()
        );
        assert!(table.is_empty());
        assert!(table.verify(&plaintext).is_none());
        config.save(&path).unwrap();
        assert!(table.verify(&plaintext).is_some());
    }

    std::fs::remove_file(&path).unwrap();
    assert!(AuthTable::from_path(&path).is_err());
    assert!(table.is_empty());
    assert!(table.verify(&plaintext).is_none());
    config.save(&path).unwrap();
    assert!(table.verify(&plaintext).is_some());
}

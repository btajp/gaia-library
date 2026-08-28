use super::*;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Barrier},
    thread,
    time::Duration,
};

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k: &str| map.get(k).map(OsString::from)
}

fn human(name: &str) -> ClientIdentity {
    ClientIdentity {
        name: name.into(),
        role: Role::Human,
        default_scope: Some("cn".into()),
    }
}

#[test]
fn config_path_prefers_gaia_config_then_xdg_then_home() {
    let p = config_path_with(&env(&[("GAIA_CONFIG", "/x/c.toml"), ("HOME", "/h")])).unwrap();
    assert_eq!(p, PathBuf::from("/x/c.toml"));
    let p = config_path_with(&env(&[("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/h")])).unwrap();
    assert_eq!(p, PathBuf::from("/xdg/gaia-library/config.toml"));
    let p = config_path_with(&env(&[("HOME", "/h")])).unwrap();
    assert_eq!(p, PathBuf::from("/h/.config/gaia-library/config.toml"));
    assert!(matches!(
        config_path_with(&env(&[])),
        Err(ConfigError::MissingHome)
    ));
}

#[test]
fn db_path_prefers_env_then_config_then_xdg_data() {
    let mut cfg = Config::default();
    let p = db_path_with(&cfg, &env(&[("GAIA_DB", "/x/g.db"), ("HOME", "/h")])).unwrap();
    assert_eq!(p, PathBuf::from("/x/g.db"));
    cfg.db_path = Some(PathBuf::from("/cfg/g.db"));
    let p = db_path_with(&cfg, &env(&[("HOME", "/h")])).unwrap();
    assert_eq!(p, PathBuf::from("/cfg/g.db"));
    cfg.db_path = None;
    let p = db_path_with(&cfg, &env(&[("HOME", "/h")])).unwrap();
    assert_eq!(p, PathBuf::from("/h/.local/share/gaia-library/gaia.db"));
}

#[test]
fn save_and_load_round_trip_with_0600() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("config.toml");
    let mut cfg = Config::default();
    cfg.cli.default_client = Some("me".into());
    cfg.add_client(human("me")).unwrap();
    cfg.add_client(ClientIdentity {
        name: "bot".into(),
        role: Role::Agent,
        default_scope: None,
    })
    .unwrap();
    cfg.save(&path).unwrap();
    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded, cfg);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(sibling_path(&path, ".lock").exists());
    assert!(
        std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-"))
    );
    assert!(matches!(
        cfg.add_client(human("me")),
        Err(ConfigError::DuplicateClient(_))
    ));
    assert_eq!(
        Config::load_or_default(&dir.path().join("missing.toml")).unwrap(),
        Config::default()
    );
}

#[test]
fn concurrent_updates_keep_every_client() {
    const UPDATE_COUNT: usize = 12;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    config.add_client(human("me")).unwrap();
    config.save(&path).unwrap();

    let path = Arc::new(path);
    let barrier = Arc::new(Barrier::new(UPDATE_COUNT));
    let handles: Vec<_> = (0..UPDATE_COUNT)
        .map(|index| {
            let path = Arc::clone(&path);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                Config::update(&path, |config| {
                    thread::sleep(Duration::from_millis(2));
                    config.add_client(ClientIdentity {
                        name: format!("agent-{index}"),
                        role: Role::Agent,
                        default_scope: Some("cn".into()),
                    })
                })
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }

    let loaded = Config::load(&path).unwrap();
    let names: HashSet<_> = loaded
        .clients
        .iter()
        .map(|client| client.name.as_str())
        .collect();
    assert_eq!(names.len(), UPDATE_COUNT + 1);
    assert!(names.contains("me"));
    for index in 0..UPDATE_COUNT {
        assert!(names.contains(format!("agent-{index}").as_str()));
    }
}

#[test]
fn update_holds_sibling_lock_while_mutating() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    config.add_client(human("me")).unwrap();
    config.save(&path).unwrap();

    Config::update(&path, |config| {
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(sibling_path(&path, ".lock"))
            .unwrap();
        assert!(matches!(
            competing.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        config.add_client(ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: Some("cn".into()),
        })
    })
    .unwrap();

    let after = OpenOptions::new()
        .read(true)
        .write(true)
        .open(sibling_path(&path, ".lock"))
        .unwrap();
    after.try_lock().unwrap();
}

#[test]
fn create_holds_sibling_lock_before_initializing_and_rejects_existing_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let config = Config::default();
    config
        .create_with::<_, ConfigError>(&path, || {
            let competing = OpenOptions::new()
                .read(true)
                .write(true)
                .open(sibling_path(&path, ".lock"))
                .unwrap();
            assert!(matches!(
                competing.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
            assert!(!path.exists());
            Ok(())
        })
        .unwrap();
    assert_eq!(Config::load(&path).unwrap(), config);

    let original = fs::read(&path).unwrap();
    let error = config
        .create_with::<(), ConfigError>(&path, || panic!("must not initialize twice"))
        .unwrap_err();
    assert!(matches!(error, ConfigError::AlreadyExists(_)));
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn create_leaves_no_config_when_initializer_fails_and_releases_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let config = Config::default();
    let error = config
        .create_with::<(), ConfigError>(&path, || {
            Err(ConfigError::Serialize("initializer failed".into()))
        })
        .unwrap_err();
    assert!(matches!(error, ConfigError::Serialize(_)));
    assert!(!path.exists());
    config
        .create_with::<_, ConfigError>(&path, || Ok(()))
        .unwrap();
}

#[test]
fn create_does_not_clobber_config_created_without_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let error = Config::default()
        .create_with::<_, ConfigError>(&path, || {
            fs::write(&path, "existing config").unwrap();
            Ok(())
        })
        .unwrap_err();
    assert!(matches!(error, ConfigError::AlreadyExists(_)));
    assert_eq!(fs::read_to_string(&path).unwrap(), "existing config");
    assert!(fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".tmp-")
    }));
}

#[test]
fn resolve_client_uses_explicit_then_default_then_sole_human() {
    let mut cfg = Config::default();
    cfg.add_client(human("me")).unwrap();
    cfg.add_client(ClientIdentity {
        name: "bot".into(),
        role: Role::Agent,
        default_scope: None,
    })
    .unwrap();
    assert_eq!(cfg.resolve_client(Some("bot")).unwrap().role, Role::Agent);
    assert!(matches!(
        cfg.resolve_client(Some("nope")),
        Err(ConfigError::UnknownClient(_))
    ));
    // default_client 未設定・human が 1 人 → その人
    assert_eq!(cfg.resolve_client(None).unwrap().name, "me");
    cfg.add_client(human("other")).unwrap();
    assert!(matches!(
        cfg.resolve_client(None),
        Err(ConfigError::NoDefaultClient)
    ));
    cfg.cli.default_client = Some("other".into());
    assert_eq!(cfg.resolve_client(None).unwrap().name, "other");
}

#[test]
fn unknown_keys_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "bogus = 1\n").unwrap();
    assert!(matches!(
        Config::load(&path),
        Err(ConfigError::Parse { .. })
    ));
}

#[test]
fn load_rejects_duplicate_client_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[[clients]]\nname = \"same\"\nrole = \"agent\"\n\
         [[clients]]\nname = \"same\"\nrole = \"human\"\n",
    )
    .unwrap();
    assert!(matches!(
        Config::load(&path),
        Err(ConfigError::DuplicateClient(name)) if name == "same"
    ));
}

#[test]
fn resolve_client_rejects_duplicate_names() {
    let mut cfg = Config::default();
    cfg.clients.push(ClientIdentity {
        name: "same".into(),
        role: Role::Agent,
        default_scope: None,
    });
    cfg.clients.push(human("same"));
    assert!(cfg.client("same").is_none());
    assert!(matches!(
        cfg.resolve_client(Some("same")),
        Err(ConfigError::DuplicateClient(name)) if name == "same"
    ));
    cfg.cli.default_client = Some("same".into());
    assert!(matches!(
        cfg.resolve_client(None),
        Err(ConfigError::DuplicateClient(name)) if name == "same"
    ));
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        cfg.save(&dir.path().join("config.toml")),
        Err(ConfigError::DuplicateClient(name)) if name == "same"
    ));
}

#[test]
fn keys_and_server_round_trip_and_default_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut cfg = Config::default();
    cfg.add_client(human("me")).unwrap();
    cfg.keys.insert("me".into(), "ab".repeat(32));
    cfg.server.port = Some(4200);
    cfg.save(&path).unwrap();
    assert_eq!(Config::load(&path).unwrap(), cfg);

    // 旧形式（keys / server なし）も読める。
    std::fs::write(&path, "[[clients]]\nname = \"x\"\nrole = \"human\"\n").unwrap();
    let old = Config::load(&path).unwrap();
    assert!(old.keys.is_empty());
    assert_eq!(old.server.port, None);
}

fn entry_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(unix)]
#[test]
fn save_through_symlink_chain_keeps_links_and_replaces_the_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    for target_exists in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let aliases = dir.path().join("aliases");
        fs::create_dir(&real).unwrap();
        fs::create_dir(&aliases).unwrap();
        let target = real.join("config.toml");
        if target_exists {
            let mut first = Config::default();
            first.add_client(human("first")).unwrap();
            first.save(&target).unwrap();
        }
        let link = aliases.join("config.toml");
        let intermediate = aliases.join("second.toml");
        symlink("second.toml", &link).unwrap();
        symlink("../real/config.toml", &intermediate).unwrap();

        let mut replacement = Config::default();
        replacement.add_client(human("second")).unwrap();
        replacement.save(&link).unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::symlink_metadata(&intermediate)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("second.toml"));
        assert_eq!(
            fs::read_link(&intermediate).unwrap(),
            PathBuf::from("../real/config.toml")
        );
        assert_eq!(Config::load(&target).unwrap(), replacement);
        assert_eq!(Config::load(&link).unwrap(), replacement);
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600,
            "target_exists={target_exists}"
        );
        // 一時ファイルと lock は解決後のターゲットの兄弟に置き、別名側には何も作らない。
        assert_eq!(entry_names(&real), ["config.toml", "config.toml.lock"]);
        assert_eq!(entry_names(&aliases), ["config.toml", "second.toml"]);
    }
}

#[cfg(unix)]
#[test]
fn update_through_symlink_holds_the_lock_beside_the_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    fs::create_dir(&real).unwrap();
    let target = real.join("config.toml");
    let mut config = Config::default();
    config.add_client(human("me")).unwrap();
    config.save(&target).unwrap();
    let link = dir.path().join("config.toml");
    symlink(&target, &link).unwrap();

    Config::update(&link, |config| {
        let competing = OpenOptions::new()
            .read(true)
            .write(true)
            .open(sibling_path(&target, ".lock"))
            .unwrap();
        assert!(matches!(
            competing.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        assert!(!sibling_path(&link, ".lock").exists());
        config.add_client(ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: Some("cn".into()),
        })
    })
    .unwrap();

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(Config::load(&target).unwrap().clients.len(), 2);
    assert_eq!(entry_names(&real), ["config.toml", "config.toml.lock"]);
    assert_eq!(entry_names(dir.path()), ["config.toml", "real"]);
}

#[cfg(unix)]
#[test]
fn symlink_loops_are_reported_without_replacing_the_link() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("config.toml");
    symlink("config.toml", &link).unwrap();
    let mut config = Config::default();
    config.add_client(human("me")).unwrap();

    assert!(matches!(
        config.save(&link),
        Err(ConfigError::Write { path, .. }) if path == link
    ));
    // update は lock を取る前の到達性確認（OS の symlink 追従）で読み取りエラーになる。
    assert!(matches!(
        Config::update(&link, |_| Ok(())),
        Err(ConfigError::Read { path, .. }) if path == link
    ));
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("config.toml"));
    assert_eq!(entry_names(dir.path()), ["config.toml"]);
}

#[cfg(unix)]
#[test]
fn create_rejects_a_dangling_symlink_without_following_it() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("config.toml");
    let target = dir.path().join("real.toml");
    symlink("real.toml", &link).unwrap();

    let error = Config::default()
        .create_with::<(), ConfigError>(&link, || panic!("must not initialize"))
        .unwrap_err();
    assert!(matches!(error, ConfigError::AlreadyExists(path) if path == link));
    assert!(!target.exists());
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // 拒否した操作はリンク先の兄弟に `.lock` を作らない。
    assert_eq!(entry_names(dir.path()), ["config.toml"]);
}

#[cfg(unix)]
#[test]
fn rejected_operations_leave_no_directories_or_lock_beside_a_dangling_target() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("config.toml");
    let missing = dir.path().join("nowhere");
    symlink(missing.join("deep").join("config.toml"), &link).unwrap();

    let error = Config::default()
        .create_with::<(), ConfigError>(&link, || panic!("must not initialize"))
        .unwrap_err();
    assert!(matches!(error, ConfigError::AlreadyExists(path) if path == link));
    assert!(matches!(
        Config::update(&link, |_| Ok(())),
        Err(ConfigError::Read { path, .. }) if path == link
    ));
    assert!(!missing.exists());
    assert_eq!(entry_names(dir.path()), ["config.toml"]);
}

#[cfg(unix)]
#[test]
fn symlink_to_a_directory_is_rejected_before_creating_a_lock() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let directory = dir.path().join("adir");
    fs::create_dir(&directory).unwrap();
    let link = dir.path().join("config.toml");
    symlink("adir", &link).unwrap();
    let mut config = Config::default();
    config.add_client(human("me")).unwrap();

    assert!(matches!(
        config.save(&link),
        Err(ConfigError::Write { path, .. }) if path == link
    ));
    assert!(matches!(
        Config::update(&link, |_| Ok(())),
        Err(ConfigError::Write { path, .. }) if path == link
    ));
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(entry_names(dir.path()), ["adir", "config.toml"]);
    assert!(entry_names(&directory).is_empty());
}

#[cfg(unix)]
#[test]
fn symlink_chains_up_to_the_limit_are_followed_and_longer_ones_are_rejected() {
    use std::os::unix::fs::symlink;

    for (depth, allowed) in [(MAX_SYMLINK_DEPTH, true), (MAX_SYMLINK_DEPTH + 1, false)] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.toml");
        // link0 -> link1 -> ... -> link{depth-1} -> real.toml
        for index in 0..depth {
            let next = if index + 1 == depth {
                "real.toml".to_string()
            } else {
                format!("link{}.toml", index + 1)
            };
            symlink(next, dir.path().join(format!("link{index}.toml"))).unwrap();
        }
        let head = dir.path().join("link0.toml");
        let mut config = Config::default();
        config.add_client(human("me")).unwrap();

        let result = config.save(&head);
        if allowed {
            result.unwrap_or_else(|error| panic!("depth={depth}: {error}"));
            assert_eq!(Config::load(&target).unwrap(), config);
        } else {
            assert!(
                matches!(&result, Err(ConfigError::Write { path, .. }) if *path == head),
                "depth={depth}: {result:?}"
            );
            assert!(!target.exists());
        }
        assert!(
            fs::symlink_metadata(&head)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_lock_file_is_not_followed() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut config = Config::default();
    config.add_client(human("me")).unwrap();
    config.save(&path).unwrap();
    let lock = sibling_path(&path, ".lock");
    fs::remove_file(&lock).unwrap();
    let other = dir.path().join("other.txt");
    fs::write(&other, "hello").unwrap();
    fs::set_permissions(&other, fs::Permissions::from_mode(0o644)).unwrap();
    symlink("other.txt", &lock).unwrap();

    assert!(matches!(
        Config::update(&path, |_| Ok(())),
        Err(ConfigError::Write { path, .. }) if path == lock
    ));
    assert!(matches!(
        config.save(&path),
        Err(ConfigError::Write { path, .. }) if path == lock
    ));
    assert_eq!(fs::read_to_string(&other).unwrap(), "hello");
    assert_eq!(
        fs::metadata(&other).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert!(
        fs::symlink_metadata(&lock)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(Config::load(&path).unwrap(), config);
}

#[test]
fn sources_default_is_all_disabled_and_not_written() {
    let cfg = Config::default();
    assert!(cfg.sources.is_default());
    assert_eq!(cfg.sources.max_content_chars, 30_000);
    assert!(cfg.sources.file.roots.is_empty());
    assert_eq!(cfg.sources.file.max_bytes, 1024 * 1024);
    assert!(cfg.sources.url.allow_hosts.is_empty());
    assert_eq!(cfg.sources.url.timeout_secs, 15);
    assert_eq!(cfg.sources.url.max_redirects, 3);
    assert!(cfg.sources.narumi.is_none());
    let text = toml::to_string_pretty(&cfg).unwrap();
    assert!(!text.contains("[sources"), "{text}");
    // [sources] 無しの設定は既定値で読める
    let loaded: Config = toml::from_str("[server]\nport = 4111\n").unwrap();
    assert!(loaded.sources.is_default());
}

#[test]
fn sources_round_trip_and_partial_sections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut cfg = Config::default();
    cfg.sources.file.roots = vec![PathBuf::from(
        "/Users/me/Library/Application Support/narumi/meetings",
    )];
    cfg.sources.url.allow_hosts = vec!["*".into(), "example.com".into()];
    cfg.sources.narumi = Some(NarumiSourceConfig {
        command: PathBuf::from("/opt/homebrew/bin/uv"),
        args: vec![
            "--directory".into(),
            "/path/to/narumi".into(),
            "run".into(),
            "narumi-server".into(),
            "--stdio-bridge".into(),
        ],
        timeout_secs: 45,
        stderr: NarumiStderr::Inherit,
        env: BTreeMap::from([("NARUMI_HOME".to_string(), "/x".to_string())]),
    });
    cfg.save(&path).unwrap();
    let loaded = Config::load(&path).unwrap();
    assert_eq!(loaded, cfg);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[sources.narumi]"));
    assert!(text.contains("stderr = \"inherit\""));
    // 節を一部だけ書いた設定
    let partial: Config = toml::from_str("[sources.file]\nroots = [\"/tmp/a\"]\n").unwrap();
    assert_eq!(partial.sources.file.roots, vec![PathBuf::from("/tmp/a")]);
    assert_eq!(partial.sources.max_content_chars, 30_000);
    assert!(partial.sources.narumi.is_none());
    let narumi_only: Config =
        toml::from_str("[sources.narumi]\ncommand = \"/usr/bin/true\"\n").unwrap();
    let narumi = narumi_only.sources.narumi.unwrap();
    assert_eq!(narumi.timeout_secs, 30);
    assert_eq!(narumi.stderr, NarumiStderr::Discard);
    assert!(narumi.env.is_empty());
}

#[test]
fn sources_validation_rejects_out_of_range_and_malformed_values() {
    let cases: &[(&str, &str)] = &[
        ("[sources]\nmax_content_chars = 999\n", "max_content_chars"),
        (
            "[sources]\nmax_content_chars = 500001\n",
            "max_content_chars",
        ),
        ("[sources.file]\nmax_bytes = 0\n", "max_bytes"),
        ("[sources.file]\nmax_bytes = 67108865\n", "max_bytes"),
        ("[sources.file]\nroots = [\"relative/dir\"]\n", "absolute"),
        ("[sources.file]\nroots = [\"/\"]\n", "filesystem root"),
        ("[sources.file]\nroots = [\"/a\", \"/a\"]\n", "duplicate"),
        ("[sources.url]\ntimeout_secs = 0\n", "timeout_secs"),
        ("[sources.url]\ntimeout_secs = 121\n", "timeout_secs"),
        ("[sources.url]\nmax_redirects = 11\n", "max_redirects"),
        (
            "[sources.url]\nallow_hosts = [\"127.0.0.1\"]\n",
            "allow_hosts",
        ),
        ("[sources.url]\nallow_hosts = [\"[::1]\"]\n", "allow_hosts"),
        (
            "[sources.url]\nallow_hosts = [\"localhost\"]\n",
            "allow_hosts",
        ),
        (
            "[sources.url]\nallow_hosts = [\"foo.localhost\"]\n",
            "allow_hosts",
        ),
        (
            "[sources.url]\nallow_hosts = [\"intranet\"]\n",
            "allow_hosts",
        ),
        (
            "[sources.url]\nallow_hosts = [\"example.com.\"]\n",
            "allow_hosts",
        ),
        (
            "[sources.url]\nallow_hosts = [\"Example.com\"]\n",
            "allow_hosts",
        ),
        ("[sources.url]\nallow_hosts = [\"*\", \"*\"]\n", "duplicate"),
        ("[sources.narumi]\ncommand = \"uv\"\n", "absolute"),
        ("[sources.narumi]\ncommand = \"\"\n", "command"),
        (
            "[sources.narumi]\ncommand = \"/usr/bin/true\"\ntimeout_secs = 301\n",
            "timeout_secs",
        ),
        (
            "[sources.narumi]\ncommand = \"/usr/bin/true\"\n[sources.narumi.env]\nGAIA_DB = \"/x\"\n",
            "GAIA_",
        ),
        (
            "[sources.narumi]\ncommand = \"/usr/bin/true\"\n[sources.narumi.env]\n\"1BAD\" = \"x\"\n",
            "env key",
        ),
        (
            "[sources.narumi]\ncommand = \"/usr/bin/true\"\n[sources.narumi.env]\n\"A-B\" = \"x\"\n",
            "env key",
        ),
        (
            "[sources.narumi]\ncommand = \"/usr/bin/true\"\nstderr = \"pipe\"\n",
            "invalid",
        ),
        ("[sources]\nunknown = 1\n", "unknown"),
        ("[sources.file]\nroot = [\"/a\"]\n", "unknown"),
    ];
    for (text, expected) in cases {
        let result: Result<(), ConfigError> = toml::from_str::<Config>(text)
            .map_err(|e| ConfigError::Parse {
                path: PathBuf::from("mem"),
                message: e.to_string(),
            })
            .and_then(|cfg| cfg.validate());
        let error = result.expect_err(text).to_string();
        assert!(
            error
                .to_ascii_lowercase()
                .contains(&expected.to_ascii_lowercase()),
            "{text:?}: {error}"
        );
    }
    // 上限ちょうどは通る
    let ok: Config = toml::from_str(
        "[sources]\nmax_content_chars = 500000\n[sources.file]\nmax_bytes = 67108864\nroots = [\"/a\", \"/b\"]\n[sources.url]\ntimeout_secs = 120\nmax_redirects = 10\nallow_hosts = [\"*\", \"example.com\", \"docs.example.co.jp\"]\n[sources.narumi]\ncommand = \"/usr/bin/true\"\ntimeout_secs = 300\n",
    )
    .unwrap();
    ok.validate().unwrap();
    // 保存経路も validate を通す
    let dir = tempfile::tempdir().unwrap();
    let mut bad = Config::default();
    bad.sources.max_content_chars = 1;
    assert!(matches!(
        bad.save(&dir.path().join("config.toml")),
        Err(ConfigError::InvalidSource(_))
    ));
}

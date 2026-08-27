//! 設定ファイル（TOML）とパス解決。仕様書 §7.1。XDG 配置を macOS でも使う。
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::identity::{ClientIdentity, Role};

pub const APP_DIR: &str = "gaia-library";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_path: Option<PathBuf>,
    #[serde(default)]
    pub cli: CliConfig,
    #[serde(default)]
    pub clients: Vec<ClientIdentity>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_client: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "HOME is not set; cannot resolve the config directory (set GAIA_CONFIG / GAIA_DB explicitly)"
    )]
    MissingHome,
    #[error("cannot read config {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot write config {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("config {path} is invalid: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("cannot serialize config: {0}")]
    Serialize(String),
    #[error("config {0} already exists")]
    AlreadyExists(PathBuf),
    #[error("unknown client `{0}` (see [[clients]] in the config file)")]
    UnknownClient(String),
    #[error("client `{0}` already exists")]
    DuplicateClient(String),
    #[error("no default client: set [cli].default_client or pass --client")]
    NoDefaultClient,
}

type Lookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

fn home_dir(lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    lookup("HOME")
        .map(PathBuf::from)
        .ok_or(ConfigError::MissingHome)
}

pub fn config_path_with(lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    if let Some(p) = lookup("GAIA_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    let base = match lookup("XDG_CONFIG_HOME") {
        Some(x) => PathBuf::from(x),
        None => home_dir(lookup)?.join(".config"),
    };
    Ok(base.join(APP_DIR).join("config.toml"))
}

pub fn config_path() -> Result<PathBuf, ConfigError> {
    config_path_with(&|k| std::env::var_os(k))
}

pub fn db_path_with(config: &Config, lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    if let Some(p) = lookup("GAIA_DB") {
        return Ok(PathBuf::from(p));
    }
    if let Some(p) = &config.db_path {
        return Ok(p.clone());
    }
    let base = match lookup("XDG_DATA_HOME") {
        Some(x) => PathBuf::from(x),
        None => home_dir(lookup)?.join(".local").join("share"),
    };
    Ok(base.join(APP_DIR).join("gaia.db"))
}

pub fn db_path(config: &Config) -> Result<PathBuf, ConfigError> {
    db_path_with(config, &|k| std::env::var_os(k))
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        toml::from_str(&text).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
    }

    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            Self::load(path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let _lock = ConfigFileLock::acquire(path)?;
        self.save_atomic(path, true)
    }

    /// 初期化処理から新規設定の公開までを同じ lock で直列化し、既存設定は上書きしない。
    /// 設定保存に失敗する場合があるため、`initialize` の副作用は再実行可能にすること。
    pub fn create_with<R, E: From<ConfigError>>(
        &self,
        path: &Path,
        initialize: impl FnOnce() -> Result<R, E>,
    ) -> Result<R, E> {
        let _lock = ConfigFileLock::acquire(path)?;
        match fs::symlink_metadata(path) {
            Ok(_) => return Err(ConfigError::AlreadyExists(path.to_path_buf()).into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                }
                .into());
            }
        }
        let output = initialize()?;
        self.save_atomic(path, false)?;
        Ok(output)
    }

    /// 同じ設定への read-modify-write 全体を兄弟 lock file で直列化する。
    pub fn update<R>(
        path: &Path,
        update: impl FnOnce(&mut Self) -> Result<R, ConfigError>,
    ) -> Result<R, ConfigError> {
        let _lock = ConfigFileLock::acquire(path)?;
        let mut config = Self::load(path)?;
        let output = update(&mut config)?;
        config.save_atomic(path, true)?;
        Ok(output)
    }

    fn save_atomic(&self, path: &Path, replace: bool) -> Result<(), ConfigError> {
        let text =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        let (mut temporary, temporary_path) = create_temporary_file(path)?;
        let result = (|| {
            temporary.write_all(text.as_bytes())?;
            temporary.sync_all()?;
            drop(temporary);
            if replace {
                fs::rename(&temporary_path, path)
            } else {
                // hard link の公開は既存パス（dangling symlink を含む）を置き換えない。
                fs::hard_link(&temporary_path, path)
            }
        })();
        let _ = fs::remove_file(&temporary_path);
        if let Err(source) = result {
            if !replace && source.kind() == std::io::ErrorKind::AlreadyExists {
                return Err(ConfigError::AlreadyExists(path.to_path_buf()));
            }
            return Err(ConfigError::Write {
                path: path.to_path_buf(),
                source,
            });
        }
        Ok(())
    }

    pub fn client(&self, name: &str) -> Option<&ClientIdentity> {
        self.clients.iter().find(|c| c.name == name)
    }

    pub fn add_client(&mut self, client: ClientIdentity) -> Result<(), ConfigError> {
        if self.client(&client.name).is_some() {
            return Err(ConfigError::DuplicateClient(client.name));
        }
        self.clients.push(client);
        Ok(())
    }

    /// `--client` 明示 → `[cli].default_client` → human が 1 人だけならその人 → エラー。
    pub fn resolve_client(&self, name: Option<&str>) -> Result<&ClientIdentity, ConfigError> {
        if let Some(n) = name {
            return self
                .client(n)
                .ok_or_else(|| ConfigError::UnknownClient(n.to_string()));
        }
        if let Some(n) = &self.cli.default_client {
            return self
                .client(n)
                .ok_or_else(|| ConfigError::UnknownClient(n.clone()));
        }
        let humans: Vec<&ClientIdentity> = self
            .clients
            .iter()
            .filter(|c| c.role == Role::Human)
            .collect();
        match humans.as_slice() {
            [only] => Ok(only),
            _ => Err(ConfigError::NoDefaultClient),
        }
    }
}

struct ConfigFileLock {
    _file: File,
}

impl ConfigFileLock {
    fn acquire(config_path: &Path) -> Result<Self, ConfigError> {
        ensure_parent(config_path)?;
        let lock_path = sibling_path(config_path, ".lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&lock_path)
            .map_err(|source| ConfigError::Write {
                path: lock_path.clone(),
                source,
            })?;
        #[cfg(unix)]
        set_private_permissions(&file, &lock_path)?;
        file.lock().map_err(|source| ConfigError::Write {
            path: lock_path,
            source,
        })?;
        Ok(Self { _file: file })
    }
}

fn ensure_parent(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .unwrap_or_else(|| OsStr::new("config"))
        .to_os_string();
    file_name.push(suffix);
    path.with_file_name(file_name)
}

fn create_temporary_file(path: &Path) -> Result<(File, PathBuf), ConfigError> {
    ensure_parent(path)?;
    loop {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let suffix = format!(".tmp-{}-{sequence}", std::process::id());
        let temporary_path = sibling_path(path, &suffix);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temporary_path) {
            Ok(file) => {
                #[cfg(unix)]
                if let Err(error) = set_private_permissions(&file, &temporary_path) {
                    drop(file);
                    let _ = fs::remove_file(&temporary_path);
                    return Err(error);
                }
                return Ok((file, temporary_path));
            }
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(ConfigError::Write {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

#[cfg(unix)]
fn set_private_permissions(file: &File, path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
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
}

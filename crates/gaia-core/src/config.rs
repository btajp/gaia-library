//! 設定ファイル（TOML）とパス解決。仕様書 §7.1。XDG 配置を macOS でも使う。
use std::{
    collections::{BTreeMap, BTreeSet},
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, String>,
    #[serde(default)]
    pub server: ServerConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_client: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
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
    #[error("API key hash for client `{0}` must be 64 hexadecimal characters")]
    InvalidKeyHash(String),
    #[error("clients `{first}` and `{second}` share the same API key hash")]
    DuplicateKeyHash { first: String, second: String },
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
        let config: Self = toml::from_str(&text).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        config.validate()?;
        Ok(config)
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
        self.validate()?;
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
        self.validate()?;
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
        let mut matches = self.clients.iter().filter(|c| c.name == name);
        let client = matches.next()?;
        matches.next().is_none().then_some(client)
    }

    pub fn add_client(&mut self, client: ClientIdentity) -> Result<(), ConfigError> {
        if self.clients.iter().any(|c| c.name == client.name) {
            return Err(ConfigError::DuplicateClient(client.name));
        }
        self.clients.push(client);
        Ok(())
    }

    fn validate_unique_clients(&self) -> Result<(), ConfigError> {
        let mut names = BTreeSet::new();
        for client in &self.clients {
            if !names.insert(client.name.as_str()) {
                return Err(ConfigError::DuplicateClient(client.name.clone()));
            }
        }
        Ok(())
    }

    /// 認証表とクライアント識別の曖昧さを、ロード時と全保存経路で拒否する。
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_unique_clients()?;
        if let Some(name) = &self.cli.default_client
            && self.client(name).is_none()
        {
            return Err(ConfigError::UnknownClient(name.clone()));
        }
        let mut hashes = BTreeMap::new();
        for (name, hash) in &self.keys {
            if self.client(name).is_none() {
                return Err(ConfigError::UnknownClient(name.clone()));
            }
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(ConfigError::InvalidKeyHash(name.clone()));
            }
            if let Some(first) = hashes.insert(hash.to_ascii_lowercase(), name.clone()) {
                return Err(ConfigError::DuplicateKeyHash {
                    first,
                    second: name.clone(),
                });
            }
        }
        Ok(())
    }

    /// `--client` 明示 → `[cli].default_client` → human が 1 人だけならその人 → エラー。
    pub fn resolve_client(&self, name: Option<&str>) -> Result<&ClientIdentity, ConfigError> {
        self.validate_unique_clients()?;
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
mod tests;

#[cfg(test)]
mod auth_tests;

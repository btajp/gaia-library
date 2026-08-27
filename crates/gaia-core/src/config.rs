//! 設定ファイル（TOML）とパス解決。仕様書 §7.1。XDG 配置を macOS でも使う。
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::identity::{ClientIdentity, Role};

pub const APP_DIR: &str = "gaia-library";

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
        let text =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        }
        fs::write(path, text).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
                ConfigError::Write {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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

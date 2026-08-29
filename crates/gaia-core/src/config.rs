//! 設定ファイル（TOML）とパス解決。仕様書 §7.1。XDG 配置を macOS でも使う。
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::identity::{ClientIdentity, Role};

pub const APP_DIR: &str = "gaia-library";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
/// 保存先の symlink 鎖を辿る上限。この段数までは辿り、超えたらループとみなして保存を拒否する。
const MAX_SYMLINK_DEPTH: usize = 40;

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
    /// resolve_source の解決器設定。既定値のままなら書き出さない（0.1.x でも読める設定を保つ）。
    #[serde(default, skip_serializing_if = "SourcesConfig::is_default")]
    pub sources: SourcesConfig,
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

/// `[sources]`。既定は全解決器が無効（default deny / explicit allow）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfig {
    /// content の上限（Unicode スカラー数）。
    #[serde(default = "SourcesConfig::default_max_content_chars")]
    pub max_content_chars: usize,
    #[serde(default)]
    pub file: FileSourceConfig,
    #[serde(default)]
    pub url: UrlSourceConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narumi: Option<NarumiSourceConfig>,
}

/// `[sources.file]`。`roots` が空なら無効。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileSourceConfig {
    /// 読み取りを許可する絶対パスのディレクトリ。
    #[serde(default)]
    pub roots: Vec<PathBuf>,
    #[serde(default = "FileSourceConfig::default_max_bytes")]
    pub max_bytes: u64,
}

/// `[sources.url]`。`allow_hosts` が空なら無効。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UrlSourceConfig {
    /// `"*"`（全公開ホスト）か FQDN（完全一致または `.<host>` 接尾辞一致）。
    #[serde(default)]
    pub allow_hosts: Vec<String>,
    /// リダイレクトの追従を含む 1 参照あたりの合計。
    #[serde(default = "UrlSourceConfig::default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "UrlSourceConfig::default_max_bytes")]
    pub max_bytes: u64,
    #[serde(default = "UrlSourceConfig::default_max_redirects")]
    pub max_redirects: u32,
}

/// `[sources.narumi]`。節ごと省略可（省略 = 無効）。起動コマンドはここでのみ指定できる。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NarumiSourceConfig {
    /// 絶対パス必須。
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    /// initialize と get_minutes の上限（起動からの締切）。子プロセスの終了処理は別に最長 3 秒かかり、
    /// 呼び出し元は timeout + 5 秒まで待つ。
    #[serde(default = "NarumiSourceConfig::default_timeout_secs")]
    pub timeout_secs: u64,
    /// `get_minutes` 応答の markdown のバイト上限。超過は TooLarge（本文は返さない）。
    #[serde(default = "NarumiSourceConfig::default_max_bytes")]
    pub max_bytes: u64,
    #[serde(default)]
    pub stderr: NarumiStderr,
    /// 親の環境に追加・上書きするキーだけを書く。`GAIA_` 接頭辞は不可。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NarumiStderr {
    #[default]
    Discard,
    Inherit,
}

const MIB: u64 = 1024 * 1024;

impl SourcesConfig {
    pub const MIN_MAX_CONTENT_CHARS: usize = 1_000;
    pub const MAX_MAX_CONTENT_CHARS: usize = 500_000;

    fn default_max_content_chars() -> usize {
        30_000
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// 純粋な範囲・形式検査。ファイルシステムや環境変数は見ない（存在・種別は解決時に検査する）。
    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |message: String| ConfigError::InvalidSource(message);
        if !(Self::MIN_MAX_CONTENT_CHARS..=Self::MAX_MAX_CONTENT_CHARS)
            .contains(&self.max_content_chars)
        {
            return Err(invalid(format!(
                "[sources].max_content_chars must be within {}..={}",
                Self::MIN_MAX_CONTENT_CHARS,
                Self::MAX_MAX_CONTENT_CHARS
            )));
        }
        self.file.validate()?;
        self.url.validate()?;
        if let Some(narumi) = &self.narumi {
            narumi.validate()?;
        }
        Ok(())
    }
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            max_content_chars: Self::default_max_content_chars(),
            file: FileSourceConfig::default(),
            url: UrlSourceConfig::default(),
            narumi: None,
        }
    }
}

fn validate_byte_limit(value: u64, setting: &str) -> Result<(), ConfigError> {
    if !(1..=64 * MIB).contains(&value) {
        return Err(ConfigError::InvalidSource(format!(
            "{setting} must be within 1..={} bytes (64 MiB)",
            64 * MIB
        )));
    }
    Ok(())
}

fn validate_timeout(value: u64, max: u64, setting: &str) -> Result<(), ConfigError> {
    if !(1..=max).contains(&value) {
        return Err(ConfigError::InvalidSource(format!(
            "{setting} must be within 1..={max} seconds"
        )));
    }
    Ok(())
}

fn contains_nul(value: &OsStr) -> bool {
    value.as_encoded_bytes().contains(&0)
}

impl FileSourceConfig {
    fn default_max_bytes() -> u64 {
        MIB
    }

    fn validate(&self) -> Result<(), ConfigError> {
        validate_byte_limit(self.max_bytes, "[sources.file].max_bytes")?;
        let mut seen = BTreeSet::new();
        for root in &self.roots {
            let invalid = |why: &str| {
                ConfigError::InvalidSource(format!(
                    "[sources.file].roots entry {} is invalid: {why}",
                    root.display()
                ))
            };
            if root.as_os_str().is_empty() || contains_nul(root.as_os_str()) {
                return Err(invalid("empty or contains NUL"));
            }
            if !root.is_absolute() {
                return Err(invalid("must be an absolute path"));
            }
            if root.parent().is_none() {
                return Err(invalid("the filesystem root cannot be a source root"));
            }
            if !seen.insert(root.as_path()) {
                return Err(invalid("duplicate"));
            }
        }
        Ok(())
    }
}

impl Default for FileSourceConfig {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            max_bytes: Self::default_max_bytes(),
        }
    }
}

impl UrlSourceConfig {
    pub const MAX_TIMEOUT_SECS: u64 = 120;
    pub const MAX_MAX_REDIRECTS: u32 = 10;

    fn default_timeout_secs() -> u64 {
        15
    }

    fn default_max_bytes() -> u64 {
        MIB
    }

    fn default_max_redirects() -> u32 {
        3
    }

    fn validate(&self) -> Result<(), ConfigError> {
        validate_timeout(
            self.timeout_secs,
            Self::MAX_TIMEOUT_SECS,
            "[sources.url].timeout_secs",
        )?;
        validate_byte_limit(self.max_bytes, "[sources.url].max_bytes")?;
        if self.max_redirects > Self::MAX_MAX_REDIRECTS {
            return Err(ConfigError::InvalidSource(format!(
                "[sources.url].max_redirects must be within 0..={}",
                Self::MAX_MAX_REDIRECTS
            )));
        }
        let mut seen = BTreeSet::new();
        for host in &self.allow_hosts {
            let invalid = |why: &str| {
                ConfigError::InvalidSource(format!(
                    "[sources.url].allow_hosts entry `{host}` is invalid: {why}"
                ))
            };
            if host != "*" && !is_allow_host_domain(host) {
                return Err(invalid(
                    "must be `*` or a lowercase fully qualified domain name (no IP literal, localhost, single label, or trailing dot)",
                ));
            }
            if !seen.insert(host.as_str()) {
                return Err(invalid("duplicate"));
            }
        }
        Ok(())
    }
}

/// `allow_hosts` の要素として妥当な FQDN か（`*` は呼び出し側で先に判定する）。
fn is_allow_host_domain(host: &str) -> bool {
    if host.is_empty()
        || host.len() > 253
        || host != host.to_ascii_lowercase()
        || host.ends_with('.')
        || !host.contains('.')
        || host == "localhost"
        || host.ends_with(".localhost")
    {
        return false;
    }
    matches!(url::Host::parse(host), Ok(url::Host::Domain(parsed)) if parsed == host)
}

impl Default for UrlSourceConfig {
    fn default() -> Self {
        Self {
            allow_hosts: Vec::new(),
            timeout_secs: Self::default_timeout_secs(),
            max_bytes: Self::default_max_bytes(),
            max_redirects: Self::default_max_redirects(),
        }
    }
}

impl NarumiSourceConfig {
    pub const MAX_TIMEOUT_SECS: u64 = 300;
    pub const MAX_ARGS: usize = 64;
    pub const MAX_ARG_BYTES: usize = 4096;
    pub const MAX_ENV: usize = 32;

    fn default_timeout_secs() -> u64 {
        30
    }

    fn default_max_bytes() -> u64 {
        MIB
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |message: String| ConfigError::InvalidSource(message);
        if self.command.as_os_str().is_empty() || contains_nul(self.command.as_os_str()) {
            return Err(invalid(
                "[sources.narumi].command must not be empty or contain NUL".into(),
            ));
        }
        if !self.command.is_absolute() {
            return Err(invalid(
                "[sources.narumi].command must be an absolute path".into(),
            ));
        }
        validate_timeout(
            self.timeout_secs,
            Self::MAX_TIMEOUT_SECS,
            "[sources.narumi].timeout_secs",
        )?;
        validate_byte_limit(self.max_bytes, "[sources.narumi].max_bytes")?;
        if self.args.len() > Self::MAX_ARGS {
            return Err(invalid(format!(
                "[sources.narumi].args must have at most {} entries",
                Self::MAX_ARGS
            )));
        }
        for arg in &self.args {
            if arg.len() > Self::MAX_ARG_BYTES || arg.contains('\0') {
                return Err(invalid(format!(
                    "[sources.narumi].args entries must be at most {} bytes and must not contain NUL",
                    Self::MAX_ARG_BYTES
                )));
            }
        }
        if self.env.len() > Self::MAX_ENV {
            return Err(invalid(format!(
                "[sources.narumi].env must have at most {} entries",
                Self::MAX_ENV
            )));
        }
        for (key, value) in &self.env {
            let mut chars = key.chars();
            let valid_key = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid_key {
                return Err(invalid(format!(
                    "[sources.narumi].env key `{key}` must match [A-Za-z_][A-Za-z0-9_]*"
                )));
            }
            if key.starts_with("GAIA_") {
                return Err(invalid(format!(
                    "[sources.narumi].env key `{key}` must not use the GAIA_ prefix"
                )));
            }
            if value.contains('\0') {
                return Err(invalid(format!(
                    "[sources.narumi].env value for `{key}` must not contain NUL"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("[sources] is invalid: {0}")]
    InvalidSource(String),
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
    #[error("client name must not be empty")]
    EmptyClientName,
    // 制御文字入りの名前は設定ファイル・通知・接続設定の表示を壊すため、書き込み系 API で拒否する
    //（名前をそのまま echo しない）。既存設定の load は拒否しない。
    #[error("client name must not contain control characters")]
    InvalidClientName,
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
    Ok(data_dir_with(lookup)?.join(APP_DIR).join("gaia.db"))
}

pub fn db_path(config: &Config) -> Result<PathBuf, ConfigError> {
    db_path_with(config, &|k| std::env::var_os(k))
}

/// XDG のデータディレクトリ（`XDG_DATA_HOME`。未設定・空なら `~/.local/share`）。
fn data_dir_with(lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    match lookup("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        Some(x) => Ok(PathBuf::from(x)),
        None => Ok(home_dir(lookup)?.join(".local").join("share")),
    }
}

/// デスクトップが平文 API キーを Keychain へ保存できないときの退避ディレクトリ
/// （`<XDG_DATA_HOME|~/.local/share>/gaia-library/keys`）。`GAIA_DB` や `db_path` の影響を受けない。
/// file 解決器の常時拒否領域として CLI / desktop の両方がこの値を使う（存在しなくてよい）。
pub fn key_store_dir_with(lookup: Lookup<'_>) -> Result<PathBuf, ConfigError> {
    Ok(data_dir_with(lookup)?.join(APP_DIR).join("keys"))
}

pub fn key_store_dir() -> Result<PathBuf, ConfigError> {
    key_store_dir_with(&|k| std::env::var_os(k))
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
        // lock 取得はリンク先の親ディレクトリと `.lock` を作るため、既存パス（dangling symlink を含む）は
        // lock を取る前に拒否し、lock 下でも再検査する。
        ensure_absent(path)?;
        let _lock = ConfigFileLock::acquire(path)?;
        ensure_absent(path)?;
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
        // 到達できない設定（dangling symlink など）は lock を取る前に読み取りエラーにし、
        // リンク先の親ディレクトリや `.lock` を残さない。
        fs::metadata(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
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
        // rename は symlink 自体を通常ファイルへ置き換えるため、鎖を辿った最終ターゲットを置換先にする。
        // 一時ファイルも同じディレクトリに作り、rename が同一ファイルシステム内で完結するようにする。
        let destination = if replace {
            resolve_write_target(path)?
        } else {
            path.to_path_buf()
        };
        let (mut temporary, temporary_path) = create_temporary_file(&destination)?;
        let result = (|| {
            temporary.write_all(text.as_bytes())?;
            temporary.sync_all()?;
            drop(temporary);
            if replace {
                fs::rename(&temporary_path, &destination)
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
        if client.name.chars().any(char::is_control) {
            return Err(ConfigError::InvalidClientName);
        }
        if self.clients.iter().any(|c| c.name == client.name) {
            return Err(ConfigError::DuplicateClient(client.name));
        }
        self.clients.push(client);
        Ok(())
    }

    /// クライアント名を変更する。role / default_scope は変えず、`[cli].default_client` と `[keys]` の
    /// 参照だけを新名へ付け替える（キーのハッシュは同じなので HTTP のキーは有効なまま）。
    /// DB の履歴（proposals の proposed_by / decided_by、audit_log の actor）は書き換えない。
    /// 呼び出しは `Config::update` の lock 内で行うこと。
    pub fn rename_client(&mut self, old: &str, new: &str) -> Result<(), ConfigError> {
        let new = new.trim();
        if new.is_empty() {
            return Err(ConfigError::EmptyClientName);
        }
        if new.chars().any(char::is_control) {
            return Err(ConfigError::InvalidClientName);
        }
        if self.clients.iter().any(|c| c.name == new) {
            return Err(ConfigError::DuplicateClient(new.to_owned()));
        }
        let client = self
            .clients
            .iter_mut()
            .find(|c| c.name == old)
            .ok_or_else(|| ConfigError::UnknownClient(old.to_owned()))?;
        client.name = new.to_owned();
        if self.cli.default_client.as_deref() == Some(old) {
            self.cli.default_client = Some(new.to_owned());
        }
        if let Some(hash) = self.keys.remove(old) {
            self.keys.insert(new.to_owned(), hash);
        }
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
        self.sources.validate()?;
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
    /// lock file は symlink の別名ではなく解決後のターゲットの兄弟に置き、
    /// 別名経由の並行更新も同じ lock で直列化する。
    fn acquire(config_path: &Path) -> Result<Self, ConfigError> {
        let target = resolve_write_target(config_path)?;
        ensure_parent(&target)?;
        let lock_path = sibling_path(&target, ".lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // lock file が symlink に差し替えられていても、その先を開いて権限を変えない。
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
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

/// 既存パス（dangling symlink を含む）があれば `AlreadyExists` を返す。
fn ensure_absent(path: &Path) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(ConfigError::AlreadyExists(path.to_path_buf())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }),
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

/// 保存先が symlink の場合はリンクを残し、鎖を辿った最終ターゲット（未作成でもよい）を返す。
/// 上限を超える鎖（ループを含む）、読めないリンク、他ユーザー所有のリンク、
/// 通常ファイル以外の既存ターゲットは `Write` エラーとして報告する。
fn resolve_write_target(path: &Path) -> Result<PathBuf, ConfigError> {
    let write_error = |source: io::Error| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut target = path.to_path_buf();
    // 1 段ごとに 1 反復、最終ターゲットの確認に 1 反復使うので、MAX_SYMLINK_DEPTH 段までは辿れる。
    for _ in 0..=MAX_SYMLINK_DEPTH {
        match fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                ensure_owned_by_current_user(&target, &metadata).map_err(write_error)?;
                let link = fs::read_link(&target).map_err(write_error)?;
                target = if link.is_absolute() {
                    link
                } else {
                    target.parent().unwrap_or_else(|| Path::new(".")).join(link)
                };
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(write_error(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} is not a regular file", target.display()),
                )));
            }
            Ok(_) => return Ok(target),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(target),
            Err(error) => return Err(write_error(error)),
        }
    }
    Err(write_error(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("too many levels of symbolic links (more than {MAX_SYMLINK_DEPTH})"),
    )))
}

/// 手動で辿る symlink には OS の追従抑止（Linux の fs.protected_symlinks など）が効かないため、
/// 実効ユーザー以外が所有するリンクは辿らない。
#[cfg(unix)]
fn ensure_owned_by_current_user(link: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    // SAFETY: geteuid は引数を取らず、常に成功する。
    let current = unsafe { libc::geteuid() };
    let owner = metadata.uid();
    if owner == current {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "symlink {} is owned by uid {owner}, not the current user (uid {current}); refusing to follow it",
            link.display()
        ),
    ))
}

#[cfg(not(unix))]
fn ensure_owned_by_current_user(_link: &Path, _metadata: &fs::Metadata) -> io::Result<()> {
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

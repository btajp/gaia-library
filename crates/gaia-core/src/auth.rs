//! API キー認証の材料。仕様書 B §4.1。平文キーは保存せず、config の [keys] に SHA-256 hex だけを置く。
use std::path::{Path, PathBuf};

use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    config::{Config, ConfigError},
    identity::ClientIdentity,
};

const KEY_PREFIX_MAX_LENGTH: usize = 64;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.is_ascii() || s.len() != 64 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

pub fn hash_key(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

/// 平文キー `gaia_<safe-prefix>_<32hex>` と SHA-256 hex を返す。prefix は識別には使わない。
/// 平文は発行時に 1 度だけ表示する。
pub fn generate_key(name: &str) -> (String, String) {
    let mut raw = [0u8; 16];
    rand::rng().fill_bytes(&mut raw);
    let plaintext = format!("gaia_{}_{}", key_prefix(name), hex(&raw));
    let hash = hash_key(&plaintext);
    (plaintext, hash)
}

fn key_prefix(name: &str) -> String {
    // Bearer token の中間に使える ASCII だけを残す。元の ClientIdentity.name は変更しない。
    let prefix: String = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || "-._~+/".contains(*character))
        .take(KEY_PREFIX_MAX_LENGTH)
        .collect();
    if prefix.is_empty() {
        "client".into()
    } else {
        prefix
    }
}

enum AuthSource {
    Snapshot(Vec<(Vec<u8>, ClientIdentity)>),
    ConfigPath(PathBuf),
}

/// `[keys]` と `[[clients]]` を突合した認証表。
pub struct AuthTable {
    source: AuthSource,
}

impl AuthTable {
    pub fn from_config(config: &Config) -> Self {
        Self {
            source: AuthSource::Snapshot(entries_from_config(config)),
        }
    }

    /// 起動中の keygen / rotation を次の認証から反映する設定ファイル追従モード。
    /// 読み直しに失敗したリクエストは fail-closed で認証失敗にする。
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref().to_path_buf();
        Config::load(&path)?;
        Ok(Self {
            source: AuthSource::ConfigPath(path),
        })
    }

    pub fn is_empty(&self) -> bool {
        match &self.source {
            AuthSource::Snapshot(entries) => entries.is_empty(),
            AuthSource::ConfigPath(path) => Config::load(path)
                .map(|config| entries_from_config(&config).is_empty())
                .unwrap_or(true),
        }
    }

    /// 全エントリと constant-time 比較する（早期 return しない）。
    pub fn verify(&self, bearer: &str) -> Option<ClientIdentity> {
        let candidate = Sha256::digest(bearer.as_bytes());
        match &self.source {
            AuthSource::Snapshot(entries) => verify_entries(entries, candidate.as_slice()),
            AuthSource::ConfigPath(path) => {
                let config = Config::load(path).ok()?;
                verify_entries(&entries_from_config(&config), candidate.as_slice())
            }
        }
    }
}

fn entries_from_config(config: &Config) -> Vec<(Vec<u8>, ClientIdentity)> {
    if config.validate().is_err() {
        return Vec::new();
    }
    config
        .keys
        .iter()
        .filter_map(|(name, hash)| Some((decode_hex(hash)?, config.client(name)?.clone())))
        .collect()
}

fn verify_entries(
    entries: &[(Vec<u8>, ClientIdentity)],
    candidate: &[u8],
) -> Option<ClientIdentity> {
    let mut found = None;
    for (hash, identity) in entries {
        let matches = hash.len() == candidate.len() && bool::from(hash.as_slice().ct_eq(candidate));
        if matches {
            found = Some(identity);
        }
    }
    found.cloned()
}

#[cfg(test)]
mod tests;

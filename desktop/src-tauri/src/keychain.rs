//! 平文キーは OS 保管を優先し、失敗時だけ private なファイルへ保存する。
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

use gaia_core::{
    auth::hash_key,
    config::{APP_DIR, key_store_dir_with},
};
use rustix::{
    fs::{self, AtFlags, FileType, Mode, OFlags},
    io::Errno,
};
use serde::Serialize;
use uuid::Uuid;

const MAX_KEY_BYTES: usize = 16 * 1024;
const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::CLOEXEC)
    .union(OFlags::NOFOLLOW);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreLocation {
    Keychain,
    File,
}

trait KeyBackend {
    fn store(&self, client: &str, plaintext: &str) -> Result<(), ()>;
    fn load(&self, client: &str) -> Result<Option<String>, ()>;
    /// 項目が無い場合も成功として扱う。
    fn delete(&self, client: &str) -> Result<(), ()>;
}

struct SystemKeychain;

impl KeyBackend for SystemKeychain {
    fn store(&self, client: &str, plaintext: &str) -> Result<(), ()> {
        keyring::Entry::new(APP_DIR, client)
            .and_then(|entry| entry.set_password(plaintext))
            .map_err(|_| ())
    }

    fn load(&self, client: &str) -> Result<Option<String>, ()> {
        match keyring::Entry::new(APP_DIR, client).and_then(|entry| entry.get_password()) {
            Ok(plaintext) => Ok(Some(plaintext)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(()),
        }
    }

    fn delete(&self, client: &str) -> Result<(), ()> {
        match keyring::Entry::new(APP_DIR, client).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(()),
        }
    }
}

pub fn store_key(client: &str, plaintext: &str) -> Result<StoreLocation, String> {
    store_with(client, plaintext, &SystemKeychain, &|name| {
        std::env::var_os(name)
    })
}

pub fn load_key(client: &str) -> Result<Option<(String, StoreLocation)>, String> {
    load_with(client, None, &SystemKeychain, &|name| {
        std::env::var_os(name)
    })
}

/// 認証設定と一致するキーだけを返す。古い Keychain の値で File を隠さない。
pub fn load_matching_key(
    client: &str,
    expected_hash: &str,
) -> Result<Option<(String, StoreLocation)>, String> {
    load_with(client, Some(expected_hash), &SystemKeychain, &|name| {
        std::env::var_os(name)
    })
}

/// クライアント名の変更に合わせて保管キーを付け替える。Keychain の account もファイル名（名前の SHA-256）も
/// クライアント名から決まるため、旧名で保管された現在のキー（`expected_hash` と一致するもの）だけを
/// 新名で保存し直し、旧名の保管を消す。一致するキーが無ければ何もせず `None` を返す。
pub fn move_key(
    old: &str,
    new: &str,
    expected_hash: &str,
) -> Result<Option<StoreLocation>, String> {
    move_with(old, new, expected_hash, &SystemKeychain, &|name| {
        std::env::var_os(name)
    })
}

type Lookup<'a> = &'a dyn Fn(&str) -> Option<OsString>;

fn move_with(
    old: &str,
    new: &str,
    expected_hash: &str,
    backend: &dyn KeyBackend,
    lookup: Lookup<'_>,
) -> Result<Option<StoreLocation>, String> {
    let Some((plaintext, _)) = load_with(old, Some(expected_hash), backend, lookup)? else {
        return Ok(None);
    };
    let location = store_with(new, &plaintext, backend, lookup)?;
    // 新名で保存できてから旧名を消す。旧名の削除に失敗しても新名の保管は有効で、
    // 旧名に残る項目は現在のハッシュと照合されるため接続設定へは出ない。
    let _ = backend.delete(old);
    if let Ok(root) = fallback_root(lookup) {
        let _ = remove_file(&root, old);
    }
    Ok(Some(location))
}

fn store_with(
    client: &str,
    plaintext: &str,
    backend: &dyn KeyBackend,
    lookup: Lookup<'_>,
) -> Result<StoreLocation, String> {
    if plaintext.is_empty() || plaintext.len() > MAX_KEY_BYTES {
        return Err("保存するキーの長さが不正です".into());
    }
    if backend.store(client, plaintext).is_ok() {
        return Ok(StoreLocation::Keychain);
    }
    let root = fallback_root(lookup)?;
    write_file(&root, client, plaintext)?;
    Ok(StoreLocation::File)
}

fn load_with(
    client: &str,
    expected_hash: Option<&str>,
    backend: &dyn KeyBackend,
    lookup: Lookup<'_>,
) -> Result<Option<(String, StoreLocation)>, String> {
    if let Some(expected) = expected_hash
        && (expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("設定されたキーハッシュが不正です".into());
    }
    let matches = |plaintext: &str| {
        !plaintext.is_empty()
            && plaintext.len() <= MAX_KEY_BYTES
            && expected_hash.is_none_or(|hash| hash_key(plaintext).eq_ignore_ascii_case(hash))
    };
    let keychain = backend.load(client);
    if let Ok(Some(plaintext)) = &keychain
        && matches(plaintext)
    {
        return Ok(Some((plaintext.clone(), StoreLocation::Keychain)));
    }
    if let Some(plaintext) = read_file(&fallback_root(lookup)?, client)?
        && matches(&plaintext)
    {
        return Ok(Some((plaintext, StoreLocation::File)));
    }
    if keychain.is_err() {
        Err("キーチェーンを読み取れず、一致するファイル保管のキーも見つかりません".into())
    } else {
        Ok(None)
    }
}

/// キー退避ディレクトリ。位置の算出は gaia-core の `config::key_store_dir_with` と共有し（CLI の file 解決器も
/// 同じ値を常時拒否する）、ここでは書き込み先として使える形かだけを検査する。
pub(crate) fn fallback_root(lookup: Lookup<'_>) -> Result<PathBuf, String> {
    let filtered = |name: &str| lookup(name).filter(|value| !value.is_empty());
    let root = key_store_dir_with(&filtered)
        .map_err(|_| "HOME が未設定のためキー保管先を決定できません".to_string())?;
    if !root.is_absolute()
        || root
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("キー保管先には親参照を含まない絶対パスを指定してください".into());
    }
    Ok(root)
}

fn key_filename(client: &str) -> String {
    format!("{}.key", hash_key(client))
}

/// 各ディレクトリを fd 相対 + NOFOLLOW で開き、途中の symlink も拒否する。
fn open_directory(root: &Path, create: bool) -> Result<Option<File>, String> {
    let mut directory = fs::open("/", DIRECTORY_FLAGS, Mode::empty())
        .map_err(|error| format!("キー保管先を開けません: {error}"))?;
    for component in root.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        directory = match fs::openat(&directory, name, DIRECTORY_FLAGS, Mode::empty()) {
            Ok(directory) => directory,
            Err(Errno::NOENT) if create => {
                match fs::mkdirat(&directory, name, Mode::from_raw_mode(0o700)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(error) => return Err(format!("キー保管先を作成できません: {error}")),
                }
                fs::openat(&directory, name, DIRECTORY_FLAGS, Mode::empty())
                    .map_err(|error| format!("キー保管先を開けません: {error}"))?
            }
            Err(Errno::NOENT) => return Ok(None),
            Err(error) => return Err(format!("キー保管先を安全に開けません: {error}")),
        };
    }
    let directory = File::from(directory);
    if create {
        fs::fchmod(&directory, Mode::from_raw_mode(0o700))
            .map_err(|error| format!("キー保管先の権限を設定できません: {error}"))?;
    } else if directory
        .metadata()
        .map_err(|error| error.to_string())?
        .mode()
        & 0o077
        != 0
    {
        return Err("キー保管ディレクトリの権限を 0700 にしてください".into());
    }
    Ok(Some(directory))
}

fn validate_destination(directory: &File, name: &str) -> Result<(), String> {
    match fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) => Ok(()),
        Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile => Ok(()),
        Ok(_) => Err("キー保存先が通常ファイルではないため保存できません".into()),
        Err(error) => Err(format!("キー保存先を確認できません: {error}")),
    }
}

fn write_file(root: &Path, client: &str, plaintext: &str) -> Result<(), String> {
    let directory = open_directory(root, true)?
        .ok_or_else(|| "キー保管ディレクトリがありません".to_string())?;
    let name = key_filename(client);
    validate_destination(&directory, &name)?;
    let temporary_name = format!(".gaia-key-{}", Uuid::new_v4());
    let fd = fs::openat(
        &directory,
        temporary_name.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|error| format!("キーの一時ファイルを作成できません: {error}"))?;
    let temporary = TemporaryKey {
        directory: &directory,
        name: temporary_name,
    };
    let mut file = File::from(fd);
    fs::fchmod(&file, Mode::from_raw_mode(0o600))
        .map_err(|error| format!("キーの権限を設定できません: {error}"))?;
    file.write_all(plaintext.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("キーを保存できません: {error}"))?;
    validate_destination(&directory, &name)?;
    fs::renameat(
        &directory,
        temporary.name.as_str(),
        &directory,
        name.as_str(),
    )
    .map_err(|error| format!("キーを原子的に保存できません: {error}"))?;
    directory
        .sync_all()
        .map_err(|error| format!("キー保管先を同期できません: {error}"))
}

struct TemporaryKey<'a> {
    directory: &'a File,
    name: String,
}

impl Drop for TemporaryKey<'_> {
    fn drop(&mut self) {
        let _ = fs::unlinkat(self.directory, self.name.as_str(), AtFlags::empty());
    }
}

fn remove_file(root: &Path, client: &str) -> Result<(), String> {
    let Some(directory) = open_directory(root, false)? else {
        return Ok(());
    };
    match fs::unlinkat(&directory, key_filename(client), AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => Ok(()),
        Err(error) => Err(format!("旧名の保管キーを削除できません: {error}")),
    }
}

fn read_file(root: &Path, client: &str) -> Result<Option<String>, String> {
    let Some(directory) = open_directory(root, false)? else {
        return Ok(None);
    };
    let fd = match fs::openat(
        &directory,
        key_filename(client),
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(error) => return Err(format!("保管キーを安全に開けません: {error}")),
    };
    let file = File::from(fd);
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o077 != 0 {
        return Err(
            "保管キーは他の利用者に公開されていない通常ファイルである必要があります".into(),
        );
    }
    let mut bytes = Vec::new();
    file.take((MAX_KEY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("保管キーを読み取れません: {error}"))?;
    if bytes.len() > MAX_KEY_BYTES {
        return Err("保管キーが長すぎます".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "保管キーの文字コードが不正です".into())
}

#[cfg(test)]
mod tests;

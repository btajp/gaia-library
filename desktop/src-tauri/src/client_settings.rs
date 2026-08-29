//! クライアント設定と接続設定。平文キーをログや設定 TOML へ渡さない。
use std::path::Path;

use gaia_core::{
    auth::{self, AuthTable},
    config::{Config, ConfigError},
    identity::{ClientIdentity, Role},
};
use serde::Serialize;
use serde_json::json;

use crate::keychain::{self, MovedKey, StoreLocation};

#[derive(Serialize)]
pub(crate) struct ClientSummary {
    pub name: String,
    pub role: Role,
    pub default_scope: Option<String>,
    pub has_key: bool,
}

#[derive(Serialize)]
pub(crate) struct KeyStorage {
    pub location: Option<StoreLocation>,
    pub error: Option<String>,
}

// 秘密を含むため Debug を実装しない。
#[derive(Serialize)]
pub(crate) struct IssuedKey {
    pub key: String,
    pub storage: KeyStorage,
}

#[derive(Serialize)]
pub(crate) struct ConnectionSnippet {
    pub text: String,
    pub key_storage: Option<StoreLocation>,
}

#[derive(Serialize)]
pub(crate) struct RenamedClient {
    pub name: String,
    /// 旧名で保管していた現在のキーを新名へ移した場合の保管場所。キー無し・保管無しなら None。
    pub key_moved: Option<StoreLocation>,
    /// 保管キーの付け替えに失敗した場合、または旧名の保管項目を削除できず有効なキーが
    /// 旧名で残っている場合の案内（秘密を含まない）。
    pub key_error: Option<String>,
}

pub(crate) fn list(path: &Path) -> Result<Vec<ClientSummary>, String> {
    let config = Config::load(path).map_err(|e| e.to_string())?;
    Ok(config
        .clients
        .iter()
        .map(|client| ClientSummary {
            name: client.name.clone(),
            role: client.role,
            default_scope: client.default_scope.clone(),
            has_key: config.keys.contains_key(&client.name),
        })
        .collect())
}

pub(crate) fn add(
    path: &Path,
    name: &str,
    role: Role,
    default_scope: Option<&str>,
    generate_key: bool,
) -> Result<Option<IssuedKey>, String> {
    add_with(
        path,
        name,
        role,
        default_scope,
        generate_key,
        keychain::store_key,
    )
}

fn add_with(
    path: &Path,
    name: &str,
    role: Role,
    default_scope: Option<&str>,
    generate_key: bool,
    store: impl FnOnce(&str, &str) -> Result<StoreLocation, String>,
) -> Result<Option<IssuedKey>, String> {
    let name = valid_name(name)?;
    let default_scope = default_scope
        .map(str::trim)
        .filter(|scope| !scope.is_empty());
    // CLI の `gaia client add` と同じ lock で read-modify-write を直列化し、並行更新を失わない。
    let key = Config::update(path, |config| {
        config.add_client(ClientIdentity {
            name: name.into(),
            role,
            default_scope: default_scope.map(str::to_owned),
        })?;
        Ok(generate_key.then(|| {
            let (key, hash) = auth::generate_key(name);
            config.keys.insert(name.into(), hash);
            key
        }))
    })
    .map_err(|error| error.to_string())?;
    Ok(key.map(|key| IssuedKey {
        storage: store_with(name, &key, store),
        key,
    }))
}

pub(crate) fn keygen(path: &Path, name: &str) -> Result<IssuedKey, String> {
    keygen_with(path, name, keychain::store_key)
}

fn keygen_with(
    path: &Path,
    name: &str,
    store: impl FnOnce(&str, &str) -> Result<StoreLocation, String>,
) -> Result<IssuedKey, String> {
    // CLI の `gaia client keygen` と同じ lock で直列化する。旧キーは保存成功時に失効する。
    // 対象クライアントの不在は closure 内の判定だけを UI 文言にし、読み込み・検証で出た
    // 同名の UnknownClient（default_client や [keys] が指す未登録名）は設定の異常として伝える。
    let mut missing = false;
    let key = Config::update(path, |config| {
        if config.client(name).is_none() {
            missing = true;
            return Err(ConfigError::UnknownClient(name.into()));
        }
        let (key, hash) = auth::generate_key(name);
        config.keys.insert(name.into(), hash);
        Ok(key)
    })
    .map_err(|error| {
        if missing {
            "指定されたクライアントがありません".into()
        } else {
            error.to_string()
        }
    })?;
    Ok(IssuedKey {
        storage: store_with(name, &key, store),
        key,
    })
}

pub(crate) fn rename(path: &Path, old: &str, new: &str) -> Result<RenamedClient, String> {
    rename_with(path, old, new, keychain::move_key)
}

fn rename_with(
    path: &Path,
    old: &str,
    new: &str,
    move_key: impl FnOnce(&str, &str, &str) -> Result<Option<MovedKey>, String>,
) -> Result<RenamedClient, String> {
    let new = valid_name(new)?;
    // CLI の `gaia client rename` と同じ lock で直列化する。設定ファイルの参照（[[clients]].name、
    // [cli].default_client、[keys]）だけを付け替え、DB の履歴（proposed_by / decided_by / actor）は書き換えない。
    let mut missing = false;
    let hash = Config::update(path, |config| {
        if config.client(old).is_none() {
            missing = true;
            return Err(ConfigError::UnknownClient(old.into()));
        }
        config.rename_client(old, new)?;
        Ok(config.keys.get(new).cloned())
    })
    .map_err(|error| match error {
        ConfigError::UnknownClient(_) if missing => "指定されたクライアントがありません".into(),
        ConfigError::DuplicateClient(_) => "同じ名前のクライアントが既にあります".into(),
        error => error.to_string(),
    })?;
    // 保管キーはクライアント名を鍵にしているため、設定の保存後に新名へ移す。
    let (key_moved, key_error) = match hash.map(|hash| move_key(old, new, &hash)) {
        None | Some(Ok(None)) => (None, None),
        Some(Ok(Some(moved))) => (
            Some(moved.location),
            // 移動はできたが旧名の項目を消せなかった場合は、有効なキーが旧名で残っている
            // ことを警告する（接続設定には現在名＋現在ハッシュ照合のため表示されない）。
            (!moved.old_removed).then(|| {
                "保管キーを新しい名前へ移しましたが、以前の名前の保管項目を削除できませんでした。Keychain の gaia-library 項目とキー退避ファイルを確認してください。"
                    .into()
            }),
        ),
        Some(Err(_)) => (
            None,
            Some(
                "保管中のキーを新しい名前へ移せませんでした。HTTP 接続設定を再表示するにはキーを再発行してください。"
                    .into(),
            ),
        ),
    };
    Ok(RenamedClient {
        name: new.into(),
        key_moved,
        key_error,
    })
}

pub(crate) fn store_key(client: &str, key: &str) -> KeyStorage {
    store_with(client, key, keychain::store_key)
}

fn store_with(
    client: &str,
    key: &str,
    store: impl FnOnce(&str, &str) -> Result<StoreLocation, String>,
) -> KeyStorage {
    match store(client, key) {
        Ok(location) => KeyStorage {
            location: Some(location),
            error: None,
        },
        Err(_) => KeyStorage {
            location: None,
            // 保存先の失敗で、発行済みのキーまで UI から失わない。
            error: Some(
                "キーを保管できませんでした。この画面を閉じる前に安全な場所へコピーしてください。接続設定の再表示には再発行が必要です。"
                    .into(),
            ),
        },
    }
}

fn valid_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("クライアント名を入力してください。制御文字は使えません".into());
    }
    Ok(name)
}

pub(crate) struct SnippetPaths<'a> {
    pub config: &'a Path,
    pub db: &'a Path,
    pub cli: &'a Path,
}

pub(crate) fn snippet(
    paths: SnippetPaths<'_>,
    name: &str,
    transport: &str,
    active_url: Option<&str>,
) -> Result<ConnectionSnippet, String> {
    let config = Config::load(paths.config).map_err(|e| e.to_string())?;
    snippet_with(
        &config,
        paths,
        name,
        transport,
        active_url,
        keychain::load_matching_key,
    )
}

fn snippet_with(
    config: &Config,
    paths: SnippetPaths<'_>,
    name: &str,
    transport: &str,
    active_url: Option<&str>,
    load: impl FnOnce(&str, &str) -> Result<Option<(String, StoreLocation)>, String>,
) -> Result<ConnectionSnippet, String> {
    if config.client(name).is_none() {
        return Err("指定されたクライアントがありません".into());
    }
    let (entry, key_storage) = match transport {
        "stdio" => (
            json!({
                "command": utf8_absolute(paths.cli)?,
                "args": ["--config", utf8_absolute(paths.config)?, "serve", "--stdio", "--client", name],
                "env": {"GAIA_DB": utf8_absolute(paths.db)?}
            }),
            None,
        ),
        "http" => {
            let url =
                active_url.ok_or("HTTP サーバーが停止中です。サーバー状態を確認してください")?;
            let hash = config.keys.get(name).ok_or("キーを発行してください")?;
            let (key, location) =
                load(name, hash)?.ok_or("現在のキーを復元できません。キーを再発行してください")?;
            if AuthTable::from_config(config)
                .verify(&key)
                .is_none_or(|identity| identity.name != name)
            {
                return Err("保管されたキーは現在の設定と一致しません。再発行してください".into());
            }
            (
                json!({
                    "type": "http",
                    "url": url,
                    "headers": {"Authorization": format!("Bearer {key}")}
                }),
                Some(location),
            )
        }
        _ => return Err("接続方式は stdio または http を指定してください".into()),
    };
    Ok(ConnectionSnippet {
        text: serde_json::to_string_pretty(&json!({"mcpServers": {"gaia_library": entry}}))
            .map_err(|_| "接続設定を作成できません".to_string())?,
        key_storage,
    })
}

fn utf8_absolute(path: &Path) -> Result<String, String> {
    std::path::absolute(path)
        .map_err(|e| e.to_string())?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "接続設定のパスは UTF-8 である必要があります".into())
}

#[cfg(test)]
mod tests;

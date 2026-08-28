//! クライアント設定と接続設定。平文キーをログや設定 TOML へ渡さない。
use std::path::Path;

use gaia_core::{
    auth::{self, AuthTable},
    config::{Config, ConfigError},
    identity::{ClientIdentity, Role},
};
use serde::Serialize;
use serde_json::json;

use crate::keychain::{self, StoreLocation};

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
    .map_err(|error| update_error(name, error))?;
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
    let key = Config::update(path, |config| {
        if config.client(name).is_none() {
            return Err(ConfigError::UnknownClient(name.into()));
        }
        let (key, hash) = auth::generate_key(name);
        config.keys.insert(name.into(), hash);
        Ok(key)
    })
    .map_err(|error| update_error(name, error))?;
    Ok(IssuedKey {
        storage: store_with(name, &key, store),
        key,
    })
}

/// 操作対象のクライアントが無い場合だけ UI 向けの文言にし、設定ファイル自体の異常はそのまま伝える。
fn update_error(name: &str, error: ConfigError) -> String {
    match error {
        ConfigError::UnknownClient(unknown) if unknown == name => {
            "指定されたクライアントがありません".into()
        }
        error => error.to_string(),
    }
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

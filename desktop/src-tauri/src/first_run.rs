//! 初回設定。DB の準備が完了してから設定を新規公開し、既存設定は上書きしない。
//! DB の初期化と設定公開は CLI の `gaia init` と同じ lock 内で行う。
use std::{fs, io, path::Path, sync::Arc};

use gaia_core::{
    admin,
    auth::generate_key,
    config::{CliConfig, Config, ConfigError},
    contracts::Catalog,
    identity::{ClientIdentity, Role},
    storage::Db,
    tools::ToolService,
};

use crate::state::{AppState, SetupResponse};

pub(crate) const AGENT_CLIENT: &str = "claude-code";

/// 設定公開と同じ lock 内で実行する DB 初期化。設定保存が失敗しても再実行できること。
type Initializer<'a> = Box<dyn FnOnce() -> Result<Db, String> + 'a>;

/// dangling symlink や読み取りエラーを「未設定」として扱わない。
pub(crate) fn config_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("設定の存在確認に失敗しました: {error}")),
    }
}

pub(crate) fn setup(
    config_path: &Path,
    db_path: &Path,
    affiliation: &str,
    user_name: &str,
) -> Result<(AppState, SetupResponse), String> {
    setup_with_publisher(config_path, db_path, affiliation, user_name, publish_config)
}

fn setup_with_publisher(
    config_path: &Path,
    db_path: &Path,
    affiliation: &str,
    user_name: &str,
    publish: impl FnOnce(&Config, &Path, Initializer<'_>) -> Result<Db, String>,
) -> Result<(AppState, SetupResponse), String> {
    let affiliation = required(affiliation, "所属名")?;
    let user_name = required(user_name, "ユーザー名")?;
    if config_exists(config_path)? {
        return Err("設定が既にあります。初回設定では上書きできません".into());
    }

    let human = ClientIdentity {
        name: format!("desktop:{user_name}"),
        role: Role::Human,
        default_scope: Some(affiliation.into()),
    };
    let mut config = Config {
        db_path: Some(db_path.to_path_buf()),
        cli: CliConfig {
            default_client: Some(human.name.clone()),
        },
        ..Config::default()
    };
    config
        .add_client(human.clone())
        .map_err(|e| e.to_string())?;
    config
        .add_client(ClientIdentity {
            name: AGENT_CLIENT.into(),
            role: Role::Agent,
            default_scope: Some(affiliation.into()),
        })
        .map_err(|e| e.to_string())?;
    let (agent_key, hash) = generate_key(AGENT_CLIENT);
    config.keys.insert(AGENT_CLIENT.into(), hash);

    let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
    // DB の初期化（所属追加と audit_log）は設定公開と同じ lock 内で行い、設定が既にある場合や
    // CLI の `gaia init` に負けた場合に、この初期設定を actor とする書き込みを DB へ残さない。
    let db = publish(
        &config,
        config_path,
        Box::new(|| {
            let db = Db::open(db_path).map_err(|e| e.to_string())?;
            // 設定保存だけが失敗した前回の DB でも、成功済みの所属追加は繰り返さない。
            let affiliations = admin::list_affiliations(&db).map_err(|e| e.to_string())?;
            if !affiliations.iter().any(|item| item.name == affiliation) {
                admin::add_affiliation(&db, &human.name, affiliation, None)
                    .map_err(|e| e.to_string())?;
            }
            Ok(db)
        }),
    )?;

    let app_state = AppState::new(
        Arc::new(ToolService::new(db, catalog)),
        human,
        config_path.to_path_buf(),
        db_path.to_path_buf(),
    );
    // 平文キーは保存成功後にだけ返し、状態やログには保持しない。
    Ok((app_state, SetupResponse { agent_key }))
}

fn required<'a>(value: &'a str, label: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{label}を入力してください"))
    } else {
        Ok(value)
    }
}

enum PublishError {
    Config(ConfigError),
    Initialize(String),
}

impl From<ConfigError> for PublishError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

fn publish_config(config: &Config, path: &Path, initialize: Initializer<'_>) -> Result<Db, String> {
    // CLI の `gaia init` や設定更新と同じ兄弟 lock file で DB 初期化から公開までを直列化し、
    // 存在する通常ファイル・リンク（dangling symlink を含む）は置き換えず、初期化も実行しない。
    config
        .create_with(path, || initialize().map_err(PublishError::Initialize))
        .map_err(|error| match error {
            PublishError::Config(ConfigError::AlreadyExists(_)) => {
                "設定が既にあります。初回設定では上書きできません".into()
            }
            PublishError::Config(error) => format!("初期設定の保存に失敗しました: {error}"),
            PublishError::Initialize(message) => message,
        })
}

#[cfg(test)]
mod tests;

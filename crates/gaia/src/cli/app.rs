//! 起動処理: 設定ロード → DB オープン → ToolService 構築 → 識別解決。
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use serde_json::Value;

use gaia_core::{
    config::{self, CliConfig, Config, ConfigError},
    contracts::Catalog,
    error::ToolError,
    identity::{ClientIdentity, Role},
    sources::ProtectedPaths,
    storage::Db,
    tools::ToolService,
};

pub struct App {
    /// HTTP 認証が起動中に再読込する設定ファイル。
    pub config_path: PathBuf,
    pub config: Config,
    pub service: ToolService,
}

impl App {
    pub fn open(config_override: Option<&PathBuf>) -> anyhow::Result<Self> {
        let config_path = resolve_config_path(config_override)?;
        if !config_path.exists() {
            bail!(
                "設定がありません: {} — まず `gaia init --affiliation <name>` を実行してください",
                config_path.display()
            );
        }
        let config = Config::load(&config_path)?;
        let db_path = config::db_path(&config)?;
        let db = Db::open(&db_path)?;
        let catalog = Catalog::embedded().context("contracts のロードに失敗")?;
        // resolve_source の解決器。設定は呼び出しごとに再読込し、保護領域は file 解決器が常時拒否する。
        let protected = protected_paths(&config_path, &db_path, &|k| std::env::var_os(k));
        let sources = gaia_mcp::sources::registry(&config_path, protected);
        Ok(Self {
            config_path,
            config,
            service: ToolService::new(db, catalog).with_sources(sources),
        })
    }

    pub fn identity(&self, name: Option<&str>) -> anyhow::Result<ClientIdentity> {
        Ok(self.config.resolve_client(name)?.clone())
    }

    pub fn call(
        &self,
        client: &ClientIdentity,
        tool: &str,
        args: Value,
    ) -> Result<Value, ToolError> {
        self.service.call(client, tool, args)
    }
}

/// file 解決器が常時拒否する領域: 設定・DB のディレクトリと、デスクトップが平文 API キーを退避する
/// ディレクトリ（同じ OS ユーザーのデスクトップと同じ場所。存在しなくてよい）。
fn protected_paths(
    config_path: &Path,
    db_path: &Path,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> ProtectedPaths {
    let mut protected = ProtectedPaths::new(
        config_path.parent().unwrap_or(Path::new("/")),
        db_path.parent().unwrap_or(Path::new("/")),
    );
    if let Ok(keys) = config::key_store_dir_with(lookup) {
        protected = protected.with_extra(keys);
    }
    protected
}

pub fn resolve_config_path(config_override: Option<&PathBuf>) -> anyhow::Result<PathBuf> {
    Ok(match config_override {
        Some(p) => p.clone(),
        None => config::config_path()?,
    })
}

pub fn print_json(value: &Value, compact: bool) {
    if compact {
        println!("{value}");
    } else {
        println!("{value:#}");
    }
}

pub fn init(
    args: &super::InitArgs,
    config_override: Option<&PathBuf>,
    cli_client: Option<&str>,
) -> anyhow::Result<()> {
    let config_path = resolve_config_path(config_override)?;
    if cli_client.is_some() && args.client_name.is_some() {
        return Err(ToolError::invalid_params(
            "--client と互換用の --client-name は同時に指定できません",
        )
        .into());
    }
    let client_name = cli_client
        .map(str::to_owned)
        .or_else(|| args.client_name.clone())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "me".to_string());
    let affiliation = args.affiliation.trim();
    if affiliation.is_empty() {
        return Err(ToolError::invalid_params("--affiliation は空にできません").into());
    }
    let mut config = Config {
        db_path: args.db.clone(),
        cli: CliConfig {
            default_client: Some(client_name.clone()),
        },
        ..Default::default()
    };
    config.add_client(ClientIdentity {
        name: client_name.clone(),
        role: Role::Human,
        default_scope: Some(affiliation.to_string()),
    })?;
    let db_path = config::db_path(&config)?;
    let result = config.create_with(&config_path, || -> anyhow::Result<()> {
        let db = Db::open(&db_path)?;
        gaia_core::admin::initialize_affiliation(
            &db,
            &client_name,
            affiliation,
            args.identity.as_deref(),
        )?;
        Ok(())
    });
    if let Err(error) = result {
        if matches!(
            error.downcast_ref::<ConfigError>(),
            Some(ConfigError::AlreadyExists(_))
        ) {
            bail!(
                "設定が既にあります: {} — affiliation は `gaia affiliation add`、クライアントは `gaia client add` を使ってください",
                config_path.display()
            );
        }
        return Err(error);
    }
    eprintln!("初期化しました:");
    eprintln!("  config: {}", config_path.display());
    eprintln!("  db:     {}", db_path.display());
    eprintln!(
        "  human client: {client_name} (default_scope={})",
        affiliation
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn protected_paths_include_config_db_and_desktop_key_store() {
        let config_path = Path::new("/cfg/gaia-library/config.toml");
        let db_path = Path::new("/data/gaia-library/gaia.db");
        let protected = protected_paths(config_path, db_path, &lookup(&[("HOME", "/h")]));
        assert_eq!(protected.config_dir, Path::new("/cfg/gaia-library"));
        assert_eq!(protected.db_dir, Path::new("/data/gaia-library"));
        assert_eq!(
            protected.extra,
            vec![PathBuf::from("/h/.local/share/gaia-library/keys")]
        );
        // XDG_DATA_HOME を尊重し、GAIA_DB で DB を別の場所に置いても退避ディレクトリは変わらない
        let protected = protected_paths(
            config_path,
            db_path,
            &lookup(&[
                ("XDG_DATA_HOME", "/xdg"),
                ("GAIA_DB", "/elsewhere/g.db"),
                ("HOME", "/h"),
            ]),
        );
        assert_eq!(
            protected.extra,
            vec![PathBuf::from("/xdg/gaia-library/keys")]
        );
        // HOME 無しでは退避ディレクトリを決められないので設定・DB のディレクトリだけになる
        let protected = protected_paths(config_path, db_path, &lookup(&[]));
        assert!(protected.extra.is_empty());
    }
}

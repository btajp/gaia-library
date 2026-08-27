//! 起動処理: 設定ロード → DB オープン → ToolService 構築 → 識別解決。
use std::path::PathBuf;

use anyhow::{Context, bail};
use serde_json::Value;

use gaia_core::{
    config::{self, CliConfig, Config, ConfigError},
    contracts::Catalog,
    error::ToolError,
    identity::{ClientIdentity, Role},
    storage::Db,
    tools::ToolService,
};

pub struct App {
    /// 現時点のコマンドでは未参照。今後の診断コマンド向けに公開インターフェースとして保持する。
    #[allow(dead_code)]
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
        let db = Db::open(&config::db_path(&config)?)?;
        let catalog = Catalog::embedded().context("contracts のロードに失敗")?;
        Ok(Self {
            config_path,
            config,
            service: ToolService::new(db, catalog),
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

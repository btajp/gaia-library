//! 管理系: affiliation（DB・audit 付き）と client（設定ファイル）。提案キュー原則の例外はここだけ。
use std::path::Path;

use clap::Subcommand;
use serde_json::json;

use gaia_core::{
    config::Config,
    identity::{ClientIdentity, Role},
};

use super::app::{App, print_json};

#[derive(Subcommand)]
pub enum AffiliationCmd {
    /// 機密境界を追加する（human。audit_log(admin_write) に記録）
    Add {
        name: String,
        #[arg(long)]
        identity: Option<String>,
    },
    /// 一覧
    List,
}

#[derive(Subcommand)]
pub enum ClientCmd {
    /// クライアント（識別）を設定ファイルへ追加する
    Add {
        name: String,
        #[arg(long)]
        role: Role,
        #[arg(long)]
        default_scope: Option<String>,
    },
    /// 一覧
    List,
}

pub fn affiliation(
    app: &App,
    cli_client: Option<&str>,
    cmd: &AffiliationCmd,
    compact: bool,
) -> anyhow::Result<()> {
    let actor = app.identity(cli_client)?;
    if actor.role != Role::Human {
        return Err(gaia_core::error::ToolError::unauthorized(
            "affiliation の管理は human クライアントのみ（--client を確認）",
        )
        .into());
    }
    match cmd {
        AffiliationCmd::Add { name, identity } => {
            let id = gaia_core::admin::add_affiliation(
                app.service.db(),
                &actor.name,
                name,
                identity.as_deref(),
            )?;
            print_json(&json!({"id": id, "name": name}), compact);
        }
        AffiliationCmd::List => {
            let list = gaia_core::admin::list_affiliations(app.service.db())?;
            let rows: Vec<_> = list
                .iter()
                .map(|a| json!({"id": a.id, "name": a.name, "identity": a.identity}))
                .collect();
            print_json(&serde_json::Value::Array(rows), compact);
        }
    }
    Ok(())
}

pub fn client(config_path: &Path, cmd: &ClientCmd, compact: bool) -> anyhow::Result<()> {
    match cmd {
        ClientCmd::Add {
            name,
            role,
            default_scope,
        } => {
            Config::update(config_path, |config| {
                config.add_client(ClientIdentity {
                    name: name.clone(),
                    role: *role,
                    default_scope: default_scope.clone(),
                })
            })?;
            eprintln!("クライアント `{name}` を追加しました（role={role}）");
        }
        ClientCmd::List => {
            let config = Config::load(config_path)?;
            print_json(&serde_json::to_value(&config.clients)?, compact);
        }
    }
    Ok(())
}

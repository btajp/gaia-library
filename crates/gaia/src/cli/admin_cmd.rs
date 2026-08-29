//! 管理系: affiliation（DB・audit 付き）と client（設定ファイル）。提案キュー原則の例外はここだけ。
use std::path::Path;

use clap::Subcommand;
use serde_json::json;

use gaia_core::{
    auth,
    config::{Config, ConfigError},
    error::ToolError,
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
        /// 追加と同時に API キーを発行する（平文は保存成功後に stdout へ 1 回だけ出力）
        #[arg(long)]
        generate_key: bool,
    },
    /// 一覧
    List,
    /// API キーを（再）発行する。旧キーは即失効
    Keygen { name: String },
    /// クライアント名を変更する（role / 既定 scope / API キーは維持。DB の履歴は書き換えない）
    Rename { old: String, new: String },
    /// MCP クライアント設定のスニペットを出力する
    McpConfig(super::mcp_config::McpConfigArgs),
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
            generate_key,
        } => {
            let key = Config::update(config_path, |config| {
                config.add_client(ClientIdentity {
                    name: name.clone(),
                    role: *role,
                    default_scope: default_scope.clone(),
                })?;
                Ok(generate_key.then(|| {
                    let (plaintext, hash) = auth::generate_key(name);
                    config.keys.insert(name.clone(), hash);
                    plaintext
                }))
            })
            .map_err(config_update_error)?;
            if compact {
                let mut output = json!({
                    "client": name, "role": role, "default_scope": default_scope,
                });
                if let Some(key) = key {
                    output["key"] = json!(key);
                }
                print_json(&output, true);
            } else {
                if let Some(key) = key {
                    print_issued_key(name, &key, false);
                }
                eprintln!("クライアント `{name}` を追加しました（role={role}）");
            }
        }
        ClientCmd::List => {
            let config = Config::load(config_path)?;
            print_json(&serde_json::to_value(&config.clients)?, compact);
        }
        ClientCmd::Keygen { name } => {
            let key = Config::update(config_path, |config| {
                if config.client(name).is_none() {
                    return Err(ConfigError::UnknownClient(name.clone()));
                }
                let (plaintext, hash) = auth::generate_key(name);
                config.keys.insert(name.clone(), hash);
                Ok(plaintext)
            })
            .map_err(config_update_error)?;
            print_issued_key(name, &key, compact);
        }
        ClientCmd::Rename { old, new } => {
            // 設定ファイルだけを付け替える。proposals の proposed_by / decided_by と audit_log の actor は
            // 旧名のまま残す（履歴の保持）。
            let renamed = Config::update(config_path, |config| {
                config.rename_client(old, new)?;
                Ok(new.trim().to_owned())
            })
            .map_err(config_update_error)?;
            let notice = format!(
                "stdio 接続設定には --client {renamed} が入るため、配布済みの接続設定を出し直してください。HTTP のキーは有効なままです"
            );
            if compact {
                print_json(
                    &json!({"client": renamed, "previous": old, "notice": notice}),
                    true,
                );
            } else {
                eprintln!("クライアント `{old}` を `{renamed}` に変更しました");
                eprintln!("{notice}");
            }
        }
        ClientCmd::McpConfig(args) => super::mcp_config::print(config_path, args, compact)?,
    }
    Ok(())
}

fn print_issued_key(name: &str, key: &str, compact: bool) {
    if compact {
        print_json(&json!({"client": name, "key": key}), true);
    } else {
        println!("{key}");
        eprintln!(
            "API キーを発行しました（旧キーは失効。平文はこの 1 回だけ表示し、config にはハッシュのみ保存）"
        );
    }
}

fn config_update_error(error: ConfigError) -> anyhow::Error {
    match error {
        ConfigError::UnknownClient(name) => {
            ToolError::not_found(format!("クライアント `{name}` がありません")).into()
        }
        ConfigError::DuplicateClient(name) => {
            ToolError::conflict(format!("クライアント `{name}` は既に存在します")).into()
        }
        ConfigError::EmptyClientName => {
            ToolError::invalid_params("クライアント名を空にはできません").into()
        }
        error => error.into(),
    }
}

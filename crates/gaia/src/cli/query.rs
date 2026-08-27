//! 参照系コマンド。引数を JSON に組んで ToolService::call に渡すだけ。
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use gaia_core::identity::ClientIdentity;

use super::app::{App, print_json};

#[derive(Args)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long)]
    pub scope: Vec<String>,
    /// 検索対象の種別（person / organization / engagement / entity / interaction / glossary）
    #[arg(long = "type")]
    pub types: Vec<String>,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Subcommand)]
pub enum GetCmd {
    /// id または name で 1 件取得
    Get {
        #[arg(long)]
        id: Option<i64>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        scope: Vec<String>,
    },
}

#[derive(Args)]
pub struct GlossaryArgs {
    #[arg(long)]
    pub engagement_id: Option<i64>,
    #[arg(long)]
    pub scope: Vec<String>,
}

#[derive(Args)]
pub struct SpeakersArgs {
    /// 会議ツールの表示名（複数可）
    pub names: Vec<String>,
    #[arg(long)]
    pub engagement_id: Option<i64>,
    #[arg(long)]
    pub scope: Vec<String>,
}

fn put_scope(payload: &mut Value, scope: &[String]) {
    if !scope.is_empty() {
        payload["scope"] = json!(scope);
    }
}

pub fn search(
    app: &App,
    client: &ClientIdentity,
    args: &SearchArgs,
    compact: bool,
) -> anyhow::Result<()> {
    let mut payload = json!({"query": args.query});
    put_scope(&mut payload, &args.scope);
    if !args.types.is_empty() {
        payload["types"] = json!(args.types);
    }
    if let Some(l) = args.limit {
        payload["limit"] = json!(l);
    }
    print_json(&app.call(client, "search_context", payload)?, compact);
    Ok(())
}

pub fn get_entity(
    app: &App,
    client: &ClientIdentity,
    tool: &str,
    id_key: &str,
    cmd: &GetCmd,
    compact: bool,
) -> anyhow::Result<()> {
    let GetCmd::Get { id, name, scope } = cmd;
    let mut payload = json!({});
    if let Some(id) = id {
        payload[id_key] = json!(id);
    }
    if let Some(name) = name {
        payload["name"] = json!(name);
    }
    put_scope(&mut payload, scope);
    print_json(&app.call(client, tool, payload)?, compact);
    Ok(())
}

pub fn glossary(
    app: &App,
    client: &ClientIdentity,
    args: &GlossaryArgs,
    compact: bool,
) -> anyhow::Result<()> {
    let mut payload = json!({});
    if let Some(eid) = args.engagement_id {
        payload["engagement_id"] = json!(eid);
    }
    put_scope(&mut payload, &args.scope);
    print_json(&app.call(client, "get_glossary", payload)?, compact);
    Ok(())
}

pub fn speakers(
    app: &App,
    client: &ClientIdentity,
    args: &SpeakersArgs,
    compact: bool,
) -> anyhow::Result<()> {
    let mut payload = json!({"display_names": args.names});
    if let Some(eid) = args.engagement_id {
        payload["engagement_id"] = json!(eid);
    }
    put_scope(&mut payload, &args.scope);
    print_json(&app.call(client, "resolve_speakers", payload)?, compact);
    Ok(())
}

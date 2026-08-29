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

#[derive(Args)]
pub struct ResolveArgs {
    /// 登録済み参照の id
    #[arg(long, required_unless_present = "uri")]
    pub ref_id: Option<i64>,
    /// 登録済み参照を uri の完全一致で検索する（実効 scope 内の最新 1 件。取得先の指定ではない）
    #[arg(long, required_unless_present = "ref_id")]
    pub uri: Option<String>,
    #[arg(long)]
    pub scope: Vec<String>,
    /// content だけを stdout に出す（ヘッダと reason は stderr）。resolved=false は終了コード 2
    #[arg(long)]
    pub content: bool,
}

pub fn resolve(
    app: &App,
    client: &ClientIdentity,
    args: &ResolveArgs,
    compact: bool,
) -> anyhow::Result<()> {
    let mut payload = json!({});
    if let Some(id) = args.ref_id {
        payload["ref_id"] = json!(id);
    }
    if let Some(uri) = &args.uri {
        payload["uri"] = json!(uri);
    }
    put_scope(&mut payload, &args.scope);
    let out = app.call(client, "resolve_source", payload)?;
    if !args.content {
        print_json(&out, compact);
        return Ok(());
    }
    let reference = &out["reference"];
    eprintln!(
        "ref #{} [{}] {}",
        reference["id"],
        reference["system"].as_str().unwrap_or(""),
        reference["title"]
            .as_str()
            .or(reference["uri"].as_str())
            .unwrap_or("")
    );
    if let Some(reason) = out["reason"].as_str() {
        eprintln!("reason: {reason}");
    }
    if out["resolved"].as_bool() != Some(true) {
        if let Some(snapshot) = reference["snapshot"].as_str() {
            eprintln!("snapshot:\n{snapshot}");
        }
        std::process::exit(2);
    }
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(out["content"].as_str().unwrap_or("").as_bytes())?;
    stdout.flush()?;
    Ok(())
}

//! 提案系コマンド。`add *` は human 向けに「提案＋即時承認」を 1 コマンド化したもの。
use clap::{Args, Subcommand};
use serde_json::{Value, json};

use gaia_core::{
    error::ToolError,
    identity::{ClientIdentity, Role},
};

use super::app::{App, print_json};

#[derive(Args)]
pub struct ProposeArgs {
    /// person / organization / engagement / interaction / entity / fact / ref / glossary
    pub target_type: String,
    /// insert / update / supersede
    pub action: String,
    /// Patch JSON（target_type ごとの形。契約 defs/common.json 参照）
    #[arg(long)]
    pub patch: String,
    #[arg(long)]
    pub target_id: Option<i64>,
    #[arg(long, default_value = "fact")]
    pub kind: String,
    #[arg(long)]
    pub scope: Option<String>,
    /// 出所 JSON（{"ref_id": N} か {"system", "uri", "note", ...}）
    #[arg(long)]
    pub provenance: Option<String>,
    /// 冪等化キー（省略時は cli-<uuid> を自動発番）
    #[arg(long)]
    pub request_id: Option<String>,
}

#[derive(Args)]
pub struct ProposalsArgs {
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub scope: Vec<String>,
    #[arg(long)]
    pub limit: Option<i64>,
}

#[derive(Subcommand)]
pub enum AddCmd {
    Person {
        #[arg(long)]
        name: String,
        #[arg(long)]
        org_id: Option<i64>,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        alias: Vec<String>,
        #[arg(long)]
        scope: Option<String>,
    },
    Org {
        #[arg(long)]
        name: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        scope: Option<String>,
    },
    Engagement {
        #[arg(long)]
        name: String,
        #[arg(long)]
        org_id: Option<i64>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        person_id: Vec<i64>,
        #[arg(long)]
        scope: Option<String>,
    },
    Fact {
        #[arg(long)]
        entity_type: String,
        #[arg(long)]
        entity_id: i64,
        #[arg(long)]
        statement: String,
        #[arg(long)]
        predicate: Option<String>,
        #[arg(long)]
        value: Option<String>,
        #[arg(long, default_value = "fact")]
        kind: String,
        #[arg(long)]
        scope: Option<String>,
    },
    Ref {
        #[arg(long)]
        target_type: String,
        #[arg(long)]
        target_id: i64,
        #[arg(long)]
        system: String,
        #[arg(long)]
        uri: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        note: String,
        #[arg(long)]
        snapshot: Option<String>,
        #[arg(long)]
        scope: Option<String>,
    },
    Glossary {
        #[arg(long)]
        term: String,
        #[arg(long)]
        reading: Option<String>,
        #[arg(long)]
        definition: Option<String>,
        #[arg(long)]
        engagement_id: Option<i64>,
        #[arg(long)]
        scope: Option<String>,
    },
    Interaction {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        occurred_at: String,
        #[arg(long)]
        summary: String,
        #[arg(long)]
        engagement_id: Option<i64>,
        #[arg(long)]
        person_id: Vec<i64>,
        #[arg(long)]
        scope: Option<String>,
    },
}

fn new_request_id() -> String {
    format!("cli-{}", uuid::Uuid::new_v4())
}

/// null を取り除いた JSON オブジェクトを作る（COALESCE 更新と噛み合わせるため）。
fn compact_object(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            Value::Object(map.into_iter().filter(|(_, v)| !v.is_null()).collect())
        }
        other => other,
    }
}

pub fn propose(
    app: &App,
    client: &ClientIdentity,
    args: &ProposeArgs,
    compact: bool,
) -> anyhow::Result<()> {
    let patch: Value = serde_json::from_str(&args.patch)
        .map_err(|e| ToolError::invalid_params(format!("--patch は JSON で指定する: {e}")))?;
    let mut payload = json!({
        "target_type": args.target_type,
        "action": args.action,
        "patch": patch,
        "kind": args.kind,
        "request_id": args.request_id.clone().unwrap_or_else(new_request_id),
    });
    if let Some(id) = args.target_id {
        payload["target_id"] = json!(id);
    }
    if let Some(s) = &args.scope {
        payload["scope"] = json!(s);
    }
    if let Some(p) = &args.provenance {
        payload["provenance"] = serde_json::from_str(p).map_err(|e| {
            ToolError::invalid_params(format!("--provenance は JSON で指定する: {e}"))
        })?;
    }
    print_json(&app.call(client, "propose_update", payload)?, compact);
    Ok(())
}

pub fn proposals(
    app: &App,
    client: &ClientIdentity,
    args: &ProposalsArgs,
    compact: bool,
) -> anyhow::Result<()> {
    let mut payload = json!({});
    if let Some(s) = &args.status {
        payload["status"] = json!(s);
    }
    if !args.scope.is_empty() {
        payload["scope"] = json!(args.scope);
    }
    if let Some(l) = args.limit {
        payload["limit"] = json!(l);
    }
    print_json(&app.call(client, "list_proposals", payload)?, compact);
    Ok(())
}

pub fn add(app: &App, client: &ClientIdentity, cmd: &AddCmd, compact: bool) -> anyhow::Result<()> {
    if client.role != Role::Human {
        return Err(ToolError::unauthorized(
            "`gaia add` は human クライアント専用（agent は `gaia propose` で提案する）",
        )
        .into());
    }
    let (target_type, patch, kind, scope) = match cmd {
        AddCmd::Person {
            name,
            org_id,
            role,
            alias,
            scope,
        } => (
            "person",
            compact_object(json!({
                "name": name, "org_id": org_id, "role": role,
                "aliases": alias.iter().map(|a| json!({"alias": a})).collect::<Vec<_>>(),
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Org { name, kind, scope } => (
            "organization",
            compact_object(json!({"name": name, "kind": kind})),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Engagement {
            name,
            org_id,
            status,
            person_id,
            scope,
        } => (
            "engagement",
            compact_object(json!({
                "name": name, "org_id": org_id, "status": status,
                "people": person_id.iter().map(|p| json!({"person_id": p})).collect::<Vec<_>>(),
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Fact {
            entity_type,
            entity_id,
            statement,
            predicate,
            value,
            kind,
            scope,
        } => (
            "fact",
            compact_object(json!({
                "entity_type": entity_type, "entity_id": entity_id, "statement": statement,
                "predicate": predicate, "value": value,
            })),
            kind.clone(),
            scope.clone(),
        ),
        AddCmd::Ref {
            target_type,
            target_id,
            system,
            uri,
            title,
            note,
            snapshot,
            scope,
        } => (
            "ref",
            compact_object(json!({
                "target_type": target_type, "target_id": target_id, "system": system, "uri": uri,
                "title": title, "note": note, "snapshot": snapshot,
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Glossary {
            term,
            reading,
            definition,
            engagement_id,
            scope,
        } => (
            "glossary",
            compact_object(json!({
                "term": term, "reading": reading, "definition": definition, "engagement_id": engagement_id,
            })),
            "fact".to_string(),
            scope.clone(),
        ),
        AddCmd::Interaction {
            kind,
            occurred_at,
            summary,
            engagement_id,
            person_id,
            scope,
        } => (
            "interaction",
            compact_object(json!({
                "kind": kind, "occurred_at": occurred_at, "summary": summary,
                "engagement_id": engagement_id, "person_ids": person_id,
            })),
            "fact".to_string(),
            scope.clone(),
        ),
    };
    let mut payload = json!({
        "target_type": target_type, "action": "insert", "patch": patch, "kind": kind,
        "request_id": new_request_id(),
    });
    if let Some(s) = scope {
        payload["scope"] = json!(s);
    }
    let proposed = app.call(client, "propose_update", payload)?;
    let proposal_id = proposed["proposal_id"]
        .as_i64()
        .ok_or_else(|| ToolError::internal("propose_update の応答に proposal_id がありません"))?;
    let approved = app
        .call(
            client,
            "approve_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .map_err(|mut error| {
            error.details = Some(match error.details.take() {
                Some(Value::Object(mut details)) => {
                    details.insert("proposal_id".to_string(), json!(proposal_id));
                    details.insert("phase".to_string(), json!("approve"));
                    details.insert("proposal_created".to_string(), json!(true));
                    Value::Object(details)
                }
                Some(details) => json!({
                    "proposal_id": proposal_id,
                    "phase": "approve",
                    "proposal_created": true,
                    "cause": details,
                }),
                None => json!({
                    "proposal_id": proposal_id,
                    "phase": "approve",
                    "proposal_created": true,
                }),
            });
            error
        })?;
    print_json(&approved, compact);
    Ok(())
}

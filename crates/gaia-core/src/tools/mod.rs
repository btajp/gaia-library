//! ToolService: CLI と MCP の唯一の入口。仕様書 §8.1。
//! 手順: ツール解決 → role 認可 → 契約スキーマで入力検証 → 型付きハンドラ → （debug/test）出力検証。
mod get_engagement;
mod get_organization;
mod get_person;
mod job_status;
mod propose;
mod server_info;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    contracts::{Catalog, ToolSpec},
    error::ToolError,
    identity::{ClientIdentity, Role},
    storage::Db,
};

/// dispatch 済みツール。Task 16 完了時に「enabled な契約 = この一覧」となる（テストで固定）。
pub const HANDLED_TOOLS: &[&str] = &[
    "get_server_info",
    "get_job_status",
    "propose_update",
    "list_proposals",
    "approve_proposal",
    "reject_proposal",
    "get_person",
    "get_organization",
    "get_engagement",
];

pub struct ToolService {
    db: Db,
    catalog: Catalog,
}

pub struct CallContext<'a> {
    pub client: &'a ClientIdentity,
    pub db: &'a Db,
    pub catalog: &'a Catalog,
}

impl ToolService {
    pub fn new(db: Db, catalog: Catalog) -> Self {
        Self { db, catalog }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn visible_tools(&self, role: Role) -> Vec<&ToolSpec> {
        self.catalog.visible(role)
    }

    pub fn call(
        &self,
        client: &ClientIdentity,
        tool: &str,
        args: Value,
    ) -> Result<Value, ToolError> {
        let spec = self
            .catalog
            .get(tool)
            .filter(|s| s.enabled)
            .ok_or_else(|| ToolError::not_found(format!("unknown tool `{tool}`")))?;
        if !spec.allows(client.role) {
            return Err(ToolError::unauthorized(format!(
                "tool `{tool}` is not allowed for role `{}` (client `{}`)",
                client.role, client.name
            )));
        }
        let args = if args.is_null() { json!({}) } else { args };
        spec.validate_input(&args)?;
        let ctx = CallContext {
            client,
            db: &self.db,
            catalog: &self.catalog,
        };
        let out = dispatch(&ctx, tool, args)?;
        if cfg!(any(test, debug_assertions)) {
            spec.validate_output(&out)?;
        }
        Ok(out)
    }
}

fn dispatch(ctx: &CallContext<'_>, tool: &str, args: Value) -> Result<Value, ToolError> {
    match tool {
        "get_server_info" => run(ctx, args, server_info::handle),
        "get_job_status" => run(ctx, args, job_status::handle),
        "propose_update" => run(ctx, args, propose::propose_update),
        "list_proposals" => run(ctx, args, propose::list_proposals),
        "approve_proposal" => run(ctx, args, propose::approve_proposal),
        "reject_proposal" => run(ctx, args, propose::reject_proposal),
        "get_person" => run(ctx, args, get_person::handle),
        "get_organization" => run(ctx, args, get_organization::handle),
        "get_engagement" => run(ctx, args, get_engagement::handle),
        other => Err(ToolError::not_implemented(format!(
            "tool `{other}` has no handler yet"
        ))),
    }
}

fn run<I, O>(
    ctx: &CallContext<'_>,
    args: Value,
    f: impl FnOnce(&CallContext<'_>, I) -> Result<O, ToolError>,
) -> Result<Value, ToolError>
where
    I: DeserializeOwned,
    O: Serialize,
{
    let input: I = serde_json::from_value(args).map_err(|e| {
        ToolError::internal(format!(
            "validated arguments failed to deserialize into contract types: {e}"
        ))
    })?;
    let out = f(ctx, input)?;
    Ok(serde_json::to_value(out)?)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::ToolService;
    use crate::{
        contracts::Catalog,
        identity::{ClientIdentity, Role},
        storage::{Db, StorageError, affiliations},
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn service() -> ToolService {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            Ok(())
        })
        .unwrap();
        ToolService::new(db, Catalog::embedded().unwrap())
    }

    pub(crate) fn human() -> ClientIdentity {
        ClientIdentity {
            name: "me".into(),
            role: Role::Human,
            default_scope: Some("cn".into()),
        }
    }

    pub(crate) fn agent() -> ClientIdentity {
        ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: Some("cn".into()),
        }
    }

    pub(crate) struct SeedIds {
        pub org: i64,
        pub person: i64,
        pub engagement: i64,
        pub fact: i64,
        pub reference: i64,
        pub glossary: i64,
        pub interaction: i64,
    }

    /// human クライアントで propose → approve を回す（書き込み経路そのものをテストデータ投入に使う）。
    pub(crate) fn write(s: &ToolService, target_type: &str, patch: serde_json::Value) -> i64 {
        let h = human();
        let out = s
            .call(&h, "propose_update", json!({
                "target_type": target_type, "action": "insert", "patch": patch, "kind": "fact",
                "request_id": format!("seed-{target_type}-{:04}", SEQ.fetch_add(1, Ordering::SeqCst)),
            }))
            .unwrap();
        let pid = out["proposal_id"].as_i64().unwrap();
        let approved = s
            .call(&h, "approve_proposal", json!({"proposal_id": pid}))
            .unwrap();
        approved["result"]["id"].as_i64().unwrap()
    }

    pub(crate) fn seed_basic(s: &ToolService) -> SeedIds {
        let org = write(
            s,
            "organization",
            json!({"name": "RELATIONS", "kind": "customer"}),
        );
        let person = write(
            s,
            "person",
            json!({
                "name": "岡村 慎太郎", "org_id": org, "role": "情シス",
                "aliases": [{"alias": "Okamura Shintaro", "kind": "romaji"}, {"alias": "okash1n", "kind": "nickname"}]
            }),
        );
        let engagement = write(
            s,
            "engagement",
            json!({
                "name": "Okta導入支援", "org_id": org, "status": "active",
                "people": [{"person_id": person, "role": "key_person"}]
            }),
        );
        let fact = write(
            s,
            "fact",
            json!({
                "entity_type": "engagement", "entity_id": engagement,
                "statement": "決定: SCIM プロビジョニングは Phase 2 で対応する", "predicate": "decision", "value": "scim-phase2"
            }),
        );
        let reference = write(
            s,
            "ref",
            json!({
                "target_type": "fact", "target_id": fact, "system": "minutes",
                "uri": "minutes://meeting/42#t=1200", "note": "決定箇所の議事録参照", "snapshot": "SCIM は Phase 2"
            }),
        );
        let glossary = write(
            s,
            "glossary",
            json!({"term": "SCIM", "reading": "スキム", "definition": "プロビジョニング標準", "engagement_id": engagement}),
        );
        let interaction = write(
            s,
            "interaction",
            json!({
                "kind": "meeting", "occurred_at": "2026-08-20T10:00:00Z",
                "summary": "定例。SCIM の段階対応を決定", "engagement_id": engagement, "person_ids": [person]
            }),
        );
        SeedIds {
            org,
            person,
            engagement,
            fact,
            reference,
            glossary,
            interaction,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support;
    use super::test_support::{agent, human, service};
    use super::*;
    use crate::error::ErrorCode;
    use serde_json::json;

    #[test]
    fn get_server_info_reports_identity_and_visible_tools() {
        let s = service();
        let out = s.call(&agent(), "get_server_info", json!({})).unwrap();
        assert_eq!(out["name"], "gaia_library");
        assert_eq!(out["contract_version"], "1.0.0");
        assert_eq!(out["client"]["role"], "agent");
        assert_eq!(out["client"]["default_scope"], "cn");
        let tools: Vec<&str> = out["capabilities"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(!tools.contains(&"approve_proposal"));
        let human_out = s.call(&human(), "get_server_info", json!({})).unwrap();
        let human_tools: Vec<&str> = human_out["capabilities"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(human_tools.contains(&"approve_proposal"));
    }

    #[test]
    fn call_enforces_existence_role_and_input_schema() {
        let s = service();
        assert_eq!(
            s.call(&agent(), "nope", json!({})).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            s.call(&agent(), "resolve_source", json!({}))
                .unwrap_err()
                .code,
            ErrorCode::NotFound,
            "disabled = 存在しない扱い"
        );
        assert_eq!(
            s.call(&agent(), "approve_proposal", json!({"proposal_id": 1}))
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );
        assert_eq!(
            s.call(&agent(), "get_job_status", json!({"job_id": 1}))
                .unwrap_err()
                .code,
            ErrorCode::InvalidParams
        );
        assert_eq!(
            s.call(&agent(), "get_job_status", json!({"job_id": "j1"}))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn handled_tools_are_enabled_contract_tools() {
        let s = service();
        for name in HANDLED_TOOLS {
            let spec = s
                .catalog()
                .get(name)
                .unwrap_or_else(|| panic!("{name} missing from contracts"));
            assert!(spec.enabled, "{name} must be enabled");
        }
    }

    #[test]
    fn admin_add_affiliation_is_audited() {
        let s = service();
        crate::admin::add_affiliation(s.db(), "me", "assoc", Some("理事")).unwrap();
        let entries = s
            .db()
            .with_conn::<_, crate::storage::StorageError>(|c| crate::storage::audit::recent(c, 5))
            .unwrap();
        assert_eq!(entries[0].action, "admin_write");
        assert_eq!(entries[0].detail["name"], "assoc");
    }

    #[test]
    fn get_person_by_name_returns_connected_context() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(&agent(), "get_person", json!({"name": "okash1n"}))
            .unwrap();
        assert_eq!(out["person"]["id"].as_i64().unwrap(), ids.person);
        assert_eq!(out["organization"]["name"], "RELATIONS");
        assert_eq!(out["engagements"][0]["name"], "Okta導入支援");
        assert_eq!(out["interactions"].as_array().unwrap().len(), 1);
        // 引数無しは invalid_params、未知 id は not_found
        assert_eq!(
            s.call(&agent(), "get_person", json!({})).unwrap_err().code,
            ErrorCode::InvalidParams
        );
        assert_eq!(
            s.call(&agent(), "get_person", json!({"person_id": 9999}))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn get_engagement_hides_out_of_scope_and_returns_members() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(
                &agent(),
                "get_engagement",
                json!({"engagement_id": ids.engagement}),
            )
            .unwrap();
        assert_eq!(
            out["people"][0]["person"]["id"].as_i64().unwrap(),
            ids.person
        );
        assert_eq!(out["people"][0]["role"], "key_person");
        assert_eq!(out["glossary"][0]["term"], "SCIM");
        assert_eq!(
            out["facts"][0]["statement"]
                .as_str()
                .unwrap()
                .contains("SCIM"),
            true
        );
        // fact の根拠参照（minutes）が refs に載る
        assert!(
            out["refs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["system"] == "minutes")
        );
        // 別 scope からは not_found
        let err = s
            .call(
                &agent(),
                "get_engagement",
                json!({"engagement_id": ids.engagement, "scope": "other"}),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn get_organization_lists_people_and_engagements() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(&agent(), "get_organization", json!({"name": "RELATIONS"}))
            .unwrap();
        assert_eq!(out["organization"]["id"].as_i64().unwrap(), ids.org);
        assert_eq!(out["people"].as_array().unwrap().len(), 1);
        assert_eq!(out["engagements"].as_array().unwrap().len(), 1);
    }
}

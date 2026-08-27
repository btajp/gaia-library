//! ToolService: CLI と MCP の唯一の入口。仕様書 §8.1。
//! 手順: ツール解決 → role 認可 → 契約スキーマで入力検証 → 型付きハンドラ → （debug/test）出力検証。
mod get_engagement;
mod get_glossary;
mod get_organization;
mod get_person;
mod job_status;
mod propose;
mod resolve_speakers;
mod search_context;
mod server_info;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    contracts::{Catalog, ToolSpec},
    error::ToolError,
    identity::{ClientIdentity, Role},
    storage::Db,
};

/// dispatch 済みツール。契約の enabled ツールと 1:1（テストで固定）。
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
    "get_glossary",
    "resolve_speakers",
    "search_context",
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
        "get_glossary" => run(ctx, args, get_glossary::handle),
        "resolve_speakers" => run(ctx, args, resolve_speakers::handle),
        "search_context" => run(ctx, args, search_context::handle),
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
    // JSON Schema の表現力を typify 互換の範囲に制限しているため、i64 の範囲など
    // Rust 型でのみ確定する入力違反はここで protocol-level invalid_params にする。
    let input: I = serde_json::from_value(args).map_err(|e| {
        ToolError::invalid_params(format!(
            "arguments cannot be represented by the contract types: {e}"
        ))
        .with_details(json!({"deserialization": e.to_string()}))
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
        assert_eq!(
            s.call(
                &agent(),
                "get_person",
                json!({"person_id": 9223372036854775808_u64})
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidParams,
            "JSON Schema を通るが i64 に収まらない整数はクライアント入力違反"
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
        assert!(
            out["facts"][0]["statement"]
                .as_str()
                .unwrap()
                .contains("SCIM")
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

    #[test]
    fn glossary_hints_include_terms_readings_and_member_aliases() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(
                &agent(),
                "get_glossary",
                json!({"engagement_id": ids.engagement}),
            )
            .unwrap();
        let hints: Vec<&str> = out["vocabulary_hints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for expected in [
            "SCIM",
            "スキム",
            "岡村 慎太郎",
            "okash1n",
            "Okamura Shintaro",
        ] {
            assert!(hints.contains(&expected), "missing {expected}: {hints:?}");
        }
        // engagement 省略で scope 内全用語
        let all = s.call(&agent(), "get_glossary", json!({})).unwrap();
        assert_eq!(all["terms"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn resolve_speakers_matches_zoom_style_display_names() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(
                &agent(),
                "resolve_speakers",
                json!({
                    "display_names": ["岡村 慎太郎 (RELATIONS)", "OKAMURA SHINTARO", "見知らぬ 人"],
                    "engagement_id": ids.engagement
                }),
            )
            .unwrap();
        let results = out["results"].as_array().unwrap();
        assert_eq!(results[0]["status"], "matched");
        assert_eq!(results[0]["person"]["id"].as_i64().unwrap(), ids.person);
        assert_eq!(
            results[1]["status"], "matched",
            "ローマ字大文字も正規化で一致する"
        );
        assert_eq!(results[2]["status"], "unmatched");
    }

    #[test]
    fn resolve_speakers_reports_ambiguity_and_narrows_by_engagement() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        // 同じ「田中」を 2 人つくる（片方だけ案件の関係者）
        let t1 = test_support::write(
            &s,
            "person",
            json!({"name": "田中 太郎", "aliases": [{"alias": "田中"}]}),
        );
        let _t2 = test_support::write(
            &s,
            "person",
            json!({"name": "田中 次郎", "aliases": [{"alias": "田中"}]}),
        );
        s.call(&human(), "propose_update", json!({
            "target_type": "engagement", "action": "update", "target_id": ids.engagement,
            "patch": {"people": [{"person_id": t1, "role": "member"}]}, "kind": "fact", "request_id": "req-add-tanaka-1"
        }))
        .and_then(|out| s.call(&human(), "approve_proposal", json!({"proposal_id": out["proposal_id"]})))
        .unwrap();
        // engagement 無し → ambiguous
        let out = s
            .call(
                &agent(),
                "resolve_speakers",
                json!({"display_names": ["田中"]}),
            )
            .unwrap();
        assert_eq!(out["results"][0]["status"], "ambiguous");
        assert_eq!(out["results"][0]["candidates"].as_array().unwrap().len(), 2);
        // engagement 指定 → 関係者の田中太郎に絞られて matched(0.9)
        let out = s
            .call(
                &agent(),
                "resolve_speakers",
                json!({"display_names": ["田中"], "engagement_id": ids.engagement}),
            )
            .unwrap();
        assert_eq!(out["results"][0]["status"], "matched");
        assert_eq!(out["results"][0]["person"]["id"].as_i64().unwrap(), t1);
        assert!((out["results"][0]["confidence"].as_f64().unwrap() - 0.9).abs() < 1e-9);
    }

    #[test]
    fn handled_tools_equal_enabled_contract_tools() {
        let s = service();
        let mut enabled: Vec<&str> = s
            .catalog()
            .tools()
            .iter()
            .filter(|t| t.enabled)
            .map(|t| t.name.as_str())
            .collect();
        let mut handled: Vec<&str> = HANDLED_TOOLS.to_vec();
        enabled.sort();
        handled.sort();
        assert_eq!(
            enabled, handled,
            "契約の enabled ツールと dispatch が 1:1 であること"
        );
    }

    #[test]
    fn search_context_returns_answer_blueprint() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(&agent(), "search_context", json!({"query": "SCIM"}))
            .unwrap();
        // fact ヒットが案件に折りたたまれ、facts と minutes への参照が同梱される
        let e = &out["entities"][0];
        assert_eq!(e["type"], "engagement");
        assert_eq!(e["id"].as_i64().unwrap(), ids.engagement);
        assert!(
            e["matched_on"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m.as_str().unwrap().starts_with("fact:"))
        );
        assert!(
            e["refs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["system"] == "minutes")
        );
        assert_eq!(out["glossary"][0]["term"], "SCIM");
        assert_eq!(out["interactions"].as_array().unwrap().len(), 1);
        assert_eq!(out["cross_scope"], false);
    }

    #[test]
    fn search_context_finds_people_by_alias_and_flags_short_queries() {
        let s = service();
        let ids = test_support::seed_basic(&s);
        let out = s
            .call(&agent(), "search_context", json!({"query": "okash1n"}))
            .unwrap();
        assert_eq!(out["entities"][0]["type"], "person");
        assert_eq!(out["entities"][0]["id"].as_i64().unwrap(), ids.person);
        assert!(
            out["entities"][0]["matched_on"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m == "alias")
        );
        // 2 文字クエリはヒント付き
        let short = s
            .call(
                &agent(),
                "search_context",
                json!({"query": "決定", "types": ["engagement"]}),
            )
            .unwrap();
        assert!(!short["hints"].as_array().unwrap().is_empty());
        // scope 外のデータは出ない
        let other = s
            .call(
                &agent(),
                "search_context",
                json!({"query": "SCIM", "scope": "other"}),
            )
            .unwrap();
        assert_eq!(other["entities"].as_array().unwrap().len(), 0);
    }
}

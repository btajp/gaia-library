//! ToolService: CLI と MCP の唯一の入口。仕様書 §8.1。
//! 手順: ツール解決 → role 認可 → 契約スキーマで入力検証 → 型付きハンドラ → （debug/test）出力検証。
mod job_status;
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
pub const HANDLED_TOOLS: &[&str] = &["get_server_info", "get_job_status"];

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
}

#[cfg(test)]
mod tests {
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
}

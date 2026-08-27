//! scope（所属元＝機密境界）の解決。仕様書 §7.2。default deny / explicit allow。
use rusqlite::Connection;
use serde_json::json;

use crate::{
    contracts::types::ScopeInput,
    error::ToolError,
    identity::ClientIdentity,
    storage::{affiliations, audit},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSet {
    scopes: Vec<String>,
}

impl ScopeSet {
    /// 引数 → クライアント既定 scope → `scope_denied`。各 scope は affiliations に存在すること（無ければ `not_found`）。
    pub fn resolve(conn: &Connection, client: &ClientIdentity, requested: Option<Vec<String>>) -> Result<Self, ToolError> {
        let mut scopes = match requested {
            Some(v) if !v.is_empty() => v,
            _ => vec![client.default_scope.clone().ok_or_else(|| {
                ToolError::scope_denied(format!(
                    "scope is required: pass `scope` or set default_scope for client `{}`",
                    client.name
                ))
            })?],
        };
        scopes.sort();
        scopes.dedup();
        for s in &scopes {
            if !affiliations::exists(conn, s)? {
                return Err(ToolError::not_found(format!("scope `{s}` (affiliation) not found")));
            }
        }
        Ok(Self { scopes })
    }

    /// 検証なしで 1 scope を包む（承認処理など scope が既に DB 由来のとき用）。
    pub fn single(name: &str) -> Self {
        Self { scopes: vec![name.to_string()] }
    }

    pub fn names(&self) -> &[String] {
        &self.scopes
    }

    pub fn is_cross(&self) -> bool {
        self.scopes.len() > 1
    }

    pub fn contains(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s == name)
    }

    /// `WHERE scope IN (SELECT value FROM json_each(?n))` に渡す JSON 配列。
    pub fn as_json(&self) -> String {
        serde_json::to_string(&self.scopes).expect("Vec<String> serializes")
    }

    /// 複数 scope の明示指定時だけ監査ログに残す。
    pub fn audit_cross_read(&self, conn: &Connection, actor: &str, tool: &str) -> Result<(), ToolError> {
        if self.is_cross() {
            audit::record(conn, actor, "cross_scope_read", &json!({ "tool": tool, "scopes": self.scopes }))?;
        }
        Ok(())
    }
}

pub fn scope_input_to_vec(input: Option<&ScopeInput>) -> Option<Vec<String>> {
    match input {
        None => None,
        Some(ScopeInput::String(s)) => Some(vec![s.clone()]),
        Some(ScopeInput::Array(v)) => Some(v.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::ErrorCode, identity::Role, storage::{Db, StorageError, affiliations, audit}};

    fn client(default_scope: Option<&str>) -> ClientIdentity {
        ClientIdentity { name: "bot".into(), role: Role::Agent, default_scope: default_scope.map(String::from) }
    }

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "a", None)?;
            affiliations::insert(c, "b", None)?;
            Ok(())
        })
        .unwrap();
        db
    }

    #[test]
    fn falls_back_to_default_scope_and_is_not_cross() {
        let db = db();
        db.with_conn::<_, ToolError>(|c| {
            let s = ScopeSet::resolve(c, &client(Some("a")), None)?;
            assert_eq!(s.names(), ["a"]);
            assert!(!s.is_cross());
            assert_eq!(s.as_json(), "[\"a\"]");
            s.audit_cross_read(c, "bot", "search_context")?;
            assert!(audit::recent(c, 10)?.is_empty(), "single scope must not be audited");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn multiple_scopes_are_sorted_deduped_and_audited() {
        let db = db();
        db.with_conn::<_, ToolError>(|c| {
            let s = ScopeSet::resolve(c, &client(None), Some(vec!["b".into(), "a".into(), "b".into()]))?;
            assert_eq!(s.names(), ["a", "b"]);
            assert!(s.is_cross());
            assert!(s.contains("b"));
            s.audit_cross_read(c, "bot", "get_person")?;
            let entries = audit::recent(c, 10)?;
            assert_eq!(entries[0].action, "cross_scope_read");
            assert_eq!(entries[0].detail["tool"], "get_person");
            assert_eq!(entries[0].detail["scopes"], serde_json::json!(["a", "b"]));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn missing_scope_is_denied_and_unknown_scope_is_not_found() {
        let db = db();
        db.with_conn::<_, StorageError>(|c| {
            assert_eq!(ScopeSet::resolve(c, &client(None), None).unwrap_err().code, ErrorCode::ScopeDenied);
            assert_eq!(ScopeSet::resolve(c, &client(None), Some(vec![])).unwrap_err().code, ErrorCode::ScopeDenied);
            assert_eq!(
                ScopeSet::resolve(c, &client(Some("a")), Some(vec!["zzz".into()])).unwrap_err().code,
                ErrorCode::NotFound
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn scope_input_converts_string_and_array() {
        use crate::contracts::types::ScopeInput;
        assert_eq!(scope_input_to_vec(None), None);
        assert_eq!(scope_input_to_vec(Some(&ScopeInput::String("a".into()))), Some(vec!["a".to_string()]));
        assert_eq!(scope_input_to_vec(Some(&ScopeInput::Array(vec!["a".into(), "b".into()]))), Some(vec!["a".to_string(), "b".to_string()]));
    }
}

//! affiliations の管理。提案キュー原則の唯一の例外（機密境界そのものの定義のため）。
//! 必ず audit_log(admin_write) に残す。CLI / デスクトップの管理操作からのみ呼ぶ。
use rusqlite::Connection;
use serde_json::json;

use crate::{
    error::ToolError,
    storage::{Db, affiliations, audit},
};

pub fn add_affiliation(
    db: &Db,
    actor: &str,
    name: &str,
    identity: Option<&str>,
) -> Result<i64, ToolError> {
    db.with_tx(|tx| insert_affiliation(tx, actor, name, identity))
}

/// 設定保存に失敗した init の再開用。同じ内容の所属だけを無変更で再利用する。
pub fn initialize_affiliation(
    db: &Db,
    actor: &str,
    name: &str,
    identity: Option<&str>,
) -> Result<i64, ToolError> {
    let name = name.trim();
    db.with_tx(|tx| {
        if let Some(existing) = affiliations::find_by_name(tx, name)? {
            if existing.identity.as_deref() != identity {
                return Err(ToolError::conflict(format!(
                    "affiliation `{name}` already exists with a different identity"
                )));
            }
            return Ok(existing.id);
        }
        insert_affiliation(tx, actor, name, identity)
    })
}

fn insert_affiliation(
    conn: &Connection,
    actor: &str,
    name: &str,
    identity: Option<&str>,
) -> Result<i64, ToolError> {
    let id = affiliations::insert(conn, name, identity)?;
    audit::record(
        conn,
        actor,
        "admin_write",
        &json!({"op": "add_affiliation", "name": name}),
    )?;
    Ok(id)
}

pub fn list_affiliations(db: &Db) -> Result<Vec<affiliations::Affiliation>, ToolError> {
    db.with_conn(|c| Ok::<_, ToolError>(affiliations::list(c)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn initialization_reuses_matching_affiliation_without_duplicate_audit() {
        let db = Db::open_in_memory().unwrap();
        let id = initialize_affiliation(&db, "me", "cloudnative", Some("member")).unwrap();
        let original = list_affiliations(&db).unwrap();
        assert_eq!(
            initialize_affiliation(&db, "retry", "  cloudnative  ", Some("member")).unwrap(),
            id
        );
        for identity in [None, Some("different")] {
            let error = initialize_affiliation(&db, "retry", "cloudnative", identity).unwrap_err();
            assert_eq!(error.code, ErrorCode::Conflict);
        }
        assert!(add_affiliation(&db, "me", "cloudnative", Some("member")).is_err());
        assert_eq!(list_affiliations(&db).unwrap(), original);
        let entries = db.with_conn(|conn| audit::recent(conn, 10)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].actor, "me");
        assert_eq!(entries[0].action, "admin_write");
        assert_eq!(entries[0].detail["op"], "add_affiliation");
    }
}

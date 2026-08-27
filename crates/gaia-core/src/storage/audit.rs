//! 監査ログ。全書き込み・承認・横断読み取り・管理操作を actor 付きで残す。
use rusqlite::{Connection, params};
use serde_json::Value;

use super::StorageError;

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub detail: Value,
    pub at: String,
}

pub fn record(
    conn: &Connection,
    actor: &str,
    action: &str,
    detail: &Value,
) -> Result<i64, StorageError> {
    conn.execute(
        "INSERT INTO audit_log(actor, action, detail) VALUES (?1, ?2, ?3)",
        params![actor, action, detail.to_string()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn recent(conn: &Connection, limit: usize) -> Result<Vec<AuditEntry>, StorageError> {
    let mut stmt = conn
        .prepare("SELECT id, actor, action, detail, at FROM audit_log ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit as i64], |r| {
        let raw: String = r.get(3)?;
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            raw,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, actor, action, raw, at) = row?;
        out.push(AuditEntry {
            id,
            actor,
            action,
            detail: serde_json::from_str(&raw)?,
            at,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;
    use serde_json::json;

    #[test]
    fn record_and_read_back_newest_first() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            record(c, "me", "propose", &json!({"proposal_id": 1}))?;
            record(c, "bot", "cross_scope_read", &json!({"scopes": ["a", "b"]}))?;
            let entries = recent(c, 10)?;
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].actor, "bot");
            assert_eq!(entries[0].detail["scopes"][1], "b");
            assert_eq!(entries[1].action, "propose");
            Ok(())
        })
        .unwrap();
    }
}

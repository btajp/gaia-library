//! 組織（名寄せ層・共有）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::contracts::types::{OrganizationPatch, OrganizationSummary};

use super::{StorageError, like_pattern, required};

pub fn insert(conn: &Connection, patch: &OrganizationPatch) -> Result<i64, StorageError> {
    let name = required(patch.name.as_deref(), "organization.name")?;
    conn.execute(
        "INSERT INTO organizations(name, kind) VALUES (?1, ?2)",
        params![name, patch.kind],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: &OrganizationPatch) -> Result<(), StorageError> {
    ensure(conn, id)?;
    conn.execute(
        "UPDATE organizations SET name = COALESCE(?2, name), kind = COALESCE(?3, kind), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.name, patch.kind],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<OrganizationSummary>, StorageError> {
    Ok(conn
        .query_row(
            "SELECT id, name, kind FROM organizations WHERE id = ?1",
            params![id],
            row,
        )
        .optional()?)
}

pub fn ensure(conn: &Connection, id: i64) -> Result<(), StorageError> {
    if get(conn, id)?.is_none() {
        return Err(StorageError::NotFound(format!("organization {id}")));
    }
    Ok(())
}

pub fn find_by_name(
    conn: &Connection,
    name: &str,
) -> Result<Vec<OrganizationSummary>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT id, name, kind FROM organizations WHERE name = ?1 ORDER BY id")?;
    let rows = stmt.query_map(params![name], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn search_like(
    conn: &Connection,
    needle: &str,
    limit: usize,
) -> Result<Vec<OrganizationSummary>, StorageError> {
    let mut stmt =
        conn.prepare("SELECT id, name, kind FROM organizations WHERE name LIKE ?1 ESCAPE '\\' ORDER BY name LIMIT ?2")?;
    let rows = stmt.query_map(params![like_pattern(needle), limit as i64], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row(r: &Row<'_>) -> rusqlite::Result<OrganizationSummary> {
    Ok(OrganizationSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;
    use serde_json::json;

    fn patch(v: serde_json::Value) -> OrganizationPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn crud_and_search() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(
                c,
                &patch(json!({"name": "CloudNative", "kind": "affiliation"})),
            )?;
            assert!(matches!(
                insert(c, &patch(json!({}))),
                Err(StorageError::Integrity(_))
            ));
            update(c, id, &patch(json!({"kind": "customer"})))?;
            let got = get(c, id)?.unwrap();
            assert_eq!(got.name, "CloudNative");
            assert_eq!(got.kind.as_deref(), Some("customer"));
            assert_eq!(find_by_name(c, "CloudNative")?.len(), 1);
            assert_eq!(search_like(c, "loud", 10)?.len(), 1);
            assert!(matches!(
                update(c, 999, &patch(json!({}))),
                Err(StorageError::NotFound(_))
            ));
            Ok(())
        })
        .unwrap();
    }
}

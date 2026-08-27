//! 汎用エンティティ（名寄せ層・共有）。attrs は JSON オブジェクトを TEXT で持つ。
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Map, Value};

use crate::contracts::types::{EntityPatch, EntitySummary};

use super::{StorageError, like_pattern, required};

pub fn insert(conn: &Connection, patch: &EntityPatch) -> Result<i64, StorageError> {
    let type_ = required(patch.type_.as_deref(), "entity.type")?;
    let name = required(patch.name.as_deref(), "entity.name")?;
    let attrs = Value::Object(patch.attrs.clone()).to_string();
    conn.execute("INSERT INTO entities(type, name, attrs) VALUES (?1, ?2, ?3)", params![type_, name, attrs])?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, patch: &EntityPatch) -> Result<(), StorageError> {
    ensure(conn, id)?;
    // attrs は「空なら変更しない、非空なら全置換」
    let attrs = if patch.attrs.is_empty() { None } else { Some(Value::Object(patch.attrs.clone()).to_string()) };
    conn.execute(
        "UPDATE entities SET type = COALESCE(?2, type), name = COALESCE(?3, name), attrs = COALESCE(?4, attrs), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.type_, patch.name, attrs],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<EntitySummary>, StorageError> {
    let raw = conn
        .query_row("SELECT id, type, name, attrs FROM entities WHERE id = ?1", params![id], raw_row)
        .optional()?;
    raw.map(convert).transpose()
}

pub fn ensure(conn: &Connection, id: i64) -> Result<(), StorageError> {
    if get(conn, id)?.is_none() {
        return Err(StorageError::NotFound(format!("entity {id}")));
    }
    Ok(())
}

pub fn search_like(conn: &Connection, needle: &str, limit: usize) -> Result<Vec<EntitySummary>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT id, type, name, attrs FROM entities WHERE name LIKE ?1 ESCAPE '\\' OR type LIKE ?1 ESCAPE '\\' ORDER BY name LIMIT ?2",
    )?;
    let raws: Vec<RawEntity> = stmt.query_map(params![like_pattern(needle), limit as i64], raw_row)?.collect::<Result<_, _>>()?;
    raws.into_iter().map(convert).collect()
}

type RawEntity = (i64, String, String, String);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawEntity> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
}

fn convert((id, type_, name, attrs): RawEntity) -> Result<EntitySummary, StorageError> {
    let attrs: Map<String, Value> = serde_json::from_str(&attrs)?;
    Ok(EntitySummary { id, type_, name, attrs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;
    use serde_json::json;

    #[test]
    fn attrs_round_trip_and_search() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(c, &serde_json::from_value::<EntityPatch>(json!({"type": "product", "name": "gaia-library", "attrs": {"lang": "rust"}})).unwrap())?;
            let got = get(c, id)?.unwrap();
            assert_eq!(got.type_, "product");
            assert_eq!(got.attrs["lang"], "rust");
            update(c, id, &serde_json::from_value::<EntityPatch>(json!({"attrs": {"lang": "rust", "db": "sqlite"}})).unwrap())?;
            assert_eq!(get(c, id)?.unwrap().attrs["db"], "sqlite");
            assert_eq!(search_like(c, "prod", 10)?.len(), 1);
            assert!(matches!(insert(c, &serde_json::from_value::<EntityPatch>(json!({"name": "x"})).unwrap()), Err(StorageError::Integrity(_))));
            Ok(())
        })
        .unwrap();
    }
}

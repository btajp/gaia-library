//! polymorphic 参照先（entity_type + entity_id / target_type + target_id）の存在検証。
use rusqlite::{Connection, OptionalExtension, params};

use super::StorageError;

fn table_for(target_type: &str) -> Result<&'static str, StorageError> {
    Ok(match target_type {
        "person" => "people",
        "organization" => "organizations",
        "engagement" => "engagements",
        "interaction" => "interactions",
        "entity" => "entities",
        "fact" => "facts",
        other => return Err(StorageError::Integrity(format!("unknown target type `{other}`"))),
    })
}

pub fn exists(conn: &Connection, target_type: &str, id: i64) -> Result<bool, StorageError> {
    let table = table_for(target_type)?;
    let found: Option<i64> = conn
        .query_row(&format!("SELECT 1 FROM {table} WHERE id = ?1"), params![id], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

pub fn ensure(conn: &Connection, target_type: &str, id: i64) -> Result<(), StorageError> {
    if exists(conn, target_type, id)? {
        Ok(())
    } else {
        Err(StorageError::NotFound(format!("{target_type} {id}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::PersonPatch, storage::{Db, people}};
    use serde_json::json;

    #[test]
    fn exists_maps_types_to_tables_and_rejects_unknown() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "p"})).unwrap())?;
            assert!(exists(c, "person", id)?);
            assert!(!exists(c, "organization", 999)?);
            assert!(matches!(exists(c, "widget", 1), Err(StorageError::Integrity(_))));
            assert!(matches!(ensure(c, "fact", 999), Err(StorageError::NotFound(_))));
            Ok(())
        })
        .unwrap();
    }
}

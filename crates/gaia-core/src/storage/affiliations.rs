//! affiliations = scope の値域（機密境界の定義）。名寄せ層・共有。
use rusqlite::{Connection, OptionalExtension, params};

use super::StorageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Affiliation {
    pub id: i64,
    pub name: String,
    pub identity: Option<String>,
}

pub fn insert(conn: &Connection, name: &str, identity: Option<&str>) -> Result<i64, StorageError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StorageError::Integrity(
            "affiliation name must not be empty".into(),
        ));
    }
    if exists(conn, name)? {
        return Err(StorageError::Integrity(format!(
            "affiliation `{name}` already exists"
        )));
    }
    conn.execute(
        "INSERT INTO affiliations(name, identity) VALUES (?1, ?2)",
        params![name, identity],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn exists(conn: &Connection, name: &str) -> Result<bool, StorageError> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT id FROM affiliations WHERE name = ?1",
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(found.is_some())
}

pub fn find_by_name(conn: &Connection, name: &str) -> Result<Option<Affiliation>, StorageError> {
    Ok(conn
        .query_row(
            "SELECT id, name, identity FROM affiliations WHERE name = ?1",
            params![name],
            |r| {
                Ok(Affiliation {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    identity: r.get(2)?,
                })
            },
        )
        .optional()?)
}

pub fn list(conn: &Connection) -> Result<Vec<Affiliation>, StorageError> {
    let mut stmt = conn.prepare("SELECT id, name, identity FROM affiliations ORDER BY name")?;
    let rows = stmt.query_map([], |r| {
        Ok(Affiliation {
            id: r.get(0)?,
            name: r.get(1)?,
            identity: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    #[test]
    fn insert_exists_list_and_reject_duplicates() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(c, "cloudnative", Some("CN"))?;
            assert!(id > 0);
            assert!(exists(c, "cloudnative")?);
            assert!(!exists(c, "other")?);
            assert!(matches!(
                insert(c, "cloudnative", None),
                Err(StorageError::Integrity(_))
            ));
            assert!(matches!(
                insert(c, "  ", None),
                Err(StorageError::Integrity(_))
            ));
            insert(c, "assoc", None)?;
            let names: Vec<String> = list(c)?.into_iter().map(|a| a.name).collect();
            assert_eq!(names, vec!["assoc", "cloudnative"]);
            Ok(())
        })
        .unwrap();
    }
}

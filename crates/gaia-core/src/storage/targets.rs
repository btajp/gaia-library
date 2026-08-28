//! polymorphic 参照先（entity_type + entity_id / target_type + target_id）の存在検証。
//! 内容層（engagement / interaction / fact）は「scope 内に存在しなければ存在しない」扱いにし、
//! 他 scope に隠れている行と未登録の行を同じ NotFound で返す（存在オラクルにしない）。
use rusqlite::{Connection, OptionalExtension, params};

use crate::scope::ScopeSet;

use super::StorageError;

/// テーブル名と、scope で絞る内容層かどうか。
fn table_for(target_type: &str) -> Result<(&'static str, bool), StorageError> {
    Ok(match target_type {
        "person" => ("people", false),
        "organization" => ("organizations", false),
        "engagement" => ("engagements", true),
        "interaction" => ("interactions", true),
        "entity" => ("entities", false),
        "fact" => ("facts", true),
        other => {
            return Err(StorageError::Integrity(format!(
                "unknown target type `{other}`"
            )));
        }
    })
}

pub fn exists(
    conn: &Connection,
    target_type: &str,
    id: i64,
    scopes: &ScopeSet,
) -> Result<bool, StorageError> {
    let (table, scoped) = table_for(target_type)?;
    let found: Option<i64> = if scoped {
        conn.query_row(
            &format!(
                "SELECT 1 FROM {table} WHERE id = ?1 AND scope IN (SELECT value FROM json_each(?2))"
            ),
            params![id, scopes.as_json()],
            |r| r.get(0),
        )
    } else {
        conn.query_row(
            &format!("SELECT 1 FROM {table} WHERE id = ?1"),
            params![id],
            |r| r.get(0),
        )
    }
    .optional()?;
    Ok(found.is_some())
}

pub fn ensure(
    conn: &Connection,
    target_type: &str,
    id: i64,
    scopes: &ScopeSet,
) -> Result<(), StorageError> {
    if exists(conn, target_type, id, scopes)? {
        return Ok(());
    }
    let (_, scoped) = table_for(target_type)?;
    if scoped {
        let names: Vec<String> = scopes.names().iter().map(|s| format!("`{s}`")).collect();
        Err(StorageError::NotFound(format!(
            "{target_type} {id} (in scope {})",
            names.join(", ")
        )))
    } else {
        Err(StorageError::NotFound(format!("{target_type} {id}")))
    }
}

#[cfg(test)]
mod tool_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::{Kind, PersonPatch},
        storage::{
            Db, affiliations, engagements, entities, facts, interactions, organizations, people,
        },
    };
    use serde_json::json;

    #[test]
    fn exists_maps_types_to_tables_and_rejects_unknown() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = people::insert(
                c,
                &serde_json::from_value::<PersonPatch>(json!({"name": "p"})).unwrap(),
            )?;
            let scopes = ScopeSet::single("cn");
            assert!(exists(c, "person", id, &scopes)?);
            assert!(!exists(c, "organization", 999, &scopes)?);
            assert!(matches!(
                exists(c, "widget", 1, &scopes),
                Err(StorageError::Integrity(_))
            ));
            assert!(matches!(
                ensure(c, "fact", 999, &scopes),
                Err(StorageError::NotFound(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn content_target_checks_hide_other_scopes_with_the_same_missing_error() {
        for target_type in ["engagement", "interaction", "fact"] {
            let db = Db::open_in_memory().unwrap();
            db.with_conn::<_, StorageError>(|c| {
                affiliations::insert(c, "cn", None)?;
                affiliations::insert(c, "other", None)?;
                let person =
                    people::insert(c, &serde_json::from_value(json!({"name": "p"})).unwrap())?;
                let scopes = ScopeSet::single("cn");
                let missing = ensure(c, target_type, 1, &scopes).unwrap_err();
                let id = match target_type {
                    "engagement" => engagements::insert(
                        c,
                        &serde_json::from_value(json!({"name": "e"})).unwrap(),
                        "other",
                    )?,
                    "interaction" => interactions::insert(
                        c,
                        &serde_json::from_value(json!({
                            "kind": "meeting", "occurred_at": "2026-08-27", "summary": "s"
                        }))
                        .unwrap(),
                        "other",
                    )?,
                    "fact" => facts::insert(
                        c,
                        &serde_json::from_value(json!({
                            "entity_type": "person", "entity_id": person, "statement": "s"
                        }))
                        .unwrap(),
                        Kind::Fact,
                        "other",
                    )?,
                    _ => unreachable!(),
                };
                assert_eq!(id, 1);
                assert!(!exists(c, target_type, id, &scopes)?);
                let hidden = ensure(c, target_type, id, &scopes).unwrap_err();
                assert!(matches!(&hidden, StorageError::NotFound(_)));
                assert_eq!(hidden.to_string(), missing.to_string(), "{target_type}");
                assert!(exists(c, target_type, id, &ScopeSet::single("other"))?);
                ensure(c, target_type, id, &ScopeSet::single("other"))?;
                Ok(())
            })
            .unwrap();
        }
    }

    #[test]
    fn identity_targets_remain_shared() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let person = people::insert(c, &serde_json::from_value(json!({"name": "p"})).unwrap())?;
            let organization =
                organizations::insert(c, &serde_json::from_value(json!({"name": "o"})).unwrap())?;
            let entity = entities::insert(
                c,
                &serde_json::from_value(json!({"type": "document", "name": "d"})).unwrap(),
            )?;
            for (target_type, id) in [
                ("person", person),
                ("organization", organization),
                ("entity", entity),
            ] {
                for scope in ["cn", "other"] {
                    assert!(exists(c, target_type, id, &ScopeSet::single(scope))?);
                    ensure(c, target_type, id, &ScopeSet::single(scope))?;
                }
            }
            Ok(())
        })
        .unwrap();
    }
}

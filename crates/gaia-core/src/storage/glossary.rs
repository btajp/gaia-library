//! 案件別用語集（内容層・scope 必須）。Whisper の initial_prompt に注入する語彙ヒントの供給源。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{GlossaryPatch, GlossaryTerm},
    scope::ScopeSet,
};

use super::{StorageError, engagements, like_pattern, required};

const COLS: &str = "id, term, reading, definition, engagement_id, scope";

pub fn insert(conn: &Connection, patch: &GlossaryPatch, scope: &str) -> Result<i64, StorageError> {
    let term = required(patch.term.as_deref(), "glossary.term")?;
    if let Some(eid) = patch.engagement_id
        && engagements::get(conn, eid, &ScopeSet::single(scope))?.is_none()
    {
        return Err(StorageError::NotFound(format!(
            "engagement {eid} (in scope `{scope}`)"
        )));
    }
    conn.execute(
        "INSERT INTO glossary(term, reading, definition, engagement_id, scope) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![term, patch.reading, patch.definition, patch.engagement_id, scope],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(
    conn: &Connection,
    id: i64,
    patch: &GlossaryPatch,
    scope: &str,
) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!(
            "glossary term {id} (in scope `{scope}`)"
        )));
    }
    conn.execute(
        "UPDATE glossary SET term = COALESCE(?2, term), reading = COALESCE(?3, reading), \
         definition = COALESCE(?4, definition), engagement_id = COALESCE(?5, engagement_id), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.term, patch.reading, patch.definition, patch.engagement_id],
    )?;
    Ok(())
}

pub fn get(
    conn: &Connection,
    id: i64,
    scopes: &ScopeSet,
) -> Result<Option<GlossaryTerm>, StorageError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM glossary WHERE id = ?1 AND scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            row,
        )
        .optional()?)
}

/// engagement_id 指定ならその案件の用語、None なら scope 内の全用語。
pub fn list(
    conn: &Connection,
    engagement_id: Option<i64>,
    scopes: &ScopeSet,
) -> Result<Vec<GlossaryTerm>, StorageError> {
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match engagement_id {
        Some(eid) => (
            format!(
                "SELECT {COLS} FROM glossary WHERE engagement_id = ?1 AND scope IN (SELECT value FROM json_each(?2)) ORDER BY term"
            ),
            vec![Box::new(eid), Box::new(scopes.as_json())],
        ),
        None => (
            format!(
                "SELECT {COLS} FROM glossary WHERE scope IN (SELECT value FROM json_each(?1)) ORDER BY term"
            ),
            vec![Box::new(scopes.as_json())],
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params_vec.iter().map(|p| p.as_ref())),
        row,
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn search_like(
    conn: &Connection,
    needle: &str,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<GlossaryTerm>, StorageError> {
    let pat = like_pattern(needle);
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM glossary WHERE (term LIKE ?1 ESCAPE '\\' OR reading LIKE ?1 ESCAPE '\\' OR definition LIKE ?1 ESCAPE '\\') \
         AND scope IN (SELECT value FROM json_each(?2)) ORDER BY term LIMIT ?3"
    ))?;
    let rows = stmt.query_map(params![pat, scopes.as_json(), limit as i64], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row(r: &Row<'_>) -> rusqlite::Result<GlossaryTerm> {
    Ok(GlossaryTerm {
        id: r.get(0)?,
        term: r.get(1)?,
        reading: r.get(2)?,
        definition: r.get(3)?,
        engagement_id: r.get(4)?,
        scope: r.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::EngagementPatch,
        storage::{Db, affiliations},
    };
    use serde_json::json;

    #[test]
    fn list_by_engagement_or_all_in_scope() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let eid = engagements::insert(
                c,
                &serde_json::from_value::<EngagementPatch>(json!({"name": "案件A"})).unwrap(),
                "cn",
            )?;
            insert(
                c,
                &serde_json::from_value::<GlossaryPatch>(
                    json!({"term": "SCIM", "reading": "スキム", "engagement_id": eid}),
                )
                .unwrap(),
                "cn",
            )?;
            insert(
                c,
                &serde_json::from_value::<GlossaryPatch>(json!({"term": "IdP"})).unwrap(),
                "cn",
            )?;
            assert_eq!(list(c, Some(eid), &ScopeSet::single("cn"))?.len(), 1);
            assert_eq!(list(c, None, &ScopeSet::single("cn"))?.len(), 2);
            assert_eq!(
                search_like(c, "スキム", &ScopeSet::single("cn"), 10)?.len(),
                1
            );
            Ok(())
        })
        .unwrap();
    }
}

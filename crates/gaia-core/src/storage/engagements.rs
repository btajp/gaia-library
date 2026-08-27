//! 案件（内容層・scope 必須）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{EngagementPatch, EngagementPerson, EngagementSummary},
    scope::ScopeSet,
};

use super::{StorageError, like_pattern, organizations, people, required};

const COLS: &str = "e.id, e.name, e.org_id, o.name, e.scope, e.status, e.started_at, e.ended_at";
const FROM: &str = "FROM engagements e LEFT JOIN organizations o ON o.id = e.org_id";

pub fn insert(
    conn: &Connection,
    patch: &EngagementPatch,
    scope: &str,
) -> Result<i64, StorageError> {
    let name = required(patch.name.as_deref(), "engagement.name")?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "INSERT INTO engagements(name, org_id, scope, status, started_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![name, patch.org_id, scope, patch.status, patch.started_at, patch.ended_at],
    )?;
    let id = conn.last_insert_rowid();
    for p in &patch.people {
        add_person(conn, id, p.person_id, p.role.as_deref())?;
    }
    Ok(id)
}

pub fn update(
    conn: &Connection,
    id: i64,
    patch: &EngagementPatch,
    scope: &str,
) -> Result<(), StorageError> {
    ensure_in_scope(conn, id, scope)?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "UPDATE engagements SET name = COALESCE(?2, name), org_id = COALESCE(?3, org_id), status = COALESCE(?4, status), \
         started_at = COALESCE(?5, started_at), ended_at = COALESCE(?6, ended_at), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.name, patch.org_id, patch.status, patch.started_at, patch.ended_at],
    )?;
    for p in &patch.people {
        add_person(conn, id, p.person_id, p.role.as_deref())?;
    }
    Ok(())
}

fn ensure_in_scope(conn: &Connection, id: i64, scope: &str) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!(
            "engagement {id} (in scope `{scope}`)"
        )));
    }
    Ok(())
}

pub fn add_person(
    conn: &Connection,
    engagement_id: i64,
    person_id: i64,
    role: Option<&str>,
) -> Result<(), StorageError> {
    people::ensure(conn, person_id)?;
    conn.execute(
        "INSERT INTO engagement_people(engagement_id, person_id, role) VALUES (?1, ?2, ?3) \
         ON CONFLICT(engagement_id, person_id) DO UPDATE SET role = COALESCE(excluded.role, role)",
        params![engagement_id, person_id, role],
    )?;
    Ok(())
}

pub fn get(
    conn: &Connection,
    id: i64,
    scopes: &ScopeSet,
) -> Result<Option<EngagementSummary>, StorageError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} {FROM} WHERE e.id = ?1 AND e.scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            row,
        )
        .optional()?)
}

pub fn find_by_name(
    conn: &Connection,
    name: &str,
    scopes: &ScopeSet,
) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} WHERE e.name = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) ORDER BY e.id"
    ))?;
    let rows = stmt.query_map(params![name, scopes.as_json()], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 案件の関係者（PersonSummary ＋ 役割）。
pub fn members(
    conn: &Connection,
    engagement_id: i64,
    scopes: &ScopeSet,
) -> Result<Vec<EngagementPerson>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT ep.person_id, ep.role FROM engagement_people ep \
         JOIN engagements e ON e.id = ep.engagement_id \
         WHERE ep.engagement_id = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY ep.person_id",
    )?;
    let pairs: Vec<(i64, Option<String>)> = stmt
        .query_map(params![engagement_id, scopes.as_json()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(pairs.len());
    for (pid, role) in pairs {
        if let Some(person) = people::get(conn, pid)? {
            out.push(EngagementPerson { person, role });
        }
    }
    Ok(out)
}

pub fn member_ids(
    conn: &Connection,
    engagement_id: i64,
    scopes: &ScopeSet,
) -> Result<Vec<i64>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT ep.person_id FROM engagement_people ep \
         JOIN engagements e ON e.id = ep.engagement_id \
         WHERE ep.engagement_id = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY ep.person_id",
    )?;
    Ok(stmt
        .query_map(params![engagement_id, scopes.as_json()], |r| r.get(0))?
        .collect::<Result<_, _>>()?)
}

pub fn for_person(
    conn: &Connection,
    person_id: i64,
    scopes: &ScopeSet,
) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} JOIN engagement_people ep ON ep.engagement_id = e.id \
         WHERE ep.person_id = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) ORDER BY e.id"
    ))?;
    let rows = stmt.query_map(params![person_id, scopes.as_json()], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn for_org(
    conn: &Connection,
    org_id: i64,
    scopes: &ScopeSet,
) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} WHERE e.org_id = ?1 AND e.scope IN (SELECT value FROM json_each(?2)) ORDER BY e.id"
    ))?;
    let rows = stmt.query_map(params![org_id, scopes.as_json()], row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn search_like(
    conn: &Connection,
    needle: &str,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<EngagementSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} {FROM} WHERE e.name LIKE ?1 ESCAPE '\\' AND e.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY e.name LIMIT ?3"
    ))?;
    let rows = stmt.query_map(
        params![like_pattern(needle), scopes.as_json(), limit as i64],
        row,
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row(r: &Row<'_>) -> rusqlite::Result<EngagementSummary> {
    Ok(EngagementSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        org_id: r.get(2)?,
        org_name: r.get(3)?,
        scope: r.get(4)?,
        status: r.get(5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::PersonPatch,
        storage::{Db, affiliations},
    };
    use serde_json::json;

    #[test]
    fn scope_filters_and_members_work() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            let pid = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap())?;
            let patch: EngagementPatch = serde_json::from_value(json!({
                "name": "RELATIONS支援", "status": "active", "people": [{"person_id": pid, "role": "key_person"}]
            }))
            .unwrap();
            let id = insert(c, &patch, "cn")?;
            assert!(get(c, id, &ScopeSet::single("cn"))?.is_some());
            assert!(get(c, id, &ScopeSet::single("other"))?.is_none(), "scope 外からは見えない");
            let cn = ScopeSet::single("cn");
            let other = ScopeSet::single("other");
            assert_eq!(members(c, id, &cn)?.len(), 1);
            assert_eq!(member_ids(c, id, &cn)?, vec![pid]);
            assert!(members(c, id, &other)?.is_empty());
            assert!(member_ids(c, id, &other)?.is_empty());
            assert_eq!(for_person(c, pid, &ScopeSet::single("cn"))?.len(), 1);
            assert_eq!(search_like(c, "RELATIONS", &ScopeSet::single("cn"), 10)?.len(), 1);
            assert!(matches!(
                update(c, id, &serde_json::from_value::<EngagementPatch>(json!({"status": "done"})).unwrap(), "other"),
                Err(StorageError::NotFound(_))
            ));
            update(c, id, &serde_json::from_value::<EngagementPatch>(json!({"status": "done"})).unwrap(), "cn")?;
            assert_eq!(get(c, id, &ScopeSet::single("cn"))?.unwrap().status.as_deref(), Some("done"));
            Ok(())
        })
        .unwrap();
    }
}

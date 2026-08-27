//! 会議・面談ログ（内容層・scope 必須）。全文は持たず要点のみ（正本は参照先）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{InteractionPatch, InteractionSummary},
    scope::ScopeSet,
};

use super::{StorageError, engagements, like_pattern, people, required};

const COLS: &str = "i.id, i.kind, i.occurred_at, i.summary, i.engagement_id, i.scope";

pub fn insert(
    conn: &Connection,
    patch: &InteractionPatch,
    scope: &str,
) -> Result<i64, StorageError> {
    let kind = required(patch.kind.as_deref(), "interaction.kind")?;
    let occurred_at = required(patch.occurred_at.as_deref(), "interaction.occurred_at")?;
    let summary = required(patch.summary.as_deref(), "interaction.summary")?;
    ensure_valid_occurred_at(conn, occurred_at)?;
    ensure_engagement_in_scope(conn, patch.engagement_id, scope)?;
    for pid in &patch.person_ids {
        people::ensure(conn, *pid)?;
    }
    conn.execute(
        "INSERT INTO interactions(kind, occurred_at, summary, engagement_id, scope) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind, occurred_at, summary, patch.engagement_id, scope],
    )?;
    let id = conn.last_insert_rowid();
    link_people(conn, id, &patch.person_ids)?;
    Ok(id)
}

pub fn update(
    conn: &Connection,
    id: i64,
    patch: &InteractionPatch,
    scope: &str,
) -> Result<(), StorageError> {
    let existing = get(conn, id, &ScopeSet::single(scope))?
        .ok_or_else(|| StorageError::NotFound(format!("interaction {id} (in scope `{scope}`)")))?;
    ensure_valid_occurred_at(
        conn,
        patch
            .occurred_at
            .as_deref()
            .unwrap_or(&existing.occurred_at),
    )?;
    ensure_engagement_in_scope(conn, patch.engagement_id, scope)?;
    for pid in &patch.person_ids {
        people::ensure(conn, *pid)?;
    }
    conn.execute(
        "UPDATE interactions SET kind = COALESCE(?2, kind), occurred_at = COALESCE(?3, occurred_at), \
         summary = COALESCE(?4, summary), engagement_id = COALESCE(?5, engagement_id) WHERE id = ?1",
        params![id, patch.kind, patch.occurred_at, patch.summary, patch.engagement_id],
    )?;
    link_people(conn, id, &patch.person_ids)?;
    Ok(())
}

fn ensure_valid_occurred_at(conn: &Connection, occurred_at: &str) -> Result<(), StorageError> {
    let epoch: Option<i64> =
        conn.query_row("SELECT unixepoch(?1)", params![occurred_at], |r| r.get(0))?;
    if epoch.is_none() {
        return Err(StorageError::Integrity(
            "interaction.occurred_at must be a valid SQLite date/time".into(),
        ));
    }
    Ok(())
}

fn ensure_engagement_in_scope(
    conn: &Connection,
    engagement_id: Option<i64>,
    scope: &str,
) -> Result<(), StorageError> {
    if let Some(eid) = engagement_id
        && engagements::get(conn, eid, &ScopeSet::single(scope))?.is_none()
    {
        return Err(StorageError::NotFound(format!(
            "engagement {eid} (in scope `{scope}`)"
        )));
    }
    Ok(())
}

fn link_people(
    conn: &Connection,
    interaction_id: i64,
    person_ids: &[i64],
) -> Result<(), StorageError> {
    for pid in person_ids {
        conn.execute(
            "INSERT INTO interaction_people(interaction_id, person_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            params![interaction_id, pid],
        )?;
    }
    Ok(())
}

pub fn get(
    conn: &Connection,
    id: i64,
    scopes: &ScopeSet,
) -> Result<Option<InteractionSummary>, StorageError> {
    let base = conn
        .query_row(
            &format!("SELECT {COLS} FROM interactions i WHERE i.id = ?1 AND i.scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            row,
        )
        .optional()?;
    fill_people(conn, base.into_iter().collect()).map(|mut v| v.pop())
}

pub fn recent_for_person(
    conn: &Connection,
    person_id: i64,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM interactions i JOIN interaction_people ip ON ip.interaction_id = i.id \
         WHERE ip.person_id = ?1 AND i.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY unixepoch(i.occurred_at) DESC, i.id DESC LIMIT ?3"
    ))?;
    let rows: Vec<InteractionSummary> = stmt
        .query_map(params![person_id, scopes.as_json(), limit as i64], row)?
        .collect::<Result<_, _>>()?;
    fill_people(conn, rows)
}

pub fn recent_for_engagement(
    conn: &Connection,
    engagement_id: i64,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM interactions i JOIN engagements e ON e.id = i.engagement_id AND e.scope = i.scope \
         WHERE i.engagement_id = ?1 AND i.scope IN (SELECT value FROM json_each(?2)) \
         AND e.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY unixepoch(i.occurred_at) DESC, i.id DESC LIMIT ?3"
    ))?;
    let rows: Vec<InteractionSummary> = stmt
        .query_map(params![engagement_id, scopes.as_json(), limit as i64], row)?
        .collect::<Result<_, _>>()?;
    fill_people(conn, rows)
}

pub fn search_like(
    conn: &Connection,
    needle: &str,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM interactions i WHERE i.summary LIKE ?1 ESCAPE '\\' AND i.scope IN (SELECT value FROM json_each(?2)) \
         ORDER BY unixepoch(i.occurred_at) DESC, i.id DESC LIMIT ?3"
    ))?;
    let rows: Vec<InteractionSummary> = stmt
        .query_map(
            params![like_pattern(needle), scopes.as_json(), limit as i64],
            row,
        )?
        .collect::<Result<_, _>>()?;
    fill_people(conn, rows)
}

fn fill_people(
    conn: &Connection,
    mut rows: Vec<InteractionSummary>,
) -> Result<Vec<InteractionSummary>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT person_id FROM interaction_people WHERE interaction_id = ?1 ORDER BY person_id",
    )?;
    for r in &mut rows {
        r.person_ids = stmt
            .query_map(params![r.id], |x| x.get(0))?
            .collect::<Result<_, _>>()?;
    }
    Ok(rows)
}

fn row(r: &Row<'_>) -> rusqlite::Result<InteractionSummary> {
    Ok(InteractionSummary {
        id: r.get(0)?,
        kind: r.get(1)?,
        occurred_at: r.get(2)?,
        summary: r.get(3)?,
        engagement_id: r.get(4)?,
        scope: r.get(5)?,
        person_ids: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::{EngagementPatch, PersonPatch},
        storage::{Db, affiliations},
    };
    use serde_json::json;

    #[test]
    fn insert_links_people_and_scope_filters_reads() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let pid = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap())?;
            let patch: InteractionPatch = serde_json::from_value(json!({
                "kind": "meeting", "occurred_at": "2026-08-27T10:00:00Z", "summary": "定例。次回は 9/3", "person_ids": [pid]
            }))
            .unwrap();
            let id = insert(c, &patch, "cn")?;
            let got = get(c, id, &ScopeSet::single("cn"))?.unwrap();
            assert_eq!(got.person_ids, vec![pid]);
            assert_eq!(recent_for_person(c, pid, &ScopeSet::single("cn"), 10)?.len(), 1);
            assert_eq!(search_like(c, "定例", &ScopeSet::single("cn"), 10)?.len(), 1);
            // summary 無しは拒否
            assert!(matches!(
                insert(c, &serde_json::from_value::<InteractionPatch>(json!({"kind": "call", "occurred_at": "2026-08-27"})).unwrap(), "cn"),
                Err(StorageError::Integrity(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn recent_queries_order_by_absolute_time_then_id() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let pid = people::insert(
                c,
                &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap(),
            )?;
            let engagement_id = engagements::insert(
                c,
                &serde_json::from_value::<EngagementPatch>(
                    json!({"name": "時刻順テスト", "people": []}),
                )
                .unwrap(),
                "cn",
            )?;

            let mut ids = Vec::new();
            for occurred_at in [
                "2026-08-27T10:00:00+09:00",
                "2026-08-27T03:00:00Z",
                "2026-08-27T12:00:00+09:00",
            ] {
                ids.push(insert(
                    c,
                    &serde_json::from_value::<InteractionPatch>(json!({
                        "kind": "meeting",
                        "occurred_at": occurred_at,
                        "summary": "offset 順序テスト",
                        "engagement_id": engagement_id,
                        "person_ids": [pid]
                    }))
                    .unwrap(),
                    "cn",
                )?);
            }
            let expected = vec![ids[2], ids[1], ids[0]];
            assert_eq!(
                recent_for_person(c, pid, &ScopeSet::single("cn"), 10)?
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                recent_for_engagement(c, engagement_id, &ScopeSet::single("cn"), 10)?
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                search_like(c, "offset", &ScopeSet::single("cn"), 10)?
                    .into_iter()
                    .map(|item| item.id)
                    .collect::<Vec<_>>(),
                expected
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn insert_and_update_reject_unparseable_occurred_at() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let invalid: InteractionPatch = serde_json::from_value(json!({
                "kind": "meeting",
                "occurred_at": "not-a-date",
                "summary": "invalid"
            }))
            .unwrap();
            assert!(matches!(
                insert(c, &invalid, "cn"),
                Err(StorageError::Integrity(_))
            ));

            let id = insert(
                c,
                &serde_json::from_value::<InteractionPatch>(json!({
                    "kind": "meeting",
                    "occurred_at": "2026-08-27T10:00:00Z",
                    "summary": "valid"
                }))
                .unwrap(),
                "cn",
            )?;
            assert!(matches!(
                update(c, id, &invalid, "cn"),
                Err(StorageError::Integrity(_))
            ));

            c.execute(
                "UPDATE interactions SET occurred_at = 'invalid-existing-value' WHERE id = ?1",
                params![id],
            )?;
            let summary_only: InteractionPatch =
                serde_json::from_value(json!({"summary": "updated"})).unwrap();
            assert!(matches!(
                update(c, id, &summary_only, "cn"),
                Err(StorageError::Integrity(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn cross_scope_engagement_links_and_reads_are_rejected() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            let other_engagement_id = engagements::insert(
                c,
                &serde_json::from_value::<EngagementPatch>(
                    json!({"name": "別 scope 案件", "people": []}),
                )
                .unwrap(),
                "other",
            )?;
            let cross_scope_patch: InteractionPatch = serde_json::from_value(json!({
                "kind": "meeting",
                "occurred_at": "2026-08-27T10:00:00Z",
                "summary": "cross scope",
                "engagement_id": other_engagement_id
            }))
            .unwrap();
            assert!(matches!(
                insert(c, &cross_scope_patch, "cn"),
                Err(StorageError::NotFound(_))
            ));

            let cn_interaction_id = insert(
                c,
                &serde_json::from_value::<InteractionPatch>(json!({
                    "kind": "meeting",
                    "occurred_at": "2026-08-27T10:00:00Z",
                    "summary": "cn interaction"
                }))
                .unwrap(),
                "cn",
            )?;
            assert!(matches!(
                update(c, cn_interaction_id, &cross_scope_patch, "cn"),
                Err(StorageError::NotFound(_))
            ));

            // 旧版や DB 直編集で不整合なリンクがあっても、案件起点では越境表示しない。
            c.execute(
                "UPDATE interactions SET engagement_id = ?2 WHERE id = ?1",
                params![cn_interaction_id, other_engagement_id],
            )?;
            assert!(
                recent_for_engagement(c, other_engagement_id, &ScopeSet::single("cn"), 10)?
                    .is_empty()
            );
            Ok(())
        })
        .unwrap();
    }
}

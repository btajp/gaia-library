//! facts（内容層・scope 必須）。「現在の fact」= superseded_by IS NULL。検索は trigram FTS（3 文字未満は LIKE）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{Fact, FactPatch, Kind},
    scope::ScopeSet,
};

use super::{StorageError, like_pattern, parse_db_enum, required, targets};

const COLS: &str = "f.id, f.entity_type, f.entity_id, f.statement, f.predicate, f.value, f.kind, f.scope, f.valid_from, f.superseded_by, f.created_at";

pub fn insert(
    conn: &Connection,
    patch: &FactPatch,
    kind: Kind,
    scope: &str,
) -> Result<i64, StorageError> {
    let entity_type = patch
        .entity_type
        .ok_or_else(|| StorageError::Integrity("fact.entity_type is required".into()))?;
    let entity_id = patch
        .entity_id
        .ok_or_else(|| StorageError::Integrity("fact.entity_id is required".into()))?;
    let statement = required(patch.statement.as_deref(), "fact.statement")?;
    // 内容層のターゲットは提案の scope 内に存在する行だけを「存在する」とみなす。
    targets::ensure(
        conn,
        &entity_type.to_string(),
        entity_id,
        &ScopeSet::single(scope),
    )?;
    conn.execute(
        "INSERT INTO facts(entity_type, entity_id, statement, predicate, value, kind, scope, valid_from) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entity_type.to_string(),
            entity_id,
            statement,
            patch.predicate,
            patch.value,
            kind.to_string(),
            scope,
            patch.valid_from
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 旧 fact を新 fact で置き換える（superseded_by リンク）。旧 fact は同じ scope 内・未置換であること。
pub fn supersede(
    conn: &Connection,
    old_id: i64,
    patch: &FactPatch,
    kind: Kind,
    scope: &str,
) -> Result<i64, StorageError> {
    let old = get(conn, old_id, &ScopeSet::single(scope))?
        .ok_or_else(|| StorageError::NotFound(format!("fact {old_id} (in scope `{scope}`)")))?;
    if let Some(by) = old.superseded_by {
        return Err(StorageError::Integrity(format!(
            "fact {old_id} is already superseded by {by}"
        )));
    }
    let entity_type = patch
        .entity_type
        .ok_or_else(|| StorageError::Integrity("fact.entity_type is required".into()))?;
    let entity_id = patch
        .entity_id
        .ok_or_else(|| StorageError::Integrity("fact.entity_id is required".into()))?;
    if entity_type != old.entity_type || entity_id != old.entity_id {
        return Err(StorageError::Integrity(
            "superseding fact must keep the same entity_type and entity_id".into(),
        ));
    }
    let new_id = insert(conn, patch, kind, scope)?;
    conn.execute(
        "UPDATE facts SET superseded_by = ?2 WHERE id = ?1",
        params![old_id, new_id],
    )?;
    Ok(new_id)
}

pub fn update(
    conn: &Connection,
    id: i64,
    patch: &FactPatch,
    scope: &str,
) -> Result<(), StorageError> {
    let current = get(conn, id, &ScopeSet::single(scope))?
        .ok_or_else(|| StorageError::NotFound(format!("fact {id} (in scope `{scope}`)")))?;
    if let Some(by) = current.superseded_by {
        return Err(StorageError::Integrity(format!(
            "fact {id} is historical and cannot be updated; superseded by {by}"
        )));
    }
    if patch.entity_type.is_some() || patch.entity_id.is_some() {
        return Err(StorageError::Integrity(
            "fact update cannot change entity_type or entity_id".into(),
        ));
    }
    let statement = patch
        .statement
        .as_deref()
        .map(|value| required(Some(value), "fact.statement"))
        .transpose()?;
    let updated = conn.execute(
        "UPDATE facts SET statement = COALESCE(?2, statement), predicate = COALESCE(?3, predicate), \
         value = COALESCE(?4, value), valid_from = COALESCE(?5, valid_from) \
         WHERE id = ?1 AND superseded_by IS NULL",
        params![id, statement, patch.predicate, patch.value, patch.valid_from],
    )?;
    if updated != 1 {
        return Err(StorageError::Integrity(format!(
            "fact {id} is no longer current and cannot be updated"
        )));
    }
    Ok(())
}

pub fn get(conn: &Connection, id: i64, scopes: &ScopeSet) -> Result<Option<Fact>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM facts f WHERE f.id = ?1 AND f.scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
}

/// エンティティに付く現在の facts（新しい順）。
pub fn for_entity(
    conn: &Connection,
    entity_type: &str,
    entity_id: i64,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<Fact>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM facts f WHERE f.entity_type = ?1 AND f.entity_id = ?2 AND f.superseded_by IS NULL \
         AND f.scope IN (SELECT value FROM json_each(?3)) ORDER BY f.id DESC LIMIT ?4"
    ))?;
    let raws: Vec<RawFact> = stmt
        .query_map(
            params![entity_type, entity_id, scopes.as_json(), limit as i64],
            raw_row,
        )?
        .collect::<Result<_, _>>()?;
    raws.into_iter().map(convert).collect()
}

/// 全文検索。3 文字（Unicode 文字数）以上は trigram FTS を bm25 順で、未満は LIKE。
pub fn search(
    conn: &Connection,
    query: &str,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<Fact>, StorageError> {
    if query.chars().count() >= 3 {
        let match_expr = format!("\"{}\"", query.replace('"', "\"\""));
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM (SELECT rowid AS fid, rank FROM facts_fts WHERE facts_fts MATCH ?1) m \
             JOIN facts f ON f.id = m.fid \
             WHERE f.superseded_by IS NULL AND f.scope IN (SELECT value FROM json_each(?2)) \
             ORDER BY m.rank LIMIT ?3"
        ))?;
        let raws: Vec<RawFact> = stmt
            .query_map(params![match_expr, scopes.as_json(), limit as i64], raw_row)?
            .collect::<Result<_, _>>()?;
        raws.into_iter().map(convert).collect()
    } else {
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM facts f WHERE f.statement LIKE ?1 ESCAPE '\\' AND f.superseded_by IS NULL \
             AND f.scope IN (SELECT value FROM json_each(?2)) ORDER BY f.id DESC LIMIT ?3"
        ))?;
        let raws: Vec<RawFact> = stmt
            .query_map(
                params![like_pattern(query), scopes.as_json(), limit as i64],
                raw_row,
            )?
            .collect::<Result<_, _>>()?;
        raws.into_iter().map(convert).collect()
    }
}

type RawFact = (
    i64,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<i64>,
    String,
);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawFact> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
    ))
}

fn convert(raw: RawFact) -> Result<Fact, StorageError> {
    let (
        id,
        entity_type,
        entity_id,
        statement,
        predicate,
        value,
        kind,
        scope,
        valid_from,
        superseded_by,
        created_at,
    ) = raw;
    Ok(Fact {
        id,
        entity_type: parse_db_enum(&entity_type, "fact entity_type")?,
        entity_id,
        statement,
        predicate,
        value,
        kind: parse_db_enum(&kind, "fact kind")?,
        scope,
        valid_from,
        superseded_by,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::{EngagementPatch, OrganizationPatch, PersonPatch},
        storage::{Db, affiliations, engagements, organizations, people},
    };
    use serde_json::json;

    fn fixture(db: &Db) -> i64 {
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            people::insert(
                c,
                &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap(),
            )
        })
        .unwrap()
    }

    fn fp(v: serde_json::Value) -> FactPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn insert_requires_existing_target_and_search_respects_scope() {
        let db = Db::open_in_memory().unwrap();
        let pid = fixture(&db);
        db.with_conn::<_, StorageError>(|c| {
            assert!(matches!(
                insert(c, &fp(json!({"entity_type": "person", "entity_id": 999, "statement": "x"})), Kind::Fact, "cn"),
                Err(StorageError::NotFound(_))
            ));
            insert(c, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "Okta の移行を支援している"})), Kind::Fact, "cn")?;
            insert(c, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "極秘の件"})), Kind::Inference, "other")?;
            // FTS（3 文字以上）は scope 内だけ
            assert_eq!(search(c, "Okta", &ScopeSet::single("cn"), 10)?.len(), 1);
            assert_eq!(search(c, "極秘の件", &ScopeSet::single("cn"), 10)?.len(), 0);
            // 2 文字は LIKE フォールバック
            assert_eq!(search(c, "移行", &ScopeSet::single("cn"), 10)?.len(), 1);
            assert_eq!(for_entity(c, "person", pid, &ScopeSet::single("cn"), 20)?.len(), 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn supersede_links_history_and_hides_old_from_search() {
        let db = Db::open_in_memory().unwrap();
        let pid = fixture(&db);
        db.with_conn::<_, StorageError>(|c| {
            let old = insert(c, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "役職はマネージャー", "predicate": "role", "value": "manager"})), Kind::Fact, "cn")?;
            let new = supersede(c, old, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "役職は部長", "predicate": "role", "value": "director"})), Kind::Fact, "cn")?;
            assert_eq!(get(c, old, &ScopeSet::single("cn"))?.unwrap().superseded_by, Some(new));
            let current = for_entity(c, "person", pid, &ScopeSet::single("cn"), 20)?;
            assert_eq!(current.len(), 1);
            assert_eq!(current[0].id, new);
            assert_eq!(search(c, "マネージャー", &ScopeSet::single("cn"), 10)?.len(), 0, "置換済みは検索に出ない");
            assert!(matches!(supersede(c, old, &fp(json!({"entity_type": "person", "entity_id": pid, "statement": "x"})), Kind::Fact, "cn"), Err(StorageError::Integrity(_))));
            assert!(matches!(
                update(c, old, &fp(json!({"statement": "履歴の改変"})), "cn"),
                Err(StorageError::Integrity(_))
            ));
            assert_eq!(
                get(c, old, &ScopeSet::single("cn"))?.unwrap().statement,
                "役職はマネージャー"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn supersede_rejects_a_different_entity_target() {
        let db = Db::open_in_memory().unwrap();
        let pid = fixture(&db);
        db.with_conn::<_, StorageError>(|c| {
            let other_pid = people::insert(
                c,
                &serde_json::from_value::<PersonPatch>(json!({"name": "別の人物"})).unwrap(),
            )?;
            let old = insert(
                c,
                &fp(json!({
                    "entity_type": "person",
                    "entity_id": pid,
                    "statement": "旧情報"
                })),
                Kind::Fact,
                "cn",
            )?;
            assert!(matches!(
                supersede(
                    c,
                    old,
                    &fp(json!({
                        "entity_type": "person",
                        "entity_id": other_pid,
                        "statement": "別人の新情報"
                    })),
                    Kind::Fact,
                    "cn"
                ),
                Err(StorageError::Integrity(_))
            ));
            let org_id = organizations::insert(
                c,
                &serde_json::from_value::<OrganizationPatch>(json!({"name": "別の組織"})).unwrap(),
            )?;
            assert!(matches!(
                supersede(
                    c,
                    old,
                    &fp(json!({
                        "entity_type": "organization",
                        "entity_id": org_id,
                        "statement": "別種別の新情報"
                    })),
                    Kind::Fact,
                    "cn"
                ),
                Err(StorageError::Integrity(_))
            ));
            assert_eq!(
                get(c, old, &ScopeSet::single("cn"))?.unwrap().superseded_by,
                None
            );
            assert!(for_entity(c, "person", other_pid, &ScopeSet::single("cn"), 10)?.is_empty());
            assert!(for_entity(c, "organization", org_id, &ScopeSet::single("cn"), 10)?.is_empty());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn update_rejects_link_changes_and_blank_statement() {
        let db = Db::open_in_memory().unwrap();
        let pid = fixture(&db);
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(
                c,
                &fp(json!({
                    "entity_type": "person",
                    "entity_id": pid,
                    "statement": "変更前"
                })),
                Kind::Fact,
                "cn",
            )?;
            assert!(matches!(
                update(c, id, &fp(json!({"entity_id": pid})), "cn"),
                Err(StorageError::Integrity(_))
            ));
            assert!(matches!(
                update(c, id, &fp(json!({"statement": "  "})), "cn"),
                Err(StorageError::Integrity(_))
            ));
            assert_eq!(
                get(c, id, &ScopeSet::single("cn"))?.unwrap().statement,
                "変更前"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn insert_rejects_engagement_target_from_a_different_scope() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            let eid = engagements::insert(
                c,
                &serde_json::from_value::<EngagementPatch>(json!({"name": "案件X"})).unwrap(),
                "other",
            )?;
            // scope を跨いだ engagement へのターゲットは拒否
            assert!(matches!(
                insert(
                    c,
                    &fp(json!({"entity_type": "engagement", "entity_id": eid, "statement": "x"})),
                    Kind::Fact,
                    "cn"
                ),
                Err(StorageError::NotFound(_))
            ));
            // 一致する scope なら成功
            assert!(
                insert(
                    c,
                    &fp(json!({"entity_type": "engagement", "entity_id": eid, "statement": "x"})),
                    Kind::Fact,
                    "other"
                )
                .is_ok()
            );
            Ok(())
        })
        .unwrap();
    }
}

//! refs = 「思い出し方の索引」の実体（内容層・scope 必須）。URI だけの参照（note 無し）は登録禁止。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{RefPatch, RefTargetType, Reference},
    scope::ScopeSet,
};

use super::{StorageError, engagements, facts, interactions, parse_db_enum, required, targets};

const COLS: &str = "id, target_type, target_id, system, uri, title, note, snapshot, scope, last_verified, created_at";

pub fn insert(conn: &Connection, patch: &RefPatch, scope: &str) -> Result<i64, StorageError> {
    let target_type = patch
        .target_type
        .ok_or_else(|| StorageError::Integrity("ref.target_type is required".into()))?;
    let target_id = patch
        .target_id
        .ok_or_else(|| StorageError::Integrity("ref.target_id is required".into()))?;
    let system = required(patch.system.as_deref(), "ref.system")?;
    let uri = required(patch.uri.as_deref(), "ref.uri")?;
    let note = required(patch.note.as_deref(), "ref.note")?;
    targets::ensure(conn, &target_type.to_string(), target_id)?;
    ensure_content_target_in_scope(conn, target_type, target_id, scope)?;
    conn.execute(
        "INSERT INTO refs(target_type, target_id, system, uri, title, note, snapshot, scope, last_verified) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            target_type.to_string(),
            target_id,
            system,
            uri,
            patch.title,
            note,
            patch.snapshot,
            scope,
            patch.last_verified
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// target_type が内容層（engagement / interaction / fact）のとき、ターゲット行が提案の scope 内に
/// 存在することを検証する。person / organization / entity（名寄せ層）は共有のため未検査。
fn ensure_content_target_in_scope(
    conn: &Connection,
    target_type: RefTargetType,
    target_id: i64,
    scope: &str,
) -> Result<(), StorageError> {
    if target_type == RefTargetType::Engagement
        && engagements::get(conn, target_id, &ScopeSet::single(scope))?.is_none()
    {
        return Err(StorageError::NotFound(format!(
            "{target_type} {target_id} (in scope `{scope}`)"
        )));
    }
    if target_type == RefTargetType::Interaction
        && interactions::get(conn, target_id, &ScopeSet::single(scope))?.is_none()
    {
        return Err(StorageError::NotFound(format!(
            "{target_type} {target_id} (in scope `{scope}`)"
        )));
    }
    if target_type == RefTargetType::Fact
        && facts::get(conn, target_id, &ScopeSet::single(scope))?.is_none()
    {
        return Err(StorageError::NotFound(format!(
            "{target_type} {target_id} (in scope `{scope}`)"
        )));
    }
    Ok(())
}

/// 紐付け先（target_type / target_id）は変更不可。内容だけ更新する。
pub fn update(
    conn: &Connection,
    id: i64,
    patch: &RefPatch,
    scope: &str,
) -> Result<(), StorageError> {
    if get(conn, id, &ScopeSet::single(scope))?.is_none() {
        return Err(StorageError::NotFound(format!(
            "ref {id} (in scope `{scope}`)"
        )));
    }
    conn.execute(
        "UPDATE refs SET system = COALESCE(?2, system), uri = COALESCE(?3, uri), title = COALESCE(?4, title), \
         note = COALESCE(?5, note), snapshot = COALESCE(?6, snapshot), last_verified = COALESCE(?7, last_verified) WHERE id = ?1",
        params![id, patch.system, patch.uri, patch.title, patch.note, patch.snapshot, patch.last_verified],
    )?;
    Ok(())
}

pub fn get(
    conn: &Connection,
    id: i64,
    scopes: &ScopeSet,
) -> Result<Option<Reference>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM refs WHERE id = ?1 AND scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
}

pub fn for_target(
    conn: &Connection,
    target_type: &str,
    target_id: i64,
    scopes: &ScopeSet,
) -> Result<Vec<Reference>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM refs WHERE target_type = ?1 AND target_id = ?2 AND scope IN (SELECT value FROM json_each(?3)) ORDER BY id"
    ))?;
    let raws: Vec<RawRef> = stmt
        .query_map(params![target_type, target_id, scopes.as_json()], raw_row)?
        .collect::<Result<_, _>>()?;
    raws.into_iter().map(convert).collect()
}

type RawRef = (
    i64,
    String,
    i64,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    Option<String>,
    String,
);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawRef> {
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

fn convert(raw: RawRef) -> Result<Reference, StorageError> {
    let (
        id,
        target_type,
        target_id,
        system,
        uri,
        title,
        note,
        snapshot,
        scope,
        last_verified,
        created_at,
    ) = raw;
    Ok(Reference {
        id,
        target_type: parse_db_enum(&target_type, "ref target_type")?,
        target_id,
        system,
        uri,
        title,
        note,
        snapshot,
        scope,
        last_verified,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::{FactPatch, Kind, PersonPatch},
        storage::{Db, affiliations, facts, people},
    };
    use serde_json::json;

    #[test]
    fn note_is_mandatory_and_fact_targets_are_allowed() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            let pid = people::insert(c, &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap())?;
            let fid = facts::insert(
                c,
                &serde_json::from_value::<FactPatch>(json!({"entity_type": "person", "entity_id": pid, "statement": "決定: SSO を導入する"})).unwrap(),
                Kind::Fact,
                "cn",
            )?;
            // URI だけ（note 無し）は禁止
            assert!(matches!(
                insert(c, &serde_json::from_value::<RefPatch>(json!({"target_type": "person", "target_id": pid, "system": "notion", "uri": "https://x"})).unwrap(), "cn"),
                Err(StorageError::Integrity(_))
            ));
            // fact への根拠参照
            let rid = insert(
                c,
                &serde_json::from_value::<RefPatch>(json!({
                    "target_type": "fact", "target_id": fid, "system": "minutes",
                    "uri": "minutes://meeting/42#t=1200", "note": "決定箇所の議事録参照", "snapshot": "SSO 導入を決定"
                }))
                .unwrap(),
                "cn",
            )?;
            let refs = for_target(c, "fact", fid, &ScopeSet::single("cn"))?;
            assert_eq!(refs.len(), 1);
            assert_eq!(refs[0].id, rid);
            assert_eq!(refs[0].snapshot.as_deref(), Some("SSO 導入を決定"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn insert_rejects_fact_target_from_a_different_scope() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            affiliations::insert(c, "other", None)?;
            let pid = people::insert(
                c,
                &serde_json::from_value::<PersonPatch>(json!({"name": "岡村"})).unwrap(),
            )?;
            let fid = facts::insert(
                c,
                &serde_json::from_value::<FactPatch>(
                    json!({"entity_type": "person", "entity_id": pid, "statement": "極秘の件"}),
                )
                .unwrap(),
                Kind::Fact,
                "other",
            )?;
            let patch = || {
                serde_json::from_value::<RefPatch>(json!({
                    "target_type": "fact", "target_id": fid, "system": "notion",
                    "uri": "https://x", "note": "n"
                }))
                .unwrap()
            };
            // scope を跨いだ fact へのターゲットは拒否
            assert!(matches!(
                insert(c, &patch(), "cn"),
                Err(StorageError::NotFound(_))
            ));
            // 一致する scope なら成功
            assert!(insert(c, &patch(), "other").is_ok());
            Ok(())
        })
        .unwrap();
    }
}

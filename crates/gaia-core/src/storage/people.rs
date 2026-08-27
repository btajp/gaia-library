//! 人物（名寄せ層・共有）。name と alias の正規化形を kind='normalized' の行として自動登録する（仕様書 §5.3）。
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    contracts::types::{Alias, PersonPatch, PersonSummary},
    domain::normalize::normalize_name,
};

use super::{StorageError, like_pattern, organizations, required};

pub fn insert(conn: &Connection, patch: &PersonPatch) -> Result<i64, StorageError> {
    let name = required(patch.name.as_deref(), "person.name")?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "INSERT INTO people(name, org_id, role, first_met, last_seen) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            name,
            patch.org_id,
            patch.role,
            patch.first_met,
            patch.last_seen
        ],
    )?;
    let id = conn.last_insert_rowid();
    add_alias(conn, id, name, Some("name"))?;
    for a in &patch.aliases {
        add_alias(conn, id, &a.alias, a.kind.as_deref())?;
    }
    Ok(id)
}

pub fn update(conn: &Connection, id: i64, patch: &PersonPatch) -> Result<(), StorageError> {
    ensure(conn, id)?;
    if let Some(org_id) = patch.org_id {
        organizations::ensure(conn, org_id)?;
    }
    conn.execute(
        "UPDATE people SET name = COALESCE(?2, name), org_id = COALESCE(?3, org_id), role = COALESCE(?4, role), \
         first_met = COALESCE(?5, first_met), last_seen = COALESCE(?6, last_seen), updated_at = datetime('now') WHERE id = ?1",
        params![id, patch.name, patch.org_id, patch.role, patch.first_met, patch.last_seen],
    )?;
    if let Some(name) = &patch.name {
        add_alias(conn, id, name, Some("name"))?;
    }
    for a in &patch.aliases {
        add_alias(conn, id, &a.alias, a.kind.as_deref())?;
    }
    Ok(())
}

/// 生の alias と、その正規化形（kind='normalized'）を登録する。既存行は上書きしない。
pub fn add_alias(
    conn: &Connection,
    person_id: i64,
    alias: &str,
    kind: Option<&str>,
) -> Result<(), StorageError> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Err(StorageError::Integrity("alias is required".into()));
    }
    conn.execute(
        "INSERT INTO person_aliases(person_id, alias, kind) VALUES (?1, ?2, ?3) ON CONFLICT(person_id, alias) DO NOTHING",
        params![person_id, alias, kind],
    )?;
    let normalized = normalize_name(alias);
    if !normalized.is_empty() && normalized != alias {
        conn.execute(
            "INSERT INTO person_aliases(person_id, alias, kind) VALUES (?1, ?2, 'normalized') ON CONFLICT(person_id, alias) DO NOTHING",
            params![person_id, normalized],
        )?;
    }
    Ok(())
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<PersonSummary>, StorageError> {
    let base = conn
        .query_row(
            "SELECT p.id, p.name, p.org_id, o.name, p.role, p.first_met, p.last_seen \
             FROM people p LEFT JOIN organizations o ON o.id = p.org_id WHERE p.id = ?1",
            params![id],
            row_to_summary,
        )
        .optional()?;
    match base {
        None => Ok(None),
        Some(mut p) => {
            p.aliases = aliases(conn, p.id)?;
            Ok(Some(p))
        }
    }
}

pub fn ensure(conn: &Connection, id: i64) -> Result<(), StorageError> {
    if get(conn, id)?.is_none() {
        return Err(StorageError::NotFound(format!("person {id}")));
    }
    Ok(())
}

/// 表示用 alias（kind='normalized' の行は隠す）。
pub fn aliases(conn: &Connection, person_id: i64) -> Result<Vec<Alias>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT alias, kind FROM person_aliases WHERE person_id = ?1 AND (kind IS NULL OR kind <> 'normalized') ORDER BY alias",
    )?;
    let rows = stmt.query_map(params![person_id], |r| {
        Ok(Alias {
            alias: r.get(0)?,
            kind: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 氏名・alias・正規化形いずれかの完全一致。
pub fn find_by_name(conn: &Connection, name: &str) -> Result<Vec<PersonSummary>, StorageError> {
    let normalized = normalize_name(name);
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id FROM people p LEFT JOIN person_aliases a ON a.person_id = p.id \
         WHERE p.name = ?1 OR a.alias = ?1 OR a.alias = ?2 ORDER BY p.id",
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![name, normalized], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

/// 正規化済み表示名の完全一致（resolve_speakers の第一経路）。
pub fn find_by_alias_normalized(
    conn: &Connection,
    normalized: &str,
) -> Result<Vec<PersonSummary>, StorageError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT person_id FROM person_aliases WHERE alias = ?1 ORDER BY person_id",
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![normalized], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

pub fn search_like(
    conn: &Connection,
    needle: &str,
    limit: usize,
) -> Result<Vec<PersonSummary>, StorageError> {
    let raw = like_pattern(needle);
    let norm = like_pattern(&normalize_name(needle));
    let mut stmt = conn.prepare(
        "SELECT DISTINCT p.id FROM people p LEFT JOIN person_aliases a ON a.person_id = p.id \
         WHERE p.name LIKE ?1 ESCAPE '\\' OR a.alias LIKE ?1 ESCAPE '\\' OR a.alias LIKE ?2 ESCAPE '\\' \
         ORDER BY p.id LIMIT ?3",
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![raw, norm, limit as i64], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

pub fn list_by_org(conn: &Connection, org_id: i64) -> Result<Vec<PersonSummary>, StorageError> {
    let mut stmt = conn.prepare("SELECT id FROM people WHERE org_id = ?1 ORDER BY name")?;
    let ids: Vec<i64> = stmt
        .query_map(params![org_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    load_many(conn, &ids)
}

pub fn load_many(conn: &Connection, ids: &[i64]) -> Result<Vec<PersonSummary>, StorageError> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(p) = get(conn, *id)? {
            out.push(p);
        }
    }
    Ok(out)
}

fn row_to_summary(r: &Row<'_>) -> rusqlite::Result<PersonSummary> {
    Ok(PersonSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        org_id: r.get(2)?,
        org_name: r.get(3)?,
        role: r.get(4)?,
        first_met: r.get(5)?,
        last_seen: r.get(6)?,
        aliases: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::types::OrganizationPatch, storage::Db};
    use serde_json::json;

    #[test]
    fn insert_registers_normalized_aliases_and_lookup_works() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let org = organizations::insert(
                c,
                &serde_json::from_value::<OrganizationPatch>(json!({"name": "CloudNative"}))
                    .unwrap(),
            )?;
            let p: PersonPatch = serde_json::from_value(json!({
                "name": "岡村 慎太郎", "org_id": org, "role": "CTO",
                "aliases": [{"alias": "Okamura Shintaro", "kind": "romaji"}]
            }))
            .unwrap();
            let id = insert(c, &p)?;
            // 表示 alias に normalized 行は含まれない
            let names: Vec<String> = aliases(c, id)?.into_iter().map(|a| a.alias).collect();
            assert_eq!(names, vec!["Okamura Shintaro", "岡村 慎太郎"]);
            // 正規化完全一致
            assert_eq!(find_by_alias_normalized(c, "岡村慎太郎")?.len(), 1);
            assert_eq!(find_by_alias_normalized(c, "okamurashintaro")?.len(), 1);
            // 表示名（括弧付き）でも find_by_name が正規化経由でヒットし、org 名が載る
            let found = find_by_name(c, "岡村 慎太郎 (CloudNative)")?;
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].org_name.as_deref(), Some("CloudNative"));
            // 部分一致
            assert_eq!(search_like(c, "岡村", 10)?.len(), 1);
            // name 無しの insert は拒否
            assert!(matches!(
                insert(
                    c,
                    &serde_json::from_value::<PersonPatch>(json!({})).unwrap()
                ),
                Err(StorageError::Integrity(_))
            ));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn update_coalesces_and_adds_aliases() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let id = insert(
                c,
                &serde_json::from_value::<PersonPatch>(json!({"name": "田中 太郎"})).unwrap(),
            )?;
            update(
                c,
                id,
                &serde_json::from_value::<PersonPatch>(
                    json!({"role": "PM", "aliases": [{"alias": "tanaka"}]}),
                )
                .unwrap(),
            )?;
            let got = get(c, id)?.unwrap();
            assert_eq!(got.name, "田中 太郎");
            assert_eq!(got.role.as_deref(), Some("PM"));
            assert_eq!(find_by_alias_normalized(c, "tanaka")?.len(), 1);
            assert!(matches!(
                update(
                    c,
                    999,
                    &serde_json::from_value::<PersonPatch>(json!({})).unwrap()
                ),
                Err(StorageError::NotFound(_))
            ));
            Ok(())
        })
        .unwrap();
    }
}

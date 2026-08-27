//! 提案キューの永続化。全書き込みの唯一の入口(適用ロジックは domain::proposals)。
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Map, Value};

use crate::{
    contracts::types::{
        Kind, Proposal, ProposalAction, ProposalStatus, ProposalTargetType, Provenance,
    },
    scope::ScopeSet,
};

use super::{StorageError, parse_db_enum};

#[derive(Debug, Clone)]
pub struct NewProposal {
    pub action: ProposalAction,
    pub target_type: ProposalTargetType,
    pub target_id: Option<i64>,
    pub patch: Map<String, Value>,
    pub kind: Kind,
    pub scope: String,
    pub provenance: Option<Provenance>,
    pub provenance_id: Option<i64>,
    pub proposed_by: String,
    pub request_id: String,
}

pub fn insert(conn: &Connection, p: &NewProposal) -> Result<i64, StorageError> {
    let provenance_json = match &p.provenance {
        Some(v) => Some(serde_json::to_string(v)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO proposals(action, target_type, target_id, patch, kind, scope, provenance, provenance_id, proposed_by, request_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            p.action.to_string(),
            p.target_type.to_string(),
            p.target_id,
            Value::Object(p.patch.clone()).to_string(),
            p.kind.to_string(),
            p.scope,
            provenance_json,
            p.provenance_id,
            p.proposed_by,
            p.request_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

const COLS: &str = "id, action, target_type, target_id, patch, kind, scope, provenance, provenance_id, proposed_by, request_id, status, result_id, decision_note, created_at, decided_at, decided_by";

type RawProposal = (
    i64,
    String,
    String,
    Option<i64>,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    String,
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
);

fn raw_row(r: &Row<'_>) -> rusqlite::Result<RawProposal> {
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
        r.get(11)?,
        r.get(12)?,
        r.get(13)?,
        r.get(14)?,
        r.get(15)?,
        r.get(16)?,
    ))
}

fn convert(raw: RawProposal) -> Result<Proposal, StorageError> {
    let (
        id,
        action,
        target_type,
        target_id,
        patch,
        kind,
        scope,
        provenance,
        provenance_id,
        proposed_by,
        request_id,
        status,
        result_id,
        decision_note,
        created_at,
        decided_at,
        decided_by,
    ) = raw;
    let patch_value: Value = serde_json::from_str(&patch)?;
    let Value::Object(patch) = patch_value else {
        return Err(StorageError::Integrity(format!(
            "proposal {id} patch is not a JSON object"
        )));
    };
    let provenance: Option<Provenance> = match provenance {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    };
    Ok(Proposal {
        id,
        action: parse_db_enum(&action, "proposal action")?,
        target_type: parse_db_enum(&target_type, "proposal target_type")?,
        target_id,
        patch,
        kind: parse_db_enum(&kind, "proposal kind")?,
        scope,
        provenance,
        provenance_id,
        proposed_by,
        request_id,
        status: parse_db_enum(&status, "proposal status")?,
        result_id,
        decision_note,
        created_at,
        decided_at,
        decided_by,
    })
}

pub fn get(
    conn: &Connection,
    id: i64,
    scopes: &ScopeSet,
) -> Result<Option<Proposal>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM proposals WHERE id = ?1 AND scope IN (SELECT value FROM json_each(?2))"),
            params![id, scopes.as_json()],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
}

pub fn find_by_request_id(
    conn: &Connection,
    request_id: &str,
    scopes: &ScopeSet,
) -> Result<Option<Proposal>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM proposals WHERE request_id = ?1 AND scope IN (SELECT value FROM json_each(?2))"),
            params![request_id, scopes.as_json()],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
}

/// 既存提案と再送内容で異なる、冪等性判定対象のフィールド名を返す。
pub fn differing_submission_fields(
    existing: &Proposal,
    candidate: &NewProposal,
    submitted_provenance: &Option<Provenance>,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if existing.target_type != candidate.target_type {
        fields.push("target_type");
    }
    if existing.action != candidate.action {
        fields.push("action");
    }
    if existing.target_id != candidate.target_id {
        fields.push("target_id");
    }
    if existing.patch != candidate.patch {
        fields.push("patch");
    }
    if existing.kind != candidate.kind {
        fields.push("kind");
    }
    if existing.scope != candidate.scope {
        fields.push("scope");
    }
    let provenance_matches = match (&existing.provenance, submitted_provenance) {
        (None, None) => existing.provenance_id.is_none(),
        (None, Some(submitted)) => {
            submitted.ref_id == existing.provenance_id
                && submitted.system.is_none()
                && submitted.uri.is_none()
                && submitted.title.is_none()
                && submitted.note.is_none()
                && submitted.snapshot.is_none()
        }
        (Some(stored), Some(submitted)) if stored == submitted => {
            // inline provenance の provenance_id は承認時に生成される結果なので比較対象外。
            stored
                .ref_id
                .is_none_or(|ref_id| existing.provenance_id == Some(ref_id))
        }
        _ => false,
    };
    if !provenance_matches {
        fields.push("provenance");
    }
    fields
}

/// クライアント・scope ごとの未決提案数。永続化上限の判定に使う。
pub fn count_pending_by_client_scope(
    conn: &Connection,
    proposed_by: &str,
    scope: &str,
) -> Result<u64, StorageError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proposals WHERE proposed_by = ?1 AND scope = ?2 AND status = 'pending'",
        params![proposed_by, scope],
        |row| row.get(0),
    )?;
    u64::try_from(count).map_err(|_| {
        StorageError::Integrity(format!(
            "negative pending proposal count for client `{proposed_by}` in scope `{scope}`"
        ))
    })
}

pub fn list(
    conn: &Connection,
    status: ProposalStatus,
    scopes: &ScopeSet,
    limit: usize,
) -> Result<Vec<Proposal>, StorageError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM proposals WHERE status = ?1 AND scope IN (SELECT value FROM json_each(?2)) ORDER BY id DESC LIMIT ?3"
    ))?;
    let raws: Vec<RawProposal> = stmt
        .query_map(
            params![status.to_string(), scopes.as_json(), limit as i64],
            raw_row,
        )?
        .collect::<Result<_, _>>()?;
    raws.into_iter().map(convert).collect()
}

pub struct Decision<'a> {
    pub status: ProposalStatus,
    pub decided_by: &'a str,
    pub result_id: Option<i64>,
    pub provenance_id: Option<i64>,
    pub note: Option<&'a str>,
}

/// pending の提案だけを決定できる(それ以外は NotFound)。
pub fn decide(conn: &Connection, id: i64, d: &Decision<'_>) -> Result<(), StorageError> {
    let n = conn.execute(
        "UPDATE proposals SET status = ?2, decided_by = ?3, decided_at = datetime('now'), result_id = ?4, \
         provenance_id = COALESCE(?5, provenance_id), decision_note = ?6 WHERE id = ?1 AND status = 'pending'",
        params![id, d.status.to_string(), d.decided_by, d.result_id, d.provenance_id, d.note],
    )?;
    if n == 0 {
        return Err(StorageError::NotFound(format!("pending proposal {id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::storage::{Db, affiliations};

    fn setup() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|conn| {
            affiliations::insert(conn, "cn", None)?;
            affiliations::insert(conn, "other", None)?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn candidate(request_id: &str) -> NewProposal {
        NewProposal {
            action: "insert".parse().unwrap(),
            target_type: "person".parse().unwrap(),
            target_id: None,
            patch: json!({"name": "田中 太郎"}).as_object().unwrap().clone(),
            kind: "fact".parse().unwrap(),
            scope: "cn".into(),
            provenance: None,
            provenance_id: None,
            proposed_by: "bot".into(),
            request_id: request_id.into(),
        }
    }

    #[test]
    fn idempotency_comparison_covers_all_submission_fields() {
        let db = setup();
        db.with_conn::<_, StorageError>(|conn| {
            let base = candidate("req-exact-1");
            let id = insert(conn, &base)?;
            let existing = get(conn, id, &ScopeSet::single("cn"))?.unwrap();
            assert!(differing_submission_fields(&existing, &base, &None).is_empty());

            let mut changed = base.clone();
            changed.target_type = "organization".parse().unwrap();
            assert_eq!(
                differing_submission_fields(&existing, &changed, &None),
                ["target_type"]
            );

            let mut changed = base.clone();
            changed.action = "update".parse().unwrap();
            assert_eq!(
                differing_submission_fields(&existing, &changed, &None),
                ["action"]
            );

            let mut changed = base.clone();
            changed.target_id = Some(42);
            assert_eq!(
                differing_submission_fields(&existing, &changed, &None),
                ["target_id"]
            );

            let mut changed = base.clone();
            changed.patch.insert("role".into(), json!("PM"));
            assert_eq!(
                differing_submission_fields(&existing, &changed, &None),
                ["patch"]
            );

            let mut changed = base.clone();
            changed.kind = "inference".parse().unwrap();
            assert_eq!(
                differing_submission_fields(&existing, &changed, &None),
                ["kind"]
            );

            let mut changed = base.clone();
            changed.scope = "other".into();
            assert_eq!(
                differing_submission_fields(&existing, &changed, &None),
                ["scope"]
            );

            let mut stored_ref = existing.clone();
            stored_ref.provenance_id = Some(42);
            let mut changed = base;
            changed.provenance_id = Some(42);
            let ref_only: Provenance = serde_json::from_value(json!({"ref_id": 42})).unwrap();
            assert!(differing_submission_fields(&stored_ref, &changed, &Some(ref_only)).is_empty());
            let mixed: Provenance =
                serde_json::from_value(json!({"ref_id": 42, "note": "changed"})).unwrap();
            assert_eq!(
                differing_submission_fields(&stored_ref, &changed, &Some(mixed)),
                ["provenance"]
            );

            let inline: Provenance = serde_json::from_value(json!({
                "system": "notion", "uri": "https://example.test", "note": "source"
            }))
            .unwrap();
            let mut stored_inline = existing.clone();
            stored_inline.provenance = Some(inline.clone());
            stored_inline.provenance_id = Some(99);
            let mut changed = candidate("req-exact-1");
            changed.provenance = Some(inline.clone());
            assert!(
                differing_submission_fields(&stored_inline, &changed, &Some(inline)).is_empty()
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn pending_count_is_partitioned_by_client_and_scope() {
        let db = setup();
        db.with_conn::<_, StorageError>(|conn| {
            let first = candidate("req-count-1");
            let first_id = insert(conn, &first)?;

            let mut second = candidate("req-count-2");
            insert(conn, &second)?;

            second.request_id = "req-count-other-scope".into();
            second.scope = "other".into();
            insert(conn, &second)?;

            second.request_id = "req-count-other-client".into();
            second.scope = "cn".into();
            second.proposed_by = "another-bot".into();
            insert(conn, &second)?;

            assert_eq!(count_pending_by_client_scope(conn, "bot", "cn")?, 2);
            assert_eq!(count_pending_by_client_scope(conn, "bot", "other")?, 1);
            assert_eq!(count_pending_by_client_scope(conn, "another-bot", "cn")?, 1);

            decide(
                conn,
                first_id,
                &Decision {
                    status: "rejected".parse().unwrap(),
                    decided_by: "human",
                    result_id: None,
                    provenance_id: None,
                    note: None,
                },
            )?;
            assert_eq!(count_pending_by_client_scope(conn, "bot", "cn")?, 1);
            Ok(())
        })
        .unwrap();
    }
}

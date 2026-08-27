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

pub fn get(conn: &Connection, id: i64) -> Result<Option<Proposal>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM proposals WHERE id = ?1"),
            params![id],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
}

pub fn find_by_request_id(
    conn: &Connection,
    request_id: &str,
) -> Result<Option<Proposal>, StorageError> {
    let raw = conn
        .query_row(
            &format!("SELECT {COLS} FROM proposals WHERE request_id = ?1"),
            params![request_id],
            raw_row,
        )
        .optional()?;
    raw.map(convert).transpose()
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

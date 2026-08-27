//! 提案系ツール。書き込みは全てここを通る。仕様書 §8.4。
use serde_json::json;

use crate::{
    contracts::types::{
        ApplyResult, ApproveProposalInput, ApproveProposalOutput, ListProposalsInput,
        ListProposalsOutput, ProposalStatus, ProposalTargetType, ProposeUpdateInput,
        ProposeUpdateOutput, RejectProposalInput, RejectProposalOutput, ScopeInput,
    },
    domain,
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{StorageError, audit, proposals, refs},
};

use super::CallContext;

const MAX_PROPOSAL_JSON_BYTES: usize = 1024 * 1024;
const MAX_PENDING_PROPOSALS_PER_CLIENT_SCOPE: u64 = 1_000;
const MAX_REQUEST_ID_BYTES: usize = 256;

enum ProposeAttempt {
    Accepted(ProposeUpdateOutput),
    Rejected(ToolError),
}

fn serialized_json_size(
    patch: &serde_json::Map<String, serde_json::Value>,
    provenance: &Option<crate::contracts::types::Provenance>,
) -> Result<(usize, usize, usize), ToolError> {
    let patch_bytes = serde_json::to_vec(patch)?.len();
    let provenance_bytes = provenance
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()?
        .map_or(0, |bytes| bytes.len());
    let total_bytes = patch_bytes.checked_add(provenance_bytes).ok_or_else(|| {
        ToolError::invalid_params("serialized patch and provenance size overflowed")
    })?;
    Ok((patch_bytes, provenance_bytes, total_bytes))
}

pub fn propose_update(
    ctx: &CallContext<'_>,
    input: ProposeUpdateInput,
) -> Result<ProposeUpdateOutput, ToolError> {
    if input.request_id.len() > MAX_REQUEST_ID_BYTES {
        return Err(
            ToolError::invalid_params("request_id must be at most 256 UTF-8 bytes").with_details(
                json!({
                    "request_id_bytes": input.request_id.len(),
                    "limit_bytes": MAX_REQUEST_ID_BYTES,
                }),
            ),
        );
    }
    if input.request_id.trim().chars().count() < 8 {
        return Err(ToolError::invalid_params(
            "request_id must be at least 8 characters",
        ));
    }
    let attempt = ctx.db.with_tx(|tx| {
        let scopes = ScopeSet::resolve(tx, ctx.client, input.scope.clone().map(|scope| vec![scope]))?;
        let scope = &scopes.names()[0];
        let (provenance, provenance_id) = match &input.provenance {
            Some(p) if p.ref_id.is_some() => (
                None,
                Some(p.ref_id.expect("checked")),
            ),
            Some(p) => (Some(p.clone()), None),
            None => (None, None),
        };
        let candidate = proposals::NewProposal {
            action: input.action,
            target_type: input.target_type,
            target_id: input.target_id,
            patch: input.patch.clone(),
            kind: input.kind,
            scope: scope.clone(),
            provenance,
            provenance_id,
            proposed_by: ctx.client.name.clone(),
            request_id: input.request_id.clone(),
        };

        // 件数・容量ガードより先に完全一致を判定し、上限到達後の正当な再送も成功させる。
        if let Some(existing) = proposals::find_by_request_id(tx, &input.request_id, &scopes)? {
            if existing.proposed_by != ctx.client.name {
                audit::record(
                    tx,
                    &ctx.client.name,
                    "propose_conflict",
                    &json!({
                        "request_id": input.request_id,
                        "reason": "request_id_owner_mismatch",
                    }),
                )?;
                return Ok(ProposeAttempt::Rejected(ToolError::conflict(format!(
                    "request_id `{}` was already used by another client",
                    input.request_id
                ))));
            }

            let differing_fields = proposals::differing_submission_fields(
                &existing,
                &candidate,
                &input.provenance,
            );
            if differing_fields.is_empty() {
                return Ok(ProposeAttempt::Accepted(ProposeUpdateOutput {
                    proposal_id: existing.id,
                    status: existing.status,
                    duplicate: true,
                }));
            }

            audit::record(
                tx,
                &ctx.client.name,
                "propose_conflict",
                &json!({
                    "proposal_id": existing.id,
                    "request_id": input.request_id,
                    "reason": "idempotency_payload_mismatch",
                    "differing_fields": differing_fields,
                }),
            )?;
            return Ok(ProposeAttempt::Rejected(
                ToolError::conflict(format!(
                    "request_id `{}` was reused with different proposal content",
                    input.request_id
                ))
                .with_details(json!({
                    "proposal_id": existing.id,
                    "request_id": input.request_id,
                    "differing_fields": differing_fields,
                })),
            ));
        }

        domain::proposals::validate(
            input.target_type,
            input.action,
            input.target_id,
            &input.patch,
        )?;

        let (patch_bytes, provenance_bytes, total_bytes) =
            serialized_json_size(&input.patch, &input.provenance)?;
        if total_bytes > MAX_PROPOSAL_JSON_BYTES {
            return Err(ToolError::invalid_params(
                "serialized patch and provenance exceed the 1 MiB proposal limit",
            )
            .with_details(json!({
                "patch_bytes": patch_bytes,
                "provenance_bytes": provenance_bytes,
                "total_bytes": total_bytes,
                "limit_bytes": MAX_PROPOSAL_JSON_BYTES,
            })));
        }

        // provenance の事前検証（ref_id は存在確認、inline は必須項目と紐付け先の型を確認）
        match &input.provenance {
            None => {}
            Some(p) if p.ref_id.is_some() => {
                if p.system.is_some()
                    || p.uri.is_some()
                    || p.title.is_some()
                    || p.note.is_some()
                    || p.snapshot.is_some()
                {
                    return Err(ToolError::invalid_params(
                        "provenance must contain either ref_id or inline source fields, not both",
                    ));
                }
                let rid = candidate.provenance_id.expect("normalized above");
                if refs::get(tx, rid, &scopes)?.is_none() {
                    return Err(ToolError::not_found(format!("provenance ref {rid} (in scope `{scope}`)")));
                }
            }
            Some(p) => {
                let blankish = |v: &Option<String>| v.as_deref().map(str::trim).map(str::is_empty).unwrap_or(true);
                if blankish(&p.system) || blankish(&p.uri) || blankish(&p.note) {
                    return Err(ToolError::invalid_params("inline provenance requires system, uri and note (or pass ref_id)"));
                }
                if matches!(input.target_type, ProposalTargetType::Ref | ProposalTargetType::Glossary) {
                    return Err(ToolError::invalid_params(format!(
                        "inline provenance cannot attach to {}; use ref_id",
                        input.target_type
                    )));
                }
            }
        }

        let pending_count =
            proposals::count_pending_by_client_scope(tx, &ctx.client.name, scope)?;
        if pending_count >= MAX_PENDING_PROPOSALS_PER_CLIENT_SCOPE {
            return Err(ToolError::busy(format!(
                "pending proposal limit reached for client `{}` in scope `{scope}`",
                ctx.client.name
            ))
            .with_details(json!({
                "proposed_by": ctx.client.name,
                "scope": scope,
                "pending_count": pending_count,
                "limit": MAX_PENDING_PROPOSALS_PER_CLIENT_SCOPE,
            })));
        }

        let id = match proposals::insert(tx, &candidate) {
            Ok(id) => id,
            Err(StorageError::Sqlite(rusqlite::Error::SqliteFailure(failure, _)))
                if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                // 別 scope の本文を読まず一意制約だけで競合とし、既存提案の ID も記録しない。
                audit::record(
                    tx,
                    &ctx.client.name,
                    "propose_conflict",
                    &json!({
                        "request_id": input.request_id,
                        "reason": "request_id_already_used",
                    }),
                )?;
                return Ok(ProposeAttempt::Rejected(ToolError::conflict(format!(
                    "request_id `{}` is already in use",
                    input.request_id
                ))));
            }
            Err(error) => return Err(error.into()),
        };
        audit::record(
            tx,
            &ctx.client.name,
            "propose",
            &json!({"proposal_id": id, "target_type": input.target_type, "action": input.action, "scope": scope, "request_id": input.request_id}),
        )?;
        Ok(ProposeAttempt::Accepted(ProposeUpdateOutput {
            proposal_id: id,
            status: ProposalStatus::Pending,
            duplicate: false,
        }))
    })?;
    match attempt {
        ProposeAttempt::Accepted(output) => Ok(output),
        ProposeAttempt::Rejected(error) => Err(error),
    }
}

pub fn list_proposals(
    ctx: &CallContext<'_>,
    input: ListProposalsInput,
) -> Result<ListProposalsOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "list_proposals")?;
        let status = input.status.unwrap_or(ProposalStatus::Pending);
        let limit = input.limit.clamp(1, 200) as usize;
        Ok(ListProposalsOutput {
            proposals: proposals::list(c, status, &scopes, limit)?,
        })
    })
}

pub fn approve_proposal(
    ctx: &CallContext<'_>,
    input: ApproveProposalInput,
) -> Result<ApproveProposalOutput, ToolError> {
    let scopes = decision_scopes(ctx, input.scope.as_ref(), "approve_proposal")?;
    ctx.db.with_tx(|tx| {
        let proposal = proposals::get(tx, input.proposal_id, &scopes)?
            .ok_or_else(|| ToolError::not_found(format!("proposal {}", input.proposal_id)))?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ToolError::conflict(format!("proposal {} is already {}", proposal.id, proposal.status)));
        }
        // 適用に失敗したら with_tx が rollback し、提案は pending のまま残る（仕様書 §8.4）
        let outcome = domain::proposals::apply(tx, &proposal)?;
        let provenance_id = domain::proposals::materialize_provenance(tx, &proposal, &outcome)?;
        proposals::decide(
            tx,
            proposal.id,
            &proposals::Decision {
                status: ProposalStatus::Approved,
                decided_by: &ctx.client.name,
                result_id: Some(outcome.id),
                provenance_id,
                note: None,
            },
        )?;
        audit::record(
            tx,
            &ctx.client.name,
            "approve",
            &json!({"proposal_id": proposal.id, "result": {"target_type": outcome.target_type, "id": outcome.id}}),
        )?;
        Ok(ApproveProposalOutput {
            proposal_id: proposal.id,
            status: ProposalStatus::Approved,
            result: ApplyResult { target_type: outcome.target_type, id: outcome.id },
        })
    })
}

pub fn reject_proposal(
    ctx: &CallContext<'_>,
    input: RejectProposalInput,
) -> Result<RejectProposalOutput, ToolError> {
    let scopes = decision_scopes(ctx, input.scope.as_ref(), "reject_proposal")?;
    ctx.db.with_tx(|tx| {
        let proposal = proposals::get(tx, input.proposal_id, &scopes)?
            .ok_or_else(|| ToolError::not_found(format!("proposal {}", input.proposal_id)))?;
        if proposal.status != ProposalStatus::Pending {
            return Err(ToolError::conflict(format!(
                "proposal {} is already {}",
                proposal.id, proposal.status
            )));
        }
        proposals::decide(
            tx,
            proposal.id,
            &proposals::Decision {
                status: ProposalStatus::Rejected,
                decided_by: &ctx.client.name,
                result_id: None,
                provenance_id: None,
                note: input.reason.as_deref(),
            },
        )?;
        audit::record(
            tx,
            &ctx.client.name,
            "reject",
            &json!({"proposal_id": proposal.id, "reason": input.reason}),
        )?;
        Ok(RejectProposalOutput {
            proposal_id: proposal.id,
            status: ProposalStatus::Rejected,
        })
    })
}

fn decision_scopes(
    ctx: &CallContext<'_>,
    requested: Option<&ScopeInput>,
    tool: &str,
) -> Result<ScopeSet, ToolError> {
    ctx.db.with_conn(|conn| {
        let scopes = ScopeSet::resolve(conn, ctx.client, scope_input_to_vec(requested))?;
        // 横断読取の監査は、適用・決定が失敗しても rollback されないよう先に確定する。
        scopes.audit_cross_read(conn, &ctx.client.name, tool)?;
        Ok(scopes)
    })
}

#[cfg(test)]
mod scope_tests;

#[cfg(test)]
mod tests;

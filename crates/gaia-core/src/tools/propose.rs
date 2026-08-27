//! 提案系ツール。書き込みは全てここを通る。仕様書 §8.4。
use serde_json::json;

use crate::{
    contracts::types::{
        ApplyResult, ApproveProposalInput, ApproveProposalOutput, ListProposalsInput,
        ListProposalsOutput, ProposalStatus, ProposalTargetType, ProposeUpdateInput,
        ProposeUpdateOutput, RejectProposalInput, RejectProposalOutput,
    },
    domain,
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{affiliations, audit, proposals, refs},
};

use super::CallContext;

pub fn propose_update(
    ctx: &CallContext<'_>,
    input: ProposeUpdateInput,
) -> Result<ProposeUpdateOutput, ToolError> {
    if input.request_id.trim().len() < 8 {
        return Err(ToolError::invalid_params(
            "request_id must be at least 8 characters",
        ));
    }
    domain::proposals::validate(
        input.target_type,
        input.action,
        input.target_id,
        &input.patch,
    )?;
    ctx.db.with_tx(|tx| {
        let scope = match input.scope.clone().or_else(|| ctx.client.default_scope.clone()) {
            Some(s) => s,
            None => {
                return Err(ToolError::scope_denied(format!(
                    "scope is required: pass `scope` or set default_scope for client `{}`",
                    ctx.client.name
                )));
            }
        };
        if !affiliations::exists(tx, &scope)? {
            return Err(ToolError::not_found(format!("scope `{scope}` (affiliation) not found")));
        }
        // request_id による冪等化
        if let Some(existing) = proposals::find_by_request_id(tx, &input.request_id)? {
            if existing.proposed_by == ctx.client.name {
                return Ok(ProposeUpdateOutput { proposal_id: existing.id, status: existing.status, duplicate: true });
            }
            return Err(ToolError::conflict(format!(
                "request_id `{}` was already used by another client",
                input.request_id
            )));
        }
        // provenance の事前検証（ref_id は存在確認、inline は必須項目と紐付け先の型を確認）
        let (provenance, provenance_id) = match &input.provenance {
            None => (None, None),
            Some(p) if p.ref_id.is_some() => {
                let rid = p.ref_id.expect("checked");
                if refs::get(tx, rid, &ScopeSet::single(&scope))?.is_none() {
                    return Err(ToolError::not_found(format!("provenance ref {rid} (in scope `{scope}`)")));
                }
                (None, Some(rid))
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
                (Some(p.clone()), None)
            }
        };
        let id = proposals::insert(
            tx,
            &proposals::NewProposal {
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
            },
        )?;
        audit::record(
            tx,
            &ctx.client.name,
            "propose",
            &json!({"proposal_id": id, "target_type": input.target_type, "action": input.action, "scope": scope, "request_id": input.request_id}),
        )?;
        Ok(ProposeUpdateOutput { proposal_id: id, status: ProposalStatus::Pending, duplicate: false })
    })
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
    ctx.db.with_tx(|tx| {
        let proposal = proposals::get(tx, input.proposal_id)?
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
    ctx.db.with_tx(|tx| {
        let proposal = proposals::get(tx, input.proposal_id)?
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

#[cfg(test)]
mod tests {
    use crate::error::ErrorCode;
    use crate::tools::test_support::{agent, human, seed_basic, service};
    use serde_json::json;

    #[test]
    fn agent_proposes_human_approves_and_duplicate_is_idempotent() {
        let s = service();
        let propose = |rid: &str| {
            s.call(
                &agent(),
                "propose_update",
                json!({
                    "target_type": "person", "action": "insert",
                    "patch": {"name": "田中 太郎"}, "kind": "fact", "request_id": rid
                }),
            )
        };
        let out = propose("req-tanaka-1").unwrap();
        assert_eq!(out["status"], "pending");
        assert_eq!(out["duplicate"], false);
        let pid = out["proposal_id"].as_i64().unwrap();
        // 同じ request_id の再送は duplicate
        let dup = propose("req-tanaka-1").unwrap();
        assert_eq!(dup["proposal_id"].as_i64().unwrap(), pid);
        assert_eq!(dup["duplicate"], true);
        // 一覧（pending 既定）
        let listed = s.call(&agent(), "list_proposals", json!({})).unwrap();
        assert!(
            listed["proposals"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"].as_i64() == Some(pid))
        );
        // human が承認 → 適用結果が返る
        let approved = s
            .call(&human(), "approve_proposal", json!({"proposal_id": pid}))
            .unwrap();
        assert_eq!(approved["status"], "approved");
        assert_eq!(approved["result"]["target_type"], "person");
        // 二重承認は conflict
        assert_eq!(
            s.call(&human(), "approve_proposal", json!({"proposal_id": pid}))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn short_request_id_and_unknown_scope_are_rejected() {
        let s = service();
        let err = s
            .call(&agent(), "propose_update", json!({
                "target_type": "person", "action": "insert", "patch": {"name": "x"}, "kind": "fact", "request_id": "short"
            }))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        let err = s
            .call(&agent(), "propose_update", json!({
                "target_type": "person", "action": "insert", "patch": {"name": "x"}, "kind": "fact",
                "scope": "zzz", "request_id": "req-00000001"
            }))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn failed_apply_keeps_proposal_pending() {
        let s = service();
        // 存在しない entity への fact 提案は propose では通り、approve で失敗して pending のまま
        let out = s
            .call(
                &agent(),
                "propose_update",
                json!({
                    "target_type": "fact", "action": "insert",
                    "patch": {"entity_type": "person", "entity_id": 9999, "statement": "孤児 fact"},
                    "kind": "inference", "request_id": "req-orphan-1"
                }),
            )
            .unwrap();
        let pid = out["proposal_id"].as_i64().unwrap();
        let err = s
            .call(&human(), "approve_proposal", json!({"proposal_id": pid}))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let listed = s
            .call(&human(), "list_proposals", json!({"status": "pending"}))
            .unwrap();
        assert!(
            listed["proposals"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["id"].as_i64() == Some(pid))
        );
        // 却下できる
        let rejected = s
            .call(
                &human(),
                "reject_proposal",
                json!({"proposal_id": pid, "reason": "対象が存在しない"}),
            )
            .unwrap();
        assert_eq!(rejected["status"], "rejected");
    }

    #[test]
    fn seed_basic_builds_a_connected_dataset() {
        let s = service();
        let ids = seed_basic(&s);
        assert!(ids.org > 0 && ids.person > 0 && ids.engagement > 0);
        assert!(ids.fact > 0 && ids.reference > 0 && ids.glossary > 0 && ids.interaction > 0);
    }
}

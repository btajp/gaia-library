use super::{
    MAX_PENDING_PROPOSALS_PER_CLIENT_SCOPE, MAX_PROPOSAL_JSON_BYTES, MAX_REQUEST_ID_BYTES,
    ProposalStatus,
};
use crate::contracts::Catalog;
use crate::error::ErrorCode;
use crate::scope::ScopeSet;
use crate::storage::{Db, StorageError, affiliations, audit, proposals};
use crate::tools::ToolService;
use crate::tools::test_support::{agent, human, seed_basic, service, write};
use serde_json::json;
use std::sync::{Arc, Barrier};

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
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert", "patch": {"name": "x"},
                "kind": "fact", "request_id": "日本語"
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidParams);
    let err = s
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert", "patch": {"name": "x"}, "kind": "fact",
                "scope": "zzz", "request_id": "req-00000001"
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn request_id_byte_limit_rejects_before_persisting() {
    let s = service();
    for request_id in ["x".repeat(MAX_REQUEST_ID_BYTES + 1), "日".repeat(86)] {
        let err = s
            .call(
                &agent(),
                "propose_update",
                json!({
                    "target_type": "person", "action": "insert", "patch": {"name": "x"},
                    "kind": "fact", "request_id": request_id
                }),
            )
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert_eq!(
            err.details.as_ref().unwrap()["limit_bytes"],
            MAX_REQUEST_ID_BYTES
        );
    }
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            assert_eq!(
                proposals::count_pending_by_client_scope(conn, "bot", "cn")?,
                0
            );
            assert!(audit::recent(conn, 10)?.is_empty());
            Ok(())
        })
        .unwrap();

    let accepted = s
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert", "patch": {"name": "x"},
                "kind": "fact", "request_id": "x".repeat(MAX_REQUEST_ID_BYTES)
            }),
        )
        .unwrap();
    assert_eq!(accepted["status"], "pending");
}

#[test]
fn reused_request_id_requires_exact_content_and_records_conflicts() {
    let s = service();
    let original = json!({
        "target_type": "person", "action": "insert",
        "patch": {"name": "田中 太郎"}, "kind": "fact",
        "request_id": "req-exact-content-1"
    });
    let out = s
        .call(&agent(), "propose_update", original.clone())
        .unwrap();
    let proposal_id = out["proposal_id"].as_i64().unwrap();

    let mut changed = original.clone();
    changed["patch"]["name"] = json!("別人");
    let err = s.call(&agent(), "propose_update", changed).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.details.as_ref().unwrap()["proposal_id"], proposal_id);
    assert_eq!(
        err.details.as_ref().unwrap()["differing_fields"],
        json!(["patch"])
    );

    let err = s.call(&human(), "propose_update", original).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);

    let listed = s.call(&agent(), "list_proposals", json!({})).unwrap();
    let retained = listed["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|proposal| proposal["id"] == proposal_id)
        .unwrap();
    assert_eq!(retained["patch"]["name"], "田中 太郎");

    s.db()
        .with_conn::<_, StorageError>(|conn| {
            let conflicts: Vec<_> = audit::recent(conn, 10)?
                .into_iter()
                .filter(|entry| entry.action == "propose_conflict")
                .collect();
            assert_eq!(conflicts.len(), 2);
            assert!(conflicts.iter().any(|entry| {
                entry.detail["reason"] == "idempotency_payload_mismatch"
                    && entry.detail["differing_fields"] == json!(["patch"])
            }));
            assert!(
                conflicts
                    .iter()
                    .any(|entry| entry.detail["reason"] == "request_id_owner_mismatch")
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn approved_inline_provenance_retry_remains_an_exact_duplicate() {
    let s = service();
    let input = json!({
        "target_type": "person", "action": "insert",
        "patch": {"name": "出所付き人物"}, "kind": "fact",
        "request_id": "req-inline-retry-1",
        "provenance": {
            "system": "notion", "uri": "https://example.test/person", "note": "source"
        }
    });
    let proposed = s.call(&agent(), "propose_update", input.clone()).unwrap();
    let proposal_id = proposed["proposal_id"].as_i64().unwrap();
    s.call(
        &human(),
        "approve_proposal",
        json!({"proposal_id": proposal_id}),
    )
    .unwrap();

    let duplicate = s.call(&agent(), "propose_update", input).unwrap();
    assert_eq!(duplicate["proposal_id"], proposal_id);
    assert_eq!(duplicate["status"], "approved");
    assert_eq!(duplicate["duplicate"], true);
}

#[test]
fn combined_patch_and_provenance_size_is_limited() {
    let s = service();
    let err = s
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert",
                "patch": {"name": "x".repeat(600_000)},
                "kind": "fact", "request_id": "req-size-limit-1",
                "provenance": {
                    "system": "test", "uri": "https://example.test/source", "note": "source",
                    "snapshot": "y".repeat(500_000)
                }
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidParams);
    let details = err.details.unwrap();
    assert!(details["patch_bytes"].as_u64().unwrap() < MAX_PROPOSAL_JSON_BYTES as u64);
    assert!(details["provenance_bytes"].as_u64().unwrap() < MAX_PROPOSAL_JSON_BYTES as u64);
    assert!(details["total_bytes"].as_u64().unwrap() > MAX_PROPOSAL_JSON_BYTES as u64);
    assert_eq!(details["limit_bytes"], MAX_PROPOSAL_JSON_BYTES);
    let pending = s
        .db()
        .with_conn::<_, StorageError>(|conn| {
            proposals::count_pending_by_client_scope(conn, "bot", "cn")
        })
        .unwrap();
    assert_eq!(pending, 0);
}

#[test]
fn pending_limit_is_per_client_and_scope_and_duplicate_wins() {
    let s = service();
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            for index in 0..MAX_PENDING_PROPOSALS_PER_CLIENT_SCOPE {
                let patch = json!({"name": format!("person-{index}")})
                    .as_object()
                    .unwrap()
                    .clone();
                proposals::insert(
                    conn,
                    &proposals::NewProposal {
                        action: "insert".parse().unwrap(),
                        target_type: "person".parse().unwrap(),
                        target_id: None,
                        patch,
                        kind: "fact".parse().unwrap(),
                        scope: "cn".into(),
                        provenance: None,
                        provenance_id: None,
                        proposed_by: "bot".into(),
                        request_id: format!("quota-{index:04}"),
                    },
                )?;
            }
            Ok(())
        })
        .unwrap();

    let duplicate = s
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert",
                "patch": {"name": "person-0"}, "kind": "fact",
                "request_id": "quota-0000"
            }),
        )
        .unwrap();
    assert_eq!(duplicate["duplicate"], true);

    let err = s
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert",
                "patch": {"name": "overflow"}, "kind": "fact",
                "request_id": "quota-overflow"
            }),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Busy);
    let details = err.details.unwrap();
    assert_eq!(
        details["pending_count"],
        MAX_PENDING_PROPOSALS_PER_CLIENT_SCOPE
    );
    assert_eq!(details["limit"], MAX_PENDING_PROPOSALS_PER_CLIENT_SCOPE);

    let other_scope = s
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert",
                "patch": {"name": "allowed elsewhere"}, "kind": "fact",
                "scope": "other", "request_id": "quota-other-scope"
            }),
        )
        .unwrap();
    assert_eq!(other_scope["duplicate"], false);
}

#[test]
fn failed_engagement_approval_rolls_back_parent_relation_and_audit() {
    let s = service();
    let valid_person_id = write(&s, "person", json!({"name": "有効な関係者"}));
    let engagement_name = "途中失敗する案件";
    let proposed = s
        .call(
            &agent(),
            "propose_update",
            json!({
                "target_type": "engagement", "action": "insert",
                "patch": {
                    "name": engagement_name,
                    "people": [
                        {"person_id": valid_person_id, "role": "owner"},
                        {"person_id": 9_999_999, "role": "member"}
                    ]
                },
                "kind": "fact", "request_id": "req-partial-rollback-1"
            }),
        )
        .unwrap();
    let proposal_id = proposed["proposal_id"].as_i64().unwrap();

    let error = s
        .call(
            &human(),
            "approve_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);

    s.db()
        .with_conn::<_, StorageError>(|conn| {
            let engagement_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM engagements WHERE name = ?1",
                rusqlite::params![engagement_name],
                |row| row.get(0),
            )?;
            assert_eq!(engagement_count, 0);

            let relation_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM engagement_people ep \
                 JOIN engagements e ON e.id = ep.engagement_id \
                 WHERE e.name = ?1 AND ep.person_id = ?2",
                rusqlite::params![engagement_name, valid_person_id],
                |row| row.get(0),
            )?;
            assert_eq!(relation_count, 0);

            let retained = proposals::get(conn, proposal_id, &ScopeSet::single("cn"))?.unwrap();
            assert_eq!(retained.status, ProposalStatus::Pending);
            assert!(retained.result_id.is_none());
            assert!(retained.decided_at.is_none());
            assert!(retained.decided_by.is_none());

            let approve_audit_count = audit::recent(conn, 100)?
                .into_iter()
                .filter(|entry| {
                    entry.action == "approve"
                        && entry.detail["proposal_id"].as_i64() == Some(proposal_id)
                })
                .count();
            assert_eq!(approve_audit_count, 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn separate_connections_serialize_competing_approve_and_reject() {
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("competing-decisions.db");
    let setup_db = Db::open(&db_path).unwrap();
    setup_db
        .with_conn::<_, StorageError>(|conn| affiliations::insert(conn, "cn", None))
        .unwrap();
    let setup_service = ToolService::new(setup_db, Catalog::embedded().unwrap());
    let proposed = setup_service
        .call(
            &human(),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert",
                "patch": {"name": "競合する決定"}, "kind": "fact",
                "request_id": "req-competing-decision-1"
            }),
        )
        .unwrap();
    let proposal_id = proposed["proposal_id"].as_i64().unwrap();
    drop(setup_service);

    let approve_service =
        ToolService::new(Db::open(&db_path).unwrap(), Catalog::embedded().unwrap());
    let reject_service =
        ToolService::new(Db::open(&db_path).unwrap(), Catalog::embedded().unwrap());
    let barrier = Arc::new(Barrier::new(3));

    let approve_barrier = Arc::clone(&barrier);
    let approve = std::thread::spawn(move || {
        approve_barrier.wait();
        approve_service.call(
            &human(),
            "approve_proposal",
            json!({"proposal_id": proposal_id}),
        )
    });
    let reject_barrier = Arc::clone(&barrier);
    let reject = std::thread::spawn(move || {
        reject_barrier.wait();
        reject_service.call(
            &human(),
            "reject_proposal",
            json!({"proposal_id": proposal_id, "reason": "concurrent decision"}),
        )
    });
    barrier.wait();

    let results = [approve.join().unwrap(), reject.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let errors: Vec<_> = results
        .iter()
        .filter_map(|result| result.as_ref().err())
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, ErrorCode::Conflict);

    let verify_db = Db::open(&db_path).unwrap();
    verify_db
        .with_conn::<_, StorageError>(|conn| {
            let decided = proposals::get(conn, proposal_id, &ScopeSet::single("cn"))?.unwrap();
            assert!(matches!(
                decided.status,
                ProposalStatus::Approved | ProposalStatus::Rejected
            ));
            assert!(decided.decided_at.is_some());
            assert!(decided.decided_by.is_some());

            let decision_audits: Vec<_> = audit::recent(conn, 100)?
                .into_iter()
                .filter(|entry| {
                    matches!(entry.action.as_str(), "approve" | "reject")
                        && entry.detail["proposal_id"].as_i64() == Some(proposal_id)
                })
                .collect();
            assert_eq!(decision_audits.len(), 1);
            assert_eq!(
                decision_audits[0].action,
                if decided.status == ProposalStatus::Approved {
                    "approve"
                } else {
                    "reject"
                }
            );
            Ok(())
        })
        .unwrap();
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

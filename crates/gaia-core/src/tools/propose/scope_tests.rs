use rusqlite::params;
use serde_json::{Value, json};

use crate::{
    contracts::types::ProposalStatus,
    error::ErrorCode,
    scope::ScopeSet,
    storage::{StorageError, audit, proposals},
    tools::{
        ToolService,
        test_support::{agent, human, service, write},
    },
};

fn proposal_input(scope: &str) -> Value {
    json!({
        "target_type": "person", "action": "insert", "patch": {"name": "境界内の人物"},
        "kind": "fact", "scope": scope, "request_id": format!("scope-boundary-{scope}")
    })
}

fn proposal_in(s: &ToolService, scope: &str) -> i64 {
    s.call(&agent(), "propose_update", proposal_input(scope))
        .unwrap()["proposal_id"]
        .as_i64()
        .unwrap()
}

fn pending_audits(s: &ToolService, id: i64, scope: &str) -> Vec<audit::AuditEntry> {
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            let proposal = proposals::get(conn, id, &ScopeSet::single(scope))?.unwrap();
            assert_eq!(proposal.status, ProposalStatus::Pending);
            assert!(proposal.result_id.is_none());
            assert!(proposal.decided_at.is_none());
            assert!(proposal.decided_by.is_none());
            audit::recent(conn, 100)
        })
        .unwrap()
}

#[test]
fn decisions_deny_proposals_outside_the_effective_scope() {
    for tool in ["approve_proposal", "reject_proposal"] {
        let s = service();
        let id = proposal_in(&s, "other");
        for args in [
            json!({"proposal_id": id}),
            json!({"proposal_id": id, "scope": "cn"}),
            json!({"proposal_id": id, "scope": "unknown"}),
        ] {
            let error = s.call(&human(), tool, args).unwrap_err();
            assert_eq!(error.code, ErrorCode::NotFound, "{tool}");
            assert!(error.details.is_none());
        }
        assert_eq!(
            s.call(&agent(), tool, json!({"proposal_id": id, "scope": "other"}))
                .unwrap_err()
                .code,
            ErrorCode::Unauthorized
        );
        assert!(
            pending_audits(&s, id, "other")
                .iter()
                .all(|entry| entry.action == "propose")
        );
    }
}

#[test]
fn decisions_require_scope_when_the_client_has_no_default() {
    for tool in ["approve_proposal", "reject_proposal"] {
        let s = service();
        let id = proposal_in(&s, "cn");
        let mut no_default = human();
        no_default.default_scope = None;
        for (args, expected) in [
            (json!({"proposal_id": id}), ErrorCode::ScopeDenied),
            (
                json!({"proposal_id": id, "scope": []}),
                ErrorCode::InvalidParams,
            ),
        ] {
            assert_eq!(
                s.call(&no_default, tool, args).unwrap_err().code,
                expected,
                "{tool}"
            );
        }
        assert_eq!(pending_audits(&s, id, "cn").len(), 1);
    }
}

#[test]
fn explicit_single_scope_allows_decisions_without_cross_scope_audit() {
    for (tool, status) in [
        ("approve_proposal", "approved"),
        ("reject_proposal", "rejected"),
    ] {
        for scope in [json!("other"), json!(["other", "other"])] {
            let s = service();
            let id = proposal_in(&s, "other");
            let args = json!({"proposal_id": id, "scope": scope});
            let out = s.call(&human(), tool, args.clone()).unwrap();
            assert_eq!(out["status"], status);
            assert_eq!(
                s.call(&human(), tool, args).unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(
                s.call(&human(), tool, json!({"proposal_id": id}))
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound,
                "a terminal status in another scope must remain hidden"
            );
            s.db()
                .with_conn::<_, StorageError>(|conn| {
                    let entries = audit::recent(conn, 20)?;
                    assert_eq!(entries.len(), 2);
                    assert!(
                        entries
                            .iter()
                            .all(|entry| entry.action != "cross_scope_read")
                    );
                    Ok(())
                })
                .unwrap();
        }
    }
}

#[test]
fn explicit_multiple_scopes_are_deduplicated_and_audited_for_decisions() {
    for tool in ["approve_proposal", "reject_proposal"] {
        let s = service();
        let id = proposal_in(&s, "other");
        let mut no_default = human();
        no_default.default_scope = None;
        s.call(
            &no_default,
            tool,
            json!({"proposal_id": id, "scope": ["other", "cn", "other"]}),
        )
        .unwrap();
        s.db()
            .with_conn::<_, StorageError>(|conn| {
                let entries = audit::recent(conn, 20)?;
                let cross: Vec<_> = entries
                    .iter()
                    .filter(|entry| entry.action == "cross_scope_read")
                    .collect();
                assert_eq!(cross.len(), 1);
                assert_eq!(cross[0].actor, no_default.name);
                assert_eq!(
                    cross[0].detail,
                    json!({"tool": tool, "scopes": ["cn", "other"]})
                );
                assert!(cross[0].id < entries[0].id);
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn failed_apply_rolls_back_partial_writes_but_keeps_cross_scope_read_audit() {
    let s = service();
    let person_id = write(&s, "person", json!({"name": "有効な関係者"}));
    let input = json!({
        "target_type": "engagement", "action": "insert",
        "patch": {
            "name": "横断承認の途中失敗",
            "people": [{"person_id": person_id}, {"person_id": 9_999_999}]
        },
        "kind": "fact", "scope": "other", "request_id": "scope-failed-apply"
    });
    let proposed = s.call(&agent(), "propose_update", input.clone()).unwrap();
    let id = proposed["proposal_id"].as_i64().unwrap();
    assert_eq!(
        s.call(
            &human(),
            "approve_proposal",
            json!({"proposal_id": id, "scope": ["cn", "other"]})
        )
        .unwrap_err()
        .code,
        ErrorCode::NotFound
    );
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            let scopes = ScopeSet::single("other");
            let engagement_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM engagements WHERE scope IN (SELECT value FROM json_each(?1))",
                params![scopes.as_json()],
                |row| row.get(0),
            )?;
            assert_eq!(engagement_count, 0);
            // この fixture に既存の関係はない。親の rollback 後に孤児も残らないことを確認する。
            let relation_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM engagement_people", [], |row| {
                    row.get(0)
                })?;
            assert_eq!(relation_count, 0);
            Ok(())
        })
        .unwrap();
    let entries = pending_audits(&s, id, "other");
    assert_eq!(entries[0].action, "cross_scope_read");
    assert_eq!(entries[0].detail["tool"], "approve_proposal");
    assert!(
        !entries
            .iter()
            .any(|entry| { entry.action == "approve" && entry.detail["proposal_id"] == id })
    );
    let retry = s.call(&agent(), "propose_update", input).unwrap();
    assert_eq!(retry["proposal_id"], id);
    assert_eq!(retry["status"], "pending");
    assert_eq!(retry["duplicate"], true);
}

#[test]
fn failed_rejection_rolls_back_decision_but_keeps_cross_scope_read_audit() {
    let s = service();
    let id = proposal_in(&s, "other");
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER fail_reject_audit BEFORE INSERT ON audit_log \
                 WHEN NEW.action = 'reject' BEGIN \
                 SELECT RAISE(ABORT, 'forced rejection audit failure'); END;",
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        s.call(
            &human(),
            "reject_proposal",
            json!({"proposal_id": id, "scope": ["cn", "other"], "reason": "却下"})
        )
        .unwrap_err()
        .code,
        ErrorCode::Internal
    );
    let entries = pending_audits(&s, id, "other");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].action, "cross_scope_read");
    assert_eq!(entries[0].detail["tool"], "reject_proposal");
}

#[test]
fn request_id_reuse_in_another_scope_is_opaque_and_audited() {
    let s = service();
    let mut original = proposal_input("other");
    original["patch"]["name"] = json!("他の所属の非公開本文");
    let proposed = s
        .call(&agent(), "propose_update", original.clone())
        .unwrap();
    let id = proposed["proposal_id"].as_i64().unwrap();
    let request_id = original["request_id"].as_str().unwrap();
    for client in [agent(), human()] {
        let error = s
            .call(
                &client,
                "propose_update",
                json!({
                    "target_type": "person", "action": "insert", "patch": {"name": "新規"},
                    "kind": "fact", "request_id": request_id
                }),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(error.details.is_none());
        assert_eq!(
            error.message,
            format!("request_id `{request_id}` is already in use")
        );
    }
    let entries = pending_audits(&s, id, "other");
    let conflicts: Vec<_> = entries
        .iter()
        .filter(|entry| entry.action == "propose_conflict")
        .collect();
    assert_eq!(conflicts.len(), 2);
    for conflict in conflicts {
        assert_eq!(
            conflict.detail,
            json!({"request_id": request_id, "reason": "request_id_already_used"})
        );
    }
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            assert!(
                proposals::find_by_request_id(conn, request_id, &ScopeSet::single("cn"))?.is_none()
            );
            let retained = proposals::get(conn, id, &ScopeSet::single("other"))?.unwrap();
            assert_eq!(retained.patch, *original["patch"].as_object().unwrap());
            Ok(())
        })
        .unwrap();
    let retry = s.call(&agent(), "propose_update", original).unwrap();
    assert_eq!(retry["proposal_id"], id);
    assert_eq!(retry["duplicate"], true);
}

#[test]
fn storage_and_tools_filter_other_scope_before_decoding_proposal_content() {
    let s = service();
    let id = proposal_in(&s, "other");
    let request_id = "scope-boundary-other";
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            let allowed = ScopeSet::single("other");
            assert_eq!(proposals::get(conn, id, &allowed)?.unwrap().id, id);
            assert_eq!(
                proposals::find_by_request_id(conn, request_id, &allowed)?
                    .unwrap()
                    .id,
                id
            );
            conn.execute(
                "UPDATE proposals SET patch = 'invalid-json' WHERE id = ?1",
                params![id],
            )?;
            let denied = ScopeSet::single("cn");
            assert!(proposals::get(conn, id, &denied)?.is_none());
            assert!(proposals::find_by_request_id(conn, request_id, &denied)?.is_none());
            assert!(proposals::get(conn, id, &allowed).is_err());
            Ok(())
        })
        .unwrap();
    for tool in ["approve_proposal", "reject_proposal"] {
        assert_eq!(
            s.call(&human(), tool, json!({"proposal_id": id}))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    let mut retry = proposal_input("cn");
    retry["request_id"] = json!(request_id);
    let error = s.call(&agent(), "propose_update", retry).unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert!(error.details.is_none());
    s.db()
        .with_conn::<_, StorageError>(|conn| {
            let entries = audit::recent(conn, 20)?;
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].action, "propose_conflict");
            assert_eq!(
                entries[0].detail,
                json!({
                    "request_id": request_id, "reason": "request_id_already_used"
                })
            );
            Ok(())
        })
        .unwrap();
}

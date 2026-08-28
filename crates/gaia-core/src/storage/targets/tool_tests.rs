//! approve_proposal 経由で、内容層ターゲットの「他 scope に隠れている」と「存在しない」が
//! 同じエラー（コード・メッセージ）になることを確認する。
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::{
    error::ErrorCode,
    scope::ScopeSet,
    storage::{StorageError, glossary, proposals},
    tools::{
        ToolService,
        test_support::{agent, human, service},
    },
};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn propose(s: &ToolService, mut input: Value) -> i64 {
    input["request_id"] = json!(format!(
        "target-boundary-{}",
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    input["kind"] = json!("fact");
    s.call(&agent(), "propose_update", input).unwrap()["proposal_id"]
        .as_i64()
        .unwrap()
}

fn write_in(s: &ToolService, target_type: &str, patch: Value, scope: &str) -> i64 {
    let id = propose(
        s,
        json!({
            "target_type": target_type, "action": "insert", "patch": patch, "scope": scope
        }),
    );
    s.call(
        &human(),
        "approve_proposal",
        json!({"proposal_id": id, "scope": scope}),
    )
    .unwrap()["result"]["id"]
        .as_i64()
        .unwrap()
}

fn content_target_in(s: &ToolService, target_type: &str, scope: &str) -> i64 {
    let patch = match target_type {
        "engagement" => json!({"name": "案件"}),
        "interaction" => {
            json!({"kind": "meeting", "occurred_at": "2026-08-27", "summary": "打合せ"})
        }
        "fact" => {
            let person = write_in(s, "person", json!({"name": "田中"}), scope);
            json!({"entity_type": "person", "entity_id": person, "statement": "確認済み"})
        }
        _ => unreachable!(),
    };
    write_in(s, target_type, patch, scope)
}

fn assert_pending(s: &ToolService, proposal_id: i64) {
    s.db()
        .with_conn::<_, StorageError>(|c| {
            let proposal = proposals::get(c, proposal_id, &ScopeSet::single("cn"))?.unwrap();
            assert_eq!(proposal.status.to_string(), "pending");
            assert!(proposal.result_id.is_none());
            Ok(())
        })
        .unwrap();
}

#[test]
fn fact_and_ref_approval_cannot_distinguish_hidden_targets_from_missing_targets() {
    for (proposal_type, target_type) in [
        ("fact", "engagement"),
        ("fact", "interaction"),
        ("ref", "engagement"),
        ("ref", "interaction"),
        ("ref", "fact"),
    ] {
        let s = service();
        let patch = if proposal_type == "fact" {
            json!({"entity_type": target_type, "entity_id": 1, "statement": "追記事項"})
        } else {
            json!({
                "target_type": target_type, "target_id": 1, "system": "minutes",
                "uri": "minutes://meeting/1", "note": "追記事項"
            })
        };
        let id = propose(
            &s,
            json!({
                "target_type": proposal_type, "action": "insert", "patch": patch
            }),
        );
        // まだどの scope にも無い → not_found
        let missing = s
            .call(&human(), "approve_proposal", json!({"proposal_id": id}))
            .unwrap_err();
        assert_eq!(
            missing.code,
            ErrorCode::NotFound,
            "{proposal_type} -> {target_type}"
        );
        assert_pending(&s, id);
        // 他 scope に同じ id で作成 → 隠れているだけだが、同じエラーを返す
        assert_eq!(content_target_in(&s, target_type, "other"), 1);
        let hidden = s
            .call(&human(), "approve_proposal", json!({"proposal_id": id}))
            .unwrap_err();
        assert_eq!(
            hidden.to_json(),
            missing.to_json(),
            "{proposal_type} -> {target_type}"
        );
        assert_pending(&s, id);
        // ターゲットと同じ scope なら登録できる
        assert!(write_in(&s, proposal_type, patch, "other") > 0);
    }
}

#[test]
fn glossary_approval_preserves_old_link_and_content_when_reassignment_is_hidden() {
    let s = service();
    let old_engagement = content_target_in(&s, "engagement", "cn");
    assert_eq!(old_engagement, 1);
    let glossary_id = write_in(
        &s,
        "glossary",
        json!({
            "term": "SCIM", "reading": "スキム", "definition": "元の説明", "engagement_id": old_engagement
        }),
        "cn",
    );
    let read_glossary = || {
        s.db()
            .with_conn::<_, StorageError>(|c| {
                Ok(glossary::get(c, glossary_id, &ScopeSet::single("cn"))?.unwrap())
            })
            .unwrap()
    };
    let original = read_glossary();
    let mut patch = json!({
        "term": "IdP", "reading": "アイディーピー", "definition": "新しい説明", "engagement_id": 2
    });
    let proposal_id = propose(
        &s,
        json!({
            "target_type": "glossary", "action": "update", "target_id": glossary_id, "patch": patch
        }),
    );
    let missing = s
        .call(
            &human(),
            "approve_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);
    assert_eq!(read_glossary(), original);
    assert_eq!(content_target_in(&s, "engagement", "other"), 2);
    let hidden = s
        .call(
            &human(),
            "approve_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .unwrap_err();
    assert_eq!(hidden.to_json(), missing.to_json());
    assert_eq!(read_glossary(), original);
    assert_pending(&s, proposal_id);
    let new_engagement = content_target_in(&s, "engagement", "cn");
    patch["engagement_id"] = json!(new_engagement);
    let new_proposal = propose(
        &s,
        json!({
            "target_type": "glossary", "action": "update", "target_id": glossary_id, "patch": patch
        }),
    );
    s.call(
        &human(),
        "approve_proposal",
        json!({"proposal_id": new_proposal}),
    )
    .unwrap();
    let updated = read_glossary();
    assert_eq!(updated.engagement_id, Some(new_engagement));
    assert_eq!(updated.term, "IdP");
    assert_eq!(updated.reading.as_deref(), Some("アイディーピー"));
    assert_eq!(updated.definition.as_deref(), Some("新しい説明"));
}

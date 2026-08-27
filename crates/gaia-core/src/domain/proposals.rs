//! 提案の検証(propose 時)と適用(approve 時)。仕様書 §8.4。
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{
    contracts::types::{
        EngagementPatch, EntityPatch, FactPatch, GlossaryPatch, InteractionPatch,
        OrganizationPatch, PersonPatch, Proposal, ProposalAction, ProposalTargetType, RefPatch,
        RefTargetType,
    },
    domain::predicates,
    error::ToolError,
    storage::{engagements, entities, facts, glossary, interactions, organizations, people, refs},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub target_type: ProposalTargetType,
    pub id: i64,
}

fn parse<T: DeserializeOwned>(
    patch: &Map<String, Value>,
    target: ProposalTargetType,
) -> Result<T, ToolError> {
    serde_json::from_value(Value::Object(patch.clone())).map_err(|e| {
        ToolError::invalid_params(format!(
            "patch does not match the {target} patch shape: {e}"
        ))
    })
}

fn blank(v: &Option<String>) -> bool {
    v.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true)
}

fn reject_blank_if_present(v: &Option<String>, field: &str) -> Result<(), ToolError> {
    if v.as_deref().is_some_and(|s| s.trim().is_empty()) {
        return Err(ToolError::invalid_params(format!(
            "{field} must not be empty"
        )));
    }
    Ok(())
}

/// propose 時の事前検証。DB は見ない(存在検証は承認時の apply で行う)。
pub fn validate(
    target_type: ProposalTargetType,
    action: ProposalAction,
    target_id: Option<i64>,
    patch: &Map<String, Value>,
) -> Result<(), ToolError> {
    match action {
        ProposalAction::Insert => {
            if target_id.is_some() {
                return Err(ToolError::invalid_params("insert must not have target_id"));
            }
        }
        ProposalAction::Update | ProposalAction::Supersede => {
            if target_id.is_none() {
                return Err(ToolError::invalid_params(format!(
                    "{action} requires target_id"
                )));
            }
        }
    }
    if action == ProposalAction::Supersede && target_type != ProposalTargetType::Fact {
        return Err(ToolError::invalid_params(
            "supersede is only valid for facts",
        ));
    }
    if action == ProposalAction::Update && patch.is_empty() {
        return Err(ToolError::invalid_params(
            "update patch must contain at least one field",
        ));
    }
    if let Some((field, _)) = patch.iter().find(|(_, value)| value.is_null()) {
        return Err(ToolError::invalid_params(format!(
            "patch field `{field}` must not be null; omit unchanged fields"
        )));
    }
    let insert_like = action != ProposalAction::Update;
    match target_type {
        ProposalTargetType::Person => {
            let p: PersonPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.name, "person.name")?;
            if insert_like && blank(&p.name) {
                return Err(ToolError::invalid_params("person insert requires name"));
            }
        }
        ProposalTargetType::Organization => {
            let p: OrganizationPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.name, "organization.name")?;
            if insert_like && blank(&p.name) {
                return Err(ToolError::invalid_params(
                    "organization insert requires name",
                ));
            }
        }
        ProposalTargetType::Engagement => {
            let p: EngagementPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.name, "engagement.name")?;
            if insert_like && blank(&p.name) {
                return Err(ToolError::invalid_params("engagement insert requires name"));
            }
        }
        ProposalTargetType::Interaction => {
            let p: InteractionPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.kind, "interaction.kind")?;
            reject_blank_if_present(&p.occurred_at, "interaction.occurred_at")?;
            reject_blank_if_present(&p.summary, "interaction.summary")?;
            if insert_like && (blank(&p.kind) || blank(&p.occurred_at) || blank(&p.summary)) {
                return Err(ToolError::invalid_params(
                    "interaction insert requires kind, occurred_at and summary",
                ));
            }
        }
        ProposalTargetType::Entity => {
            let p: EntityPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.type_, "entity.type")?;
            reject_blank_if_present(&p.name, "entity.name")?;
            if insert_like && (blank(&p.type_) || blank(&p.name)) {
                return Err(ToolError::invalid_params(
                    "entity insert requires type and name",
                ));
            }
        }
        ProposalTargetType::Fact => {
            let p: FactPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.statement, "fact.statement")?;
            if action == ProposalAction::Update
                && (patch.contains_key("entity_type") || patch.contains_key("entity_id"))
            {
                return Err(ToolError::invalid_params(
                    "fact update cannot change entity_type or entity_id",
                ));
            }
            if insert_like
                && (p.entity_type.is_none() || p.entity_id.is_none() || blank(&p.statement))
            {
                return Err(ToolError::invalid_params(
                    "fact insert/supersede requires entity_type, entity_id and statement",
                ));
            }
            if action == ProposalAction::Update {
                predicates::check_update_patch(p.predicate.as_deref(), p.value.as_deref())?;
            } else {
                predicates::check(p.predicate.as_deref(), p.value.as_deref())?;
            }
        }
        ProposalTargetType::Ref => {
            let p: RefPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.system, "ref.system")?;
            reject_blank_if_present(&p.uri, "ref.uri")?;
            reject_blank_if_present(&p.note, "ref.note")?;
            if action == ProposalAction::Update
                && (patch.contains_key("target_type") || patch.contains_key("target_id"))
            {
                return Err(ToolError::invalid_params(
                    "ref update cannot change target_type or target_id",
                ));
            }
            if insert_like
                && (p.target_type.is_none()
                    || p.target_id.is_none()
                    || blank(&p.system)
                    || blank(&p.uri)
                    || blank(&p.note))
            {
                return Err(ToolError::invalid_params(
                    "ref insert requires target_type, target_id, system, uri and note (URI-only refs are forbidden)",
                ));
            }
        }
        ProposalTargetType::Glossary => {
            let p: GlossaryPatch = parse(patch, target_type)?;
            reject_blank_if_present(&p.term, "glossary.term")?;
            if insert_like && blank(&p.term) {
                return Err(ToolError::invalid_params("glossary insert requires term"));
            }
        }
    }
    Ok(())
}

/// 承認時の適用。トランザクション内で呼ぶこと。scope は proposal.scope。
pub fn apply(conn: &Connection, proposal: &Proposal) -> Result<ApplyOutcome, ToolError> {
    validate(
        proposal.target_type,
        proposal.action,
        proposal.target_id,
        &proposal.patch,
    )?;
    let t = proposal.target_type;
    let scope = proposal.scope.as_str();
    let target_id = proposal.target_id;
    let id = match (t, proposal.action) {
        (ProposalTargetType::Person, ProposalAction::Insert) => {
            people::insert(conn, &parse(&proposal.patch, t)?)?
        }
        (ProposalTargetType::Person, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            people::update(conn, id, &parse(&proposal.patch, t)?)?;
            id
        }
        (ProposalTargetType::Organization, ProposalAction::Insert) => {
            organizations::insert(conn, &parse(&proposal.patch, t)?)?
        }
        (ProposalTargetType::Organization, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            organizations::update(conn, id, &parse(&proposal.patch, t)?)?;
            id
        }
        (ProposalTargetType::Engagement, ProposalAction::Insert) => {
            engagements::insert(conn, &parse(&proposal.patch, t)?, scope)?
        }
        (ProposalTargetType::Engagement, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            engagements::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (ProposalTargetType::Interaction, ProposalAction::Insert) => {
            interactions::insert(conn, &parse(&proposal.patch, t)?, scope)?
        }
        (ProposalTargetType::Interaction, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            interactions::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (ProposalTargetType::Entity, ProposalAction::Insert) => {
            entities::insert(conn, &parse(&proposal.patch, t)?)?
        }
        (ProposalTargetType::Entity, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            entities::update(conn, id, &parse(&proposal.patch, t)?)?;
            id
        }
        (ProposalTargetType::Fact, ProposalAction::Insert) => {
            facts::insert(conn, &parse(&proposal.patch, t)?, proposal.kind, scope)?
        }
        (ProposalTargetType::Fact, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            let patch: FactPatch = parse(&proposal.patch, t)?;
            let current = facts::get(conn, id, &crate::scope::ScopeSet::single(scope))?
                .ok_or_else(|| {
                    ToolError::not_found(format!("fact {id} (in scope `{scope}`) not found"))
                })?;
            let predicate = patch.predicate.as_deref().or(current.predicate.as_deref());
            let value = patch.value.as_deref().or(current.value.as_deref());
            predicates::check(predicate, value)?;
            facts::update(conn, id, &patch, scope)?;
            id
        }
        (ProposalTargetType::Fact, ProposalAction::Supersede) => facts::supersede(
            conn,
            target_id.expect("validated"),
            &parse(&proposal.patch, t)?,
            proposal.kind,
            scope,
        )?,
        (ProposalTargetType::Ref, ProposalAction::Insert) => {
            refs::insert(conn, &parse(&proposal.patch, t)?, scope)?
        }
        (ProposalTargetType::Ref, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            refs::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (ProposalTargetType::Glossary, ProposalAction::Insert) => {
            glossary::insert(conn, &parse(&proposal.patch, t)?, scope)?
        }
        (ProposalTargetType::Glossary, ProposalAction::Update) => {
            let id = target_id.expect("validated");
            glossary::update(conn, id, &parse(&proposal.patch, t)?, scope)?;
            id
        }
        (_, ProposalAction::Supersede) => unreachable!("validate: supersede only for facts"),
    };
    Ok(ApplyOutcome { target_type: t, id })
}

/// 出所の実体化。inline 指定(system/uri/note)は適用結果のレコードに紐付く ref として登録する。
/// `ref` / `glossary` を対象とする提案は RefTargetType に変換できないため ref_id 形式のみ許す。
pub fn materialize_provenance(
    conn: &Connection,
    proposal: &Proposal,
    outcome: &ApplyOutcome,
) -> Result<Option<i64>, ToolError> {
    let Some(p) = &proposal.provenance else {
        return Ok(proposal.provenance_id);
    };
    if let Some(ref_id) = p.ref_id {
        return Ok(Some(ref_id));
    }
    let target_type: RefTargetType = outcome.target_type.to_string().parse().map_err(|_| {
        ToolError::invalid_params(format!(
            "inline provenance cannot attach to {}; use {{\"ref_id\": ...}}",
            outcome.target_type
        ))
    })?;
    let patch: RefPatch = serde_json::from_value(serde_json::json!({
        "target_type": target_type,
        "target_id": outcome.id,
        "system": p.system,
        "uri": p.uri,
        "title": p.title,
        "note": p.note,
        "snapshot": p.snapshot,
    }))?;
    Ok(Some(refs::insert(conn, &patch, &proposal.scope)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::types::Kind,
        error::ErrorCode,
        scope::ScopeSet,
        storage::{Db, StorageError, affiliations, facts, people, proposals, refs},
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn setup() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            affiliations::insert(c, "cn", None)?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn make_proposal(
        db: &Db,
        target_type: &str,
        action: &str,
        target_id: Option<i64>,
        patch: serde_json::Value,
        provenance: Option<serde_json::Value>,
    ) -> crate::contracts::types::Proposal {
        db.with_conn::<_, StorageError>(|c| {
            let new = proposals::NewProposal {
                action: action.parse().unwrap(),
                target_type: target_type.parse().unwrap(),
                target_id,
                patch: patch.as_object().unwrap().clone(),
                kind: Kind::Fact,
                scope: "cn".into(),
                provenance: provenance.map(|v| serde_json::from_value(v).unwrap()),
                provenance_id: None,
                proposed_by: "bot".into(),
                request_id: format!("req-{}", SEQ.fetch_add(1, Ordering::SeqCst)),
            };
            let id = proposals::insert(c, &new)?;
            Ok(proposals::get(c, id)?.unwrap())
        })
        .unwrap()
    }

    #[test]
    fn validate_rejects_malformed_proposals() {
        let patch = json!({"name": "x"}).as_object().unwrap().clone();
        let empty = json!({}).as_object().unwrap().clone();
        let bogus = json!({"name": "x", "bogus": 1})
            .as_object()
            .unwrap()
            .clone();
        let ok = validate(
            "person".parse().unwrap(),
            "insert".parse().unwrap(),
            None,
            &patch,
        );
        assert!(ok.is_ok());
        for (t, a, id, p) in [
            ("person", "insert", Some(1), &patch),    // insert に target_id
            ("person", "update", None, &patch),       // update に target_id 無し
            ("person", "supersede", Some(1), &patch), // supersede は fact のみ
            ("person", "insert", None, &empty),       // name 必須
            ("person", "insert", None, &bogus),       // 未知フィールド
            ("fact", "insert", None, &patch),         // entity_type 等必須
        ] {
            let err = validate(t.parse().unwrap(), a.parse().unwrap(), id, p).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams, "{t}/{a}");
        }
        // レジストリ外 predicate
        let bad_pred = json!({"entity_type": "person", "entity_id": 1, "statement": "s", "predicate": "mood", "value": "x"})
            .as_object().unwrap().clone();
        assert_eq!(
            validate(
                "fact".parse().unwrap(),
                "insert".parse().unwrap(),
                None,
                &bad_pred
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidParams
        );
    }

    #[test]
    fn validate_rejects_blank_required_fields_on_update() {
        for (target, patch) in [
            ("person", json!({"name": "  "})),
            ("organization", json!({"name": "  "})),
            ("engagement", json!({"name": "  "})),
            ("interaction", json!({"kind": "  "})),
            ("interaction", json!({"occurred_at": "  "})),
            ("interaction", json!({"summary": "  "})),
            ("entity", json!({"type": "  "})),
            ("entity", json!({"name": "  "})),
            ("fact", json!({"statement": "  "})),
            ("ref", json!({"system": "  "})),
            ("ref", json!({"uri": "  "})),
            ("ref", json!({"note": "  "})),
            ("glossary", json!({"term": "  "})),
        ] {
            let patch = patch.as_object().unwrap().clone();
            let err = validate(
                target.parse().unwrap(),
                ProposalAction::Update,
                Some(1),
                &patch,
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams, "{target}: {patch:?}");
        }
    }

    #[test]
    fn validate_rejects_null_fields_and_empty_updates() {
        for (target, patch) in [
            ("person", json!({"role": null})),
            ("organization", json!({"kind": null})),
            ("engagement", json!({"status": null})),
            ("interaction", json!({"engagement_id": null})),
            ("entity", json!({"name": null})),
            ("fact", json!({"valid_from": null})),
            ("ref", json!({"title": null})),
            ("glossary", json!({"reading": null})),
        ] {
            let patch = patch.as_object().unwrap().clone();
            let err = validate(
                target.parse().unwrap(),
                ProposalAction::Update,
                Some(1),
                &patch,
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams, "{target}: {patch:?}");
        }

        let empty = Map::new();
        for target in [
            "person",
            "organization",
            "engagement",
            "interaction",
            "entity",
            "fact",
            "ref",
            "glossary",
        ] {
            let err = validate(
                target.parse().unwrap(),
                ProposalAction::Update,
                Some(1),
                &empty,
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams, "{target}");
        }
    }

    #[test]
    fn validate_rejects_orphan_fact_values_and_immutable_link_fields() {
        for (action, target_id) in [
            (ProposalAction::Insert, None),
            (ProposalAction::Supersede, Some(1)),
        ] {
            let patch = json!({
                "entity_type": "person",
                "entity_id": 1,
                "statement": "役職情報",
                "value": "director"
            })
            .as_object()
            .unwrap()
            .clone();
            let err = validate(ProposalTargetType::Fact, action, target_id, &patch).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams);
        }

        for (target, patch) in [
            ("fact", json!({"entity_type": "person"})),
            ("fact", json!({"entity_type": null})),
            ("fact", json!({"entity_id": 1})),
            ("fact", json!({"entity_id": null})),
            ("ref", json!({"target_type": "person"})),
            ("ref", json!({"target_type": null})),
            ("ref", json!({"target_id": 1})),
            ("ref", json!({"target_id": null})),
        ] {
            let patch = patch.as_object().unwrap().clone();
            let err = validate(
                target.parse().unwrap(),
                ProposalAction::Update,
                Some(1),
                &patch,
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams, "{target}: {patch:?}");
        }

        let partial = json!({"value": "director"}).as_object().unwrap().clone();
        assert!(
            validate(
                ProposalTargetType::Fact,
                ProposalAction::Update,
                Some(1),
                &partial,
            )
            .is_ok()
        );
    }

    #[test]
    fn person_insert_apply_then_inline_provenance_ref() {
        let db = setup();
        let proposal = make_proposal(
            &db,
            "person",
            "insert",
            None,
            json!({"name": "岡村 慎太郎", "aliases": [{"alias": "okash1n"}]}),
            Some(
                json!({"system": "minutes", "uri": "minutes://meeting/1", "note": "初回打合せの議事録"}),
            ),
        );
        db.with_conn::<_, ToolError>(|c| {
            let outcome = apply(c, &proposal)?;
            assert_eq!(outcome.target_type.to_string(), "person");
            assert!(
                people::get(c, outcome.id)
                    .map_err(ToolError::from)?
                    .is_some()
            );
            let ref_id = materialize_provenance(c, &proposal, &outcome)?.unwrap();
            let r = refs::get(c, ref_id, &ScopeSet::single("cn"))
                .map_err(ToolError::from)?
                .unwrap();
            assert_eq!(r.target_id, outcome.id);
            assert_eq!(r.system, "minutes");
            proposals::decide(
                c,
                proposal.id,
                &proposals::Decision {
                    status: "approved".parse().unwrap(),
                    decided_by: "me",
                    result_id: Some(outcome.id),
                    provenance_id: Some(ref_id),
                    note: None,
                },
            )
            .map_err(ToolError::from)?;
            // 二重承認は不可
            assert!(
                proposals::decide(
                    c,
                    proposal.id,
                    &proposals::Decision {
                        status: "approved".parse().unwrap(),
                        decided_by: "me",
                        result_id: None,
                        provenance_id: None,
                        note: None,
                    }
                )
                .is_err()
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn fact_supersede_rejects_history_update_and_update_not_found() {
        let db = setup();
        let pid = db
            .with_conn::<_, StorageError>(|c| {
                people::insert(c, &serde_json::from_value(json!({"name": "田中"})).unwrap())
            })
            .unwrap();
        let ins = make_proposal(
            &db,
            "fact",
            "insert",
            None,
            json!({"entity_type": "person", "entity_id": pid, "statement": "旧情報"}),
            None,
        );
        let fid = db
            .with_conn::<_, ToolError>(|c| Ok(apply(c, &ins)?.id))
            .unwrap();
        let sup = make_proposal(
            &db,
            "fact",
            "supersede",
            Some(fid),
            json!({"entity_type": "person", "entity_id": pid, "statement": "新情報"}),
            None,
        );
        let new_id = db
            .with_conn::<_, ToolError>(|c| Ok(apply(c, &sup)?.id))
            .unwrap();
        db.with_conn::<_, StorageError>(|c| {
            assert_eq!(
                facts::get(c, fid, &ScopeSet::single("cn"))?
                    .unwrap()
                    .superseded_by,
                Some(new_id)
            );
            Ok(())
        })
        .unwrap();
        let historical_update = make_proposal(
            &db,
            "fact",
            "update",
            Some(fid),
            json!({"statement": "履歴の改変"}),
            None,
        );
        let err = db
            .with_conn::<_, ToolError>(|c| apply(c, &historical_update).map(|_| ()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        db.with_conn::<_, StorageError>(|c| {
            let old = facts::get(c, fid, &ScopeSet::single("cn"))?.unwrap();
            let current = facts::get(c, new_id, &ScopeSet::single("cn"))?.unwrap();
            assert_eq!(old.statement, "旧情報");
            assert_eq!(old.superseded_by, Some(new_id));
            assert_eq!(current.statement, "新情報");
            Ok(())
        })
        .unwrap();
        let upd = make_proposal(
            &db,
            "person",
            "update",
            Some(9999),
            json!({"role": "PM"}),
            None,
        );
        let err = db
            .with_conn::<_, ToolError>(|c| apply(c, &upd).map(|_| ()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
    }

    #[test]
    fn fact_update_validates_the_merged_predicate_and_value() {
        let db = setup();
        let pid = db
            .with_conn::<_, StorageError>(|c| {
                people::insert(c, &serde_json::from_value(json!({"name": "田中"})).unwrap())
            })
            .unwrap();
        let structured = make_proposal(
            &db,
            "fact",
            "insert",
            None,
            json!({
                "entity_type": "person",
                "entity_id": pid,
                "statement": "役職はマネージャー",
                "predicate": "role",
                "value": "manager"
            }),
            None,
        );
        let structured_id = db
            .with_conn::<_, ToolError>(|c| Ok(apply(c, &structured)?.id))
            .unwrap();
        let valid_update = make_proposal(
            &db,
            "fact",
            "update",
            Some(structured_id),
            json!({"value": "director"}),
            None,
        );
        db.with_conn::<_, ToolError>(|c| {
            apply(c, &valid_update)?;
            let got = facts::get(c, structured_id, &ScopeSet::single("cn"))?.expect("updated fact");
            assert_eq!(got.predicate.as_deref(), Some("role"));
            assert_eq!(got.value.as_deref(), Some("director"));
            Ok(())
        })
        .unwrap();

        let free_text = make_proposal(
            &db,
            "fact",
            "insert",
            None,
            json!({"entity_type": "person", "entity_id": pid, "statement": "自由文"}),
            None,
        );
        let free_text_id = db
            .with_conn::<_, ToolError>(|c| Ok(apply(c, &free_text)?.id))
            .unwrap();
        let invalid_update = make_proposal(
            &db,
            "fact",
            "update",
            Some(free_text_id),
            json!({"value": "orphan"}),
            None,
        );
        let err = db
            .with_conn::<_, ToolError>(|c| apply(c, &invalid_update).map(|_| ()))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
    }

    #[test]
    fn inline_provenance_rejected_for_glossary_target() {
        let db = setup();
        let proposal = make_proposal(
            &db,
            "glossary",
            "insert",
            None,
            json!({"term": "SCIM"}),
            Some(json!({"system": "notion", "uri": "https://x", "note": "n"})),
        );
        let err = db
            .with_conn::<_, ToolError>(|c| {
                let outcome = apply(c, &proposal)?;
                materialize_provenance(c, &proposal, &outcome).map(|_| ())
            })
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
    }

    #[test]
    fn request_id_lookup_and_scoped_list() {
        let db = setup();
        let p = make_proposal(
            &db,
            "organization",
            "insert",
            None,
            json!({"name": "ACME"}),
            None,
        );
        db.with_conn::<_, StorageError>(|c| {
            assert_eq!(
                proposals::find_by_request_id(c, &p.request_id)?.unwrap().id,
                p.id
            );
            assert!(proposals::find_by_request_id(c, "missing")?.is_none());
            let pending =
                proposals::list(c, "pending".parse().unwrap(), &ScopeSet::single("cn"), 10)?;
            assert!(pending.iter().any(|x| x.id == p.id));
            assert!(
                proposals::list(c, "approved".parse().unwrap(), &ScopeSet::single("cn"), 10)?
                    .is_empty()
            );
            Ok(())
        })
        .unwrap();
    }
}

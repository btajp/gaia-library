//! resolve_source。登録済み参照を特定し、`system` に対応する解決器で本文を取得する。
//! DB へは戻らない（`last_verified` も更新しない）。唯一の書き込みは複数 scope 明示時の
//! `audit_log(cross_scope_read)`。参照特定後の失敗はすべて `resolved=false` ＋ 固定文言 reason。
use std::time::Instant;

use crate::{
    contracts::types::{Reference, ResolveSourceInput, ResolveSourceOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    sources::{Availability, Note, Reason, ResolveRequest, Unresolved, join_notes, shape_content},
    storage::refs,
};

use super::CallContext;

const NOT_FOUND: &str = "reference not found in the effective scope";
const MAX_URI_BYTES: usize = 2048;

pub fn handle(
    ctx: &CallContext<'_>,
    input: ResolveSourceInput,
) -> Result<ResolveSourceOutput, ToolError> {
    let uri = match input.uri.as_deref().map(str::trim) {
        None => None,
        Some(uri) => {
            if uri.is_empty() || uri.len() > MAX_URI_BYTES || uri.chars().any(char::is_control) {
                return Err(ToolError::invalid_params(
                    "uri must be a non-empty string of at most 2048 bytes without control characters",
                ));
            }
            Some(uri.to_string())
        }
    };
    if input.ref_id.is_none() && uri.is_none() {
        return Err(ToolError::invalid_params("pass ref_id or uri"));
    }
    // 参照の特定。閉包は Reference を返し、ここで DB ロックを手放す（解決中は DB を塞がない）。
    let reference: Reference = ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "resolve_source")?;
        let found = match (input.ref_id, uri.as_deref()) {
            (Some(id), None) => refs::get(c, id, &scopes)?,
            (None, Some(uri)) => refs::latest_by_uri(c, uri, &scopes)?,
            (Some(id), Some(uri)) => refs::get(c, id, &scopes)?.filter(|r| r.uri == uri),
            (None, None) => None,
        };
        found.ok_or_else(|| ToolError::not_found(NOT_FOUND))
    })?;
    let started = Instant::now();
    let system = reference.system.clone();
    let (resolved, content, reason) = resolve(ctx, &reference)?;
    tracing::info!(
        tool = "resolve_source",
        client = %ctx.client.name,
        system = %system,
        ref_id = reference.id,
        resolved,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "resolve_source finished"
    );
    Ok(ResolveSourceOutput {
        reference,
        resolved,
        content,
        reason,
    })
}

type Outcome = (bool, Option<String>, Option<String>);

/// 解決器の選択と呼び出し。`ToolError` は同時実行上限（busy）のみ。それ以外は `resolved=false`。
fn resolve(ctx: &CallContext<'_>, reference: &Reference) -> Result<Outcome, ToolError> {
    let unresolved = |reason: Reason| {
        let mut notes = vec![];
        if reference.snapshot.is_some() {
            notes.push(Note::SnapshotFallback);
        }
        let reason = match join_notes(&notes) {
            Some(notes) => format!("{reason}; {notes}"),
            None => reason.to_string(),
        };
        Ok((false, None, Some(reason)))
    };
    let settings = match ctx.sources.settings() {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(error = %error, "resolve_source: source settings could not be loaded");
            return unresolved(Reason::SettingsUnavailable);
        }
    };
    let Some(resolver) = ctx.sources.get(&reference.system) else {
        return unresolved(Reason::NoResolver {
            system: reference.system.clone(),
            available: ctx.sources.systems(),
        });
    };
    if let Availability::Unconfigured { setting } = resolver.availability(&settings) {
        return unresolved(Reason::NotConfigured {
            system: resolver.system(),
            setting,
        });
    }
    let system = resolver.system();
    let Some(_permit) = ctx.sources.acquire(system) else {
        return Err(ToolError::busy(format!(
            "resolver `{system}` is busy; retry later"
        )));
    };
    match resolver.resolve(ResolveRequest {
        reference,
        settings: &settings,
    }) {
        Ok(resolved) => {
            let (content, mut notes) = shape_content(resolved.content, settings.max_content_chars);
            let mut all = resolved.notes;
            all.append(&mut notes);
            Ok((true, Some(content), join_notes(&all)))
        }
        Err(Unresolved::Unavailable(reason)) => unresolved(reason),
        Err(Unresolved::Internal(detail)) => {
            tracing::warn!(
                system,
                ref_id = reference.id,
                detail = %detail,
                "resolve_source: resolver failed"
            );
            unresolved(Reason::ResolverFailed)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Barrier},
        thread,
        time::Duration,
    };

    use serde_json::json;

    use crate::{
        error::ErrorCode,
        sources::{
            Reason, SourceRegistry,
            test_support::{StubBehavior, StubResolver},
        },
        storage::{StorageError, audit},
        tools::{
            ToolService,
            test_support::{agent, human, seed_basic, service, write},
        },
    };

    fn service_with(stub: StubResolver) -> (ToolService, Arc<StubResolver>) {
        let stub = Arc::new(stub);
        let mut registry = SourceRegistry::empty();
        registry.register(stub.clone()).unwrap();
        (service().with_sources(registry), stub)
    }

    fn add_stub_ref(s: &ToolService, fact: i64, uri: &str, scope: &str) -> i64 {
        let h = human();
        let out = s
            .call(&h, "propose_update", json!({
                "target_type": "ref", "action": "insert", "kind": "fact",
                "patch": {"target_type": "fact", "target_id": fact, "system": "stub", "uri": uri, "note": "n", "snapshot": "要点"},
                "scope": scope, "request_id": format!("stub-ref-{uri}-{scope}"),
            }))
            .unwrap();
        let approved = s
            .call(
                &h,
                "approve_proposal",
                json!({"proposal_id": out["proposal_id"], "scope": scope}),
            )
            .unwrap();
        approved["result"]["id"].as_i64().unwrap()
    }

    fn audit_count(s: &ToolService) -> usize {
        s.db()
            .with_conn::<_, StorageError>(|c| audit::recent(c, 1000))
            .unwrap()
            .len()
    }

    fn last_verified(s: &ToolService, id: i64) -> Option<String> {
        s.db()
            .with_conn::<_, StorageError>(|c| {
                Ok(
                    c.query_row("SELECT last_verified FROM refs WHERE id = ?1", [id], |r| {
                        r.get::<_, Option<String>>(0)
                    })?,
                )
            })
            .unwrap()
    }

    #[test]
    fn input_and_reference_selection() {
        let s = service();
        let ids = seed_basic(&s);
        let err = s.call(&agent(), "resolve_source", json!({})).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        for bad in ["", "   ", "a\nb", &"x".repeat(2049)] {
            let err = s
                .call(&agent(), "resolve_source", json!({"uri": bad}))
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidParams, "{bad:?}");
        }
        // 不在・他 scope・uri 他 scope・両指定不一致 → 同一文言の not_found
        let mut messages = vec![];
        for args in [
            json!({"ref_id": 9999}),
            json!({"ref_id": ids.reference, "scope": "other"}),
            json!({"uri": "minutes://meeting/42#t=1200", "scope": "other"}),
            json!({"ref_id": ids.reference, "uri": "minutes://meeting/43"}),
        ] {
            let err = s.call(&agent(), "resolve_source", args).unwrap_err();
            assert_eq!(err.code, ErrorCode::NotFound);
            messages.push(err.message);
        }
        assert!(messages.iter().all(|m| m == &messages[0]), "{messages:?}");
        // uri 指定は最新 1 件（id 最大）
        let second = write(
            &s,
            "ref",
            json!({"target_type": "fact", "target_id": ids.fact, "system": "minutes",
                   "uri": "minutes://meeting/42#t=1200", "note": "新しい方"}),
        );
        let out = s
            .call(
                &agent(),
                "resolve_source",
                json!({"uri": " minutes://meeting/42#t=1200 "}),
            )
            .unwrap();
        assert_eq!(out["reference"]["id"].as_i64().unwrap(), second);
        // 両指定一致は通る
        let out = s
            .call(
                &agent(),
                "resolve_source",
                json!({"ref_id": ids.reference, "uri": "minutes://meeting/42#t=1200"}),
            )
            .unwrap();
        assert_eq!(out["reference"]["id"].as_i64().unwrap(), ids.reference);
    }

    #[test]
    fn unknown_system_is_unresolved_with_snapshot_and_no_writes() {
        let s = service();
        let ids = seed_basic(&s);
        let audits = audit_count(&s);
        let out = s
            .call(&agent(), "resolve_source", json!({"ref_id": ids.reference}))
            .unwrap();
        assert_eq!(out["resolved"], false);
        assert!(out.get("content").is_none());
        assert_eq!(
            out["reason"],
            "no resolver for system `minutes` (available: none); fallback: see reference.snapshot"
        );
        assert_eq!(out["reference"]["snapshot"], "SCIM は Phase 2");
        assert_eq!(audit_count(&s), audits, "single scope: no audit row");
        assert_eq!(last_verified(&s, ids.reference), None);
        // 複数 scope 明示時のみ cross_scope_read が 1 件増える
        let out = s
            .call(
                &agent(),
                "resolve_source",
                json!({"ref_id": ids.reference, "scope": ["cn", "other"]}),
            )
            .unwrap();
        assert_eq!(out["resolved"], false);
        assert_eq!(audit_count(&s), audits + 1);
    }

    #[test]
    fn stub_success_failure_and_internal_paths() {
        let (s, stub) = service_with(StubResolver::new(StubBehavior::Ok("本文".repeat(10))));
        let ids = seed_basic(&s);
        let rid = add_stub_ref(&s, ids.fact, "stub://one", "cn");
        let before = last_verified(&s, rid);
        let out = s
            .call(&agent(), "resolve_source", json!({"ref_id": rid}))
            .unwrap();
        assert_eq!(out["resolved"], true);
        assert_eq!(out["content"], "本文".repeat(10));
        assert!(out.get("reason").is_none());
        assert_eq!(
            last_verified(&s, rid),
            before,
            "last_verified must not change"
        );

        *stub.behavior.lock().unwrap() =
            StubBehavior::Ok(format!("\u{feff}a\u{1}{}", "字".repeat(40_000)));
        let out = s
            .call(&agent(), "resolve_source", json!({"ref_id": rid}))
            .unwrap();
        assert_eq!(out["resolved"], true);
        assert_eq!(out["content"].as_str().unwrap().chars().count(), 30_000);
        assert_eq!(
            out["reason"],
            "control characters removed; content truncated to 30000 of 40001 chars"
        );

        *stub.behavior.lock().unwrap() = StubBehavior::Unavailable(Reason::TimedOut { secs: 3 });
        let out = s
            .call(&agent(), "resolve_source", json!({"ref_id": rid}))
            .unwrap();
        assert_eq!(out["resolved"], false);
        assert_eq!(
            out["reason"],
            "resolution timed out after 3s; fallback: see reference.snapshot"
        );
        assert!(out.get("content").is_none());

        *stub.behavior.lock().unwrap() = StubBehavior::Internal("secret path /x".into());
        let out = s
            .call(&agent(), "resolve_source", json!({"ref_id": rid}))
            .unwrap();
        assert_eq!(out["resolved"], false);
        assert_eq!(
            out["reason"],
            "resolver failed unexpectedly (see server log); fallback: see reference.snapshot"
        );
        assert!(!out["reason"].as_str().unwrap().contains("/x"));
    }

    #[test]
    fn unconfigured_resolver_is_not_called_and_system_lookup_is_normalized() {
        let mut stub = StubResolver::new(StubBehavior::Ok("x".into()));
        stub.ready = false;
        let (s, stub) = service_with(stub);
        let ids = seed_basic(&s);
        let rid = add_stub_ref(&s, ids.fact, "stub://two", "cn");
        s.db()
            .with_conn::<_, StorageError>(|c| {
                c.execute("UPDATE refs SET system = ' Stub ' WHERE id = ?1", [rid])?;
                Ok(())
            })
            .unwrap();
        let out = s
            .call(&agent(), "resolve_source", json!({"ref_id": rid}))
            .unwrap();
        assert_eq!(out["resolved"], false);
        assert_eq!(
            out["reason"],
            "resolver `stub` is not configured (set [sources.stub]); fallback: see reference.snapshot"
        );
        assert_eq!(stub.calls(), 0);
    }

    #[test]
    fn concurrency_limit_yields_busy_and_db_stays_unlocked_during_resolution() {
        let mut stub = StubResolver::new(StubBehavior::Ok("slow".into()));
        stub.concurrency = 1;
        let (s, stub) = service_with(stub);
        let ids = seed_basic(&s);
        let rid = add_stub_ref(&s, ids.fact, "stub://three", "cn");
        let s = Arc::new(s);
        let barrier = Arc::new(Barrier::new(2));
        *stub.barrier.lock().unwrap() = Some(barrier.clone());
        let worker = {
            let s = s.clone();
            thread::spawn(move || s.call(&agent(), "resolve_source", json!({"ref_id": rid})))
        };
        // 解決器が Barrier で待つ間、別スレッドの読み取りと 2 件目の要求が完了する
        thread::sleep(Duration::from_millis(100));
        let person = s
            .call(&agent(), "get_person", json!({"person_id": ids.person}))
            .unwrap();
        assert_eq!(person["person"]["id"].as_i64().unwrap(), ids.person);
        let second = s
            .call(&agent(), "resolve_source", json!({"ref_id": rid}))
            .unwrap_err();
        assert_eq!(second.code, ErrorCode::Busy);
        assert!(
            second.message.contains("resolver `stub` is busy"),
            "{second}"
        );
        barrier.wait();
        let first = worker.join().unwrap().unwrap();
        assert_eq!(first["resolved"], true);
        assert_eq!(first["content"], "slow");
    }

    #[test]
    fn server_info_lists_ready_resolvers_only() {
        let s = service();
        let info = s.call(&agent(), "get_server_info", json!({})).unwrap();
        assert_eq!(info["contract_version"], "1.1.0");
        assert_eq!(info["capabilities"]["resolvers"], json!([]));
        let (s, _) = service_with(StubResolver::new(StubBehavior::Ok("x".into())));
        let info = s.call(&agent(), "get_server_info", json!({})).unwrap();
        assert_eq!(info["capabilities"]["resolvers"], json!(["stub"]));
        let mut stub = StubResolver::new(StubBehavior::Ok("x".into()));
        stub.ready = false;
        let (s, _) = service_with(stub);
        let info = s.call(&agent(), "get_server_info", json!({})).unwrap();
        assert_eq!(info["capabilities"]["resolvers"], json!([]));
    }
}

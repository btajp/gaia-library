//! resolve_speakers。会議ツールの表示名 → people の突合（話者実名化用）。仕様書 §8.3。
use rusqlite::Connection;

use crate::{
    contracts::types::{
        ResolveSpeakersInput, ResolveSpeakersOutput, SpeakerCandidate, SpeakerResult, SpeakerStatus,
    },
    domain::normalize::normalize_name,
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, people},
};

use super::CallContext;

pub fn handle(
    ctx: &CallContext<'_>,
    input: ResolveSpeakersInput,
) -> Result<ResolveSpeakersOutput, ToolError> {
    ctx.db.with_conn(|c| {
        // 人物は名寄せ層（共有）なので、scope が要るのは engagement の関係者を引くときだけ。
        let preferred: Vec<i64> = match input.engagement_id {
            Some(eid) => {
                let scopes =
                    ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
                scopes.audit_cross_read(c, &ctx.client.name, "resolve_speakers")?;
                if engagements::get(c, eid, &scopes)?.is_none() {
                    return Err(ToolError::not_found(format!("engagement {eid}")));
                }
                engagements::member_ids(c, eid, &scopes)?
            }
            None => Vec::new(),
        };
        let mut results = Vec::with_capacity(input.display_names.len());
        for raw in &input.display_names {
            results.push(resolve_one(c, raw, &preferred)?);
        }
        Ok(ResolveSpeakersOutput { results })
    })
}

fn resolve_one(c: &Connection, raw: &str, preferred: &[i64]) -> Result<SpeakerResult, ToolError> {
    let normalized = normalize_name(raw);
    if normalized.is_empty() {
        return Ok(result(
            raw,
            normalized,
            SpeakerStatus::Unmatched,
            0.0,
            None,
            Vec::new(),
        ));
    }
    let matches = people::find_by_alias_normalized(c, &normalized)?;
    match matches.len() {
        1 => {
            let person = matches.into_iter().next().expect("len checked");
            Ok(result(
                raw,
                normalized,
                SpeakerStatus::Matched,
                1.0,
                Some(person),
                Vec::new(),
            ))
        }
        0 => {
            let candidates: Vec<SpeakerCandidate> = people::search_like(c, raw, 5)?
                .into_iter()
                .map(|p| SpeakerCandidate {
                    confidence: if preferred.contains(&p.id) { 0.6 } else { 0.4 },
                    reason: "partial match".to_string(),
                    person_id: p.id,
                    name: p.name,
                })
                .collect();
            Ok(result(
                raw,
                normalized,
                SpeakerStatus::Unmatched,
                0.0,
                None,
                candidates,
            ))
        }
        _ => {
            // 完全一致が複数。engagement の関係者で 1 人に絞れれば matched(0.9)。
            let narrowed: Vec<_> = matches
                .iter()
                .filter(|p| preferred.contains(&p.id))
                .cloned()
                .collect();
            if narrowed.len() == 1 {
                let person = narrowed.into_iter().next().expect("len checked");
                return Ok(result(
                    raw,
                    normalized,
                    SpeakerStatus::Matched,
                    0.9,
                    Some(person),
                    Vec::new(),
                ));
            }
            let candidates: Vec<SpeakerCandidate> = matches
                .iter()
                .map(|p| SpeakerCandidate {
                    person_id: p.id,
                    name: p.name.clone(),
                    confidence: 0.5,
                    reason: "exact alias match".to_string(),
                })
                .collect();
            Ok(result(
                raw,
                normalized,
                SpeakerStatus::Ambiguous,
                0.5,
                None,
                candidates,
            ))
        }
    }
}

fn result(
    raw: &str,
    normalized: String,
    status: SpeakerStatus,
    confidence: f64,
    person: Option<crate::contracts::types::PersonSummary>,
    candidates: Vec<SpeakerCandidate>,
) -> SpeakerResult {
    SpeakerResult {
        input: raw.to_string(),
        normalized,
        status,
        confidence,
        person,
        candidates,
    }
}

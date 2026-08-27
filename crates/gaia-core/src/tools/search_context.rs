//! search_context =「回答の設計図」。仕様書 §8.3。
use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::{
    contracts::types::{
        Fact, SearchContextInput, SearchContextOutput, SearchEntity, SearchEntityType, SearchType,
    },
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, entities, facts, glossary, interactions, organizations, people, refs},
};

use super::CallContext;

struct Hit {
    type_: SearchEntityType,
    name: String,
    summary: String,
    score: f64,
    matched_on: Vec<String>,
}

pub fn handle(
    ctx: &CallContext<'_>,
    input: SearchContextInput,
) -> Result<SearchContextOutput, ToolError> {
    let query = input.query.trim().to_string();
    if query.is_empty() {
        return Err(ToolError::invalid_params("query must not be blank"));
    }
    let limit = input.limit.clamp(1, 50) as usize;
    let wants = |t: SearchType| input.types.is_empty() || input.types.contains(&t);
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "search_context")?;
        let mut hints: Vec<String> = Vec::new();
        if query.chars().count() < 3 {
            hints.push("query is shorter than 3 characters; substring match was used instead of full-text search".into());
        }
        if scopes.is_cross() {
            hints.push("cross-scope read (recorded in the audit log)".into());
        }

        let mut hits: BTreeMap<(String, i64), Hit> = BTreeMap::new();
        if wants(SearchType::Person) {
            for p in people::search_like(c, &query, limit)? {
                let by_name = p.name.to_lowercase().contains(&query.to_lowercase());
                let summary = describe_person(&p);
                add(&mut hits, SearchEntityType::Person, p.id, p.name.clone(), summary,
                    if by_name { 3.0 } else { 2.0 }, if by_name { "name" } else { "alias" });
            }
        }
        if wants(SearchType::Organization) {
            for o in organizations::search_like(c, &query, limit)? {
                add(&mut hits, SearchEntityType::Organization, o.id, o.name.clone(), o.kind.clone().unwrap_or_default(), 3.0, "name");
            }
        }
        if wants(SearchType::Engagement) {
            for e in engagements::search_like(c, &query, &scopes, limit)? {
                let summary = format!(
                    "{}{}",
                    e.org_name.clone().map(|o| format!("{o} / ")).unwrap_or_default(),
                    e.status.clone().unwrap_or_default()
                );
                add(&mut hits, SearchEntityType::Engagement, e.id, e.name.clone(), summary, 3.0, "name");
            }
        }
        if wants(SearchType::Entity) {
            for e in entities::search_like(c, &query, limit)? {
                add(&mut hits, SearchEntityType::Entity, e.id, e.name.clone(), e.type_.clone(), 3.0, "name");
            }
        }
        // facts 全文ヒット → 親エンティティに折りたたむ
        // storage が返す BM25 / LIKE 順を単調減少の score にし、先頭の従来値 1.0 は保つ。
        for (rank, f) in facts::search(c, &query, &scopes, limit * 2)?
            .into_iter()
            .enumerate()
        {
            let et: SearchEntityType = f
                .entity_type
                .to_string()
                .parse()
                .map_err(|_| ToolError::internal("EntityType must map to SearchEntityType"))?;
            let wanted = match et {
                SearchEntityType::Person => wants(SearchType::Person),
                SearchEntityType::Organization => wants(SearchType::Organization),
                SearchEntityType::Engagement => wants(SearchType::Engagement),
                SearchEntityType::Entity => wants(SearchType::Entity),
                SearchEntityType::Interaction => wants(SearchType::Interaction),
            };
            if !wanted {
                continue;
            }
            if let Some((name, summary)) = entity_headline(c, &f, &scopes)? {
                let score = 1.0 / (rank as f64 + 1.0);
                add(
                    &mut hits,
                    et,
                    f.entity_id,
                    name,
                    summary,
                    score,
                    &format!("fact:{}", f.id),
                );
            }
        }

        let mut entity_list: Vec<SearchEntity> = Vec::with_capacity(hits.len());
        for ((type_str, id), hit) in hits {
            let fact_list = facts::for_entity(c, &type_str, id, &scopes, 20)?;
            let mut ref_list = refs::for_target(c, &type_str, id, &scopes)?;
            for f in &fact_list {
                ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
            }
            entity_list.push(SearchEntity {
                type_: hit.type_,
                id,
                name: hit.name,
                summary: hit.summary,
                score: hit.score,
                matched_on: hit.matched_on,
                facts: fact_list,
                refs: ref_list,
            });
        }
        entity_list.sort_by(|a, b| {
            b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.name.cmp(&b.name))
        });
        entity_list.truncate(limit);

        let glossary_list = if wants(SearchType::Glossary) { glossary::search_like(c, &query, &scopes, limit)? } else { Vec::new() };
        let interaction_list =
            if wants(SearchType::Interaction) { interactions::search_like(c, &query, &scopes, limit)? } else { Vec::new() };
        Ok(SearchContextOutput {
            query: query.clone(),
            scopes: scopes.names().to_vec(),
            cross_scope: scopes.is_cross(),
            entities: entity_list,
            glossary: glossary_list,
            interactions: interaction_list,
            hints,
        })
    })
}

fn describe_person(p: &crate::contracts::types::PersonSummary) -> String {
    match (&p.role, &p.org_name) {
        (Some(role), Some(org)) => format!("{role} @ {org}"),
        (Some(role), None) => role.clone(),
        (None, Some(org)) => format!("@ {org}"),
        (None, None) => String::new(),
    }
}

fn entity_headline(
    c: &Connection,
    f: &Fact,
    scopes: &ScopeSet,
) -> Result<Option<(String, String)>, ToolError> {
    Ok(match f.entity_type.to_string().as_str() {
        "person" => people::get(c, f.entity_id)?.map(|p| {
            let s = describe_person(&p);
            (p.name, s)
        }),
        "organization" => {
            organizations::get(c, f.entity_id)?.map(|o| (o.name, o.kind.unwrap_or_default()))
        }
        "engagement" => engagements::get(c, f.entity_id, scopes)?
            .map(|e| (e.name, e.status.unwrap_or_default())),
        "interaction" => interactions::get(c, f.entity_id, scopes)?
            .map(|i| (format!("{} {}", i.occurred_at, i.kind), i.summary)),
        "entity" => entities::get(c, f.entity_id)?.map(|e| (e.name, e.type_)),
        _ => None,
    })
}

fn add(
    hits: &mut BTreeMap<(String, i64), Hit>,
    t: SearchEntityType,
    id: i64,
    name: String,
    summary: String,
    score: f64,
    matched: &str,
) {
    let entry = hits.entry((t.to_string(), id)).or_insert_with(|| Hit {
        type_: t,
        name,
        summary,
        score: 0.0,
        matched_on: Vec::new(),
    });
    entry.score += score;
    entry.matched_on.push(matched.to_string());
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::tools::test_support::{agent, service, write};

    #[test]
    fn fact_rank_affects_final_entity_order() {
        let service = service();
        let alpha = write(
            &service,
            "entity",
            json!({"type": "topic", "name": "Alpha"}),
        );
        let zulu = write(&service, "entity", json!({"type": "topic", "name": "Zulu"}));
        write(
            &service,
            "fact",
            json!({
                "entity_type": "entity",
                "entity_id": alpha,
                "statement": "rankingneedle is mentioned once among several filler words"
            }),
        );
        write(
            &service,
            "fact",
            json!({
                "entity_type": "entity",
                "entity_id": zulu,
                "statement": "rankingneedle rankingneedle rankingneedle"
            }),
        );

        let out = service
            .call(
                &agent(),
                "search_context",
                json!({"query": "rankingneedle", "types": ["entity"]}),
            )
            .unwrap();
        let entities = out["entities"].as_array().unwrap();

        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0]["id"], zulu);
        assert_eq!(entities[1]["id"], alpha);
        assert!(entities[0]["score"].as_f64().unwrap() > entities[1]["score"].as_f64().unwrap());
    }

    #[test]
    fn fact_scores_accumulate_with_name_score() {
        let service = service();
        let entity = write(
            &service,
            "entity",
            json!({"type": "topic", "name": "rankingneedle aggregate"}),
        );
        for statement in [
            "rankingneedle appears in the primary fact",
            "rankingneedle appears in the secondary fact",
        ] {
            write(
                &service,
                "fact",
                json!({
                    "entity_type": "entity",
                    "entity_id": entity,
                    "statement": statement
                }),
            );
        }

        let out = service
            .call(
                &agent(),
                "search_context",
                json!({"query": "rankingneedle", "types": ["entity"]}),
            )
            .unwrap();
        let hit = &out["entities"][0];
        let matched_on = hit["matched_on"].as_array().unwrap();

        assert_eq!(hit["id"], entity);
        assert!(hit["score"].as_f64().unwrap() > 4.0);
        assert!(matched_on.iter().any(|matched| matched == "name"));
        assert_eq!(
            matched_on
                .iter()
                .filter(|matched| matched.as_str().unwrap().starts_with("fact:"))
                .count(),
            2
        );
    }
}

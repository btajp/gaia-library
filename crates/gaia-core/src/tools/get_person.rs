//! get_person。人物の詳細＋facts＋refs＋直近の interactions。
use serde_json::json;

use crate::{
    contracts::types::{GetPersonInput, GetPersonOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, facts, interactions, organizations, people, refs},
};

use super::CallContext;

pub fn handle(ctx: &CallContext<'_>, input: GetPersonInput) -> Result<GetPersonOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let person = match (input.person_id, input.name.as_deref()) {
            (Some(id), _) => people::get(c, id)?.ok_or_else(|| ToolError::not_found(format!("person {id}")))?,
            (None, Some(name)) => {
                let found = people::find_by_name(c, name)?;
                match found.len() {
                    0 => return Err(ToolError::not_found(format!("person `{name}`"))),
                    1 => found.into_iter().next().expect("len checked"),
                    _ => {
                        let candidates: Vec<_> = found
                            .iter()
                            .map(|p| json!({"person_id": p.id, "name": p.name, "org_name": p.org_name}))
                            .collect();
                        return Err(ToolError::conflict(format!("multiple people match `{name}`; pass person_id"))
                            .with_details(json!({"candidates": candidates})));
                    }
                }
            }
            (None, None) => return Err(ToolError::invalid_params("pass person_id or name")),
        };
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_person")?;
        let organization = match person.org_id {
            Some(oid) => organizations::get(c, oid)?,
            None => None,
        };
        let engagement_list = engagements::for_person(c, person.id, &scopes)?;
        let fact_list = facts::for_entity(c, "person", person.id, &scopes, 50)?;
        let mut ref_list = refs::for_target(c, "person", person.id, &scopes)?;
        for f in &fact_list {
            ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
        }
        let interaction_list = interactions::recent_for_person(c, person.id, &scopes, 20)?;
        Ok(GetPersonOutput {
            person,
            organization,
            engagements: engagement_list,
            facts: fact_list,
            refs: ref_list,
            interactions: interaction_list,
        })
    })
}

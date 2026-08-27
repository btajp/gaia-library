//! get_organization。組織の詳細＋所属人物＋案件（scope 内）＋facts＋refs。
use serde_json::json;

use crate::{
    contracts::types::{GetOrganizationInput, GetOrganizationOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, facts, organizations, people, refs},
};

use super::CallContext;

pub fn handle(
    ctx: &CallContext<'_>,
    input: GetOrganizationInput,
) -> Result<GetOrganizationOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let organization = match (input.organization_id, input.name.as_deref()) {
            (Some(id), _) => organizations::get(c, id)?.ok_or_else(|| ToolError::not_found(format!("organization {id}")))?,
            (None, Some(name)) => {
                let found = organizations::find_by_name(c, name)?;
                match found.len() {
                    0 => return Err(ToolError::not_found(format!("organization `{name}`"))),
                    1 => found.into_iter().next().expect("len checked"),
                    _ => {
                        let candidates: Vec<_> =
                            found.iter().map(|o| json!({"organization_id": o.id, "name": o.name, "kind": o.kind})).collect();
                        return Err(ToolError::conflict(format!("multiple organizations match `{name}`; pass organization_id"))
                            .with_details(json!({"candidates": candidates})));
                    }
                }
            }
            (None, None) => return Err(ToolError::invalid_params("pass organization_id or name")),
        };
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_organization")?;
        let people_list = people::list_by_org(c, organization.id)?;
        let engagement_list = engagements::for_org(c, organization.id, &scopes)?;
        let fact_list = facts::for_entity(c, "organization", organization.id, &scopes, 50)?;
        let mut ref_list = refs::for_target(c, "organization", organization.id, &scopes)?;
        for f in &fact_list {
            ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
        }
        Ok(GetOrganizationOutput {
            organization,
            people: people_list,
            engagements: engagement_list,
            facts: fact_list,
            refs: ref_list,
        })
    })
}

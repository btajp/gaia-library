//! get_engagement。案件の詳細＋関係者（alias 込み）＋facts＋refs＋用語集＋直近 interactions。
//! 案件自体が scope 外なら not_found（存在を漏らさない）。
use serde_json::json;

use crate::{
    contracts::types::{GetEngagementInput, GetEngagementOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, facts, glossary, interactions, organizations, refs},
};

use super::CallContext;

pub fn handle(
    ctx: &CallContext<'_>,
    input: GetEngagementInput,
) -> Result<GetEngagementOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_engagement")?;
        let engagement = match (input.engagement_id, input.name.as_deref()) {
            (Some(id), _) => engagements::get(c, id, &scopes)?.ok_or_else(|| ToolError::not_found(format!("engagement {id}")))?,
            (None, Some(name)) => {
                let found = engagements::find_by_name(c, name, &scopes)?;
                match found.len() {
                    0 => return Err(ToolError::not_found(format!("engagement `{name}`"))),
                    1 => found.into_iter().next().expect("len checked"),
                    _ => {
                        let candidates: Vec<_> =
                            found.iter().map(|e| json!({"engagement_id": e.id, "name": e.name, "scope": e.scope})).collect();
                        return Err(ToolError::conflict(format!("multiple engagements match `{name}`; pass engagement_id"))
                            .with_details(json!({"candidates": candidates})));
                    }
                }
            }
            (None, None) => return Err(ToolError::invalid_params("pass engagement_id or name")),
        };
        let organization = match engagement.org_id {
            Some(oid) => organizations::get(c, oid)?,
            None => None,
        };
        let member_list = engagements::members(c, engagement.id)?;
        let fact_list = facts::for_entity(c, "engagement", engagement.id, &scopes, 50)?;
        let mut ref_list = refs::for_target(c, "engagement", engagement.id, &scopes)?;
        for f in &fact_list {
            ref_list.extend(refs::for_target(c, "fact", f.id, &scopes)?);
        }
        let glossary_list = glossary::list(c, Some(engagement.id), &scopes)?;
        let interaction_list = interactions::recent_for_engagement(c, engagement.id, &scopes, 20)?;
        Ok(GetEngagementOutput {
            engagement,
            organization,
            people: member_list,
            facts: fact_list,
            refs: ref_list,
            glossary: glossary_list,
            interactions: interaction_list,
        })
    })
}

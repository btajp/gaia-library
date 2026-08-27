//! get_glossary。用語集と、Whisper initial_prompt 用の語彙ヒント。
use std::collections::HashSet;

use crate::{
    contracts::types::{GetGlossaryInput, GetGlossaryOutput},
    error::ToolError,
    scope::{ScopeSet, scope_input_to_vec},
    storage::{engagements, glossary},
};

use super::CallContext;

pub fn handle(
    ctx: &CallContext<'_>,
    input: GetGlossaryInput,
) -> Result<GetGlossaryOutput, ToolError> {
    ctx.db.with_conn(|c| {
        let scopes = ScopeSet::resolve(c, ctx.client, scope_input_to_vec(input.scope.as_ref()))?;
        scopes.audit_cross_read(c, &ctx.client.name, "get_glossary")?;
        if let Some(eid) = input.engagement_id
            && engagements::get(c, eid, &scopes)?.is_none()
        {
            return Err(ToolError::not_found(format!("engagement {eid}")));
        }
        let terms = glossary::list(c, input.engagement_id, &scopes)?;
        let mut hints: Vec<String> = Vec::new();
        for t in &terms {
            hints.push(t.term.clone());
            if let Some(r) = &t.reading {
                hints.push(r.clone());
            }
        }
        if let Some(eid) = input.engagement_id {
            for m in engagements::members(c, eid)? {
                hints.push(m.person.name.clone());
                for a in &m.person.aliases {
                    hints.push(a.alias.clone());
                }
            }
        }
        let mut seen = HashSet::new();
        hints.retain(|h| seen.insert(h.clone()));
        Ok(GetGlossaryOutput {
            terms,
            vocabulary_hints: hints,
        })
    })
}

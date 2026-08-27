//! get_job_status。v1 にジョブは無い（narumi と契約規約を揃えるためのツール）。
use serde_json::Value;

use crate::{contracts::types::GetJobStatusInput, error::ToolError};

use super::CallContext;

pub fn handle(_ctx: &CallContext<'_>, input: GetJobStatusInput) -> Result<Value, ToolError> {
    Err(ToolError::not_found(format!(
        "job `{}` not found (gaia_library v1 has no jobs)",
        input.job_id
    )))
}

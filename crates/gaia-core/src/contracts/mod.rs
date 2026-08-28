//! 契約カタログ。build.rs が生成した自己完結スキーマの束（contracts.json）と typify 型（contract_types.rs）を読む。
use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{error::ToolError, identity::Role};

/// typify が契約から生成した型。ツール `foo_bar` → `FooBarInput` / `FooBarOutput`、`$defs` の名前はそのまま型名。
pub mod types {
    #![allow(clippy::all, dead_code, unused_imports)]
    include!(concat!(env!("OUT_DIR"), "/contract_types.rs"));
}

const BUNDLE: &str = include_str!(concat!(env!("OUT_DIR"), "/contracts.json"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolAnnotationsSpec {
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
}

impl Default for ToolAnnotationsSpec {
    fn default() -> Self {
        // MCP 仕様の既定値
        Self {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: false,
            open_world_hint: true,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawBundle {
    contract_version: String,
    server_name: String,
    tools: Vec<RawTool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTool {
    name: String,
    #[serde(default)]
    title: Option<String>,
    description: String,
    roles: Vec<Role>,
    enabled: bool,
    #[serde(default)]
    annotations: ToolAnnotationsSpec,
    input_schema: Value,
    #[serde(default)]
    output_schema: Option<Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("contract bundle is invalid: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("tool `{name}` has an invalid {which} schema: {reason}")]
    Schema {
        name: String,
        which: &'static str,
        reason: String,
    },
    #[error("duplicate tool name `{0}`")]
    Duplicate(String),
}

pub struct ToolSpec {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub roles: Vec<Role>,
    pub enabled: bool,
    pub annotations: ToolAnnotationsSpec,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    input_validator: jsonschema::Validator,
    output_validator: Option<jsonschema::Validator>,
}

impl ToolSpec {
    fn from_raw(raw: RawTool) -> Result<Self, ContractError> {
        let input_validator =
            jsonschema::validator_for(&raw.input_schema).map_err(|e| ContractError::Schema {
                name: raw.name.clone(),
                which: "input",
                reason: e.to_string(),
            })?;
        let output_validator = match &raw.output_schema {
            Some(s) => Some(
                jsonschema::validator_for(s).map_err(|e| ContractError::Schema {
                    name: raw.name.clone(),
                    which: "output",
                    reason: e.to_string(),
                })?,
            ),
            None => None,
        };
        Ok(Self {
            name: raw.name,
            title: raw.title,
            description: raw.description,
            roles: raw.roles,
            enabled: raw.enabled,
            annotations: raw.annotations,
            input_schema: raw.input_schema,
            output_schema: raw.output_schema,
            input_validator,
            output_validator,
        })
    }

    pub fn allows(&self, role: Role) -> bool {
        self.enabled && self.roles.contains(&role)
    }

    pub fn validate_input(&self, args: &Value) -> Result<(), ToolError> {
        let errors: Vec<Value> = self
            .input_validator
            .iter_errors(args)
            .map(|e| json!({ "path": e.instance_path().to_string(), "message": e.to_string() }))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ToolError::invalid_params(format!(
                "arguments for `{}` do not match the contract",
                self.name
            ))
            .with_details(json!({ "errors": errors })))
        }
    }

    pub fn validate_output(&self, out: &Value) -> Result<(), ToolError> {
        let Some(v) = &self.output_validator else {
            return Ok(());
        };
        let errors: Vec<String> = v
            .iter_errors(out)
            .map(|e| format!("{}: {e}", e.instance_path()))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ToolError::internal(format!(
                "output of `{}` violates the contract: {}",
                self.name,
                errors.join("; ")
            )))
        }
    }
}

pub struct Catalog {
    pub contract_version: String,
    pub server_name: String,
    tools: Vec<ToolSpec>,
    index: HashMap<String, usize>,
}

impl Catalog {
    pub fn embedded() -> Result<Self, ContractError> {
        Self::from_json(BUNDLE)
    }

    pub fn from_json(text: &str) -> Result<Self, ContractError> {
        let raw: RawBundle = serde_json::from_str(text)?;
        let mut tools = Vec::with_capacity(raw.tools.len());
        let mut index = HashMap::new();
        for t in raw.tools {
            if index.contains_key(&t.name) {
                return Err(ContractError::Duplicate(t.name));
            }
            index.insert(t.name.clone(), tools.len());
            tools.push(ToolSpec::from_raw(t)?);
        }
        Ok(Self {
            contract_version: raw.contract_version,
            server_name: raw.server_name,
            tools,
            index,
        })
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.index.get(name).map(|i| &self.tools[*i])
    }

    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// role に見せてよい（enabled かつ roles に含む）ツール。manifest の順序を保つ。
    pub fn visible(&self, role: Role) -> Vec<&ToolSpec> {
        self.tools.iter().filter(|t| t.allows(role)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn embedded_catalog_loads_all_tools() {
        let c = Catalog::embedded().expect("catalog");
        assert_eq!(c.server_name, "gaia_library");
        assert_eq!(c.contract_version, "1.1.0");
        assert!(c.get("get_server_info").is_some());
        assert!(c.get("get_job_status").is_some());
        assert!(c.get("nope").is_none());
    }

    #[test]
    fn schemas_are_self_contained() {
        let c = Catalog::embedded().unwrap();
        let text = serde_json::to_string(&c.get("get_server_info").unwrap().output_schema).unwrap();
        assert!(
            !text.contains("common.json"),
            "external $ref leaked: {text}"
        );
        assert!(text.contains("\"ClientInfo\""));
    }

    #[test]
    fn validate_input_reports_path_and_message() {
        let c = Catalog::embedded().unwrap();
        let spec = c.get("get_job_status").unwrap();
        assert!(spec.validate_input(&json!({"job_id": "j1"})).is_ok());
        let err = spec
            .validate_input(&json!({"job_id": 1, "extra": true}))
            .unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidParams);
        let details = err.details.unwrap();
        let paths: Vec<&str> = details["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&"/job_id"), "{paths:?}");
    }

    #[test]
    fn visible_filters_by_role_and_enabled() {
        let c = Catalog::embedded().unwrap();
        let names: Vec<&str> = c
            .visible(Role::Agent)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert!(names.contains(&"get_server_info"));
    }

    #[test]
    fn generated_types_round_trip() {
        let v = json!({"job_id": "abc"});
        let input: types::GetJobStatusInput = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(&input).unwrap(), v);
        let out = types::GetJobStatusOutput {
            job_id: "abc".into(),
            status: "unknown".into(),
            result: None,
        };
        assert_eq!(serde_json::to_value(&out).unwrap()["status"], "unknown");
    }

    #[test]
    fn all_thirteen_tools_load_and_roles_match_spec() {
        let c = Catalog::embedded().unwrap();
        assert_eq!(c.tools().len(), 13);
        let agent: Vec<&str> = c
            .visible(Role::Agent)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert!(!agent.contains(&"approve_proposal"));
        assert!(!agent.contains(&"reject_proposal"));
        assert!(
            agent.contains(&"resolve_source"),
            "enabled tool must be visible (v0.2.0)"
        );
        let human: Vec<&str> = c
            .visible(Role::Human)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert!(human.contains(&"approve_proposal"));
        assert_eq!(human.len(), 13);
    }

    #[test]
    fn scope_input_accepts_string_and_array() {
        let a: types::ScopeInput = serde_json::from_value(json!("cloudnative")).unwrap();
        let b: types::ScopeInput = serde_json::from_value(json!(["a", "b"])).unwrap();
        assert!(matches!(a, types::ScopeInput::String(_)));
        assert!(matches!(b, types::ScopeInput::Array(_)));
        let input: types::SearchContextInput =
            serde_json::from_value(json!({"query": "q"})).unwrap();
        assert_eq!(input.limit, 10);
        assert!(input.types.is_empty());
    }

    #[test]
    fn input_validators_reject_unknown_fields() {
        let c = Catalog::embedded().unwrap();
        let err = c.get("propose_update").unwrap().validate_input(&json!({
            "target_type": "person", "action": "insert", "patch": {}, "kind": "fact", "request_id": "r-00000001", "bogus": 1
        })).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidParams);
    }
}

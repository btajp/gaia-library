//! rmcp ServerHandler。gaia_core::tools::ToolService への薄いアダプタ（ツールの解釈はしない）。
use std::{borrow::Cow, sync::Arc};

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode as RpcErrorCode,
        Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
};
use serde_json::{Value, json};

use gaia_core::{
    contracts::ToolSpec, error::ToolError, identity::ClientIdentity, tools::ToolService,
};

pub struct GaiaServer {
    service: Arc<ToolService>,
    identity: ClientIdentity,
}

impl GaiaServer {
    pub fn new(service: Arc<ToolService>, identity: ClientIdentity) -> Self {
        Self { service, identity }
    }

    fn tools(&self) -> Vec<Tool> {
        self.service
            .visible_tools(self.identity.role)
            .into_iter()
            .map(to_tool)
            .collect()
    }
}

pub(crate) fn to_tool(spec: &ToolSpec) -> Tool {
    let schema = match &spec.input_schema {
        Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    let mut tool = Tool::new_with_raw(
        spec.name.clone(),
        Some(Cow::Owned(spec.description.clone())),
        Arc::new(schema),
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(spec.annotations.read_only_hint)
            .destructive(spec.annotations.destructive_hint)
            .idempotent(spec.annotations.idempotent_hint)
            .open_world(spec.annotations.open_world_hint),
    );
    if let Some(title) = &spec.title {
        tool = tool.with_title(title.clone());
    }
    if let Some(Value::Object(out)) = &spec.output_schema {
        tool = tool.with_raw_output_schema(Arc::new(out.clone()));
    }
    tool
}

fn to_rpc_error(e: &ToolError) -> ErrorData {
    use gaia_core::error::ErrorCode;
    let code = match e.code {
        ErrorCode::Unauthorized => RpcErrorCode(-32001),
        _ => RpcErrorCode::INVALID_PARAMS,
    };
    ErrorData::new(code, e.message.clone(), Some(e.to_json()))
}

fn unknown_tool_error(name: &str) -> ErrorData {
    to_rpc_error(
        &ToolError::not_found(format!("unknown tool `{name}`")).with_details(json!({"tool": name})),
    )
}

impl ServerHandler for GaiaServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("gaia_library", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "仕事の記憶の索引。search_context が要点と注記付き参照（回答の設計図）を返すので、返った refs は \
                 クライアント側のコネクタ（Notion / Box / ファイル等）で辿ること。書き込みは propose_update で提案し、\
                 人間の承認（approve_proposal）を待つ。",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // 未知ツールはプロトコルエラー（業務データの not_found と区別する）
        if self
            .service
            .catalog()
            .get(request.name.as_ref())
            .filter(|s| s.enabled)
            .is_none()
        {
            return Err(unknown_tool_error(request.name.as_ref()));
        }
        let args = request
            .arguments
            .clone()
            .map(Value::Object)
            .unwrap_or(json!({}));
        match self
            .service
            .call(&self.identity, request.name.as_ref(), args)
        {
            Ok(v) => Ok(CallToolResult::structured(v).into()),
            Err(e) if e.code.is_protocol_error() => Err(to_rpc_error(&e)),
            Err(e) => Ok(CallToolResult::structured_error(json!({"error": e.to_json()})).into()),
        }
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.service
            .catalog()
            .get(name)
            .filter(|s| s.allows(self.identity.role))
            .map(to_tool)
    }
}

#[cfg(test)]
mod tests {
    use super::{to_tool, unknown_tool_error};
    use gaia_core::contracts::Catalog;

    #[test]
    fn to_tool_carries_schema_title_and_annotations() {
        let catalog = Catalog::embedded().unwrap();
        let spec = catalog.get("search_context").unwrap();
        let tool = to_tool(spec);
        assert_eq!(tool.name, "search_context");
        assert!(tool.title.is_some());
        assert_eq!(
            tool.input_schema.get("type").and_then(|v| v.as_str()),
            Some("object")
        );
        assert!(tool.output_schema.is_some());
        let ann = tool.annotations.expect("annotations");
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.open_world_hint, Some(false));
        // 自己完結スキーマ（外部 $ref なし）で公開される
        let text = serde_json::to_string(&*tool.input_schema).unwrap();
        assert!(!text.contains("common.json"));
    }

    #[test]
    fn unknown_tool_error_has_machine_readable_product_code() {
        let error = unknown_tool_error("missing");
        assert_eq!(error.code.0, -32602);
        let data = error.data.expect("structured error data");
        assert_eq!(data["code"], "not_found");
        assert_eq!(data["details"]["tool"], "missing");
    }
}

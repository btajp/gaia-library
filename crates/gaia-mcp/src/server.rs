//! rmcp ServerHandler。gaia_core::tools::ToolService への薄いアダプタ（ツールの解釈はしない）。
use std::{borrow::Cow, sync::Arc};

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ErrorCode as RpcErrorCode,
        Extensions, Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
};
use serde_json::{Value, json};

use gaia_core::{
    contracts::ToolSpec, error::ToolError, identity::ClientIdentity, tools::ToolService,
};

enum IdentitySource {
    Fixed(ClientIdentity),
    FromRequest,
}

pub struct GaiaServer {
    service: Arc<ToolService>,
    identity: IdentitySource,
}

impl GaiaServer {
    pub fn new(service: Arc<ToolService>, identity: ClientIdentity) -> Self {
        Self {
            service,
            identity: IdentitySource::Fixed(identity),
        }
    }

    /// HTTP は Bearer middleware がリクエストごとに検証した識別を使う。
    pub fn new_http(service: Arc<ToolService>) -> Self {
        Self {
            service,
            identity: IdentitySource::FromRequest,
        }
    }

    fn resolve_identity(&self, extensions: &Extensions) -> Result<ClientIdentity, ErrorData> {
        match &self.identity {
            IdentitySource::Fixed(identity) => Ok(identity.clone()),
            IdentitySource::FromRequest => extensions
                .get::<http::request::Parts>()
                .and_then(|parts| parts.extensions.get::<ClientIdentity>())
                .cloned()
                .ok_or_else(|| {
                    to_rpc_error(&ToolError::unauthorized(
                        "missing authenticated client identity",
                    ))
                }),
        }
    }

    fn tools(&self, identity: &ClientIdentity) -> Vec<Tool> {
        self.service
            .visible_tools(identity.role)
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
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let identity = self.resolve_identity(&context.extensions)?;
        Ok(ListToolsResult::with_all_items(self.tools(&identity)))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let identity = self.resolve_identity(&context.extensions)?;
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
        match self.service.call(&identity, request.name.as_ref(), args) {
            Ok(v) => Ok(CallToolResult::structured(v).into()),
            Err(e) if e.code.is_protocol_error() => Err(to_rpc_error(&e)),
            Err(e) => Ok(CallToolResult::structured_error(json!({"error": e.to_json()})).into()),
        }
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.service
            .catalog()
            .get(name)
            // HTTP の事前スキーマ検証には request identity がない。
            // 実際の可視性と認可は list_tools / ToolService::call で強制する。
            .filter(|spec| match &self.identity {
                IdentitySource::Fixed(identity) => spec.allows(identity.role),
                IdentitySource::FromRequest => spec.enabled,
            })
            .map(to_tool)
    }
}

#[cfg(test)]
mod tests {
    use super::{GaiaServer, to_tool, unknown_tool_error};
    use gaia_core::{
        contracts::Catalog,
        identity::{ClientIdentity, Role},
        storage::Db,
        tools::ToolService,
    };
    use rmcp::{ServerHandler, model::Extensions};
    use std::sync::Arc;

    fn service() -> Arc<ToolService> {
        Arc::new(ToolService::new(
            Db::open_in_memory().unwrap(),
            Catalog::embedded().unwrap(),
        ))
    }

    fn agent() -> ClientIdentity {
        ClientIdentity {
            name: "bot".into(),
            role: Role::Agent,
            default_scope: Some("cn".into()),
        }
    }

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

    #[test]
    fn http_identity_is_resolved_from_each_request() {
        let server = GaiaServer::new_http(service());
        for identity in [
            agent(),
            ClientIdentity {
                role: Role::Human,
                ..agent()
            },
        ] {
            let (mut parts, ()) = http::Request::new(()).into_parts();
            parts.extensions.insert(identity.clone());
            let mut extensions = Extensions::new();
            extensions.insert(parts);
            assert_eq!(server.resolve_identity(&extensions).unwrap(), identity);
        }
    }

    #[test]
    fn http_identity_missing_is_structured_unauthorized() {
        let server = GaiaServer::new_http(service());
        let error = server.resolve_identity(&Extensions::new()).unwrap_err();
        assert_eq!(error.code.0, -32001);
        assert_eq!(error.data.unwrap()["code"], "unauthorized");
    }

    #[test]
    fn stdio_identity_and_tool_visibility_remain_fixed() {
        let server = GaiaServer::new(service(), agent());
        assert_eq!(
            server.resolve_identity(&Extensions::new()).unwrap(),
            agent()
        );
        assert!(server.get_tool("approve_proposal").is_none());
        let http = GaiaServer::new_http(service());
        assert!(http.get_tool("approve_proposal").is_some());
        assert!(http.get_tool("resolve_source").is_none());
    }
}

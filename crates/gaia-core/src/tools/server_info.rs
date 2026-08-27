//! get_server_info。契約版と能力、接続クライアントの識別を返す。
use crate::{
    contracts::types::{
        ClientInfo, GetServerInfoInput, GetServerInfoOutput, SearchCapabilities,
        ServerCapabilitiesInfo, ServerProtocolInfo,
    },
    error::ToolError,
};

use super::CallContext;

pub fn handle(
    ctx: &CallContext<'_>,
    _input: GetServerInfoInput,
) -> Result<GetServerInfoOutput, ToolError> {
    let tools = ctx
        .catalog
        .visible(ctx.client.role)
        .iter()
        .map(|t| t.name.clone())
        .collect();
    Ok(GetServerInfoOutput {
        name: ctx.catalog.server_name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        contract_version: ctx.catalog.contract_version.clone(),
        protocol: ServerProtocolInfo {
            transports: vec!["stdio".to_string()],
        },
        capabilities: ServerCapabilitiesInfo {
            tools,
            resolvers: Vec::new(),
            search: SearchCapabilities {
                fts: "trigram".to_string(),
            },
        },
        client: ClientInfo {
            name: ctx.client.name.clone(),
            role: ctx.client.role.to_string(),
            default_scope: ctx.client.default_scope.clone(),
        },
    })
}

//! テスト用の偽 narumi。stdio で MCP を提供し、`get_minutes` の挙動を環境変数 `FAKE_NARUMI_MODE` で切り替える。
//! 配布物には含めない。libtest の stdout 捕捉と衝突しないよう、テストはこのバイナリを子プロセスとして起動する。
//!
//! モード: ok（既定）/ not_found / scope_denied / hang / exit / junk_stdout / huge / stderr_noise / grandchild /
//! wrong_name / text_only / unresolved。`FAKE_NARUMI_PID_FILE` があれば自身（grandchild では孫）の pid を書く。
use std::{borrow::Cow, sync::Arc};

use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, InitializeResult,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
};
use serde_json::{Value, json};

struct FakeNarumi {
    mode: String,
}

fn minutes(arguments: &Value, markdown: String, unresolved: Vec<&str>) -> Value {
    let version = arguments["version"].as_u64().unwrap_or(2);
    json!({
        "meeting_id": arguments["meeting_id"],
        "version": version,
        "markdown": markdown,
        "generated_at": "2026-08-27T03:05:00Z",
        "provider": "none",
        "unresolved_speakers": unresolved,
        "available_versions": [1, 2],
    })
}

impl ServerHandler for FakeNarumi {
    fn get_info(&self) -> ServerInfo {
        let name = if self.mode == "wrong_name" {
            "someone-else"
        } else {
            "narumi"
        };
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(name, "0.0.0-fake"))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let schema = json!({
            "type": "object",
            "required": ["meeting_id"],
            "properties": {
                "meeting_id": {"type": "string"},
                "version": {"type": "integer"},
                "scope": {"type": "string"}
            }
        });
        let Value::Object(schema) = schema else {
            unreachable!()
        };
        Ok(ListToolsResult::with_all_items(vec![Tool::new_with_raw(
            "get_minutes",
            Some(Cow::Borrowed("fake get_minutes")),
            Arc::new(schema),
        )]))
    }

    // grandchild モードは「wait されない孫」を意図的に残し、gaia 側のプロセスグループ kill を検証する。
    #[allow(clippy::zombie_processes)]
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != "get_minutes" {
            return Err(ErrorData::invalid_params("unknown tool", None));
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let result = match self.mode.as_str() {
            "not_found" | "scope_denied" => CallToolResult::structured_error(json!({
                "error": {"code": self.mode, "message": "fake narumi rejected the request"}
            })),
            "hang" => std::future::pending().await,
            "grandchild" => {
                let child = std::process::Command::new("sleep")
                    .arg("300")
                    .spawn()
                    .expect("spawn grandchild");
                write_pid(child.id());
                std::future::pending().await
            }
            "huge" => CallToolResult::structured(minutes(&arguments, "字".repeat(40_000), vec![])),
            "text_only" => CallToolResult::success(vec![rmcp::model::ContentBlock::text(
                minutes(&arguments, "text-only minutes".into(), vec![]).to_string(),
            )]),
            "unresolved" => CallToolResult::structured(minutes(
                &arguments,
                "# minutes\n話者不明: 発言".into(),
                vec!["Speaker 1", "Speaker 2"],
            )),
            _ => CallToolResult::structured(minutes(
                &arguments,
                format!("# minutes\n```json\n{arguments}\n```\n"),
                vec![],
            )),
        };
        Ok(result.into())
    }
}

fn write_pid(pid: u32) {
    if let Some(path) = std::env::var_os("FAKE_NARUMI_PID_FILE") {
        let _ = std::fs::write(path, pid.to_string());
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mode = std::env::var("FAKE_NARUMI_MODE").unwrap_or_else(|_| "ok".into());
    if mode != "grandchild" {
        write_pid(std::process::id());
    }
    match mode.as_str() {
        "exit" => {
            eprintln!("fake narumi: no resident server; exiting");
            std::process::exit(1);
        }
        "junk_stdout" => println!("this is not JSON-RPC"),
        "stderr_noise" => eprintln!("fake narumi: noise on stderr"),
        _ => {}
    }
    let server = FakeNarumi { mode };
    match server.serve(rmcp::transport::stdio()).await {
        Ok(running) => {
            let _ = running.waiting().await;
        }
        Err(error) => {
            eprintln!("fake narumi: initialize failed: {error}");
            std::process::exit(2);
        }
    }
}

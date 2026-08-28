//! UI から許可済みの core API へ渡す薄いコマンド。
use gaia_core::error::ToolError;
use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::{
    client_settings::KeyStorage,
    state::{DesktopState, ServerStatus},
};

#[derive(Serialize)]
pub(crate) struct FirstRunResponse {
    agent_key: String,
    storage: KeyStorage,
}

#[tauri::command]
pub fn is_initialized(state: State<'_, DesktopState>) -> Result<bool, String> {
    state.initialized()
}

#[tauri::command]
pub(crate) async fn first_run_setup(
    state: State<'_, DesktopState>,
    affiliation: String,
    user_name: String,
) -> Result<FirstRunResponse, String> {
    let (response, storage) = state.initialize_and_store(&affiliation, &user_name).await?;
    // HTTP の失敗は server_status に残す。初期化済みの UI はそのまま利用できる。
    let _ = state.start_http().await;
    Ok(FirstRunResponse {
        agent_key: response.agent_key,
        storage,
    })
}

#[tauri::command]
pub async fn call_tool(
    state: State<'_, DesktopState>,
    name: String,
    args: Value,
) -> Result<Value, Value> {
    let runtime = state
        .runtime()
        .map_err(|error| ToolError::internal(error).to_json())?;
    // ToolService::call は同期。resolve_source のように最長数十秒ブロックするツールがあるため、
    // 全ツール一律でブロッキング用スレッドへ逃がし、画面や他のコマンドを止めない。
    tauri::async_runtime::spawn_blocking(move || {
        runtime
            .service
            .call(&runtime.human, &name, args)
            .map_err(|error| error.to_json())
    })
    .await
    .unwrap_or_else(|error| {
        Err(ToolError::internal(format!("tool task did not complete: {error}")).to_json())
    })
}

#[tauri::command]
pub async fn server_status(state: State<'_, DesktopState>) -> Result<ServerStatus, String> {
    Ok(state.server_status().await)
}

#[tauri::command]
pub fn check_updates(app: tauri::AppHandle) {
    crate::updater::spawn_manual_check(app);
}

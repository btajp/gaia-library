//! 設定画面専用の IPC。設定の読込・保存と終了を DesktopState で直列化する。
use gaia_core::{admin, identity::Role};
use serde::Serialize;
use tauri::State;

use crate::{
    cli_link::{self, LinkStatus},
    client_settings::{
        self, ClientSummary, ConnectionSnippet, IssuedKey, RenamedClient, SnippetPaths,
    },
    state::DesktopState,
};

#[derive(Serialize)]
pub(crate) struct AffiliationSummary {
    id: i64,
    name: String,
    identity: Option<String>,
}

#[tauri::command]
pub(crate) async fn admin_affiliation_list(
    state: State<'_, DesktopState>,
) -> Result<Vec<AffiliationSummary>, String> {
    state
        .run_settings(|runtime| {
            admin::list_affiliations(runtime.service.db())
                .map(|items| {
                    items
                        .into_iter()
                        .map(|item| AffiliationSummary {
                            id: item.id,
                            name: item.name,
                            identity: item.identity,
                        })
                        .collect()
                })
                .map_err(|e| e.to_string())
        })
        .await
}

#[tauri::command]
pub(crate) async fn admin_affiliation_add(
    state: State<'_, DesktopState>,
    name: String,
    identity: Option<String>,
) -> Result<i64, String> {
    state
        .run_settings(move |runtime| {
            admin::add_affiliation(
                runtime.service.db(),
                &runtime.human()?.name,
                name.trim(),
                identity
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
            )
            .map_err(|e| e.to_string())
        })
        .await
}

#[tauri::command]
pub(crate) async fn admin_client_list(
    state: State<'_, DesktopState>,
) -> Result<Vec<ClientSummary>, String> {
    state
        .run_settings(|runtime| client_settings::list(&runtime.config_path))
        .await
}

#[tauri::command]
pub(crate) async fn admin_client_add(
    state: State<'_, DesktopState>,
    name: String,
    role: Role,
    default_scope: Option<String>,
    generate_key: bool,
) -> Result<Option<IssuedKey>, String> {
    let result = state
        .run_settings(move |runtime| {
            let scope = default_scope
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(scope) = scope {
                let affiliations =
                    admin::list_affiliations(runtime.service.db()).map_err(|e| e.to_string())?;
                if !affiliations.iter().any(|item| item.name == scope) {
                    return Err(
                        "既定 scope に指定した所属元がありません。先に所属元を追加してください"
                            .into(),
                    );
                }
            }
            client_settings::add(&runtime.config_path, &name, role, scope, generate_key)
        })
        .await?;
    if result.is_some() {
        let _ = state.start_http().await;
    }
    Ok(result)
}

#[tauri::command]
pub(crate) async fn admin_client_keygen(
    state: State<'_, DesktopState>,
    name: String,
) -> Result<IssuedKey, String> {
    let result = state
        .run_settings(move |runtime| client_settings::keygen(&runtime.config_path, &name))
        .await?;
    let _ = state.start_http().await;
    Ok(result)
}

/// クライアント名の変更。設定ファイルの参照と保管キーだけを付け替え、DB の履歴は書き換えない。
/// アプリ自身の human を改名しても、以降の呼び出しは設定を読み直して新名で行う。
#[tauri::command]
pub(crate) async fn admin_client_rename(
    state: State<'_, DesktopState>,
    old_name: String,
    new_name: String,
) -> Result<RenamedClient, String> {
    state
        .run_settings(move |runtime| {
            client_settings::rename(&runtime.config_path, &old_name, &new_name)
        })
        .await
}

#[tauri::command]
pub(crate) async fn mcp_config_snippet(
    state: State<'_, DesktopState>,
    name: String,
    transport: String,
) -> Result<ConnectionSnippet, String> {
    let url = state.server_status().await.url;
    state
        .run_settings(move |runtime| {
            let cli = if transport == "stdio" {
                cli_link::bundled_cli()?
            } else {
                std::path::PathBuf::new()
            };
            client_settings::snippet(
                SnippetPaths {
                    config: &runtime.config_path,
                    db: &runtime.db_path,
                    cli: &cli,
                },
                &name,
                &transport,
                url.as_deref(),
            )
        })
        .await
}

#[tauri::command]
pub(crate) async fn cli_link_status(state: State<'_, DesktopState>) -> Result<LinkStatus, String> {
    state.run_settings(|_| cli_link::status()).await
}

#[tauri::command]
pub(crate) async fn cli_link_create(
    state: State<'_, DesktopState>,
    expected_target: Option<String>,
) -> Result<(), String> {
    state
        .run_settings(move |_| cli_link::create(expected_target.as_deref()))
        .await
}

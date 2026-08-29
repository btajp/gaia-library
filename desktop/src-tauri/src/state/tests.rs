use std::{fs, path::Path, time::Duration};

use gaia_core::auth::{generate_key, hash_key};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::*;

fn bootstrap_at(dir: &Path) -> DesktopState {
    bootstrap_with(&|name| match name {
        "GAIA_CONFIG" => Some(dir.join("config.toml").into_os_string()),
        "GAIA_DB" => Some(dir.join("gaia.db").into_os_string()),
        _ => None,
    })
}

fn client(name: &str, role: Role) -> ClientIdentity {
    ClientIdentity {
        name: name.into(),
        role,
        default_scope: Some("scope".into()),
    }
}

fn config_with_human() -> Config {
    let mut config = Config::default();
    config.add_client(client("person", Role::Human)).unwrap();
    config
}

fn save_config(dir: &Path, config: &Config) {
    config.save(&dir.join("config.toml")).unwrap();
}

#[test]
fn human_selection_uses_default_human_then_the_only_human() {
    let mut config = config_with_human();
    config.add_client(client("bot", Role::Agent)).unwrap();
    config.cli.default_client = Some("bot".into());
    assert_eq!(select_human(&config).unwrap().name, "person");
    config.add_client(client("other", Role::Human)).unwrap();
    assert!(select_human(&config).is_err());
    config.cli.default_client = Some("other".into());
    assert_eq!(select_human(&config).unwrap().name, "other");
    config
        .clients
        .retain(|identity| identity.role == Role::Agent);
    assert!(select_human(&config).is_err());
}

#[test]
fn malformed_or_unreadable_config_is_not_uninitialized() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("config.toml"), "not valid = [").unwrap();
    let state = bootstrap_at(dir.path());
    assert!(state.initialized().is_err());
    assert!(state.runtime().is_err());
    assert!(!dir.path().join("gaia.db").exists());

    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("config.toml")).unwrap();
    assert!(bootstrap_at(directory.path()).initialized().is_err());
    assert!(!directory.path().join("gaia.db").exists());
}

#[cfg(unix)]
#[test]
fn dangling_config_symlink_is_a_bootstrap_error() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("missing.toml", dir.path().join("config.toml")).unwrap();
    assert!(bootstrap_at(dir.path()).initialized().is_err());
    assert!(!dir.path().join("gaia.db").exists());
}

#[test]
fn missing_paths_are_a_bootstrap_error_without_side_effects() {
    let state = bootstrap_with(&|_| None);
    assert!(state.initialized().is_err());
}

#[test]
fn invalid_human_selection_does_not_open_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.add_client(client("bot", Role::Agent)).unwrap();
    config.cli.default_client = Some("bot".into());
    save_config(dir.path(), &config);
    let state = bootstrap_at(dir.path());
    assert!(state.initialized().is_err());
    assert!(!dir.path().join("gaia.db").exists());
}

#[tokio::test]
async fn initialization_is_serialized_and_publishes_runtime_after_save() {
    let dir = tempfile::tempdir().unwrap();
    let state = bootstrap_at(dir.path());
    assert!(!state.initialized().unwrap());
    assert!(state.runtime().is_err());
    assert!(state.server_status().await.error.is_none());
    let (first, second) = tokio::join!(
        state.initialize("scope", "user"),
        state.initialize("scope", "user")
    );
    assert!(first.is_ok());
    assert!(second.is_err());
    let response = first.unwrap();
    assert!(state.initialized().unwrap());
    let config = Config::load(&dir.path().join("config.toml")).unwrap();
    assert_eq!(config.keys["claude-code"], hash_key(&response.agent_key));
    let runtime = state.runtime().unwrap();
    assert_eq!(runtime.human().unwrap().name, "desktop:user");
    assert!(state.server_status().await.url.is_none());
    let restored = bootstrap_at(dir.path());
    assert!(restored.initialized().unwrap());
    assert_eq!(
        restored.runtime().unwrap().human().unwrap(),
        runtime.human().unwrap()
    );
}

#[tokio::test]
async fn renaming_the_desktop_human_records_later_decisions_under_the_new_name() {
    let dir = tempfile::tempdir().unwrap();
    let state = bootstrap_at(dir.path());
    state.initialize("scope", "user").await.unwrap();
    let runtime = state.runtime().unwrap();
    assert_eq!(runtime.human().unwrap().name, "desktop:user");
    assert_eq!(
        state.server_status().await.client.as_deref(),
        Some("desktop:user")
    );
    // 改名前に agent が提案しておく
    let proposed = runtime
        .service
        .call(
            &client("claude-code", Role::Agent),
            "propose_update",
            json!({
                "target_type": "person", "action": "insert", "patch": {"name": "田中 太郎"},
                "kind": "fact", "request_id": "rename-human-0001"
            }),
        )
        .unwrap();
    let proposal_id = proposed["proposal_id"].as_i64().unwrap();

    // 設定画面と同じ経路（run_settings → Config::update）で default_client の human を改名する
    state
        .run_settings(|runtime| {
            Config::update(&runtime.config_path, |config| {
                config.rename_client("desktop:user", "desktop:renamed")
            })
            .map_err(|e| e.to_string())
        })
        .await
        .unwrap();

    // 起動時の名前を保持せず、設定を読み直した新名で以降の承認を行う
    let human = runtime.human().unwrap();
    assert_eq!(human.name, "desktop:renamed");
    assert_eq!(human.role, Role::Human);
    assert_eq!(human.default_scope.as_deref(), Some("scope"));
    assert_eq!(
        state.server_status().await.client.as_deref(),
        Some("desktop:renamed")
    );
    let approved = runtime
        .service
        .call(
            &human,
            "approve_proposal",
            json!({"proposal_id": proposal_id}),
        )
        .unwrap();
    assert_eq!(approved["status"], "approved");
    let listed = runtime
        .service
        .call(&human, "list_proposals", json!({"status": "approved"}))
        .unwrap();
    let proposal = listed["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == proposal_id)
        .unwrap();
    // 履歴は書き換えない: 提案者は旧名のまま、承認者は新名
    assert_eq!(proposal["proposed_by"], "claude-code");
    assert_eq!(proposal["decided_by"], "desktop:renamed");
    // human を選べない設定にすると、以降の呼び出しは起動時の名前で続行せず失敗する
    Config::update(&dir.path().join("config.toml"), |config| {
        config.cli.default_client = None;
        config.add_client(client("desktop:other", Role::Human))
    })
    .unwrap();
    assert!(runtime.human().is_err());
    assert!(state.server_status().await.client.is_none());
    assert!(state.run_settings(|_| Ok(())).await.is_err());
}

#[tokio::test]
async fn failed_validation_can_be_retried_without_replacing_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = bootstrap_at(dir.path());
    assert!(state.initialize(" ", "user").await.is_err());
    assert!(!state.initialized().unwrap());
    assert!(state.initialize("scope", "user").await.is_ok());
    assert!(state.initialized().unwrap());
}

#[tokio::test]
async fn bootstrap_error_does_not_allow_setup_to_replace_existing_config() {
    let dir = tempfile::tempdir().unwrap();
    let original = "not valid = [";
    fs::write(dir.path().join("config.toml"), original).unwrap();
    let state = bootstrap_at(dir.path());
    assert!(state.initialize("scope", "user").await.is_err());
    assert!(state.server_status().await.error.is_some());
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        original
    );
}

#[tokio::test]
async fn http_start_stop_and_rotation_use_the_live_config() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config_with_human();
    config.add_client(client("bot", Role::Agent)).unwrap();
    config.server.port = Some(0);
    let (key, hash) = generate_key("bot");
    config.keys.insert("bot".into(), hash);
    save_config(dir.path(), &config);
    let state = bootstrap_at(dir.path());
    assert!(state.server_status().await.url.is_none());
    let (first, second) = tokio::join!(state.start_http(), state.start_http());
    first.unwrap();
    second.unwrap();
    let status = state.server_status().await;
    assert!(status.error.is_none());
    assert_eq!(status.client.as_deref(), Some("person"));
    assert_eq!(status.default_scope.as_deref(), Some("scope"));
    let url = status.url.unwrap();
    assert!(url.starts_with("http://127.0.0.1:"));
    assert!(!url.contains(":0/"));
    assert!(http_request(&url, &key).await.starts_with("HTTP/1.1 200"));
    let (new_key, new_hash) = generate_key("bot");
    config.keys.insert("bot".into(), new_hash);
    save_config(dir.path(), &config);
    assert!(http_request(&url, &key).await.starts_with("HTTP/1.1 401"));
    assert!(
        http_request(&url, &new_key)
            .await
            .starts_with("HTTP/1.1 200")
    );
    tokio::time::timeout(Duration::from_secs(3), state.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert!(state.server_status().await.url.is_none());
    assert!(state.server_status().await.error.is_none());
    let address = url.trim_start_matches("http://").trim_end_matches("/mcp");
    let reclaimed = TcpListener::bind(address).await.unwrap();
    drop(reclaimed);
    state.shutdown().await.unwrap();
    assert!(state.start_http().await.is_err());
}

#[tokio::test]
async fn missing_keys_and_bind_failure_keep_the_tool_service_available() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config_with_human();
    save_config(dir.path(), &config);
    let state = bootstrap_at(dir.path());
    assert!(state.start_http().await.is_err());
    let status = state.server_status().await;
    assert!(status.url.is_none());
    assert!(status.error.is_some());
    let runtime = state.runtime().unwrap();
    assert!(
        runtime
            .service
            .call(&runtime.human().unwrap(), "get_server_info", json!({}))
            .is_ok()
    );

    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    config.server.port = Some(occupied.local_addr().unwrap().port());
    config.add_client(client("bot", Role::Agent)).unwrap();
    config.keys.insert("bot".into(), generate_key("bot").1);
    save_config(dir.path(), &config);
    assert!(state.start_http().await.is_err());
    assert!(state.server_status().await.error.is_some());
    assert!(state.initialized().unwrap());
    drop(occupied);
    state.start_http().await.unwrap();
    assert!(state.server_status().await.error.is_none());
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_before_initialization_is_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let state = bootstrap_at(dir.path());
    state.shutdown().await.unwrap();
    assert!(state.initialize("scope", "user").await.is_err());
    assert!(state.start_http().await.is_err());
    assert!(!state.initialized().unwrap());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn shutdown_after_bootstrap_failure_preserves_the_startup_error_without_shutdown_error() {
    let state = bootstrap_with(&|_| None);
    let startup_error = state.initialized().unwrap_err();
    state.shutdown().await.unwrap();
    state.shutdown().await.unwrap();
    assert_eq!(state.initialized().unwrap_err(), startup_error);
    assert_eq!(state.server_status().await.error, Some(startup_error));
    assert!(state.initialize("scope", "user").await.is_err());
    assert!(state.start_http().await.is_err());
}

#[tokio::test]
async fn shutdown_waits_for_initialization_and_blocks_later_http_start() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(bootstrap_at(dir.path()));
    let (entered, entered_receiver) = tokio::sync::oneshot::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let initializing = {
        let state = state.clone();
        tokio::spawn(async move {
            state
                .initialize_with(move |paths| {
                    entered.send(()).unwrap();
                    resumed.recv().unwrap();
                    first_run::setup(&paths.config_path, &paths.db_path, "scope", "user")
                })
                .await
        })
    };
    entered_receiver.await.unwrap();
    let stopping = {
        let state = state.clone();
        tokio::spawn(async move { state.shutdown().await })
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        while !state.closing.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!state.initialized().unwrap());
    assert!(!stopping.is_finished());
    assert!(state.initialize("scope", "another").await.is_err());
    assert!(state.start_http().await.is_err());
    resume.send(()).unwrap();
    initializing.await.unwrap().unwrap();
    stopping.await.unwrap().unwrap();
    assert!(state.initialized().unwrap());
    assert!(state.server_status().await.url.is_none());
    assert!(state.start_http().await.is_err());
}

#[tokio::test]
async fn settings_mutations_reload_and_serialize_config_writes() {
    let dir = tempfile::tempdir().unwrap();
    save_config(dir.path(), &config_with_human());
    let state = Arc::new(bootstrap_at(dir.path()));
    let mut tasks = Vec::new();
    for index in 0..8 {
        let state = state.clone();
        tasks.push(tokio::spawn(async move {
            state
                .run_settings(move |runtime| {
                    Config::update(&runtime.config_path, |config| {
                        config.add_client(client(&format!("bot-{index}"), Role::Agent))
                    })
                    .map_err(|e| e.to_string())
                })
                .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert_eq!(
        Config::load(&dir.path().join("config.toml"))
            .unwrap()
            .clients
            .len(),
        9
    );
    state.shutdown().await.unwrap();
    assert!(state.run_settings(|_| Ok(())).await.is_err());
}

#[tokio::test]
async fn cancelled_settings_waiter_does_not_release_the_write_before_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    save_config(dir.path(), &config_with_human());
    let state = Arc::new(bootstrap_at(dir.path()));
    let (entered, entered_receiver) = tokio::sync::oneshot::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let operation = {
        let state = state.clone();
        tokio::spawn(async move {
            state
                .run_settings(move |runtime| {
                    entered.send(()).unwrap();
                    resumed.recv().unwrap();
                    Config::update(&runtime.config_path, |config| {
                        config.server.port = Some(4321);
                        Ok(())
                    })
                    .map_err(|e| e.to_string())
                })
                .await
        })
    };
    entered_receiver.await.unwrap();
    operation.abort();
    assert!(matches!(operation.await, Err(error) if error.is_cancelled()));
    let stopping = {
        let state = state.clone();
        tokio::spawn(async move { state.shutdown().await })
    };
    tokio::task::yield_now().await;
    assert!(!stopping.is_finished());
    resume.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), stopping)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        Config::load(&dir.path().join("config.toml"))
            .unwrap()
            .server
            .port,
        Some(4321)
    );
}

#[tokio::test]
async fn cancelled_setup_waiter_still_publishes_runtime_before_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(bootstrap_at(dir.path()));
    let (entered, entered_receiver) = tokio::sync::oneshot::channel();
    let (resume, resumed) = std::sync::mpsc::channel();
    let operation = {
        let state = state.clone();
        tokio::spawn(async move {
            state
                .initialize_with(move |paths| {
                    entered.send(()).unwrap();
                    resumed.recv().unwrap();
                    first_run::setup(&paths.config_path, &paths.db_path, "scope", "user")
                })
                .await
        })
    };
    entered_receiver.await.unwrap();
    operation.abort();
    assert!(matches!(operation.await, Err(error) if error.is_cancelled()));
    let stopping = {
        let state = state.clone();
        tokio::spawn(async move { state.shutdown().await })
    };
    tokio::task::yield_now().await;
    assert!(!stopping.is_finished());
    resume.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), stopping)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(state.initialized().unwrap());
    assert_eq!(
        state.runtime().unwrap().human().unwrap().name,
        "desktop:user"
    );
}

#[test]
fn runtime_remembers_the_opened_database_even_when_config_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = config_with_human();
    config.db_path = Some(dir.path().join("different.db"));
    save_config(dir.path(), &config);
    let state = bootstrap_at(dir.path());
    let runtime = state.runtime().unwrap();
    assert_eq!(runtime.db_path, dir.path().join("gaia.db"));
    assert!(!dir.path().join("different.db").exists());
}

async fn http_request(url: &str, key: &str) -> String {
    tokio::time::timeout(Duration::from_secs(3), async {
        let address = url.trim_start_matches("http://").trim_end_matches("/mcp");
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26", "capabilities": {},
                "clientInfo": {"name": "desktop-test", "version": "1"}
            }
        })
        .to_string();
        let request = format!(
            "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {key}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        String::from_utf8(response).unwrap()
    })
    .await
    .unwrap()
}

#[test]
fn protected_paths_include_config_db_and_the_key_fallback_directory() {
    let config_path = Path::new("/cfg/gaia-library/config.toml");
    let db_path = Path::new("/data/gaia-library/gaia.db");
    let protected = protected_paths(config_path, db_path, &|name| match name {
        "XDG_DATA_HOME" => Some("/xdg".into()),
        "HOME" => Some("/h".into()),
        _ => None,
    });
    assert_eq!(protected.config_dir, Path::new("/cfg/gaia-library"));
    assert_eq!(protected.db_dir, Path::new("/data/gaia-library"));
    // keychain::fallback_root と同じ場所（gaia-core の config::key_store_dir_with で算出）
    assert_eq!(
        protected.extra,
        vec![PathBuf::from("/xdg/gaia-library/keys")]
    );
    let protected = protected_paths(config_path, db_path, &|name| match name {
        "HOME" => Some("/h".into()),
        _ => None,
    });
    assert_eq!(
        protected.extra,
        vec![PathBuf::from("/h/.local/share/gaia-library/keys")]
    );
    // 退避先を決められない環境では設定・DB のディレクトリだけを拒否する
    let protected = protected_paths(config_path, db_path, &|_| None);
    assert!(protected.extra.is_empty());
}

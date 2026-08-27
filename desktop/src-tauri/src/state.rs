//! 常に一つだけ manage する状態コンテナ。未設定・起動失敗も UI から判別できる。
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use gaia_core::{
    auth::AuthTable,
    config::{self, Config},
    contracts::Catalog,
    identity::{ClientIdentity, Role},
    storage::Db,
    tools::ToolService,
};
use gaia_mcp::BoundServer;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::first_run;

#[derive(Serialize)]
pub struct SetupResponse {
    pub agent_key: String,
}

#[derive(Debug, Serialize)]
pub struct ServerStatus {
    pub url: Option<String>,
    pub error: Option<String>,
    pub client: Option<String>,
    pub default_scope: Option<String>,
}

#[derive(Clone)]
struct SetupPaths {
    config_path: PathBuf,
    db_path: PathBuf,
}

enum Initialization {
    Waiting(SetupPaths),
    Ready(Arc<AppState>),
    Failed(String),
}

pub struct DesktopState {
    initialization: RwLock<Initialization>,
    setup_lock: Mutex<()>,
    closing: AtomicBool,
}

#[derive(Default)]
struct HttpState {
    server: Option<BoundServer>,
    error: Option<String>,
}

pub struct AppState {
    pub service: Arc<ToolService>,
    pub human: ClientIdentity,
    pub config_path: PathBuf,
    http: Mutex<HttpState>,
    // 所有用の http guard は shutdown().await をまたがず、開始・終了だけを直列化する。
    http_lifecycle: Mutex<()>,
}

impl AppState {
    pub(crate) fn new(
        service: Arc<ToolService>,
        human: ClientIdentity,
        config_path: PathBuf,
    ) -> Self {
        Self {
            service,
            human,
            config_path,
            http: Mutex::default(),
            http_lifecycle: Mutex::default(),
        }
    }
}

pub fn bootstrap() -> DesktopState {
    bootstrap_with(&|name| std::env::var_os(name))
}

fn bootstrap_with(lookup: &dyn Fn(&str) -> Option<OsString>) -> DesktopState {
    let initialization = load_initialization(lookup).unwrap_or_else(Initialization::Failed);
    DesktopState {
        initialization: RwLock::new(initialization),
        setup_lock: Mutex::default(),
        closing: AtomicBool::new(false),
    }
}

fn load_initialization(
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Initialization, String> {
    let config_path =
        std::path::absolute(config::config_path_with(lookup).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let existing = first_run::config_exists(&config_path)?;
    let config = if existing {
        Config::load(&config_path).map_err(|e| e.to_string())?
    } else {
        Config::default()
    };
    let db_path =
        std::path::absolute(config::db_path_with(&config, lookup).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if !existing {
        return Ok(Initialization::Waiting(SetupPaths {
            config_path,
            db_path,
        }));
    }
    let human = select_human(&config)?;
    let catalog = Catalog::embedded().map_err(|e| e.to_string())?;
    let db = Db::open(&db_path).map_err(|e| e.to_string())?;
    let app_state = AppState::new(Arc::new(ToolService::new(db, catalog)), human, config_path);
    Ok(Initialization::Ready(Arc::new(app_state)))
}

fn select_human(config: &Config) -> Result<ClientIdentity, String> {
    if let Some(default) = config
        .cli
        .default_client
        .as_deref()
        .and_then(|name| config.client(name))
        .filter(|client| client.role == Role::Human)
    {
        return Ok(default.clone());
    }
    let mut humans = config
        .clients
        .iter()
        .filter(|client| client.role == Role::Human);
    match (humans.next(), humans.next()) {
        (Some(human), None) => Ok(human.clone()),
        (None, _) => Err("human クライアントがありません。設定を確認してください".into()),
        _ => Err(
            "human クライアントが複数あります。[cli].default_client に human を指定してください"
                .into(),
        ),
    }
}

impl DesktopState {
    pub fn initialized(&self) -> Result<bool, String> {
        let initialization = self
            .initialization
            .read()
            .map_err(|_| "初期化状態を読み取れません".to_string())?;
        match &*initialization {
            Initialization::Waiting(_) => Ok(false),
            Initialization::Ready(_) => Ok(true),
            Initialization::Failed(error) => Err(error.clone()),
        }
    }

    pub fn runtime(&self) -> Result<Arc<AppState>, String> {
        let initialization = self
            .initialization
            .read()
            .map_err(|_| "初期化状態を読み取れません".to_string())?;
        match &*initialization {
            Initialization::Ready(runtime) => Ok(runtime.clone()),
            Initialization::Waiting(_) => Err("初回セットアップが完了していません".into()),
            Initialization::Failed(error) => Err(error.clone()),
        }
    }

    pub async fn initialize(&self, affiliation: &str, user: &str) -> Result<SetupResponse, String> {
        let affiliation = affiliation.to_owned();
        let user = user.to_owned();
        self.initialize_with(move |paths| {
            first_run::setup(&paths.config_path, &paths.db_path, &affiliation, &user)
        })
        .await
    }

    async fn initialize_with(
        &self,
        initialize: impl FnOnce(SetupPaths) -> Result<(AppState, SetupResponse), String>
        + Send
        + 'static,
    ) -> Result<SetupResponse, String> {
        self.ensure_open()?;
        let _setup = self.setup_lock.lock().await;
        self.ensure_open()?;
        let paths = {
            let initialization = self
                .initialization
                .read()
                .map_err(|_| "初期化状態を読み取れません".to_string())?;
            match &*initialization {
                Initialization::Waiting(paths) => paths.clone(),
                Initialization::Ready(_) => return Err("初回セットアップは完了しています".into()),
                Initialization::Failed(error) => return Err(error.clone()),
            }
        };
        let (runtime, response) = tokio::task::spawn_blocking(move || initialize(paths))
            .await
            .map_err(|e| format!("初回セットアップの処理に失敗しました: {e}"))??;
        *self
            .initialization
            .write()
            .map_err(|_| "初期化状態を保存できません".to_string())? =
            Initialization::Ready(Arc::new(runtime));
        Ok(response)
    }

    pub async fn start_http(&self) -> Result<(), String> {
        self.ensure_open()?;
        let runtime = self.runtime()?;
        let _lifecycle = runtime.http_lifecycle.lock().await;
        self.ensure_open()?;
        if runtime.http.lock().await.server.is_some() {
            return Ok(());
        }
        let result = async {
            let config = Config::load(&runtime.config_path).map_err(|e| e.to_string())?;
            let auth = AuthTable::from_path(&runtime.config_path).map_err(|e| e.to_string())?;
            gaia_mcp::serve_http(runtime.service.clone(), Arc::new(auth), config.server.port)
                .await
                .map_err(|e| e.to_string())
        }
        .await;
        let mut http = runtime.http.lock().await;
        match result {
            Ok(server) => {
                http.server = Some(server);
                http.error = None;
                Ok(())
            }
            Err(error) => {
                http.error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub async fn server_status(&self) -> ServerStatus {
        match self.runtime() {
            Ok(runtime) => {
                let http = runtime.http.lock().await;
                ServerStatus {
                    url: http.server.as_ref().map(BoundServer::url),
                    error: http.error.clone(),
                    client: Some(runtime.human.name.clone()),
                    default_scope: runtime.human.default_scope.clone(),
                }
            }
            Err(error) => ServerStatus {
                url: None,
                error: if matches!(self.initialized(), Ok(false)) {
                    None
                } else {
                    Some(error)
                },
                client: None,
                default_scope: None,
            },
        }
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.closing.store(true, Ordering::SeqCst);
        let _setup = self.setup_lock.lock().await;
        if !self.initialized()? {
            return Ok(());
        }
        let runtime = self.runtime()?;
        let _lifecycle = runtime.http_lifecycle.lock().await;
        let server = {
            let mut http = runtime.http.lock().await;
            http.error = None;
            http.server.take()
        };
        if let Some(server) = server
            && let Err(error) = server.shutdown().await
        {
            let error = error.to_string();
            runtime.http.lock().await.error = Some(error.clone());
            return Err(error);
        }
        Ok(())
    }

    fn ensure_open(&self) -> Result<(), String> {
        if self.closing.load(Ordering::SeqCst) {
            Err("アプリを終了しています".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;

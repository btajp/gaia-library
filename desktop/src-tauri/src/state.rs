//! 常に一つだけ manage する状態コンテナ。未設定・起動失敗も UI から判別できる。
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
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
    sources::{ProtectedPaths, SourceRegistry},
    storage::Db,
    tools::ToolService,
};
use gaia_mcp::BoundServer;
use serde::Serialize;
use tokio::sync::Mutex;

use crate::{client_settings, first_run, keychain};

/// resolve_source の解決器。設定は呼び出しごとに読み直し、設定・DB・キー退避ディレクトリは file 解決器が常時拒否する。
pub(crate) fn sources_for(config_path: &Path, db_path: &Path) -> SourceRegistry {
    let mut protected = ProtectedPaths::new(
        config_path.parent().unwrap_or(Path::new("/")),
        db_path.parent().unwrap_or(Path::new("/")),
    );
    if let Some(keys) = keychain::fallback_root_for_current_env() {
        protected = protected.with_extra(keys);
    }
    gaia_mcp::sources::registry(config_path, protected)
}

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
    initialization: Arc<RwLock<Initialization>>,
    setup_lock: Arc<Mutex<()>>,
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
    pub db_path: PathBuf,
    http: Mutex<HttpState>,
    // 所有用の http guard は shutdown().await をまたがず、開始・終了だけを直列化する。
    http_lifecycle: Mutex<()>,
}

impl AppState {
    pub(crate) fn new(
        service: Arc<ToolService>,
        human: ClientIdentity,
        config_path: PathBuf,
        db_path: PathBuf,
    ) -> Self {
        Self {
            service,
            human,
            config_path,
            db_path,
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
        initialization: Arc::new(RwLock::new(initialization)),
        setup_lock: Arc::default(),
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
    let sources = sources_for(&config_path, &db_path);
    let app_state = AppState::new(
        Arc::new(ToolService::new(db, catalog).with_sources(sources)),
        human,
        config_path,
        db_path,
    );
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

    #[cfg(test)]
    pub async fn initialize(&self, affiliation: &str, user: &str) -> Result<SetupResponse, String> {
        let affiliation = affiliation.to_owned();
        let user = user.to_owned();
        self.initialize_with(move |paths| {
            first_run::setup(&paths.config_path, &paths.db_path, &affiliation, &user)
        })
        .await
    }

    pub(crate) async fn initialize_and_store(
        &self,
        affiliation: &str,
        user: &str,
    ) -> Result<(SetupResponse, client_settings::KeyStorage), String> {
        let affiliation = affiliation.to_owned();
        let user = user.to_owned();
        self.initialize_with(move |paths| {
            let (runtime, response) =
                first_run::setup(&paths.config_path, &paths.db_path, &affiliation, &user)?;
            let storage = client_settings::store_key(first_run::AGENT_CLIENT, &response.agent_key);
            Ok((runtime, (response, storage)))
        })
        .await
    }

    async fn initialize_with<R: Send + 'static>(
        &self,
        initialize: impl FnOnce(SetupPaths) -> Result<(AppState, R), String> + Send + 'static,
    ) -> Result<R, String> {
        self.ensure_open()?;
        let guard = self.setup_lock.clone().lock_owned().await;
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
        let initialization = self.initialization.clone();
        tokio::task::spawn_blocking(move || {
            // 待機側が取り消されても、設定の公開と状態への反映は最後まで一組で行う。
            let _guard = guard;
            let (runtime, response) = initialize(paths)?;
            *initialization
                .write()
                .map_err(|_| "初期化状態を保存できません".to_string())? =
                Initialization::Ready(Arc::new(runtime));
            Ok(response)
        })
        .await
        .map_err(|_| "初回セットアップの処理に失敗しました".to_string())?
    }

    pub(crate) async fn run_settings<R: Send + 'static>(
        &self,
        operation: impl FnOnce(Arc<AppState>) -> Result<R, String> + Send + 'static,
    ) -> Result<R, String> {
        self.ensure_open()?;
        let guard = self.setup_lock.clone().lock_owned().await;
        self.ensure_open()?;
        let runtime = self.runtime()?;
        if runtime.human.role != Role::Human {
            return Err("設定操作には human クライアントが必要です".into());
        }
        // IPC の待機が取り消されても、書込完了までは終了処理を待たせる。
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            operation(runtime)
        })
        .await
        .map_err(|_| "設定操作の処理に失敗しました".to_string())?
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
        let runtime = {
            let initialization = self
                .initialization
                .read()
                .map_err(|_| "初期化状態を読み取れません".to_string())?;
            match &*initialization {
                Initialization::Ready(runtime) => runtime.clone(),
                // 初期化前・起動失敗では、停止する HTTP サーバー自体がない。
                Initialization::Waiting(_) | Initialization::Failed(_) => return Ok(()),
            }
        };
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

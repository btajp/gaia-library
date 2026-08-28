//! 半自動アップデート: 起動時チェック → ネイティブダイアログ → 1クリック更新 → 再起動。
//! 更新ダイアログとメニューは Rust 側で扱う。設定画面からの手動チェックも
//! 同じ単一実行ガードを通す。

/// 手動DL先。適用失敗・チェック失敗時の情報的な案内に使う。
pub(crate) const RELEASES_URL: &str = "https://github.com/btajp/gaia-library/releases";

/// ダイアログ文言の言語。クライアント側i18n（webview内）とは独立した、ネイティブUI専用の選択。
#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum Lang {
    Ja,
    En,
}

/// システムロケール文字列からダイアログ言語を選ぶ。`ja*` のみ日本語、他は英語。
pub(crate) fn pick_lang(locale: Option<&str>) -> Lang {
    match locale {
        Some(l) if l.starts_with("ja") => Lang::Ja,
        _ => Lang::En,
    }
}

/// 確認ダイアログ（2ボタン）用の文言一式。
pub(crate) struct PromptText {
    pub title: String,
    pub body: String,
    pub ok: String,
    pub cancel: String,
}

/// 情報ダイアログ（1ボタン）用の文言。
pub(crate) struct InfoText {
    pub title: String,
    pub body: String,
}

/// 新版検知時の確認ダイアログ。通常起動では明示的な同意後に更新する。
pub(crate) fn update_prompt_text(lang: Lang, current: &str, latest: &str) -> PromptText {
    match lang {
        Lang::Ja => PromptText {
            title: "アップデート".to_string(),
            body: format!(
                "gaia-library v{latest} が利用可能です（現在 v{current}）。\n\
                 今すぐダウンロードして更新しますか？\n\
                 ダウンロード後、再起動する前にもう一度確認します。"
            ),
            ok: "更新する".to_string(),
            cancel: "今回はしない".to_string(),
        },
        Lang::En => PromptText {
            title: "Update Available".to_string(),
            body: format!(
                "gaia-library v{latest} is available (you have v{current}).\n\
                 Download and install now?\n\
                 You'll be asked once more before the app restarts."
            ),
            ok: "Update".to_string(),
            cancel: "Not Now".to_string(),
        },
    }
}

/// 更新の適用が済み、再起動するかを利用者に選んでもらう確認ダイアログ。
pub(crate) fn restart_ready_text(lang: Lang) -> PromptText {
    match lang {
        Lang::Ja => PromptText {
            title: "アップデート".to_string(),
            body: "アップデートの準備が完了しました。\n再起動すると最新版を使えます。".to_string(),
            ok: "今すぐ再起動".to_string(),
            cancel: "あとで再起動".to_string(),
        },
        Lang::En => PromptText {
            title: "Update Ready".to_string(),
            body: "The update is ready.\nRestart the app to use the latest version.".to_string(),
            ok: "Restart Now".to_string(),
            cancel: "Restart Later".to_string(),
        },
    }
}

/// 更新フローが既に動いているときの手動チェックへの情報表示。
pub(crate) fn update_in_progress_text(lang: Lang) -> InfoText {
    match lang {
        Lang::Ja => InfoText {
            title: "アップデート".to_string(),
            body: "アップデートの確認またはダウンロードが進行中です。\n\
                   現在の状態はアプリメニューに表示しています。"
                .to_string(),
        },
        Lang::En => InfoText {
            title: "Update in Progress".to_string(),
            body: "An update check or download is already in progress.\n\
                   Its current status is shown in the app menu."
                .to_string(),
        },
    }
}

/// 手動チェックで「最新だった」場合の情報表示。
pub(crate) fn manual_latest_text(lang: Lang, current: &str) -> InfoText {
    match lang {
        Lang::Ja => InfoText {
            title: "アップデート".to_string(),
            body: format!("お使いのバージョン（v{current}）が最新です。"),
        },
        Lang::En => InfoText {
            title: "Up to Date".to_string(),
            body: format!("You're on the latest version (v{current})."),
        },
    }
}

/// 更新の適用（DL・差し替え）に失敗した場合の情報表示。
/// App Translocation（/Applications 未移動）が典型原因のため、移動のヒントも添える。
pub(crate) fn install_failed_text(lang: Lang) -> InfoText {
    match lang {
        Lang::Ja => InfoText {
            title: "アップデート".to_string(),
            body: format!(
                "自動更新を完了できませんでした。\n最新版は以下から手動でダウンロードできます:\n{RELEASES_URL}\n\
                 （アプリを /Applications に移動してから起動すると自動更新できるようになります）"
            ),
        },
        Lang::En => InfoText {
            title: "Update".to_string(),
            body: format!(
                "The update could not be installed automatically.\nYou can download the latest version manually:\n{RELEASES_URL}\n\
                 (Moving the app into /Applications enables automatic updates.)"
            ),
        },
    }
}

/// 手動チェックで確認自体ができなかった（オフライン等）場合の情報表示。
/// 「更新に失敗した」と誤解させないよう、適用失敗（`install_failed_text`）とは別文言にする。
pub(crate) fn manual_check_failed_text(lang: Lang) -> InfoText {
    match lang {
        Lang::Ja => InfoText {
            title: "アップデート".to_string(),
            body: format!(
                "アップデートを確認できませんでした（ネットワーク接続をご確認ください）。\n\
                 最新版の有無は以下でも確認できます:\n{RELEASES_URL}"
            ),
        },
        Lang::En => InfoText {
            title: "Update".to_string(),
            body: format!(
                "Could not check for updates (please check your network connection).\n\
                 You can also check the latest release here:\n{RELEASES_URL}"
            ),
        },
    }
}

/// メニュー項目「アップデートを確認…」のラベル。
pub(crate) fn check_menu_label(lang: Lang) -> &'static str {
    match lang {
        Lang::Ja => "アップデートを確認…",
        Lang::En => "Check for Updates…",
    }
}

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager, menu::MenuItem};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

/// 更新確認は起動を妨げず、ネットワークが応答しない場合も単一実行ガードを解放する。
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// 受信が止まった更新ダウンロードを中断するまでの時間。
const UPDATE_DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(90);
/// 低速だが継続中の回線を許容しつつ、更新通信全体を必ず打ち切る上限。
const UPDATE_DOWNLOAD_MAX_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// アプリメニューに表示する更新フローの状態。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum UpdateMenuStatus {
    Idle,
    Checking,
    Downloading { percent: Option<u8> },
    Installing,
    RestartPrompt,
    RestartReady,
}

impl UpdateMenuStatus {
    fn is_enabled(self) -> bool {
        matches!(self, Self::Idle | Self::RestartReady)
    }
}

fn update_menu_text(lang: Lang, status: UpdateMenuStatus) -> String {
    match (lang, status) {
        (Lang::Ja, UpdateMenuStatus::Idle) => check_menu_label(Lang::Ja).to_string(),
        (Lang::En, UpdateMenuStatus::Idle) => check_menu_label(Lang::En).to_string(),
        (Lang::Ja, UpdateMenuStatus::Checking) => "アップデートを確認中…".to_string(),
        (Lang::En, UpdateMenuStatus::Checking) => "Checking for Updates…".to_string(),
        (
            Lang::Ja,
            UpdateMenuStatus::Downloading {
                percent: Some(percent),
            },
        ) => {
            format!("アップデートをダウンロード中… {percent}%")
        }
        (
            Lang::En,
            UpdateMenuStatus::Downloading {
                percent: Some(percent),
            },
        ) => {
            format!("Downloading Update… {percent}%")
        }
        (Lang::Ja, UpdateMenuStatus::Downloading { percent: None }) => {
            "アップデートをダウンロード中…".to_string()
        }
        (Lang::En, UpdateMenuStatus::Downloading { percent: None }) => {
            "Downloading Update…".to_string()
        }
        (Lang::Ja, UpdateMenuStatus::Installing) => "アップデートを適用中…".to_string(),
        (Lang::En, UpdateMenuStatus::Installing) => "Applying Update…".to_string(),
        (Lang::Ja, UpdateMenuStatus::RestartPrompt) => "アップデートの再起動確認中…".to_string(),
        (Lang::En, UpdateMenuStatus::RestartPrompt) => {
            "Update Ready — Confirm Restart…".to_string()
        }
        (Lang::Ja, UpdateMenuStatus::RestartReady) => "アップデートの準備完了 — 再起動".to_string(),
        (Lang::En, UpdateMenuStatus::RestartReady) => "Update Ready — Restart".to_string(),
    }
}

/// 更新用メニュー項目と状態をアプリ全体で保持する。
pub(crate) struct UpdateMenuState {
    item: MenuItem<tauri::Wry>,
    status: Mutex<UpdateMenuStatus>,
}

impl UpdateMenuState {
    pub(crate) fn new(item: MenuItem<tauri::Wry>) -> Self {
        Self {
            item,
            status: Mutex::new(UpdateMenuStatus::Idle),
        }
    }

    fn status(&self) -> UpdateMenuStatus {
        match self.status.lock() {
            Ok(status) => *status,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    fn set_status(&self, next: UpdateMenuStatus) {
        match self.status.lock() {
            Ok(mut status) => *status = next,
            Err(poisoned) => *poisoned.into_inner() = next,
        }
    }
}

fn current_menu_status(app: &AppHandle) -> UpdateMenuStatus {
    app.try_state::<UpdateMenuState>()
        .map(|state| state.status())
        .unwrap_or(UpdateMenuStatus::Idle)
}

fn set_menu_status(app: &AppHandle, next: UpdateMenuStatus) {
    let Some(state) = app.try_state::<UpdateMenuState>() else {
        return;
    };
    state.set_status(next);
    if let Err(error) = state.item.set_text(update_menu_text(current_lang(), next)) {
        log::warn!("updater: failed to update menu text: {error}");
    }
    if let Err(error) = state.item.set_enabled(next.is_enabled()) {
        log::warn!("updater: failed to update menu enabled state: {error}");
    }
}

/// 更新フローの単一実行ガード。起動時チェックとメニューの手動チェックが同時に走って
/// download/install が二重実行されるのを防ぐ。Drop時にメニューを戻し、ロックも必ず解放する。
static UPDATE_FLOW_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct FlowGuard {
    app: AppHandle,
    restore_idle_menu: bool,
}

impl FlowGuard {
    fn retain_menu_status(&mut self, status: UpdateMenuStatus) {
        set_menu_status(&self.app, status);
        self.restore_idle_menu = false;
    }
}

impl Drop for FlowGuard {
    fn drop(&mut self) {
        // 先に表示を戻してからロックを解放する。逆順だと次のフローが状態を更新した直後に
        // このDropがIdleへ戻す競合が起こり得る。
        if self.restore_idle_menu {
            set_menu_status(&self.app, UpdateMenuStatus::Idle);
        }
        UPDATE_FLOW_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
}

fn claim_flow() -> bool {
    UPDATE_FLOW_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

fn try_begin_flow(app: &AppHandle, initial_status: UpdateMenuStatus) -> Option<FlowGuard> {
    if !claim_flow() {
        return None;
    }
    set_menu_status(app, initial_status);
    Some(FlowGuard {
        app: app.clone(),
        restore_idle_menu: true,
    })
}

/// 現在のシステムロケールからダイアログ言語を決める。
pub(crate) fn current_lang() -> Lang {
    pick_lang(sys_locale::get_locale().as_deref())
}

/// 起動時の自動チェック。非ブロッキング（起動・内蔵 HTTP サーバー起動を一切待たせない）。
/// 失敗（オフライン・GitHub不達等）はログのみで無言スキップする（起動時にダイアログを出さない）。
pub fn spawn_startup_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = check_and_prompt(app, false).await {
            log::warn!("updater: startup check failed (skipped quietly): {e}");
        }
    });
}

/// メニュー「アップデートを確認…」からの手動チェック。
/// 起動時と違いユーザーが結果を待っているため、「最新」「確認失敗」も必ずダイアログで返す。
pub fn spawn_manual_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let lang = current_lang();
        if current_menu_status(&app) == UpdateMenuStatus::RestartReady {
            let Some(mut flow_guard) = try_begin_flow(&app, UpdateMenuStatus::RestartPrompt) else {
                show_info(&app, update_in_progress_text(lang));
                return;
            };
            if prompt_restart_after_install(&app, lang).await {
                app.restart();
            } else {
                flow_guard.retain_menu_status(UpdateMenuStatus::RestartReady);
            }
            return;
        }
        if let Err(e) = check_and_prompt(app.clone(), true).await {
            log::warn!("updater: manual check failed: {e}");
            show_info(&app, manual_check_failed_text(lang));
        }
    });
}

/// E2E検証専用の自動承認判定（純関数）。値が正確に "1" のときだけ有効（誤爆防止）。
pub(crate) fn should_auto_confirm(env_value: Option<&str>) -> bool {
    env_value == Some("1")
}

/// ネイティブダイアログはスクリプトから操作できないため、実機E2E（desktop/e2e-updater/）では
/// `GAIA_UPDATER_AUTO=1` で更新・再起動の確認ダイアログをスキップする。
/// 配布ユーザーが通常起動で踏むことはない（環境変数を明示設定した場合のみ）。
fn auto_confirm_forced() -> bool {
    should_auto_confirm(std::env::var("GAIA_UPDATER_AUTO").ok().as_deref())
}

fn show_info(app: &AppHandle, text: InfoText) {
    app.dialog()
        .message(&text.body)
        .title(&text.title)
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

async fn prompt_restart_after_install(app: &AppHandle, lang: Lang) -> bool {
    if auto_confirm_forced() {
        log::warn!("updater: GAIA_UPDATER_AUTO=1 (E2E hook); restarting after install");
        return true;
    }
    let text = restart_ready_text(lang);
    let dialog = app
        .dialog()
        .message(&text.body)
        .title(&text.title)
        .buttons(MessageDialogButtons::OkCancelCustom(text.ok, text.cancel));
    tauri::async_runtime::spawn_blocking(move || dialog.blocking_show())
        .await
        .unwrap_or(false)
}

async fn check_and_prompt(app: AppHandle, manual: bool) -> tauri_plugin_updater::Result<()> {
    let lang = current_lang();
    let Some(mut flow_guard) = try_begin_flow(&app, UpdateMenuStatus::Checking) else {
        // 既に別の更新フロー（起動時チェックのDL中など）が進行中。二重DL・二重installを避けて
        // 更新中であることを手動起点には必ず返す。
        log::info!("updater: another update flow is already in flight");
        if manual {
            show_info(&app, update_in_progress_text(lang));
        }
        return Ok(());
    };
    let Some(mut update) = app
        .updater_builder()
        .timeout(UPDATE_CHECK_TIMEOUT)
        .build()?
        .check()
        .await?
    else {
        if manual {
            let current = app.package_info().version.to_string();
            let t = manual_latest_text(lang, &current);
            app.dialog()
                .message(&t.body)
                .title(&t.title)
                .kind(MessageDialogKind::Info)
                .show(|_| {});
        }
        return Ok(());
    };

    let confirmed = if auto_confirm_forced() {
        log::warn!("updater: GAIA_UPDATER_AUTO=1 (E2E hook); skipping confirmation dialog");
        true
    } else {
        let t = update_prompt_text(lang, &update.current_version, &update.version);
        let dialog = app
            .dialog()
            .message(&t.body)
            .title(&t.title)
            .buttons(MessageDialogButtons::OkCancelCustom(t.ok, t.cancel));
        // blocking_showはメインスレッド禁止（docs.rs明記: 内部でメインスレッドへディスパッチするため
        // メインスレッドで待つとデッドロック）。tokioのblockingプールに逃がして待つ。
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_show())
            .await
            .unwrap_or(false)
    };
    if !confirmed {
        // 「今回はしない」: 何も記録しない・次回起動時にまた1回だけ聞く（情報的・ペナルティなし）。
        log::info!("updater: user declined update to v{}", update.version);
        return Ok(());
    }

    log::info!(
        "updater: downloading and installing v{} ...",
        update.version
    );
    update.timeout = Some(UPDATE_DOWNLOAD_MAX_TIMEOUT);
    set_menu_status(&app, UpdateMenuStatus::Downloading { percent: None });
    match download_and_install_with_watchdog(&update, &app).await {
        Ok(()) => {
            log::info!(
                "updater: installed v{}; waiting for restart choice",
                update.version
            );
            set_menu_status(&app, UpdateMenuStatus::RestartPrompt);
            if prompt_restart_after_install(&app, lang).await {
                // gaia は in-process HTTP のため sidecar 回収は不要。
                app.restart();
            } else {
                log::info!("updater: restart deferred by user");
                flow_guard.retain_menu_status(UpdateMenuStatus::RestartReady);
            }
        }
        Err(e) => {
            log::error!("updater: install failed: {e}");
            show_info(&app, install_failed_text(lang));
        }
    }
    Ok(())
}

struct DownloadProgress {
    received_bytes: u64,
    last_chunk_at: Instant,
    last_announced_percent: Option<u8>,
}

impl DownloadProgress {
    fn new() -> Self {
        Self {
            received_bytes: 0,
            last_chunk_at: Instant::now(),
            last_announced_percent: None,
        }
    }
}

fn download_percent(received_bytes: u64, total_bytes: Option<u64>) -> Option<u8> {
    total_bytes.filter(|total| *total > 0).map(|total| {
        (received_bytes.saturating_mul(100) / total)
            .min(100)
            .try_into()
            .expect("percentage is capped at 100")
    })
}

fn record_download_chunk(
    progress: &Mutex<DownloadProgress>,
    chunk_size: usize,
    total_bytes: Option<u64>,
) -> Option<u8> {
    let mut progress = match progress.lock() {
        Ok(progress) => progress,
        Err(poisoned) => poisoned.into_inner(),
    };
    if chunk_size > 0 {
        progress.received_bytes = progress.received_bytes.saturating_add(chunk_size as u64);
        progress.last_chunk_at = Instant::now();
    }
    let percent = download_percent(progress.received_bytes, total_bytes);
    if percent.is_some() && progress.last_announced_percent != percent {
        progress.last_announced_percent = percent;
        percent
    } else {
        None
    }
}

fn last_download_chunk_at(progress: &Mutex<DownloadProgress>) -> Instant {
    match progress.lock() {
        Ok(progress) => progress.last_chunk_at,
        Err(poisoned) => poisoned.into_inner().last_chunk_at,
    }
}

fn is_download_stalled(last_chunk_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_chunk_at) >= UPDATE_DOWNLOAD_STALL_TIMEOUT
}

async fn download_and_install_with_watchdog(
    update: &tauri_plugin_updater::Update,
    app: &AppHandle,
) -> Result<(), String> {
    let progress = Arc::new(Mutex::new(DownloadProgress::new()));
    let progress_for_callback = Arc::clone(&progress);
    let app_for_callback = app.clone();
    let download = update.download(
        move |chunk_size, total_bytes| {
            if let Some(percent) =
                record_download_chunk(&progress_for_callback, chunk_size, total_bytes)
            {
                set_menu_status(
                    &app_for_callback,
                    UpdateMenuStatus::Downloading {
                        percent: Some(percent),
                    },
                );
            }
        },
        || {},
    );
    tokio::pin!(download);
    let mut watchdog = tokio::time::interval(Duration::from_secs(1));

    loop {
        tokio::select! {
            result = &mut download => {
                let bytes = result.map_err(|error| format!("updater download failed: {error}"))?;
                set_menu_status(app, UpdateMenuStatus::Installing);
                update
                    .install(bytes)
                    .map_err(|error| format!("updater install failed: {error}"))?;
                return Ok(());
            }
            _ = watchdog.tick() => {
                if is_download_stalled(last_download_chunk_at(&progress), Instant::now()) {
                    return Err(format!(
                        "updater download made no progress for {} seconds",
                        UPDATE_DOWNLOAD_STALL_TIMEOUT.as_secs()
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_lang_ja_for_japanese_locales() {
        assert_eq!(pick_lang(Some("ja")), Lang::Ja);
        assert_eq!(pick_lang(Some("ja-JP")), Lang::Ja);
    }

    #[test]
    fn pick_lang_en_for_others_and_missing() {
        assert_eq!(pick_lang(Some("en-US")), Lang::En);
        assert_eq!(pick_lang(Some("fr")), Lang::En);
        assert_eq!(pick_lang(None), Lang::En);
    }

    #[test]
    fn update_prompt_contains_both_versions() {
        let t = update_prompt_text(Lang::Ja, "0.29.0", "0.30.0");
        assert!(t.body.contains("0.29.0") && t.body.contains("0.30.0"));
        assert!(t.body.contains("もう一度確認"));
        let t = update_prompt_text(Lang::En, "0.29.0", "0.30.0");
        assert!(t.body.contains("0.29.0") && t.body.contains("0.30.0"));
        assert!(t.body.contains("once more"));
    }

    #[test]
    fn install_failed_text_mentions_releases_url() {
        // 適用失敗時は手動DL先（Releases）を必ず情報的に案内する（研究制約: 警告調・強要なし）
        assert!(install_failed_text(Lang::Ja).body.contains(RELEASES_URL));
        assert!(install_failed_text(Lang::En).body.contains(RELEASES_URL));
    }

    #[test]
    fn manual_latest_text_mentions_current_version() {
        assert!(
            manual_latest_text(Lang::Ja, "0.29.0")
                .body
                .contains("0.29.0")
        );
        assert!(
            manual_latest_text(Lang::En, "0.29.0")
                .body
                .contains("0.29.0")
        );
    }

    #[test]
    fn update_flow_guard_prevents_concurrent_entry() {
        // 起動時チェックと手動チェックの同時実行で download/install が二重に走らないこと。
        UPDATE_FLOW_IN_FLIGHT.store(false, Ordering::SeqCst);
        assert!(claim_flow(), "first acquire should succeed");
        assert!(!claim_flow(), "second concurrent acquire must fail");
        UPDATE_FLOW_IN_FLIGHT.store(false, Ordering::SeqCst);
        assert!(claim_flow(), "acquire after release should succeed");
        UPDATE_FLOW_IN_FLIGHT.store(false, Ordering::SeqCst);
    }

    #[test]
    fn should_auto_confirm_only_when_env_is_exactly_1() {
        // E2E専用フック: 明示的に "1" を設定したときだけ有効（誤爆防止）。
        assert!(should_auto_confirm(Some("1")));
        assert!(!should_auto_confirm(Some("0")));
        assert!(!should_auto_confirm(Some("true")));
        assert!(!should_auto_confirm(None));
    }

    #[test]
    fn manual_check_failed_text_mentions_releases_url() {
        // 手動チェックの通信失敗は「確認できなかった」事実のみ伝える（更新失敗とは別文言）
        assert!(
            manual_check_failed_text(Lang::Ja)
                .body
                .contains(RELEASES_URL)
        );
        assert!(
            manual_check_failed_text(Lang::En)
                .body
                .contains(RELEASES_URL)
        );
    }

    #[test]
    fn menu_status_shows_progress_and_disables_parallel_trigger() {
        assert_eq!(
            update_menu_text(
                Lang::Ja,
                UpdateMenuStatus::Downloading { percent: Some(42) }
            ),
            "アップデートをダウンロード中… 42%"
        );
        assert_eq!(
            update_menu_text(Lang::En, UpdateMenuStatus::RestartReady),
            "Update Ready — Restart"
        );
        assert!(!UpdateMenuStatus::Checking.is_enabled());
        assert!(!UpdateMenuStatus::Downloading { percent: None }.is_enabled());
        assert!(UpdateMenuStatus::RestartReady.is_enabled());
    }

    #[test]
    fn download_progress_is_clamped_and_reported_once_per_percent() {
        assert_eq!(download_percent(50, Some(100)), Some(50));
        assert_eq!(download_percent(200, Some(100)), Some(100));
        assert_eq!(download_percent(1, None), None);

        let progress = Mutex::new(DownloadProgress::new());
        assert_eq!(record_download_chunk(&progress, 50, Some(100)), Some(50));
        let last_received_at = last_download_chunk_at(&progress);
        assert_eq!(record_download_chunk(&progress, 0, Some(100)), None);
        assert_eq!(last_download_chunk_at(&progress), last_received_at);
        assert_eq!(record_download_chunk(&progress, 50, Some(100)), Some(100));
    }

    #[test]
    fn download_watchdog_only_times_out_when_progress_has_stopped() {
        let now = Instant::now();
        assert!(!is_download_stalled(
            now,
            now + UPDATE_DOWNLOAD_STALL_TIMEOUT - Duration::from_secs(1)
        ));
        assert!(is_download_stalled(
            now,
            now + UPDATE_DOWNLOAD_STALL_TIMEOUT
        ));
    }

    #[test]
    fn updater_network_timeouts_are_finite() {
        assert_eq!(UPDATE_CHECK_TIMEOUT, Duration::from_secs(30));
        assert_eq!(UPDATE_DOWNLOAD_STALL_TIMEOUT, Duration::from_secs(90));
        assert_eq!(UPDATE_DOWNLOAD_MAX_TIMEOUT, Duration::from_secs(20 * 60));
        assert!(UPDATE_DOWNLOAD_MAX_TIMEOUT > UPDATE_DOWNLOAD_STALL_TIMEOUT);
    }

    #[test]
    fn restart_and_in_progress_messages_are_informational() {
        let restart = restart_ready_text(Lang::Ja);
        assert!(restart.body.contains("再起動"));
        assert_eq!(restart.ok, "今すぐ再起動");
        assert_eq!(restart.cancel, "あとで再起動");
        assert!(update_in_progress_text(Lang::En).body.contains("app menu"));
    }
}

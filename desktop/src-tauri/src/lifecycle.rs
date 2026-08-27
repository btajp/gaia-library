//! 常駐ウィンドウの再表示と、HTTP を停止してからの終了をまとめる。
use std::sync::atomic::{AtomicU8, Ordering};

use tauri::{
    App, AppHandle, Manager, RunEvent,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::state::DesktopState;

#[derive(Default)]
pub(crate) struct ExitState(AtomicU8);

impl ExitState {
    fn begin(&self) -> bool {
        self.0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn ready(&self) -> bool {
        self.0.load(Ordering::Acquire) == 2
    }

    fn finish(&self) {
        self.0.store(2, Ordering::Release);
    }
}

pub(crate) fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.show() {
            log::warn!("ウィンドウを表示できません: {error}");
        }
        if let Err(error) = window.set_focus() {
            log::warn!("ウィンドウを選択できません: {error}");
        }
    }
}

pub(crate) fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show-main", "gaia-library を開く", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit-app", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("gaia-library")
        .tooltip("gaia-library")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show-main" => show_main(app),
            "quit-app" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<DesktopState>();
        if matches!(state.initialized(), Ok(true))
            && let Err(error) = state.start_http().await
        {
            log::warn!("HTTP サーバーは起動できませんでした: {error}");
        }
    });
    Ok(())
}

pub(crate) fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { api, code, .. } => {
            if code == Some(tauri::RESTART_EXIT_CODE) {
                // restart は prevent_exit を受け付けないため、イベント内で停止を完了する。
                let state = app.state::<DesktopState>();
                if let Err(error) = tauri::async_runtime::block_on(state.shutdown()) {
                    log::warn!("再起動前の HTTP 停止に失敗しました: {error}");
                }
                return;
            }
            let exit = app.state::<ExitState>();
            if exit.ready() {
                return;
            }
            api.prevent_exit();
            // 終了処理中の二度目の終了要求も、停止完了までは保留する。
            if exit.begin() {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = handle.state::<DesktopState>().shutdown().await {
                        log::warn!("終了前の HTTP 停止に失敗しました: {error}");
                    }
                    handle.state::<ExitState>().finish();
                    handle.exit(code.unwrap_or(0));
                });
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show_main(app),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::ExitState;

    #[test]
    fn repeated_exit_requests_wait_for_the_first_shutdown() {
        let exit = ExitState::default();
        assert!(!exit.ready());
        assert!(exit.begin());
        assert!(!exit.begin());
        assert!(!exit.ready());
        exit.finish();
        assert!(exit.ready());
        assert!(!exit.begin());
    }
}

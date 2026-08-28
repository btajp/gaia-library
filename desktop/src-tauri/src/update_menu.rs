//! アプリメニューの更新項目を、起動時・設定画面と同じ更新フローへ接続する。
use tauri::{
    App, AppHandle, Manager,
    menu::{Menu, MenuEvent, MenuItem},
};

use crate::updater;

const CHECK_UPDATES: &str = "check-for-updates";

pub(crate) fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let menu = Menu::default(app.handle())?;
    let submenu = menu
        .items()?
        .first()
        .and_then(|item| item.as_submenu().cloned())
        .ok_or("アプリメニューを作成できません")?;
    let item = MenuItem::with_id(
        app,
        CHECK_UPDATES,
        updater::check_menu_label(updater::current_lang()),
        true,
        None::<&str>,
    )?;
    app.manage(updater::UpdateMenuState::new(item.clone()));
    submenu.insert(&item, 1)?;
    app.set_menu(menu)?;
    Ok(())
}

pub(crate) fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    if event.id().as_ref() == CHECK_UPDATES {
        updater::spawn_manual_check(app.clone());
    }
}

//! Menu bar item. See spec §3.2.

use tauri::{
    AppHandle,
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

use crate::{platform::permissions, window};

const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let status_label = if permissions::is_accessibility_trusted() {
        "Ready"
    } else {
        "Needs Accessibility permission"
    };
    let status = MenuItem::with_id(app, "status", status_label, false, None::<&str>)?;
    let open = MenuItem::with_id(
        app,
        "open-settings",
        "Open Settings…",
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit MouseTune", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[&status, &PredefinedMenuItem::separator(app)?, &open, &quit],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(TRAY_ICON)?)
        .icon_as_template(true)
        .tooltip("MouseTune")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-settings" => window::open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

//! Menu bar item. See spec §3.2.

use tauri::{
    AppHandle, Manager, Wry,
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

use crate::{platform::permissions, window};

const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");

/// Menu items that change at runtime.
struct TrayItems {
    status: MenuItem<Wry>,
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
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
            // Only enabled while permission is missing.
            "status" | "open-settings" => window::open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayItems { status });
    set_accessibility(app, permissions::is_accessibility_trusted());
    Ok(())
}

pub fn set_accessibility(app: &AppHandle, trusted: bool) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let (text, enabled) = if trusted {
        ("MouseTune is ready", false)
    } else {
        ("Grant Accessibility permission…", true)
    };
    if let Err(err) = items
        .status
        .set_text(text)
        .and_then(|()| items.status.set_enabled(enabled))
    {
        tracing::warn!(%err, "failed to update tray status");
    }
}

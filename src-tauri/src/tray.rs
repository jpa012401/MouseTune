//! Menu bar item. See spec §3.2.

use tauri::{
    AppHandle, Manager, Wry,
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

use crate::{permission_monitor::PermissionObserver, window};

const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");

/// Menu items that change at runtime.
struct TrayItems {
    status: MenuItem<Wry>,
}

pub fn build(app: &AppHandle, accessibility_trusted: bool) -> tauri::Result<()> {
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
    set_accessibility(app, accessibility_trusted);
    Ok(())
}

/// What the tray's status line shows for a given permission state.
#[derive(Debug, PartialEq, Eq)]
struct StatusLine {
    text: &'static str,
    /// Clickable: opens Settings so the user can act on it.
    actionable: bool,
}

fn status_line(trusted: bool) -> StatusLine {
    if trusted {
        StatusLine {
            text: "MouseTune is ready",
            actionable: false,
        }
    } else {
        StatusLine {
            text: "Grant Accessibility permission…",
            actionable: true,
        }
    }
}

fn set_accessibility(app: &AppHandle, trusted: bool) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let line = status_line(trusted);
    if let Err(err) = items
        .status
        .set_text(line.text)
        .and_then(|()| items.status.set_enabled(line.actionable))
    {
        tracing::warn!(%err, "failed to update tray status");
    }
}

/// Keeps the tray status line in sync with the permission.
pub struct TrayPermissionStatus(pub AppHandle);

impl PermissionObserver for TrayPermissionStatus {
    fn permission_changed(&self, trusted: bool) {
        set_accessibility(&self.0, trusted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_permission_is_a_clickable_call_to_action() {
        assert!(status_line(false).actionable);
    }

    #[test]
    fn granted_permission_is_informational_only() {
        assert!(!status_line(true).actionable);
    }

    #[test]
    fn each_state_reads_differently() {
        assert_ne!(status_line(true).text, status_line(false).text);
    }
}

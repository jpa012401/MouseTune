//! Menu bar item. See spec §3.2.

use std::sync::{Mutex, PoisonError};

use tauri::{
    AppHandle, Manager, Wry,
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

use crate::{
    config::{
        schema::{Config, General},
        store::{ConfigObserver, ConfigStore},
    },
    permission_monitor::PermissionObserver,
    window,
};

const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");

/// Menu items that change at runtime.
struct TrayItems {
    status: MenuItem<Wry>,
    buttons: CheckMenuItem<Wry>,
    scroll: CheckMenuItem<Wry>,
}

pub fn build(app: &AppHandle, accessibility_trusted: bool, general: &General) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
    let buttons = CheckMenuItem::with_id(
        app,
        "toggle-buttons",
        "Enable buttons",
        true,
        general.buttons_enabled,
        None::<&str>,
    )?;
    let scroll = CheckMenuItem::with_id(
        app,
        "toggle-scroll",
        "Enable scrolling",
        true,
        general.scroll_enabled,
        None::<&str>,
    )?;
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
        &[
            &status,
            &buttons,
            &scroll,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &quit,
        ],
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
            "toggle-buttons" => toggle(app, Switch::Buttons),
            "toggle-scroll" => toggle(app, Switch::Scroll),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayItems {
        status,
        buttons,
        scroll,
    });
    set_accessibility(app, accessibility_trusted);
    Ok(())
}

fn toggle(app: &AppHandle, which: Switch) {
    let Some(store) = app.try_state::<Mutex<ConfigStore>>() else {
        return;
    };
    let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let next = toggled(&store.current().general, which);
    if let Err(err) = store.update_general(|g| *g = next) {
        tracing::warn!(%err, "couldn't save the tray switch");
    }
    // macOS flips a check item on click by itself, so a failed save must be
    // visibly undone. `set_switches` never touches the store.
    set_switches(app, &store.current().general);
}

/// Shows the saved switches as checkmarks.
fn set_switches(app: &AppHandle, general: &General) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let checks = switch_checks(general);
    if let Err(err) = items
        .buttons
        .set_checked(checks.buttons)
        .and_then(|()| items.scroll.set_checked(checks.scroll))
    {
        tracing::warn!(%err, "failed to update tray switches");
    }
}

/// Keeps the tray checkmarks in sync with saved settings.
pub struct TrayConfigStatus(pub AppHandle);

impl ConfigObserver for TrayConfigStatus {
    fn config_changed(&self, config: &Config) {
        set_switches(&self.0, &config.general);
    }
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

/// Which tray switches show a checkmark.
#[derive(Debug, PartialEq, Eq)]
struct SwitchChecks {
    buttons: bool,
    scroll: bool,
}

#[derive(Clone, Copy)]
enum Switch {
    Buttons,
    Scroll,
}

fn switch_checks(general: &General) -> SwitchChecks {
    SwitchChecks {
        buttons: general.buttons_enabled,
        scroll: general.scroll_enabled,
    }
}

/// The settings after clicking `which` in the tray.
fn toggled(general: &General, which: Switch) -> General {
    let mut next = general.clone();
    match which {
        Switch::Buttons => next.buttons_enabled = !next.buttons_enabled,
        Switch::Scroll => next.scroll_enabled = !next.scroll_enabled,
    }
    next
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
    fn checkmarks_follow_the_saved_switches() {
        let general = General {
            buttons_enabled: false,
            scroll_enabled: true,
        };
        assert_eq!(
            switch_checks(&general),
            SwitchChecks {
                buttons: false,
                scroll: true
            }
        );
    }

    #[test]
    fn clicking_a_switch_flips_only_that_switch() {
        let general = General {
            buttons_enabled: true,
            scroll_enabled: true,
        };
        assert_eq!(
            toggled(&general, Switch::Scroll),
            General {
                buttons_enabled: true,
                scroll_enabled: false
            }
        );
        assert_eq!(
            toggled(&general, Switch::Buttons),
            General {
                buttons_enabled: false,
                scroll_enabled: true
            }
        );
    }

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

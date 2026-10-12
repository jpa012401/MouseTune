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
        schema::Config,
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
    smooth: CheckMenuItem<Wry>,
    invert_vertical: CheckMenuItem<Wry>,
}

pub fn build(app: &AppHandle, accessibility_trusted: bool, config: &Config) -> tauri::Result<()> {
    let checks = switch_checks(config);
    let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
    let buttons = CheckMenuItem::with_id(
        app,
        "toggle-buttons",
        "Enable buttons",
        true,
        checks.buttons,
        None::<&str>,
    )?;
    let scroll = CheckMenuItem::with_id(
        app,
        "toggle-scroll",
        "Enable scrolling",
        true,
        checks.scroll,
        None::<&str>,
    )?;
    let smooth = CheckMenuItem::with_id(
        app,
        "toggle-smooth",
        "Smooth scrolling",
        checks.scroll_options_enabled,
        checks.smooth,
        None::<&str>,
    )?;
    let invert_vertical = CheckMenuItem::with_id(
        app,
        "toggle-invert-vertical",
        "Invert vertical scrolling",
        checks.scroll_options_enabled,
        checks.invert_vertical,
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
            &smooth,
            &invert_vertical,
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
            "toggle-smooth" => toggle(app, Switch::Smooth),
            "toggle-invert-vertical" => toggle(app, Switch::InvertVertical),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayItems {
        status,
        buttons,
        scroll,
        smooth,
        invert_vertical,
    });
    set_accessibility(app, accessibility_trusted);
    Ok(())
}

fn toggle(app: &AppHandle, which: Switch) {
    let Some(store) = app.try_state::<Mutex<ConfigStore>>() else {
        return;
    };
    let mut store = store.lock().unwrap_or_else(PoisonError::into_inner);
    let next = toggled(store.current(), which);
    if let Err(err) = store.update(next) {
        tracing::warn!(%err, "couldn't save the tray switch");
    }
    // macOS flips a check item on click by itself, so a failed save must be
    // visibly undone. `set_switches` never touches the store.
    set_switches(app, store.current());
}

/// Shows the saved switches as checkmarks.
fn set_switches(app: &AppHandle, config: &Config) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let checks = switch_checks(config);
    if let Err(err) = items
        .buttons
        .set_checked(checks.buttons)
        .and_then(|()| items.scroll.set_checked(checks.scroll))
        .and_then(|()| items.smooth.set_checked(checks.smooth))
        .and_then(|()| items.smooth.set_enabled(checks.scroll_options_enabled))
        .and_then(|()| items.invert_vertical.set_checked(checks.invert_vertical))
        .and_then(|()| {
            items
                .invert_vertical
                .set_enabled(checks.scroll_options_enabled)
        })
    {
        tracing::warn!(%err, "failed to update tray switches");
    }
}

/// Keeps the tray checkmarks in sync with saved settings.
pub struct TrayConfigStatus(pub AppHandle);

impl ConfigObserver for TrayConfigStatus {
    fn config_changed(&self, config: &Config) {
        set_switches(&self.0, config);
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

/// Which tray switches show a checkmark, and whether the scroll options
/// can be clicked.
#[derive(Debug, PartialEq, Eq)]
struct SwitchChecks {
    buttons: bool,
    scroll: bool,
    smooth: bool,
    invert_vertical: bool,
    /// The scroll options are greyed out while scrolling is off.
    scroll_options_enabled: bool,
}

#[derive(Clone, Copy)]
enum Switch {
    Buttons,
    Scroll,
    Smooth,
    InvertVertical,
}

fn switch_checks(config: &Config) -> SwitchChecks {
    SwitchChecks {
        buttons: config.general.buttons_enabled,
        scroll: config.general.scroll_enabled,
        smooth: config.scroll.smooth,
        invert_vertical: config.scroll.invert_vertical,
        scroll_options_enabled: config.general.scroll_enabled,
    }
}

/// The settings after clicking `which` in the tray.
fn toggled(config: &Config, which: Switch) -> Config {
    let mut next = config.clone();
    match which {
        Switch::Buttons => next.general.buttons_enabled = !next.general.buttons_enabled,
        Switch::Scroll => next.general.scroll_enabled = !next.general.scroll_enabled,
        Switch::Smooth => next.scroll.smooth = !next.scroll.smooth,
        Switch::InvertVertical => next.scroll.invert_vertical = !next.scroll.invert_vertical,
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

    fn config(scroll_enabled: bool) -> Config {
        let mut c = Config::default();
        c.general.scroll_enabled = scroll_enabled;
        c
    }

    #[test]
    fn checkmarks_follow_the_saved_settings() {
        let mut c = config(true);
        c.general.buttons_enabled = false;
        c.scroll.invert_vertical = true;
        assert_eq!(
            switch_checks(&c),
            SwitchChecks {
                buttons: false,
                scroll: true,
                smooth: true,
                invert_vertical: true,
                scroll_options_enabled: true,
            }
        );
    }

    #[test]
    fn scroll_options_are_greyed_out_while_scrolling_is_off() {
        assert!(!switch_checks(&config(false)).scroll_options_enabled);
    }

    #[test]
    fn clicking_a_switch_flips_only_that_setting() {
        let c = config(true);
        let flipped = |which| toggled(&c, which);

        let mut expected = c.clone();
        expected.general.scroll_enabled = false;
        assert_eq!(flipped(Switch::Scroll), expected);

        let mut expected = c.clone();
        expected.general.buttons_enabled = false;
        assert_eq!(flipped(Switch::Buttons), expected);

        let mut expected = c.clone();
        expected.scroll.smooth = false;
        assert_eq!(flipped(Switch::Smooth), expected);

        let mut expected = c.clone();
        expected.scroll.invert_vertical = true;
        assert_eq!(flipped(Switch::InvertVertical), expected);
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

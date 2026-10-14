//! Settings window lifecycle. The window is created on demand and destroyed on
//! close so the WebKit process doesn't stay resident (spec §2.1).

use std::sync::{Arc, Mutex, PoisonError};

use tauri::{AppHandle, Manager, TitleBarStyle, WebviewUrl, WebviewWindowBuilder};

use crate::{
    config::store::ConfigStore,
    input::thread::{Control, InputHandle},
    permission_monitor::PermissionObserver,
};

const SETTINGS: &str = "settings";

pub fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let result = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::default())
        .title("MouseTune")
        .inner_size(780.0, 540.0)
        .min_inner_size(640.0, 440.0)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .build();

    match result {
        Ok(window) => {
            let _ = window.set_focus();
            let handle = app.clone();
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    // A closed window must never leave a button or the keyboard captured.
                    if let Some(input) = handle.try_state::<Arc<InputHandle>>() {
                        input.send(Control::StopCapture);
                        input.send(Control::StopKeyCapture);
                    }
                }
            });
        }
        Err(err) => tracing::error!(%err, "failed to open settings window"),
    }
}

/// Settings opens by itself only when MouseTune needs the user, so a login
/// launch puts nothing on screen (Phase 7a design §3).
pub fn opens_at_launch(trusted: bool) -> bool {
    !trusted
}

/// With the menu bar icon hidden, lost Accessibility would stop input with
/// nothing on screen to say why.
pub fn opens_on_trust_change(trusted: bool, show_menu_bar_item: bool) -> bool {
    !trusted && !show_menu_bar_item
}

/// Opens Settings when Accessibility is lost and there's no menu bar icon.
pub struct SurfaceLostPermission(pub AppHandle);

impl PermissionObserver for SurfaceLostPermission {
    fn permission_changed(&self, trusted: bool) {
        // Copy the flag out so the store lock isn't held while a window opens.
        let shown = self
            .0
            .try_state::<Mutex<ConfigStore>>()
            .is_none_or(|store| {
                store
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .current()
                    .general
                    .show_menu_bar_item
            });
        if opens_on_trust_change(trusted, shown) {
            let app = self.0.clone();
            // Called on `mt-permissions`; windows are built on the main thread.
            if let Err(err) = self.0.run_on_main_thread(move || open_settings(&app)) {
                tracing::warn!(%err, "couldn’t open Settings after losing Accessibility");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_opens_at_launch_only_while_accessibility_is_missing() {
        assert!(opens_at_launch(false));
        assert!(!opens_at_launch(true));
    }

    #[test]
    fn losing_accessibility_with_the_menu_bar_item_hidden_opens_settings() {
        assert!(opens_on_trust_change(false, false));
    }

    #[test]
    fn the_menu_bar_item_reports_lost_accessibility_by_itself() {
        assert!(!opens_on_trust_change(false, true));
    }

    #[test]
    fn regaining_accessibility_never_opens_settings() {
        assert!(!opens_on_trust_change(true, false));
        assert!(!opens_on_trust_change(true, true));
    }
}

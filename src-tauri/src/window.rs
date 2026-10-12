//! Settings window lifecycle. The window is created on demand and destroyed on
//! close so the WebKit process doesn't stay resident (spec §2.1).

use std::sync::Arc;

use tauri::{AppHandle, Manager, TitleBarStyle, WebviewUrl, WebviewWindowBuilder};

use crate::input::thread::{Control, InputHandle};

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

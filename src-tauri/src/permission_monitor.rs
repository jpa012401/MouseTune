//! Watches Accessibility trust and broadcasts changes (spec §3.1).
//!
//! Polls quickly while untrusted so onboarding reacts as soon as the user
//! flips the switch, and slowly while trusted to notice revocation.

use std::{thread, time::Duration};

use tauri::AppHandle;
use tauri_specta::Event;

use crate::{events::PermissionChanged, platform::permissions, tray};

const UNTRUSTED_INTERVAL: Duration = Duration::from_millis(500);
const TRUSTED_INTERVAL: Duration = Duration::from_secs(5);

pub fn spawn(app: AppHandle) {
    thread::Builder::new()
        .name("mt-permissions".into())
        .spawn(move || {
            let mut trusted = permissions::is_accessibility_trusted();
            loop {
                thread::sleep(if trusted {
                    TRUSTED_INTERVAL
                } else {
                    UNTRUSTED_INTERVAL
                });
                let now = permissions::is_accessibility_trusted();
                if now != trusted {
                    trusted = now;
                    on_change(&app, trusted);
                }
            }
        })
        .expect("failed to spawn permission monitor thread");
}

fn on_change(app: &AppHandle, trusted: bool) {
    tracing::info!(trusted, "Accessibility permission changed");
    tray::set_accessibility(app, trusted);
    let event = PermissionChanged {
        accessibility_trusted: trusted,
    };
    if let Err(err) = event.emit(app) {
        tracing::warn!(%err, "failed to emit permission-changed");
    }
}

//! Tauri commands exposed to the React UI. See spec §10.
//!
//! Commands are thin adapters: they pull dependencies from managed state and
//! delegate to tested use cases.

use std::sync::{Arc, atomic::Ordering};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::{
    accessibility::{self, Accessibility},
    input::{
        engine::{EngineSnapshot, ScrollStats},
        taps::TapSet,
        thread::InputHandle,
    },
    platform::os_version,
};

type AccessibilityState<'a> = State<'a, Arc<dyn Accessibility>>;

#[derive(Debug, Clone, Serialize, Type)]
pub struct Status {
    pub app_version: String,
    pub os_version: os_version::OsVersion,
    pub accessibility_trusted: bool,
    pub input: InputStatus,
}

#[tauri::command]
#[specta::specta]
pub fn get_status(
    app: AppHandle,
    ax: AccessibilityState<'_>,
    input: State<'_, Arc<InputHandle>>,
) -> Status {
    let shared = input.shared();
    Status {
        app_version: app.package_info().version.to_string(),
        os_version: os_version::current(),
        accessibility_trusted: ax.is_trusted(),
        input: input_status(
            &shared.snapshot.load(),
            shared.counters.stats(),
            shared.degraded.load(Ordering::Relaxed),
        ),
    }
}

/// A tracked mouse as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Device {
    /// IORegistry entry ID in hex; stable while the device stays connected.
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub button_count: u8,
    pub has_wheel: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct InputStatus {
    pub taps: TapSet,
    pub devices: Vec<Device>,
    pub scroll_stats: ScrollStats,
    pub degraded: bool,
}

pub fn input_status(snapshot: &EngineSnapshot, stats: ScrollStats, degraded: bool) -> InputStatus {
    InputStatus {
        taps: snapshot.taps,
        devices: snapshot
            .devices
            .iter()
            .map(|d| Device {
                id: format!("{:x}", d.id),
                name: d.name.clone(),
                manufacturer: d.manufacturer.clone(),
                button_count: d.button_count,
                has_wheel: d.has_wheel,
            })
            .collect(),
        scroll_stats: stats,
        degraded,
    }
}

/// Shows the system prompt (first time only) and opens the Accessibility pane.
#[tauri::command]
#[specta::specta]
pub fn request_accessibility(ax: AccessibilityState<'_>) -> Result<(), String> {
    accessibility::request_access(ax.as_ref())
}

/// Clears a stale TCC entry, then asks again.
#[tauri::command]
#[specta::specta]
pub fn reset_accessibility(app: AppHandle, ax: AccessibilityState<'_>) -> Result<(), String> {
    let bundle_id = &app.config().identifier;
    tracing::info!(bundle_id, "resetting Accessibility permission");
    accessibility::reset_access(ax.as_ref(), bundle_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        input::{
            engine::{EngineSnapshot, ScrollStats},
            taps::TapSet,
        },
        test_support::mouse,
    };

    #[test]
    fn input_status_exposes_devices_without_internal_ids() {
        let snapshot = EngineSnapshot {
            trusted: true,
            taps: TapSet {
                buttons: true,
                scroll: true,
                flags: false,
            },
            devices: vec![mouse(255)],
        };
        let status = input_status(
            &snapshot,
            ScrollStats {
                mouse_wheel: 3,
                passed_through: 9,
            },
            true,
        );
        assert_eq!(
            status.taps,
            TapSet {
                buttons: true,
                scroll: true,
                flags: false
            }
        );
        assert_eq!(
            status.scroll_stats,
            ScrollStats {
                mouse_wheel: 3,
                passed_through: 9
            }
        );
        assert!(status.degraded);
        assert_eq!(status.devices.len(), 1);
        let d = &status.devices[0];
        assert_eq!(
            (
                d.id.as_str(),
                d.name.as_str(),
                d.manufacturer.as_str(),
                d.button_count,
                d.has_wheel
            ),
            ("ff", "MX Master 3", "Logitech", 5, true)
        );
    }
}

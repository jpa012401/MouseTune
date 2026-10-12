//! Events emitted from Rust to the React UI. See spec §10.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

use crate::{
    actions::ActionStats,
    config::{schema::Config, store::ConfigObserver},
    input::{
        engine::{EngineObserver, EngineSnapshot, ScrollStats},
        taps::TapSet,
    },
    ipc::{Device, input_status},
    permission_monitor::PermissionObserver,
};

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct PermissionChanged {
    pub accessibility_trusted: bool,
}

/// Forwards permission changes to the UI.
pub struct EmitPermissionChanged(pub AppHandle);

impl PermissionObserver for EmitPermissionChanged {
    fn permission_changed(&self, trusted: bool) {
        let event = PermissionChanged {
            accessibility_trusted: trusted,
        };
        if let Err(err) = event.emit(&self.0) {
            tracing::warn!(%err, "failed to emit permission-changed");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct InputStatusChanged {
    pub taps: TapSet,
    pub devices: Vec<Device>,
}

/// Forwards input-engine changes to the UI.
pub struct EmitInputStatus(pub AppHandle);

impl EngineObserver for EmitInputStatus {
    fn snapshot_changed(&self, snapshot: &EngineSnapshot) {
        let status = input_status(
            snapshot,
            ScrollStats::default(),
            ActionStats::default(),
            false,
        );
        let event = InputStatusChanged {
            taps: status.taps,
            devices: status.devices,
        };
        if let Err(err) = event.emit(&self.0) {
            tracing::warn!(%err, "failed to emit input-status-changed");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct ConfigChanged(pub Config);

/// Forwards saved settings to the UI.
pub struct EmitConfigChanged(pub AppHandle);

impl ConfigObserver for EmitConfigChanged {
    fn config_changed(&self, config: &Config) {
        if let Err(err) = ConfigChanged(config.clone()).emit(&self.0) {
            tracing::warn!(%err, "failed to emit config-changed");
        }
    }
}

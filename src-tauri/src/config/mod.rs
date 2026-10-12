//! Persistent settings (spec §9; design doc docs/superpowers/specs/2026-10-11-phase-3-config-store-design.md).
//! Pure core: no Tauri, no filesystem outside `SystemConfigFile`.

// Wired into the app in Task 5, which removes this allow.
#![allow(dead_code)]

pub mod file;
pub mod load;
pub mod migrate;
pub mod schema;
pub mod store;
pub mod system_file;

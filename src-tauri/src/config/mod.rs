//! Persistent settings (spec §9; design doc docs/superpowers/specs/2026-10-11-phase-3-config-store-design.md).
//! Pure core: no Tauri, no filesystem outside `SystemConfigFile`.

pub mod file;
pub mod load;
pub mod migrate;
pub mod schema;
pub mod store;
pub mod system_file;
pub mod validate;

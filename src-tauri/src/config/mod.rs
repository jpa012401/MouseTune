//! Persistent settings (spec §9).
//! Pure core: no Tauri, no filesystem outside `SystemConfigFile`.

pub mod file;
pub mod load;
pub mod migrate;
pub mod schema;
pub mod store;
pub mod system_file;
pub mod validate;

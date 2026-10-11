//! OS integration. MouseTune is macOS-only; everything lives under `macos`.

mod macos;

pub use macos::{os_version, permissions};

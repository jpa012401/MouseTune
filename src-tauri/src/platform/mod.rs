//! OS integration. MouseTune is macOS-only; everything lives under `macos`.

mod macos;

pub use macos::{event_tap, hid, os_version, permissions};

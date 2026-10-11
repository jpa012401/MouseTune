//! OS integration. MouseTune is macOS-only; everything lives under `macos`.

mod macos;

// `event_tap` is first used in Task 8, which removes this allow.
#[allow(unused_imports)]
pub use macos::{event_tap, os_version, permissions};

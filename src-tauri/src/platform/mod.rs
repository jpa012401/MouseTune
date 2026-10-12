//! OS integration. MouseTune is macOS-only; everything lives under `macos`.

mod macos;

#[allow(unused_imports)] // Wired up in Phase 4a Task 8.
pub use macos::{event_poster, frontmost_app, symbolic_hotkeys};
pub use macos::{event_tap, hid, input_loop, os_version, permissions};

//! OS integration. MouseTune is macOS-only; everything lives under `macos`.

mod macos;

pub use macos::{
    click_timers, event_poster, event_tap, frontmost_app, hid, input_loop, os_version, permissions,
    symbolic_hotkeys,
};

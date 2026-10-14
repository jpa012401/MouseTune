//! OS integration. MouseTune is macOS-only; everything lives under `macos`.

mod macos;

#[allow(unused_imports)] // login_item is wired in Task 6
pub use macos::{
    click_timers, display_link, dock_swipe, event_poster, event_tap, frontmost_app, hid,
    input_loop, login_item, magnify, os_version, output_support, permissions, pointer,
    scroll_poster, symbolic_hotkeys,
};

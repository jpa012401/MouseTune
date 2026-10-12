pub mod click_timers;
pub mod display_link;
pub mod dock_swipe;
pub mod event_poster;
pub mod event_tap;
pub mod frontmost_app;
pub mod hid;
pub mod input_loop;
pub mod magnify;
pub mod os_version;
pub mod permissions;
pub mod pointer;
pub mod scroll_poster;
pub mod symbolic_hotkeys;

/// Private outputs verified on this macOS (Phase 6 design §3). Each flips
/// to `true` only for the versions its spike verified; see
/// `dock_swipe::VERIFIED_ON` and `magnify::VERIFIED_ON` (Tasks 9 and 11).
pub fn output_support() -> crate::input::remaps::OutputSupport {
    crate::input::remaps::OutputSupport::NONE
}

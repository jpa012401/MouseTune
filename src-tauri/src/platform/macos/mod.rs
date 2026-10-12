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

use crate::input::remaps::OutputSupport;

/// Private outputs verified on this macOS (Phase 6 design §3). Each flips
/// to `true` only for the versions its spike verified; see
/// `dock_swipe::VERIFIED_ON` and `magnify::VERIFIED_ON` (Tasks 9 and 11).
/// A verified dock swipe whose private symbols don't resolve stays off, so
/// `SpacesAndMissionControl` compiles to its Discrete fallback.
pub fn output_support() -> OutputSupport {
    let support = support_for(os_version::current().major);
    OutputSupport {
        dock_swipe: support.dock_swipe && dock_swipe::loads(),
        ..support
    }
}

/// What this macOS major version is verified for.
pub fn support_for(major: u32) -> OutputSupport {
    support_for_with(dock_swipe::VERIFIED_ON, major)
}

fn support_for_with(dock_swipe_verified_on: &[u32], major: u32) -> OutputSupport {
    OutputSupport {
        dock_swipe: dock_swipe_verified_on.contains(&major),
        ..OutputSupport::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_swipes_stay_off_everywhere_until_a_version_is_verified() {
        assert!(!support_for(27).dock_swipe);
        assert!(!support_for(26).dock_swipe);
    }

    #[test]
    fn output_support_turns_on_dock_swipe_only_on_verified_versions() {
        assert_eq!(
            support_for_with(&[27], 27),
            OutputSupport {
                dock_swipe: true,
                magnify: false
            }
        );
        assert_eq!(support_for_with(&[27], 26), OutputSupport::NONE);
    }
}

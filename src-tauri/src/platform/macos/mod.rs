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
/// `dock_swipe::VERIFIED_ON` and `magnify::VERIFIED_ON`.
/// A verified dock swipe whose private symbols don't resolve stays off, so
/// `SpacesAndMissionControl` compiles to its Discrete fallback. The load is
/// done once and cached (`dock_swipe::api`), so this answer and whether
/// `input_loop` gets a `MacDockSwipe` can't disagree.
pub fn output_support() -> OutputSupport {
    support_for(os_version::current().major)
}

/// What this macOS major version is verified for, and can load.
pub fn support_for(major: u32) -> OutputSupport {
    support_for_with(
        dock_swipe::VERIFIED_ON,
        magnify::VERIFIED_ON,
        major,
        dock_swipe::loads,
    )
}

/// `loads` (the dock swipe's private symbols) is asked only on a version
/// verified for dock swipes, so an unverified macOS never touches the
/// private frameworks. The magnify gesture loads nothing: it is built with
/// public CoreGraphics calls (`magnify` module doc).
fn support_for_with(
    dock_swipe_verified_on: &[u32],
    magnify_verified_on: &[u32],
    major: u32,
    loads: impl FnOnce() -> bool,
) -> OutputSupport {
    OutputSupport {
        dock_swipe: dock_swipe_verified_on.contains(&major) && loads(),
        magnify: magnify_verified_on.contains(&major),
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
            support_for_with(&[27], &[], 27, || true),
            OutputSupport {
                dock_swipe: true,
                magnify: false
            }
        );
        assert_eq!(
            support_for_with(&[27], &[], 26, || true),
            OutputSupport::NONE
        );
    }

    #[test]
    fn magnify_stays_off_everywhere_until_a_version_is_verified() {
        assert!(!support_for(27).magnify);
        assert!(!support_for(26).magnify);
    }

    #[test]
    fn output_support_turns_on_magnify_only_on_verified_versions() {
        let never_loads = || panic!("magnify needs no private symbols");
        assert_eq!(
            support_for_with(&[], &[27], 27, never_loads),
            OutputSupport {
                dock_swipe: false,
                magnify: true
            }
        );
        assert_eq!(
            support_for_with(&[], &[27], 26, never_loads),
            OutputSupport::NONE
        );
    }

    #[test]
    fn a_verified_version_whose_symbols_dont_load_keeps_dock_swipes_off() {
        assert_eq!(
            support_for_with(&[27], &[], 27, || false),
            OutputSupport::NONE
        );
    }

    #[test]
    fn an_unverified_version_never_loads_the_private_symbols() {
        let support = support_for_with(&[27], &[], 26, || panic!("loaded on an unverified macOS"));
        assert_eq!(support, OutputSupport::NONE);
    }
}

//! Which scroll events MouseTune may take over (spec §4.2). Only discrete
//! wheel ticks from real mice; everything else passes through untouched.

use crate::input::{
    devices::DeviceRegistry,
    router::{ButtonEvent, ButtonKind},
};

/// Stamped into `kCGEventSourceUserData` on every event MouseTune posts.
pub const MOUSETUNE_EVENT_TAG: i64 = 0x004D_5455_4E45; // "MTUNE"

/// The CGEvent fields the filter reads, copied out by the tap adapter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollFields {
    pub is_continuous: i64,
    pub scroll_phase: i64,
    pub momentum_phase: i64,
    pub tablet_device_id: i64,
    pub delta_axis1: i64,
    pub delta_axis2: i64,
    pub sender_id: u64,
    pub user_data: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassReason {
    OwnEvent,
    Continuous,
    Phased,
    Tablet,
    Diagonal,
    ExcludedDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollClass {
    MouseWheel,
    PassThrough(PassReason),
}

pub fn classify_scroll(fields: &ScrollFields, devices: &DeviceRegistry) -> ScrollClass {
    let reason = if fields.user_data == MOUSETUNE_EVENT_TAG {
        Some(PassReason::OwnEvent)
    } else if fields.is_continuous != 0 {
        Some(PassReason::Continuous)
    } else if fields.scroll_phase != 0 || fields.momentum_phase != 0 {
        Some(PassReason::Phased)
    } else if fields.tablet_device_id != 0 {
        Some(PassReason::Tablet)
    } else if fields.delta_axis1 != 0 && fields.delta_axis2 != 0 {
        Some(PassReason::Diagonal)
    } else if devices
        .by_sender(fields.sender_id)
        .is_some_and(|d| d.is_excluded())
    {
        Some(PassReason::ExcludedDevice)
    } else {
        None
    };
    reason.map_or(ScrollClass::MouseWheel, ScrollClass::PassThrough)
}

/// `kCGEventMouseSubtypeTabletPoint`.
pub const MOUSE_SUBTYPE_TABLET_POINT: i64 = 1;
/// `kCGEventMouseSubtypeTabletProximity`.
pub const MOUSE_SUBTYPE_TABLET_PROXIMITY: i64 = 2;

/// A button down from a device MouseTune leaves alone: an excluded device
/// (Apple, trackpads) or a tablet (AGENTS.md invariant 2). Unknown senders
/// are not foreign; they are processed (spec §4.2). Ups are never foreign:
/// they follow their down's verdict. Called from the button tap; must not
/// allocate.
pub fn is_foreign_button_down(event: &ButtonEvent, devices: &DeviceRegistry) -> bool {
    event.kind == ButtonKind::Down
        && (event.mouse_subtype == MOUSE_SUBTYPE_TABLET_POINT
            || event.mouse_subtype == MOUSE_SUBTYPE_TABLET_PROXIMITY
            || devices
                .by_sender(event.sender_id)
                .is_some_and(|d| d.is_excluded()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{apple_trackpad, mouse};

    fn registry() -> DeviceRegistry {
        let mut r = DeviceRegistry::default();
        r.insert(mouse(1)); // senders 10, 11
        r.insert(apple_trackpad(2)); // sender 20
        r
    }

    /// One notch down on the tracked mouse.
    fn wheel_tick() -> ScrollFields {
        ScrollFields {
            delta_axis1: -1,
            sender_id: 10,
            ..Default::default()
        }
    }

    fn classify(fields: ScrollFields) -> ScrollClass {
        classify_scroll(&fields, &registry())
    }

    #[test]
    fn discrete_tick_from_tracked_mouse_is_mouse_wheel() {
        assert_eq!(classify(wheel_tick()), ScrollClass::MouseWheel);
    }

    #[test]
    fn horizontal_only_tick_is_mouse_wheel() {
        let f = ScrollFields {
            delta_axis1: 0,
            delta_axis2: 2,
            ..wheel_tick()
        };
        assert_eq!(classify(f), ScrollClass::MouseWheel);
    }

    #[test]
    fn unknown_sender_is_treated_as_a_mouse() {
        assert_eq!(
            classify(ScrollFields {
                sender_id: 0,
                ..wheel_tick()
            }),
            ScrollClass::MouseWheel
        );
        assert_eq!(
            classify(ScrollFields {
                sender_id: 999,
                ..wheel_tick()
            }),
            ScrollClass::MouseWheel
        );
    }

    #[test]
    fn continuous_events_pass_through() {
        let f = ScrollFields {
            is_continuous: 1,
            ..wheel_tick()
        };
        assert_eq!(
            classify(f),
            ScrollClass::PassThrough(PassReason::Continuous)
        );
    }

    #[test]
    fn scroll_phase_passes_through() {
        let f = ScrollFields {
            scroll_phase: 1,
            ..wheel_tick()
        };
        assert_eq!(classify(f), ScrollClass::PassThrough(PassReason::Phased));
    }

    #[test]
    fn momentum_phase_passes_through() {
        let f = ScrollFields {
            momentum_phase: 2,
            ..wheel_tick()
        };
        assert_eq!(classify(f), ScrollClass::PassThrough(PassReason::Phased));
    }

    #[test]
    fn tablet_events_pass_through() {
        let f = ScrollFields {
            tablet_device_id: 7,
            ..wheel_tick()
        };
        assert_eq!(classify(f), ScrollClass::PassThrough(PassReason::Tablet));
    }

    #[test]
    fn diagonal_deltas_pass_through() {
        let f = ScrollFields {
            delta_axis1: -1,
            delta_axis2: 1,
            ..wheel_tick()
        };
        assert_eq!(classify(f), ScrollClass::PassThrough(PassReason::Diagonal));
    }

    #[test]
    fn discrete_tick_from_excluded_device_passes_through() {
        let f = ScrollFields {
            sender_id: 20,
            ..wheel_tick()
        };
        assert_eq!(
            classify(f),
            ScrollClass::PassThrough(PassReason::ExcludedDevice)
        );
    }

    #[test]
    fn own_events_pass_through_before_any_other_rule() {
        let f = ScrollFields {
            user_data: MOUSETUNE_EVENT_TAG,
            is_continuous: 1,
            ..wheel_tick()
        };
        assert_eq!(classify(f), ScrollClass::PassThrough(PassReason::OwnEvent));
    }

    #[test]
    fn other_user_data_values_are_not_ours() {
        let f = ScrollFields {
            user_data: 1,
            ..wheel_tick()
        };
        assert_eq!(classify(f), ScrollClass::MouseWheel);
    }
}

#[cfg(test)]
mod button_tests {
    use super::*;
    use crate::test_support::{apple_trackpad, mouse};

    fn registry() -> DeviceRegistry {
        let mut r = DeviceRegistry::default();
        r.insert(mouse(1)); // senders 10, 11
        r.insert(apple_trackpad(2)); // sender 20
        r
    }

    fn middle_down(sender_id: u64) -> ButtonEvent {
        ButtonEvent {
            kind: ButtonKind::Down,
            button: 2,
            sender_id,
            user_data: 0,
            mouse_subtype: 0,
        }
    }

    #[test]
    fn a_down_from_a_tracked_mouse_is_not_foreign() {
        assert!(!is_foreign_button_down(&middle_down(10), &registry()));
    }

    #[test]
    fn a_down_from_an_unknown_sender_is_not_foreign() {
        assert!(!is_foreign_button_down(&middle_down(999), &registry()));
        assert!(!is_foreign_button_down(&middle_down(0), &registry()));
    }

    #[test]
    fn a_down_from_an_excluded_device_is_foreign() {
        assert!(is_foreign_button_down(&middle_down(20), &registry()));
    }

    #[test]
    fn tablet_downs_are_foreign() {
        for subtype in [1, 2] {
            let tablet = ButtonEvent {
                mouse_subtype: subtype,
                ..middle_down(10)
            };
            assert!(is_foreign_button_down(&tablet, &registry()), "{subtype}");
        }
    }

    #[test]
    fn ups_are_never_foreign() {
        let up = ButtonEvent {
            kind: ButtonKind::Up,
            mouse_subtype: 1,
            ..middle_down(20)
        };
        assert!(!is_foreign_button_down(&up, &registry()));
    }
}

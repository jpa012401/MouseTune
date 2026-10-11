//! Which scroll events MouseTune may take over (spec §4.2). Only discrete
//! wheel ticks from real mice; everything else passes through untouched.

use crate::input::devices::DeviceRegistry;

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

//! Which event taps should run, and keeping the real taps in line with
//! that (spec §4.1, §4.5).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::input::devices::DeviceRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapKind {
    Buttons,
    Scroll,
    Flags,
    /// Key downs and ups, only while a shortcut is being recorded.
    Keys,
    /// Pointer movement, only while a drag-bound button is down.
    Drag,
}

impl TapKind {
    pub const ALL: [TapKind; 5] = [
        TapKind::Buttons,
        TapKind::Scroll,
        TapKind::Flags,
        TapKind::Keys,
        TapKind::Drag,
    ];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TapSet {
    pub buttons: bool,
    pub scroll: bool,
    pub flags: bool,
    pub keys: bool,
    pub drag: bool,
}

impl TapSet {
    pub const NONE: TapSet = TapSet {
        buttons: false,
        scroll: false,
        flags: false,
        keys: false,
        drag: false,
    };

    pub fn get(self, kind: TapKind) -> bool {
        match kind {
            TapKind::Buttons => self.buttons,
            TapKind::Scroll => self.scroll,
            TapKind::Flags => self.flags,
            TapKind::Keys => self.keys,
            TapKind::Drag => self.drag,
        }
    }

    pub fn set(&mut self, kind: TapKind, on: bool) {
        match kind {
            TapKind::Buttons => self.buttons = on,
            TapKind::Scroll => self.scroll = on,
            TapKind::Flags => self.flags = on,
            TapKind::Keys => self.keys = on,
            TapKind::Drag => self.drag = on,
        }
    }
}

/// What the configuration needs (`config::schema::tap_demand`). The engine
/// sets `keys` (a shortcut is being recorded) and `drag` (a drag-bound
/// button is down) itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapDemand {
    pub buttons: bool,
    pub scroll: bool,
    pub flags: bool,
    pub keys: bool,
    pub drag: bool,
}

#[cfg(test)]
impl TapDemand {
    pub const ALL: TapDemand = TapDemand {
        buttons: true,
        scroll: true,
        flags: true,
        keys: true,
        drag: true,
    };
}

/// Taps run only when they can do something: permission granted, a tracked
/// mouse with the relevant hardware present, and the config wants them. The
/// keys tap needs only permission and a recording in progress.
pub fn desired_taps(trusted: bool, devices: &DeviceRegistry, demand: TapDemand) -> TapSet {
    if !trusted {
        return TapSet::NONE;
    }
    let mut any = false;
    let mut buttons = false;
    let mut wheel = false;
    for device in devices.tracked() {
        any = true;
        buttons |= device.has_extra_buttons();
        wheel |= device.has_wheel;
    }
    TapSet {
        buttons: demand.buttons && buttons,
        scroll: demand.scroll && wheel,
        flags: demand.flags && any,
        // Recording a shortcut works from the keyboard alone.
        keys: demand.keys,
        // Only while a drag-bound button is down (the engine sets it).
        drag: demand.drag && buttons,
    }
}

/// Port to the OS event taps.
pub trait TapController {
    /// Enables or disables a tap, creating it on first enable (or when its port has been invalidated).
    /// Callback-safe: may run inside a tap callout, so implementations must never discard a valid tap.
    /// Returns whether the tap is in the requested state afterwards (`false` if it couldn't be created or won't re-enable).
    fn set_enabled(&mut self, kind: TapKind, enabled: bool) -> bool;
    fn is_enabled(&self, kind: TapKind) -> bool;
    /// Turns a desired tap back on, rebuilding it if it won't come back.
    /// Only call this outside tap callbacks (watchdog). Returns whether it is on afterwards.
    fn revive(&mut self, kind: TapKind) -> bool;
}

pub struct TapManager<C: TapController> {
    controller: C,
    desired: TapSet,
}

impl<C: TapController> TapManager<C> {
    pub fn new(controller: C) -> Self {
        Self {
            controller,
            desired: TapSet::NONE,
        }
    }

    /// Makes the real taps match `desired`, touching only taps that differ.
    pub fn reconcile(&mut self, desired: TapSet) {
        self.desired = desired;
        for kind in TapKind::ALL {
            let want = desired.get(kind);
            if self.controller.is_enabled(kind) != want {
                self.controller.set_enabled(kind, want);
            }
        }
    }

    /// Turns one tap off and stops wanting it. Callback-safe and O(1): it
    /// touches no other tap, and disabling never discards a port.
    pub fn release(&mut self, kind: TapKind) {
        self.desired.set(kind, false);
        self.controller.set_enabled(kind, false);
    }

    /// Turns a tap back on if it's supposed to be on. Returns whether it is on afterwards.
    /// Callback-safe: never rebuilds a tap; a tap that won't come back is left for the watchdog.
    pub fn reenable(&mut self, kind: TapKind) -> bool {
        self.desired.get(kind) && self.controller.set_enabled(kind, true)
    }

    /// Revives every desired tap that's off. Returns the kinds it revived.
    pub fn watchdog(&mut self) -> TapSet {
        let mut revived = TapSet::NONE;
        for kind in TapKind::ALL {
            if self.desired.get(kind)
                && !self.controller.is_enabled(kind)
                && self.controller.revive(kind)
            {
                revived.set(kind, true);
            }
        }
        revived
    }

    pub fn enabled(&self) -> TapSet {
        let mut set = TapSet::NONE;
        for kind in TapKind::ALL {
            set.set(kind, self.controller.is_enabled(kind));
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        input::devices::DeviceInfo,
        test_support::{FakeTapController, apple_trackpad, mouse},
    };

    const ALL_ON: TapSet = TapSet {
        buttons: true,
        scroll: true,
        flags: true,
        keys: false,
        drag: false,
    };

    /// Everything the config can ask for; no recording.
    const MOUSE: TapDemand = TapDemand {
        keys: false,
        drag: false,
        ..TapDemand::ALL
    };

    fn with(devices: &[DeviceInfo]) -> DeviceRegistry {
        let mut r = DeviceRegistry::default();
        for d in devices {
            r.insert(d.clone());
        }
        r
    }

    #[test]
    fn the_drag_tap_runs_only_when_asked_and_trusted() {
        let mut reg = DeviceRegistry::default();
        reg.insert(crate::test_support::mouse(1));
        let demand = TapDemand {
            drag: true,
            ..TapDemand::ALL
        };
        assert!(desired_taps(true, &reg, demand).drag);
        assert!(!desired_taps(false, &reg, demand).drag);
        assert!(
            !desired_taps(
                true,
                &reg,
                TapDemand {
                    drag: false,
                    ..TapDemand::ALL
                }
            )
            .drag
        );
    }

    #[test]
    fn nothing_runs_without_permission() {
        assert_eq!(desired_taps(false, &with(&[mouse(1)]), MOUSE), TapSet::NONE);
    }

    #[test]
    fn nothing_runs_without_a_mouse() {
        assert_eq!(desired_taps(true, &with(&[]), MOUSE), TapSet::NONE);
    }

    #[test]
    fn excluded_devices_do_not_count() {
        assert_eq!(
            desired_taps(true, &with(&[apple_trackpad(1)]), MOUSE),
            TapSet::NONE
        );
    }

    #[test]
    fn five_button_wheel_mouse_runs_everything() {
        assert_eq!(desired_taps(true, &with(&[mouse(1)]), MOUSE), ALL_ON);
    }

    #[test]
    fn two_button_mouse_gets_no_button_tap() {
        let basic = DeviceInfo {
            button_count: 2,
            ..mouse(1)
        };
        let want = TapSet {
            buttons: false,
            scroll: true,
            flags: true,
            keys: false,
            drag: false,
        };
        assert_eq!(desired_taps(true, &with(&[basic]), MOUSE), want);
    }

    #[test]
    fn wheelless_mouse_gets_no_scroll_tap() {
        let no_wheel = DeviceInfo {
            has_wheel: false,
            ..mouse(1)
        };
        let want = TapSet {
            buttons: true,
            scroll: false,
            flags: true,
            keys: false,
            drag: false,
        };
        assert_eq!(desired_taps(true, &with(&[no_wheel]), MOUSE), want);
    }

    #[test]
    fn a_trackpad_next_to_a_mouse_adds_nothing() {
        let basic_mouse = DeviceInfo {
            button_count: 2,
            has_wheel: false,
            ..mouse(1)
        };
        // Hardware that would turn every tap on if trackpads counted.
        let trackpad = DeviceInfo {
            button_count: 3,
            has_wheel: true,
            ..apple_trackpad(2)
        };
        let want = TapSet {
            buttons: false,
            scroll: false,
            flags: true,
            keys: false,
            drag: false,
        };
        assert_eq!(
            desired_taps(true, &with(&[basic_mouse, trackpad]), MOUSE),
            want
        );
    }

    #[test]
    fn two_mice_union_their_hardware() {
        let buttons_no_wheel = DeviceInfo {
            has_wheel: false,
            ..mouse(1)
        };
        let wheel_two_buttons = DeviceInfo {
            button_count: 2,
            ..mouse(2)
        };
        assert_eq!(
            desired_taps(true, &with(&[buttons_no_wheel, wheel_two_buttons]), MOUSE),
            ALL_ON
        );
    }

    #[test]
    fn demand_switches_taps_off() {
        let demand = TapDemand {
            buttons: true,
            scroll: false,
            flags: false,
            keys: false,
            drag: false,
        };
        let want = TapSet {
            buttons: true,
            scroll: false,
            flags: false,
            keys: false,
            drag: false,
        };
        assert_eq!(desired_taps(true, &with(&[mouse(1)]), demand), want);
    }

    #[test]
    fn the_keys_tap_runs_while_recording_even_without_a_mouse() {
        let demand = TapDemand {
            buttons: false,
            scroll: false,
            flags: false,
            keys: true,
            drag: false,
        };
        assert!(desired_taps(true, &DeviceRegistry::default(), demand).keys);
        assert!(!desired_taps(false, &DeviceRegistry::default(), demand).keys);
    }

    #[test]
    fn release_turns_off_only_that_tap_and_the_watchdog_leaves_it_off() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet {
            buttons: false,
            scroll: true,
            flags: false,
            keys: true,
            drag: false,
        });
        fake.os_disables(TapKind::Scroll);

        manager.release(TapKind::Keys);

        assert_eq!(
            fake.calls(),
            [
                (TapKind::Scroll, true),
                (TapKind::Keys, true),
                (TapKind::Keys, false)
            ]
        );
        assert_eq!(manager.enabled(), TapSet::NONE);
        assert_eq!(
            manager.watchdog(),
            TapSet {
                buttons: false,
                scroll: true,
                flags: false,
                keys: false,
                drag: false
            }
        );
    }

    #[test]
    fn reconcile_touches_only_taps_that_differ() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());

        manager.reconcile(TapSet {
            buttons: true,
            scroll: false,
            flags: false,
            keys: false,
            drag: false,
        });
        manager.reconcile(TapSet {
            buttons: true,
            scroll: false,
            flags: false,
            keys: false,
            drag: false,
        });
        manager.reconcile(TapSet {
            buttons: false,
            scroll: true,
            flags: false,
            keys: false,
            drag: false,
        });

        assert_eq!(
            fake.calls(),
            [
                (TapKind::Buttons, true),
                (TapKind::Buttons, false),
                (TapKind::Scroll, true)
            ]
        );
        assert_eq!(
            manager.enabled(),
            TapSet {
                buttons: false,
                scroll: true,
                flags: false,
                keys: false,
                drag: false,
            }
        );
    }

    #[test]
    fn watchdog_revives_desired_taps_the_os_disabled() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet {
            buttons: true,
            scroll: true,
            flags: false,
            keys: false,
            drag: false,
        });

        fake.os_disables(TapKind::Scroll);

        assert_eq!(
            manager.watchdog(),
            TapSet {
                buttons: false,
                scroll: true,
                flags: false,
                keys: false,
                drag: false,
            }
        );
        assert_eq!(
            manager.enabled(),
            TapSet {
                buttons: true,
                scroll: true,
                flags: false,
                keys: false,
                drag: false,
            }
        );
    }

    #[test]
    fn watchdog_leaves_undesired_taps_off() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet::NONE);
        assert_eq!(manager.watchdog(), TapSet::NONE);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn failed_enable_is_retried_by_watchdog() {
        let fake = FakeTapController::default();
        fake.fail(TapKind::Scroll);
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet {
            buttons: false,
            scroll: true,
            flags: false,
            keys: false,
            drag: false,
        });
        assert_eq!(manager.enabled(), TapSet::NONE);

        fake.succeed(TapKind::Scroll);

        assert_eq!(
            manager.watchdog(),
            TapSet {
                buttons: false,
                scroll: true,
                flags: false,
                keys: false,
                drag: false,
            }
        );
        assert!(manager.enabled().scroll);
    }

    #[test]
    fn reenable_only_applies_to_desired_taps() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet {
            buttons: true,
            scroll: false,
            flags: false,
            keys: false,
            drag: false,
        });
        fake.os_disables(TapKind::Buttons);

        assert!(manager.reenable(TapKind::Buttons));
        assert!(!manager.reenable(TapKind::Scroll));
        assert_eq!(
            manager.enabled(),
            TapSet {
                buttons: true,
                scroll: false,
                flags: false,
                keys: false,
                drag: false,
            }
        );
    }

    #[test]
    fn watchdog_rebuilds_a_wedged_tap() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet {
            buttons: false,
            scroll: true,
            flags: false,
            keys: false,
            drag: false,
        });
        fake.wedge(TapKind::Scroll);

        assert_eq!(
            manager.watchdog(),
            TapSet {
                buttons: false,
                scroll: true,
                flags: false,
                keys: false,
                drag: false,
            }
        );
        assert!(manager.enabled().scroll);
        assert_eq!(fake.revives(), [TapKind::Scroll]);
    }

    #[test]
    fn reenable_never_rebuilds_a_tap() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet {
            buttons: false,
            scroll: true,
            flags: false,
            keys: false,
            drag: false,
        });
        fake.wedge(TapKind::Scroll);

        assert!(!manager.reenable(TapKind::Scroll));
        assert!(!manager.enabled().scroll);
        assert!(fake.revives().is_empty());
    }

    #[test]
    fn watchdog_does_not_rebuild_undesired_taps() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());
        manager.reconcile(TapSet::NONE);
        fake.wedge(TapKind::Buttons);
        assert_eq!(manager.watchdog(), TapSet::NONE);
        assert!(fake.revives().is_empty());
    }
}

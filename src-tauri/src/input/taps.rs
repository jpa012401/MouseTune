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
}

impl TapKind {
    pub const ALL: [TapKind; 3] = [TapKind::Buttons, TapKind::Scroll, TapKind::Flags];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TapSet {
    pub buttons: bool,
    pub scroll: bool,
    pub flags: bool,
}

impl TapSet {
    pub const NONE: TapSet = TapSet {
        buttons: false,
        scroll: false,
        flags: false,
    };

    pub fn get(self, kind: TapKind) -> bool {
        match kind {
            TapKind::Buttons => self.buttons,
            TapKind::Scroll => self.scroll,
            TapKind::Flags => self.flags,
        }
    }

    pub fn set(&mut self, kind: TapKind, on: bool) {
        match kind {
            TapKind::Buttons => self.buttons = on,
            TapKind::Scroll => self.scroll = on,
            TapKind::Flags => self.flags = on,
        }
    }
}

/// What the configuration needs. Until the config store exists (Phase 3)
/// the app asks for everything (`TapDemand::ALL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapDemand {
    pub buttons: bool,
    pub scroll: bool,
    pub flags: bool,
}

impl TapDemand {
    pub const ALL: TapDemand = TapDemand {
        buttons: true,
        scroll: true,
        flags: true,
    };
}

/// Taps run only when they can do something: permission granted, a tracked
/// mouse with the relevant hardware present, and the config wants them.
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
    }
}

/// Port to the OS event taps.
pub trait TapController {
    /// Enables or disables a tap, creating it on first enable. Returns
    /// `false` if the tap couldn't be created (e.g. no permission).
    fn set_enabled(&mut self, kind: TapKind, enabled: bool) -> bool;
    fn is_enabled(&self, kind: TapKind) -> bool;
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

    /// Turns a tap back on if it's supposed to be on. Returns whether it is on afterwards.
    pub fn reenable(&mut self, kind: TapKind) -> bool {
        self.desired.get(kind) && self.controller.set_enabled(kind, true)
    }

    /// Revives every desired tap that's off. Returns the kinds it revived.
    pub fn watchdog(&mut self) -> TapSet {
        let mut revived = TapSet::NONE;
        for kind in TapKind::ALL {
            if self.desired.get(kind) && !self.controller.is_enabled(kind) && self.reenable(kind) {
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
    };

    fn with(devices: &[DeviceInfo]) -> DeviceRegistry {
        let mut r = DeviceRegistry::default();
        for d in devices {
            r.insert(d.clone());
        }
        r
    }

    #[test]
    fn nothing_runs_without_permission() {
        assert_eq!(
            desired_taps(false, &with(&[mouse(1)]), TapDemand::ALL),
            TapSet::NONE
        );
    }

    #[test]
    fn nothing_runs_without_a_mouse() {
        assert_eq!(desired_taps(true, &with(&[]), TapDemand::ALL), TapSet::NONE);
    }

    #[test]
    fn excluded_devices_do_not_count() {
        assert_eq!(
            desired_taps(true, &with(&[apple_trackpad(1)]), TapDemand::ALL),
            TapSet::NONE
        );
    }

    #[test]
    fn five_button_wheel_mouse_runs_everything() {
        assert_eq!(
            desired_taps(true, &with(&[mouse(1)]), TapDemand::ALL),
            ALL_ON
        );
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
        };
        assert_eq!(desired_taps(true, &with(&[basic]), TapDemand::ALL), want);
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
        };
        assert_eq!(desired_taps(true, &with(&[no_wheel]), TapDemand::ALL), want);
    }

    #[test]
    fn demand_switches_taps_off() {
        let demand = TapDemand {
            buttons: true,
            scroll: false,
            flags: false,
        };
        let want = TapSet {
            buttons: true,
            scroll: false,
            flags: false,
        };
        assert_eq!(desired_taps(true, &with(&[mouse(1)]), demand), want);
    }

    #[test]
    fn reconcile_touches_only_taps_that_differ() {
        let fake = FakeTapController::default();
        let mut manager = TapManager::new(fake.clone());

        manager.reconcile(TapSet {
            buttons: true,
            scroll: false,
            flags: false,
        });
        manager.reconcile(TapSet {
            buttons: true,
            scroll: false,
            flags: false,
        });
        manager.reconcile(TapSet {
            buttons: false,
            scroll: true,
            flags: false,
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
                flags: false
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
        });

        fake.os_disables(TapKind::Scroll);

        assert_eq!(
            manager.watchdog(),
            TapSet {
                buttons: false,
                scroll: true,
                flags: false
            }
        );
        assert_eq!(
            manager.enabled(),
            TapSet {
                buttons: true,
                scroll: true,
                flags: false
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
        });
        assert_eq!(manager.enabled(), TapSet::NONE);

        fake.succeed(TapKind::Scroll);

        assert_eq!(
            manager.watchdog(),
            TapSet {
                buttons: false,
                scroll: true,
                flags: false
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
        });
        fake.os_disables(TapKind::Buttons);

        assert!(manager.reenable(TapKind::Buttons));
        assert!(!manager.reenable(TapKind::Scroll));
        assert_eq!(
            manager.enabled(),
            TapSet {
                buttons: true,
                scroll: false,
                flags: false
            }
        );
    }
}

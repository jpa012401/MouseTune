//! Owns all input state on the input thread and decides what the taps do
//! (spec §4.4). OS access goes through `TapController`; results go out
//! through `EngineObserver`.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use serde::Serialize;
use specta::Type;

use crate::input::{
    devices::{DeviceEvent, DeviceInfo, DeviceRegistry},
    filter::{ScrollClass, ScrollFields, classify_scroll},
    guard::{DisabledAction, DisabledReason, on_tap_disabled},
    taps::{TapController, TapDemand, TapKind, TapManager, TapSet, desired_taps},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
pub struct ScrollStats {
    pub mouse_wheel: u32,
    pub passed_through: u32,
}

/// Lock-free counters, safe to bump from a tap callback.
#[derive(Debug, Default)]
pub struct ScrollCounters {
    mouse_wheel: AtomicU32,
    passed_through: AtomicU32,
}

impl ScrollCounters {
    pub fn record(&self, class: ScrollClass) {
        let counter = match class {
            ScrollClass::MouseWheel => &self.mouse_wheel,
            ScrollClass::PassThrough(_) => &self.passed_through,
        };
        // fetch_update with saturating_add: never wraps to 0.
        let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            Some(n.saturating_add(1))
        });
    }

    pub fn stats(&self) -> ScrollStats {
        ScrollStats {
            mouse_wheel: self.mouse_wheel.load(Ordering::Relaxed),
            passed_through: self.passed_through.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineSnapshot {
    pub trusted: bool,
    /// Taps actually running right now.
    pub taps: TapSet,
    /// Tracked (non-excluded) devices.
    pub devices: Vec<DeviceInfo>,
}

pub trait EngineObserver {
    fn snapshot_changed(&self, snapshot: &EngineSnapshot);
}

/// What tap callbacks call into. Implemented by `InputEngine`.
pub trait TapHandler {
    fn on_scroll(&mut self, fields: &ScrollFields);
    fn on_button(&mut self, sender_id: u64);
    fn on_tap_disabled(&mut self, kind: TapKind, reason: DisabledReason);
}

pub struct InputEngine<C: TapController> {
    devices: DeviceRegistry,
    taps: TapManager<C>,
    trusted: bool,
    demand: TapDemand,
    counters: Arc<ScrollCounters>,
    observer: Box<dyn EngineObserver>,
    last_published: Option<EngineSnapshot>,
}

impl<C: TapController> InputEngine<C> {
    pub fn new(
        controller: C,
        trusted: bool,
        demand: TapDemand,
        counters: Arc<ScrollCounters>,
        observer: Box<dyn EngineObserver>,
    ) -> Self {
        Self {
            devices: DeviceRegistry::default(),
            taps: TapManager::new(controller),
            trusted,
            demand,
            counters,
            observer,
            last_published: None,
        }
    }

    /// Applies the initial state. Separate from `new` so adapters can finish
    /// wiring callbacks before any tap is created.
    pub fn start(&mut self) {
        self.refresh();
    }

    pub fn set_trusted(&mut self, trusted: bool) {
        self.trusted = trusted;
        self.refresh();
    }

    pub fn apply_device_event(&mut self, event: DeviceEvent) {
        match event {
            DeviceEvent::Added(device) => self.devices.insert(device),
            DeviceEvent::Removed { id } => {
                self.devices.remove(id);
            }
        }
        self.refresh();
    }

    /// Periodic check (every 5 s) that the taps that should run are running.
    pub fn watchdog(&mut self) {
        let revived = self.taps.watchdog();
        if revived != TapSet::NONE {
            tracing::warn!(?revived, "watchdog re-enabled event taps");
        }
        self.publish();
    }

    fn refresh(&mut self) {
        let desired = desired_taps(self.trusted, &self.devices, self.demand);
        self.taps.reconcile(desired);
        self.publish();
    }

    fn publish(&mut self) {
        let snapshot = self.snapshot();
        if self.last_published.as_ref() != Some(&snapshot) {
            self.observer.snapshot_changed(&snapshot);
            self.last_published = Some(snapshot);
        }
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        EngineSnapshot {
            trusted: self.trusted,
            taps: self.taps.enabled(),
            devices: self.devices.tracked().cloned().collect(),
        }
    }
}

impl<C: TapController> TapHandler for InputEngine<C> {
    fn on_scroll(&mut self, fields: &ScrollFields) {
        self.counters.record(classify_scroll(fields, &self.devices));
    }

    /// Buttons are routed in Phase 4; Phase 2 passes them through.
    fn on_button(&mut self, _sender_id: u64) {}

    fn on_tap_disabled(&mut self, kind: TapKind, reason: DisabledReason) {
        if on_tap_disabled(reason) == DisabledAction::Reenable {
            self.taps.reenable(kind);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::{
        input::filter::{MOUSETUNE_EVENT_TAG, PassReason},
        test_support::{FakeTapController, apple_trackpad, mouse},
    };

    const ALL_ON: TapSet = TapSet {
        buttons: true,
        scroll: true,
        flags: true,
    };

    #[derive(Clone, Default)]
    struct Published(Arc<Mutex<Vec<EngineSnapshot>>>);

    impl Published {
        fn all(&self) -> Vec<EngineSnapshot> {
            self.0.lock().unwrap().clone()
        }
        fn last(&self) -> EngineSnapshot {
            self.all().last().cloned().expect("nothing published")
        }
    }

    impl EngineObserver for Published {
        fn snapshot_changed(&self, snapshot: &EngineSnapshot) {
            self.0.lock().unwrap().push(snapshot.clone());
        }
    }

    struct Rig {
        engine: InputEngine<FakeTapController>,
        taps: FakeTapController,
        published: Published,
        counters: Arc<ScrollCounters>,
    }

    fn rig(trusted: bool) -> Rig {
        let taps = FakeTapController::default();
        let published = Published::default();
        let counters = Arc::new(ScrollCounters::default());
        let mut engine = InputEngine::new(
            taps.clone(),
            trusted,
            TapDemand::ALL,
            Arc::clone(&counters),
            Box::new(published.clone()),
        );
        engine.start();
        Rig {
            engine,
            taps,
            published,
            counters,
        }
    }

    #[test]
    fn start_publishes_initial_state() {
        let r = rig(true);
        assert_eq!(
            r.published.all(),
            [EngineSnapshot {
                trusted: true,
                taps: TapSet::NONE,
                devices: vec![]
            }]
        );
    }

    #[test]
    fn plugging_in_a_mouse_turns_taps_on() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(r.published.last().taps, ALL_ON);
        assert_eq!(r.published.last().devices, vec![mouse(1)]);
    }

    #[test]
    fn unplugging_last_mouse_disables_taps() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine.apply_device_event(DeviceEvent::Removed { id: 1 });
        assert_eq!(
            r.published.last(),
            EngineSnapshot {
                trusted: true,
                taps: TapSet::NONE,
                devices: vec![]
            }
        );
    }

    #[test]
    fn excluded_devices_neither_enable_taps_nor_appear() {
        let mut r = rig(true);
        r.engine
            .apply_device_event(DeviceEvent::Added(apple_trackpad(2)));
        assert_eq!(
            r.engine.snapshot(),
            EngineSnapshot {
                trusted: true,
                taps: TapSet::NONE,
                devices: vec![]
            }
        );
    }

    #[test]
    fn revoking_trust_disables_all_taps() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine.set_trusted(false);
        assert_eq!(r.published.last().taps, TapSet::NONE);
        assert!(!r.published.last().trusted);
    }

    #[test]
    fn regranting_trust_enables_taps_again() {
        let mut r = rig(false);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(r.engine.snapshot().taps, TapSet::NONE);
        r.engine.set_trusted(true);
        assert_eq!(r.published.last().taps, ALL_ON);
    }

    #[test]
    fn publishes_only_when_something_changed() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine.set_trusted(true);
        r.engine.watchdog();
        assert_eq!(r.published.all().len(), 2, "initial + mouse added");
    }

    #[test]
    fn timeout_reenables_the_tap() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.taps.os_disables(TapKind::Scroll);
        r.engine
            .on_tap_disabled(TapKind::Scroll, DisabledReason::Timeout);
        assert!(r.engine.snapshot().taps.scroll);
    }

    #[test]
    fn user_input_disable_is_left_alone() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.taps.os_disables(TapKind::Scroll);
        r.engine
            .on_tap_disabled(TapKind::Scroll, DisabledReason::UserInput);
        assert!(!r.engine.snapshot().taps.scroll);
    }

    #[test]
    fn watchdog_revives_a_silently_disabled_tap_and_publishes() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.taps.os_disables(TapKind::Buttons);
        r.engine.watchdog();
        assert_eq!(r.published.last().taps, ALL_ON);
    }

    #[test]
    fn tap_creation_failure_shows_as_off_until_watchdog_succeeds() {
        let mut r = rig(true);
        r.taps.fail(TapKind::Scroll);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert!(!r.published.last().taps.scroll);

        r.taps.succeed(TapKind::Scroll);
        r.engine.watchdog();
        assert!(r.published.last().taps.scroll);
    }

    #[test]
    fn scroll_events_are_counted_by_class() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        let wheel = ScrollFields {
            delta_axis1: -1,
            sender_id: 10,
            ..Default::default()
        };
        let trackpad = ScrollFields {
            is_continuous: 1,
            ..wheel
        };
        let own = ScrollFields {
            user_data: MOUSETUNE_EVENT_TAG,
            ..wheel
        };

        r.engine.on_scroll(&wheel);
        r.engine.on_scroll(&wheel);
        r.engine.on_scroll(&trackpad);
        r.engine.on_scroll(&own);

        assert_eq!(
            r.counters.stats(),
            ScrollStats {
                mouse_wheel: 2,
                passed_through: 2
            }
        );
    }

    #[test]
    fn counters_saturate_instead_of_wrapping() {
        let counters = ScrollCounters::default();
        counters.mouse_wheel.store(u32::MAX, Ordering::Relaxed);
        counters.record(ScrollClass::MouseWheel);
        counters.record(ScrollClass::PassThrough(PassReason::Continuous));
        assert_eq!(
            counters.stats(),
            ScrollStats {
                mouse_wheel: u32::MAX,
                passed_through: 1
            }
        );
    }
}

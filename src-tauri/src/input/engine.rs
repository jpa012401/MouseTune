//! Owns all input state on the input thread and decides what the taps do
//! (spec §4.4). OS access goes through `TapController`; results go out
//! through `EngineObserver`.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use serde::Serialize;
use specta::Type;

use crate::{
    actions::{ActionCounters, ActionSink, Job},
    input::{
        devices::{DeviceEvent, DeviceInfo, DeviceRegistry},
        filter::{ScrollClass, ScrollFields, classify_scroll, is_foreign_button_down},
        guard::{DisabledAction, DisabledReason, on_tap_disabled},
        remaps::CompiledRemaps,
        router::{ButtonEvent, ButtonRouter, ClickTimers, Decision, Outcome, TimerKind, Verdict},
        taps::{TapController, TapDemand, TapKind, TapManager, TapSet, desired_taps},
    },
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

/// What tap callbacks and click timers call into. Implemented by `InputEngine`.
pub trait TapHandler {
    fn on_scroll(&mut self, fields: &ScrollFields);
    fn on_button(&mut self, event: &ButtonEvent) -> Verdict;
    fn on_click_timer(&mut self, kind: TimerKind);
    fn on_tap_disabled(&mut self, kind: TapKind, reason: DisabledReason);
}

/// What the engine needs to route buttons: the compiled bindings, the click
/// timers it arms, and where fired actions go.
pub struct ButtonSetup {
    pub remaps: CompiledRemaps,
    pub timers: Box<dyn ClickTimers>,
    pub sink: Box<dyn ActionSink>,
    pub counters: Arc<ActionCounters>,
}

pub struct InputEngine<C: TapController> {
    devices: DeviceRegistry,
    taps: TapManager<C>,
    trusted: bool,
    demand: TapDemand,
    counters: Arc<ScrollCounters>,
    router: ButtonRouter,
    timers: Box<dyn ClickTimers>,
    sink: Box<dyn ActionSink>,
    actions: Arc<ActionCounters>,
    observer: Box<dyn EngineObserver>,
    last_published: Option<EngineSnapshot>,
}

impl<C: TapController> InputEngine<C> {
    pub fn new(
        controller: C,
        trusted: bool,
        demand: TapDemand,
        counters: Arc<ScrollCounters>,
        buttons: ButtonSetup,
        observer: Box<dyn EngineObserver>,
    ) -> Self {
        Self {
            devices: DeviceRegistry::default(),
            taps: TapManager::new(controller),
            trusted,
            demand,
            counters,
            router: ButtonRouter::new(buttons.remaps),
            timers: buttons.timers,
            sink: buttons.sink,
            actions: buttons.counters,
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

    pub fn set_demand(&mut self, demand: TapDemand) {
        self.demand = demand;
        self.refresh();
    }

    pub fn set_remaps(&mut self, remaps: CompiledRemaps) {
        let decision = self.router.set_remaps(remaps);
        self.apply(decision);
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

    /// Every trust, demand or device change ends the click cycle. While the
    /// button tap stays on, the router keeps its swallowed bits, so a press
    /// in flight still has its up eaten. Once it is off, ups go to the OS
    /// unseen, so the bits are dropped (design §2, the up/down invariant).
    fn refresh(&mut self) {
        let reset = self.router.reset();
        self.apply(reset);
        let desired = desired_taps(self.trusted, &self.devices, self.demand);
        self.taps.reconcile(desired);
        if !self.taps.enabled().buttons {
            self.router.forget_swallowed();
        }
        self.publish();
    }

    /// Carries out a router decision. Hot path: no allocation, no locks, no
    /// logging.
    fn apply(&mut self, decision: Decision) -> Verdict {
        for outcome in decision.outcomes.iter() {
            match outcome {
                Outcome::Arm(kind, after) => self.timers.arm(kind, after),
                Outcome::Disarm(kind) => self.timers.disarm(kind),
                Outcome::Fire(action) => self.submit(Job::Perform(action)),
                Outcome::Click { button } => self.submit(Job::Click { button }),
            }
        }
        decision.verdict
    }

    fn submit(&self, job: Job) {
        if !self.sink.submit(job) {
            self.actions.record_dropped();
        }
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

    fn on_button(&mut self, event: &ButtonEvent) -> Verdict {
        let decision = if is_foreign_button_down(event, &self.devices) {
            self.router.on_foreign_down(event.button)
        } else {
            self.router.on_button(event)
        };
        self.apply(decision)
    }

    fn on_click_timer(&mut self, kind: TimerKind) {
        let decision = self.router.on_timer(kind);
        self.apply(decision);
    }

    /// A disabled button tap missed button events, so any cycle in flight is
    /// stale and any swallowed bit may belong to an up that already reached
    /// the OS, whatever the reason. Other taps miss no button events.
    fn on_tap_disabled(&mut self, kind: TapKind, reason: DisabledReason) {
        if kind == TapKind::Buttons {
            let reset = self.router.reset();
            self.apply(reset);
            self.router.forget_swallowed();
        }
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
        actions::{CompiledAction, Job},
        config::schema::{Config, NavDirection, SymbolicHotkeyId},
        input::{
            filter::{MOUSETUNE_EVENT_TAG, PassReason},
            remaps::compile,
            router::ButtonKind,
        },
        test_support::{FakeSink, FakeTapController, FakeTimers, apple_trackpad, mouse},
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
        timers: FakeTimers,
        sink: FakeSink,
        actions: Arc<ActionCounters>,
    }

    fn rig(trusted: bool) -> Rig {
        let taps = FakeTapController::default();
        let published = Published::default();
        let counters = Arc::new(ScrollCounters::default());
        let timers = FakeTimers::default();
        let sink = FakeSink::default();
        let actions = Arc::new(ActionCounters::default());
        let mut engine = InputEngine::new(
            taps.clone(),
            trusted,
            TapDemand::ALL,
            Arc::clone(&counters),
            ButtonSetup {
                remaps: compile(&Config::default()),
                timers: Box::new(timers.clone()),
                sink: Box::new(sink.clone()),
                counters: Arc::clone(&actions),
            },
            Box::new(published.clone()),
        );
        engine.start();
        Rig {
            engine,
            taps,
            published,
            counters,
            timers,
            sink,
            actions,
        }
    }

    fn button(r: &mut Rig, kind: ButtonKind, button: u8, sender_id: u64) -> Verdict {
        r.engine.on_button(&ButtonEvent {
            kind,
            button,
            sender_id,
            user_data: 0,
            mouse_subtype: 0,
        })
    }
    const MC: Job = Job::Perform(CompiledAction::SymbolicHotkey(
        SymbolicHotkeyId::MissionControl,
    ));
    const DESKTOP: Job = Job::Perform(CompiledAction::SymbolicHotkey(
        SymbolicHotkeyId::ShowDesktop,
    ));

    #[test]
    fn a_middle_click_swallows_and_submits_mission_control() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Swallow);
        assert_eq!(*r.sink.jobs.lock().unwrap(), vec![MC]);
        assert_eq!(
            r.timers.calls(),
            [
                "arm Expiry 260ms",
                "arm Hold 250ms",
                "disarm Hold",
                "disarm Expiry"
            ]
        );
    }

    #[test]
    fn the_hold_timer_submits_show_desktop() {
        let mut r = rig(true);
        button(&mut r, ButtonKind::Down, 2, 10);
        r.engine.on_click_timer(TimerKind::Hold);
        assert_eq!(*r.sink.jobs.lock().unwrap(), vec![DESKTOP]);
    }

    #[test]
    fn a_button_from_an_unknown_sender_is_still_routed() {
        let mut r = rig(true); // no devices registered at all
        assert_eq!(
            button(&mut r, ButtonKind::Down, 3, 999_999),
            Verdict::Swallow
        );
        assert_eq!(
            *r.sink.jobs.lock().unwrap(),
            vec![Job::Perform(CompiledAction::Navigate(NavDirection::Back))]
        );
    }

    #[test]
    fn buttons_from_an_excluded_device_pass_untouched() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine
            .apply_device_event(DeviceEvent::Added(apple_trackpad(2)));
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 20), Verdict::Pass);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 20), Verdict::Pass);
        assert!(r.sink.jobs.lock().unwrap().is_empty());
        assert!(r.timers.calls().is_empty());
    }

    #[test]
    fn a_tablet_button_down_passes_untouched() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        let tablet_down = ButtonEvent {
            kind: ButtonKind::Down,
            button: 2,
            sender_id: 10,
            user_data: 0,
            mouse_subtype: 1,
        };
        assert_eq!(r.engine.on_button(&tablet_down), Verdict::Pass);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
        assert!(r.sink.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn a_full_queue_counts_a_drop_and_still_swallows() {
        let mut r = rig(true);
        r.sink.full.store(true, Ordering::SeqCst);
        assert_eq!(button(&mut r, ButtonKind::Down, 3, 10), Verdict::Swallow);
        assert_eq!(r.actions.stats().dropped, 1);
    }

    #[test]
    fn config_change_mid_press_still_swallows_the_up() {
        let mut r = rig(true);
        button(&mut r, ButtonKind::Down, 2, 10);
        r.engine.set_remaps(CompiledRemaps::empty());
        r.engine.on_click_timer(TimerKind::Hold); // cancelled: must not fire
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Swallow);
        assert!(r.sink.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn a_kill_switch_mid_press_lets_the_up_pass() {
        // The button tap is off, so the up (and any later down) may have
        // reached the app; swallowing it could leave a button stuck there.
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        r.engine.set_demand(TapDemand {
            buttons: false,
            scroll: true,
            flags: true,
        });
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
        assert!(
            r.timers
                .calls()
                .ends_with(&["disarm Hold".to_string(), "disarm Expiry".to_string()])
        );
        // The cycle was reset, so the release fires nothing.
        assert!(r.sink.jobs.lock().unwrap().is_empty());
    }

    #[test]
    fn a_refresh_that_keeps_the_button_tap_on_still_swallows_the_up() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        r.engine.set_demand(TapDemand {
            buttons: true,
            scroll: false,
            flags: true,
        });
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Swallow);
    }

    #[test]
    fn unplugging_the_only_mouse_mid_press_lets_a_later_up_pass() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        r.engine.apply_device_event(DeviceEvent::Removed { id: 1 });
        assert_eq!(r.engine.snapshot().taps, TapSet::NONE);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
    }

    #[test]
    fn revoking_trust_mid_press_lets_a_later_up_pass() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        r.engine.set_trusted(false);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
    }

    /// Down 1 swallowed; the button tap times out; up 1 and down 2 reach the
    /// OS, so the app now holds the middle button; the tap comes back and
    /// up 2 arrives. It must reach the app, or the app's button sticks.
    #[test]
    fn an_up_after_a_button_tap_gap_reaches_the_app() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        r.taps.os_disables(TapKind::Buttons);
        r.engine
            .on_tap_disabled(TapKind::Buttons, DisabledReason::Timeout);
        // up 1 and down 2 happen here, unseen.
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
        // The next full press is routed as usual.
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Swallow);
    }

    #[test]
    fn a_passing_down_after_a_lost_up_leaves_its_up_alone() {
        let mut r = rig(true);
        button(&mut r, ButtonKind::Down, 2, 10); // its up went straight to the OS
        r.engine.set_remaps(CompiledRemaps::empty());
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Pass);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
    }

    #[test]
    fn a_scroll_tap_timeout_mid_press_keeps_the_hold() {
        let mut r = rig(true);
        button(&mut r, ButtonKind::Down, 2, 10);
        r.engine
            .on_tap_disabled(TapKind::Scroll, DisabledReason::Timeout);
        r.engine.on_click_timer(TimerKind::Hold);
        assert_eq!(*r.sink.jobs.lock().unwrap(), vec![DESKTOP]);
        // The button tap saw every event, so the up is still ours.
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Swallow);
    }

    #[test]
    fn a_tap_timeout_mid_press_cancels_the_hold() {
        let mut r = rig(true);
        button(&mut r, ButtonKind::Down, 2, 10);
        r.engine
            .on_tap_disabled(TapKind::Buttons, DisabledReason::Timeout);
        r.engine.on_click_timer(TimerKind::Hold);
        assert!(r.sink.jobs.lock().unwrap().is_empty());
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
    }

    #[test]
    fn a_user_input_disable_mid_press_cancels_the_hold() {
        let mut r = rig(true);
        button(&mut r, ButtonKind::Down, 2, 10);
        r.engine
            .on_tap_disabled(TapKind::Buttons, DisabledReason::UserInput);
        r.engine.on_click_timer(TimerKind::Hold);
        assert!(r.sink.jobs.lock().unwrap().is_empty());
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Pass);
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
    fn turning_scroll_off_in_settings_disables_the_scroll_tap() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));

        r.engine.set_demand(TapDemand {
            buttons: true,
            scroll: false,
            flags: true,
        });
        assert_eq!(
            r.published.last().taps,
            TapSet {
                buttons: true,
                scroll: false,
                flags: true
            }
        );

        r.engine.set_demand(TapDemand::ALL);
        assert_eq!(r.published.last().taps, ALL_ON);
    }

    #[test]
    fn everything_off_runs_no_taps() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine.set_demand(TapDemand {
            buttons: false,
            scroll: false,
            flags: false,
        });
        assert_eq!(r.published.last().taps, TapSet::NONE);
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

    #[test]
    fn timeout_on_a_wedged_tap_is_left_to_the_watchdog() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.taps.wedge(TapKind::Scroll);

        r.engine
            .on_tap_disabled(TapKind::Scroll, DisabledReason::Timeout);
        assert!(
            !r.engine.snapshot().taps.scroll,
            "callback path must not rebuild"
        );
        assert!(r.taps.revives().is_empty());

        r.engine.watchdog();
        assert!(r.published.last().taps.scroll);
    }
}

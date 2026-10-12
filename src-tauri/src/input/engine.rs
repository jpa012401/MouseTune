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
    actions::{ActionCounters, ActionSink, Job, Notice},
    config::schema::Scroll,
    input::{
        capture::{ButtonCapture, Clock},
        devices::{DeviceEvent, DeviceInfo, DeviceRegistry},
        filter::{
            MOUSETUNE_EVENT_TAG, ScrollClass, ScrollFields, classify_scroll, is_foreign_button_down,
        },
        guard::{DisabledAction, DisabledReason, on_tap_disabled},
        key_recorder::{KeyEvent, KeyRecorder},
        remaps::CompiledRemaps,
        router::{
            ButtonEvent, ButtonKind, ButtonRouter, ClickTimers, Decision, Outcome, TimerKind,
            Verdict,
        },
        scroll::{ScrollEngine, ScrollVerdict},
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
    fn on_scroll(&mut self, fields: &ScrollFields) -> ScrollVerdict;
    fn on_button(&mut self, event: &ButtonEvent) -> Verdict;
    fn on_click_timer(&mut self, kind: TimerKind);
    fn on_key(&mut self, event: &KeyEvent) -> Verdict;
    fn on_tap_disabled(&mut self, kind: TapKind, reason: DisabledReason);
    /// One display frame for the scroll animation.
    fn on_frame(&mut self);
}

/// What the engine needs to route buttons: the compiled bindings, the click
/// timers it arms, where fired actions go, and the clock capture mode's
/// deadline is checked against.
pub struct ButtonSetup {
    pub remaps: CompiledRemaps,
    pub timers: Box<dyn ClickTimers>,
    pub sink: Box<dyn ActionSink>,
    pub counters: Arc<ActionCounters>,
    pub clock: Box<dyn Clock>,
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
    capture: ButtonCapture,
    recorder: KeyRecorder,
    clock: Box<dyn Clock>,
    scroll: ScrollEngine,
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
        scroll: ScrollEngine,
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
            capture: ButtonCapture::default(),
            recorder: KeyRecorder::default(),
            clock: buttons.clock,
            scroll,
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

    pub fn set_scroll(&mut self, settings: Scroll) {
        self.scroll.set_settings(settings);
    }

    pub fn set_remaps(&mut self, remaps: CompiledRemaps) {
        let decision = self.router.set_remaps(remaps);
        self.apply(decision);
    }

    /// The next press of mouse button 3 or higher is reported as a
    /// `ButtonCaptured` notice instead of acting, within `CAPTURE_TIMEOUT`.
    pub fn start_capture(&mut self) {
        self.capture.start(self.clock.now());
    }

    pub fn stop_capture(&mut self) {
        self.capture.stop();
    }

    /// The next real key-down, with its modifiers, is reported as a
    /// `KeyCaptured` notice and swallowed, within `CAPTURE_TIMEOUT`. Runs the
    /// keys tap only while needed.
    ///
    /// Doesn't reset the router: a button press in flight carries on.
    pub fn start_key_capture(&mut self) {
        self.recorder.start(self.clock.now());
        self.update_taps();
    }

    pub fn stop_key_capture(&mut self) {
        self.recorder.stop();
        self.update_taps();
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
        // No app may be left mid-gesture once trust, demand or devices change (design §5).
        self.scroll.cancel();
        let reset = self.router.reset();
        self.apply(reset);
        self.update_taps();
    }

    /// Brings the taps in line with the current state, without touching the
    /// click cycle, then publishes.
    fn update_taps(&mut self) {
        self.taps.reconcile(self.desired());
        if !self.taps.enabled().buttons {
            self.router.forget_swallowed();
        }
        // Likewise for keys: with the keys tap off, a held key's up goes to
        // the OS unseen.
        if !self.taps.enabled().keys {
            self.recorder.forget_swallowed();
        }
        self.publish();
    }

    /// The config's demand, plus the keys tap while the recorder needs it.
    fn desired(&self) -> TapSet {
        let demand = TapDemand {
            keys: self.recorder.wants_tap(),
            ..self.demand
        };
        desired_taps(self.trusted, &self.devices, demand)
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
                Outcome::DragBegin { .. }
                | Outcome::DragMove { .. }
                | Outcome::DragEnd
                | Outcome::HoldThrough { .. }
                | Outcome::WantsMotion(_)
                | Outcome::ModifierReleased => {} // Task 5 routes these.
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
    /// Hot path: no allocation, no locks, no logging, nothing posted.
    fn on_scroll(&mut self, fields: &ScrollFields) -> ScrollVerdict {
        let class = classify_scroll(fields, &self.devices);
        self.counters.record(class);
        match class {
            ScrollClass::MouseWheel => self.scroll.on_wheel(fields, self.clock.now()),
            ScrollClass::PassThrough(_) => ScrollVerdict::Pass,
        }
    }

    fn on_frame(&mut self) {
        self.scroll.on_frame(self.clock.now());
    }

    fn on_button(&mut self, event: &ButtonEvent) -> Verdict {
        // The clock is read only while capture is armed.
        let decision = if is_foreign_button_down(event, &self.devices) {
            self.router.on_foreign_down(event.button)
        } else if event.kind == ButtonKind::Down
            && event.user_data != MOUSETUNE_EVENT_TAG
            && self.capture.is_armed()
            && self.capture.take(event.button, self.clock.now())
        {
            self.submit(Job::Notice(Notice::ButtonCaptured {
                button: event.button + 1,
            }));
            self.router.on_captured_down(event.button)
        } else {
            self.router.on_button(event)
        };
        self.apply(decision)
    }

    fn on_click_timer(&mut self, kind: TimerKind) {
        let decision = self.router.on_timer(kind);
        self.apply(decision);
    }

    /// Hot path: no allocation, no locks, no logging.
    fn on_key(&mut self, event: &KeyEvent) -> Verdict {
        let had_tap = self.recorder.wants_tap();
        let (verdict, notice) = self.recorder.on_key(event, self.clock.now());
        if let Some(notice) = notice {
            self.submit(Job::Notice(notice));
        }
        if had_tap && !self.recorder.wants_tap() {
            // Only the keys tap is touched: reconciling every tap could create
            // or log for another one inside this callout. Disabling never
            // discards a port, so it is safe in the tap's own callout. No
            // publish here: it allocates; the next watchdog tick publishes.
            self.taps.release(TapKind::Keys);
        }
        verdict
    }

    /// A disabled button tap missed button events, so any cycle in flight is
    /// stale and any swallowed bit may belong to an up that already reached
    /// the OS, whatever the reason. Other taps miss no button events. A
    /// disabled keys tap may likewise have let a swallowed key's up through.
    fn on_tap_disabled(&mut self, kind: TapKind, reason: DisabledReason) {
        match kind {
            TapKind::Buttons => {
                let reset = self.router.reset();
                self.apply(reset);
                self.router.forget_swallowed();
            }
            TapKind::Keys => {
                self.recorder.forget_swallowed();
                if !self.recorder.wants_tap() {
                    // Nothing left to record or swallow: don't bring back a
                    // tap that would only pass everything.
                    self.taps.release(TapKind::Keys);
                    return;
                }
            }
            TapKind::Scroll | TapKind::Flags => {}
        }
        if on_tap_disabled(reason) == DisabledAction::Reenable {
            self.taps.reenable(kind);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Mutex, time::Duration};

    use super::*;
    use crate::{
        actions::{CompiledAction, RawKeyCapture, keys::ModifierMask},
        config::schema::{Config, NavDirection, Scroll, Smoothness, SymbolicHotkeyId},
        input::{
            filter::PassReason,
            key_recorder::{KeyEvent, KeyKind},
            remaps::compile,
            scroll::{ScrollEngine, ScrollVerdict},
        },
        test_support::{
            FakeClock, FakeFrames, FakeScrollPoster, FakeSink, FakeTapController, FakeTimers,
            apple_trackpad, mouse, phase4_buttons,
        },
    };

    const ALL_ON: TapSet = TapSet {
        buttons: true,
        scroll: true,
        flags: true,
        keys: false,
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
        clock: FakeClock,
        frames: FakeFrames,
        posted: FakeScrollPoster,
    }

    fn rig(trusted: bool) -> Rig {
        let taps = FakeTapController::default();
        let published = Published::default();
        let counters = Arc::new(ScrollCounters::default());
        let timers = FakeTimers::default();
        let sink = FakeSink::default();
        let actions = Arc::new(ActionCounters::default());
        let clock = FakeClock::new();
        let frames = FakeFrames::default();
        let posted = FakeScrollPoster::default();
        let mut engine = InputEngine::new(
            taps.clone(),
            trusted,
            TapDemand::ALL,
            Arc::clone(&counters),
            ButtonSetup {
                remaps: compile(&Config {
                    buttons: phase4_buttons(),
                    ..Config::default()
                }),
                timers: Box::new(timers.clone()),
                sink: Box::new(sink.clone()),
                counters: Arc::clone(&actions),
                clock: Box::new(clock.clone()),
            },
            ScrollEngine::new(
                Scroll::default(),
                Box::new(frames.clone()),
                Box::new(posted.clone()),
            ),
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
            clock,
            frames,
            posted,
        }
    }

    fn jobs(r: &Rig) -> Vec<Job> {
        r.sink.jobs.lock().unwrap().clone()
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
    const BACK: Job = Job::Perform(CompiledAction::Navigate(NavDirection::Back));
    const CAPTURED_3: Job = Job::Notice(Notice::ButtonCaptured { button: 3 });

    fn key_down(key_code: u16, flags: u64, ch: char) -> KeyEvent {
        KeyEvent {
            kind: KeyKind::Down,
            key_code,
            flags,
            chars: [ch as u16, 0, 0, 0],
            chars_len: 1,
            user_data: 0,
        }
    }

    fn key_up(key_code: u16) -> KeyEvent {
        KeyEvent {
            kind: KeyKind::Up,
            key_code,
            flags: 0,
            chars: [0; 4],
            chars_len: 0,
            user_data: 0,
        }
    }

    #[test]
    fn recording_turns_the_keys_tap_on_and_releasing_the_key_turns_it_off() {
        let mut r = rig(true);
        r.engine.start_key_capture();
        assert!(r.engine.snapshot().taps.keys);
        assert_eq!(
            r.engine.on_key(&key_down(0x01, 0x100000, 's')),
            Verdict::Swallow
        );
        assert!(
            r.engine.snapshot().taps.keys,
            "the recorded key is still down"
        );
        assert_eq!(r.engine.on_key(&key_up(0x01)), Verdict::Swallow);
        assert!(!r.engine.snapshot().taps.keys);
        assert_eq!(
            jobs(&r),
            vec![Job::Notice(Notice::KeyCaptured(RawKeyCapture {
                key_code: 0x01,
                modifiers: ModifierMask(0x100000),
                chars: [0x73, 0, 0, 0],
                chars_len: 1
            }))]
        );
    }

    #[test]
    fn ending_a_recording_in_the_key_callback_touches_no_other_tap() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine.start_key_capture();
        r.taps.os_disables(TapKind::Scroll);
        let before = r.taps.calls().len();
        r.engine.on_key(&key_down(0x01, 0, 's'));
        r.engine.on_key(&key_up(0x01));
        assert_eq!(r.taps.calls()[before..], [(TapKind::Keys, false)]);
        assert!(!r.engine.snapshot().taps.keys);
    }

    #[test]
    fn a_key_held_when_the_keys_tap_went_off_is_recorded_next_time() {
        let mut r = rig(true);
        r.engine.start_key_capture();
        r.engine.on_key(&key_down(0x01, 0, 's'));
        r.engine.set_trusted(false);
        r.engine.set_trusted(true);
        assert!(!r.engine.snapshot().taps.keys, "nothing left to record");
        r.engine.start_key_capture();
        assert_eq!(r.engine.on_key(&key_down(0x01, 0, 's')), Verdict::Swallow);
        assert_eq!(jobs(&r).len(), 2, "recorded twice: {:?}", jobs(&r));
        assert_eq!(r.engine.on_key(&key_up(0x02)), Verdict::Pass);
    }

    #[test]
    fn a_keys_tap_timeout_with_nothing_to_record_leaves_the_tap_off() {
        let mut r = rig(true);
        r.engine.start_key_capture();
        r.engine.on_key(&key_down(0x01, 0, 's'));
        r.taps.os_disables(TapKind::Keys);
        r.engine
            .on_tap_disabled(TapKind::Keys, DisabledReason::Timeout);
        assert!(!r.engine.snapshot().taps.keys);
        r.engine.watchdog();
        assert!(!r.engine.snapshot().taps.keys);
    }

    #[test]
    fn recording_needs_accessibility() {
        let mut r = rig(false);
        r.engine.start_key_capture();
        assert!(!r.engine.snapshot().taps.keys);
    }

    #[test]
    fn stop_key_capture_drops_the_keys_tap() {
        let mut r = rig(true);
        r.engine.start_key_capture();
        r.engine.stop_key_capture();
        assert!(!r.engine.snapshot().taps.keys);
    }

    #[test]
    fn a_keys_tap_timeout_forgets_swallowed_keys() {
        let mut r = rig(true);
        r.engine.start_key_capture();
        r.engine.on_key(&key_down(0x01, 0, 's'));
        r.engine
            .on_tap_disabled(TapKind::Keys, DisabledReason::Timeout);
        assert_eq!(r.engine.on_key(&key_up(0x01)), Verdict::Pass);
    }

    #[test]
    fn starting_a_recording_doesnt_cancel_a_button_press_in_flight() {
        let mut r = rig(true);
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        button(&mut r, ButtonKind::Down, 2, 10);
        r.engine.start_key_capture();
        r.engine.on_click_timer(TimerKind::Hold);
        assert_eq!(jobs(&r), vec![DESKTOP]);
    }

    #[test]
    fn a_captured_press_is_swallowed_reported_once_and_ends_capture() {
        let mut r = rig(true);
        r.engine.start_capture();
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        assert_eq!(button(&mut r, ButtonKind::Up, 2, 10), Verdict::Swallow);
        assert_eq!(jobs(&r), vec![CAPTURED_3]);
        button(&mut r, ButtonKind::Down, 2, 10);
        button(&mut r, ButtonKind::Up, 2, 10);
        assert_eq!(jobs(&r).last(), Some(&MC));
    }

    #[test]
    fn after_the_deadline_a_press_routes_normally() {
        let mut r = rig(true);
        r.engine.start_capture();
        r.clock.advance(Duration::from_millis(10_001));
        assert_eq!(button(&mut r, ButtonKind::Down, 3, 10), Verdict::Swallow);
        assert_eq!(jobs(&r), vec![BACK]);
    }

    #[test]
    fn foreign_and_tagged_presses_are_never_captured() {
        let mut r = rig(true);
        r.engine
            .apply_device_event(DeviceEvent::Added(apple_trackpad(2)));
        r.engine.start_capture();
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 20), Verdict::Pass);
        let tagged = ButtonEvent {
            kind: ButtonKind::Down,
            button: 2,
            sender_id: 10,
            user_data: MOUSETUNE_EVENT_TAG,
            mouse_subtype: 0,
        };
        assert_eq!(r.engine.on_button(&tagged), Verdict::Pass);
        assert!(jobs(&r).is_empty());
        button(&mut r, ButtonKind::Down, 2, 10);
        assert_eq!(jobs(&r), vec![CAPTURED_3]);
    }

    #[test]
    fn stop_capture_returns_the_button_to_its_remap() {
        let mut r = rig(true);
        r.engine.start_capture();
        r.engine.stop_capture();
        button(&mut r, ButtonKind::Down, 3, 10);
        assert_eq!(jobs(&r), vec![BACK]);
    }

    #[test]
    fn a_full_queue_drops_the_capture_result_but_still_swallows() {
        let mut r = rig(true);
        r.sink.full.store(true, Ordering::SeqCst);
        r.engine.start_capture();
        assert_eq!(button(&mut r, ButtonKind::Down, 2, 10), Verdict::Swallow);
        assert_eq!(r.actions.stats().dropped, 1);
    }

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
            keys: false,
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
            keys: false,
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
            keys: false,
        });
        assert_eq!(
            r.published.last().taps,
            TapSet {
                buttons: true,
                scroll: false,
                flags: true,
                keys: false
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
            keys: false,
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

    fn low() -> Scroll {
        Scroll {
            smoothness: Smoothness::Low,
            ..Scroll::default()
        }
    }

    fn wheel_from(sender_id: u64) -> ScrollFields {
        ScrollFields {
            delta_axis1: 1,
            sender_id,
            ..Default::default()
        }
    }

    /// A tracked mouse with a wheel tick mid-animation: "Began" posted.
    fn scrolling(r: &mut Rig) {
        r.engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        r.engine.set_scroll(low());
        assert_eq!(r.engine.on_scroll(&wheel_from(10)), ScrollVerdict::Swallow);
        r.engine.on_frame();
        assert_eq!(r.posted.log(), ["Began"]);
    }

    #[test]
    fn frames_advance_the_animation_on_the_engine_clock() {
        let mut r = rig(true);
        scrolling(&mut r);
        r.clock.advance(Duration::from_millis(90));
        r.engine.on_frame();
        assert_eq!(r.posted.log(), ["Began", "Changed dy 90", "Ended"]);
        assert_eq!(r.frames.calls().last(), Some(&"stop"));
    }

    #[test]
    fn a_trackpad_scroll_passes_untouched_while_an_animation_runs() {
        let mut r = rig(true);
        r.engine
            .apply_device_event(DeviceEvent::Added(apple_trackpad(2)));
        scrolling(&mut r);
        let trackpad = ScrollFields {
            is_continuous: 1,
            ..wheel_from(20)
        };
        assert_eq!(r.engine.on_scroll(&trackpad), ScrollVerdict::Pass);
        let tablet = ScrollFields {
            tablet_device_id: 3,
            ..wheel_from(10)
        };
        assert_eq!(r.engine.on_scroll(&tablet), ScrollVerdict::Pass);
        assert_eq!(r.posted.log(), ["Began"], "the animation is undisturbed");
        assert_eq!(r.frames.calls(), ["request"]);
    }

    #[test]
    fn turning_scrolling_off_mid_animation_closes_the_gesture() {
        let mut r = rig(true);
        scrolling(&mut r);
        r.engine.set_demand(TapDemand {
            scroll: false,
            ..TapDemand::ALL
        });
        assert_eq!(r.posted.log(), ["Began", "Ended"]);
        assert_eq!(r.frames.calls(), ["request", "stop"]);
    }

    #[test]
    fn unplugging_the_mouse_mid_animation_closes_the_gesture() {
        let mut r = rig(true);
        scrolling(&mut r);
        r.engine.apply_device_event(DeviceEvent::Removed { id: 1 });
        assert_eq!(r.posted.log(), ["Began", "Ended"]);
    }

    #[test]
    fn losing_accessibility_mid_animation_closes_the_gesture() {
        let mut r = rig(true);
        scrolling(&mut r);
        r.engine.set_trusted(false);
        assert_eq!(r.posted.log(), ["Began", "Ended"]);
    }

    #[test]
    fn a_scroll_tap_timeout_keeps_the_animation_going() {
        let mut r = rig(true);
        scrolling(&mut r);
        r.engine
            .on_tap_disabled(TapKind::Scroll, DisabledReason::Timeout);
        r.clock.advance(Duration::from_millis(90));
        r.engine.on_frame();
        assert_eq!(r.posted.log(), ["Began", "Changed dy 90", "Ended"]);
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

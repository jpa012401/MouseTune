//! The `mt-input` thread (spec §2, §4.4): a CFRunLoop that owns the input
//! engine, the event taps, the HID device monitor and the watchdog timer.
//! Other threads talk to it only through `InputHandle`. The run-loop code
//! itself lives in `platform::input_loop`.

use std::{
    sync::{Arc, Mutex, PoisonError, atomic::AtomicBool},
    thread::JoinHandle,
};

use arc_swap::ArcSwap;
use crossbeam_channel::Sender;

use crate::{
    actions::{ActionCounters, ActionSink},
    config::{
        schema::{Config, Scroll, tap_demand},
        store::ConfigObserver,
    },
    input::{
        engine::{EngineObserver, EngineSnapshot, InputEngine, ScrollCounters},
        remaps::{CompiledRemaps, OutputSupport, compile_with},
        taps::{TapController, TapDemand},
    },
    permission_monitor::PermissionObserver,
    platform::input_loop,
};

/// Messages from other threads to `mt-input`.
#[derive(Debug, PartialEq, Eq)]
pub enum Control {
    SetTrusted(bool),
    SetDemand(TapDemand),
    /// Boxed: the compiled tables are large, and every message pays for the
    /// biggest variant.
    SetRemaps(Box<CompiledRemaps>),
    /// New scroll settings; closes any gesture in flight.
    SetScroll(Scroll),
    /// Report the next press of button 3 or higher instead of acting on it.
    StartCapture,
    StopCapture,
    /// Record the next key-down (with modifiers) instead of letting it through.
    StartKeyCapture,
    StopKeyCapture,
    Shutdown,
}

/// Whether the run loop keeps going after a control message.
#[derive(Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

/// Nudges `mt-input` to drain its control channel.
pub trait Waker: Send + Sync {
    fn wake(&self);
}

/// State other threads read without touching `mt-input`.
#[derive(Default)]
pub struct InputShared {
    pub snapshot: ArcSwap<EngineSnapshot>,
    pub counters: Arc<ScrollCounters>,
    pub degraded: Arc<AtomicBool>,
    /// Bumped by the engine (dropped jobs) and by `mt-exec` (performed, failed).
    pub actions: Arc<ActionCounters>,
}

pub struct InputHandle {
    tx: Sender<Control>,
    waker: Arc<dyn Waker>,
    shared: Arc<InputShared>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl InputHandle {
    pub fn new(
        tx: Sender<Control>,
        waker: Arc<dyn Waker>,
        shared: Arc<InputShared>,
        join: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            tx,
            waker,
            shared,
            join: Mutex::new(join),
        }
    }

    pub fn send(&self, msg: Control) {
        if self.tx.send(msg).is_ok() {
            self.waker.wake();
        }
    }

    pub fn shared(&self) -> &InputShared {
        &self.shared
    }

    /// Stops `mt-input` and waits for it to tear down its taps. Idempotent.
    pub fn shutdown(&self) {
        self.send(Control::Shutdown);
        let join = self
            .join
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(join) = join
            && join.join().is_err()
        {
            tracing::error!("input thread panicked");
        }
    }
}

/// Tells the input engine when Accessibility is granted or revoked.
pub struct PermissionBridge(pub Arc<InputHandle>);

impl PermissionObserver for PermissionBridge {
    fn permission_changed(&self, trusted: bool) {
        self.0.send(Control::SetTrusted(trusted));
    }
}

/// Tells the input engine which taps the saved settings want.
pub struct InputConfigBridge {
    pub handle: Arc<InputHandle>,
    pub support: OutputSupport,
}

impl ConfigObserver for InputConfigBridge {
    fn config_changed(&self, config: &Config) {
        self.handle.send(Control::SetDemand(tap_demand(config)));
        self.handle.send(Control::SetRemaps(Box::new(compile_with(
            config,
            self.support,
        ))));
        self.handle.send(Control::SetScroll(config.scroll));
    }
}

pub fn handle_control<C: TapController>(engine: &mut InputEngine<C>, msg: Control) -> Flow {
    match msg {
        Control::SetTrusted(trusted) => {
            engine.set_trusted(trusted);
            Flow::Continue
        }
        Control::SetDemand(demand) => {
            engine.set_demand(demand);
            Flow::Continue
        }
        Control::SetRemaps(remaps) => {
            engine.set_remaps(*remaps);
            Flow::Continue
        }
        Control::SetScroll(settings) => {
            engine.set_scroll(settings);
            Flow::Continue
        }
        Control::StartCapture => {
            engine.start_capture();
            Flow::Continue
        }
        Control::StopCapture => {
            engine.stop_capture();
            Flow::Continue
        }
        Control::StartKeyCapture => {
            engine.start_key_capture();
            Flow::Continue
        }
        Control::StopKeyCapture => {
            engine.stop_key_capture();
            Flow::Continue
        }
        Control::Shutdown => Flow::Stop,
    }
}

/// Publishes each engine snapshot to `InputShared` (for `get_status`), then
/// to the UI observer.
struct StoreSnapshot {
    shared: Arc<InputShared>,
    ui: Box<dyn EngineObserver + Send>,
}

impl EngineObserver for StoreSnapshot {
    fn snapshot_changed(&self, snapshot: &EngineSnapshot) {
        self.shared.snapshot.store(Arc::new(snapshot.clone()));
        self.ui.snapshot_changed(snapshot);
    }
}

/// Starts `mt-input` and returns once its run loop is ready for messages.
/// `remaps` and `scroll` are the saved settings at startup; `sink` is where fired
/// actions go (the only sender of `mt-exec`'s queue).
pub fn spawn(
    trusted: bool,
    demand: TapDemand,
    remaps: CompiledRemaps,
    scroll: Scroll,
    sink: Box<dyn ActionSink + Send>,
    ui: Box<dyn EngineObserver + Send>,
) -> Arc<InputHandle> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let shared = Arc::new(InputShared::default());
    let observer = Box::new(StoreSnapshot {
        shared: Arc::clone(&shared),
        ui,
    });
    let setup = input_loop::StartSetup {
        trusted,
        demand,
        remaps,
        scroll,
        sink,
        observer,
    };
    let (join, waker) = input_loop::start(setup, rx, Arc::clone(&shared));
    Arc::new(InputHandle::new(tx, waker, shared, Some(join)))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::input::remaps::compile;
    use crate::{
        actions::{CompiledAction, Job, Notice},
        config::schema::{General, NavDirection, Scroll},
        input::{
            devices::DeviceEvent,
            engine::{ButtonSetup, EngineObserver, InputEngine, TapHandler},
            filter::ScrollFields,
            router::{ButtonEvent, ButtonKind, Verdict},
            scroll::{ScrollEngine, ScrollVerdict},
            taps::{TapDemand, TapSet},
        },
        test_support::{
            FakeClock, FakeFrames, FakeScrollPoster, FakeSink, FakeTapController, FakeTimers, mouse,
        },
    };

    #[derive(Default)]
    struct CountingWaker(AtomicUsize);
    impl Waker for CountingWaker {
        fn wake(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Ignore;
    impl EngineObserver for Ignore {
        fn snapshot_changed(&self, _: &EngineSnapshot) {}
    }

    fn engine(trusted: bool) -> (InputEngine<FakeTapController>, FakeTapController) {
        engine_with(trusted, FakeSink::default())
    }

    fn engine_with(
        trusted: bool,
        sink: FakeSink,
    ) -> (InputEngine<FakeTapController>, FakeTapController) {
        let taps = FakeTapController::default();
        let mut engine = InputEngine::new(
            taps.clone(),
            trusted,
            TapDemand::ALL,
            Arc::default(),
            ButtonSetup {
                remaps: compile(&Config::default()),
                timers: Box::new(FakeTimers::default()),
                sink: Box::new(sink),
                counters: Arc::default(),
                clock: Box::new(FakeClock::new()),
            },
            ScrollEngine::new(
                Scroll::default(),
                Box::new(FakeFrames::default()),
                Box::new(FakeScrollPoster::default()),
            ),
            Box::new(Ignore),
        );
        engine.start();
        engine.apply_device_event(DeviceEvent::Added(mouse(1)));
        (engine, taps)
    }

    #[test]
    fn set_trusted_reaches_the_engine_and_continues() {
        let (mut engine, _) = engine(true);
        assert_eq!(
            handle_control(&mut engine, Control::SetTrusted(false)),
            Flow::Continue
        );
        assert_eq!(engine.snapshot().taps, TapSet::NONE);
    }

    #[test]
    fn set_demand_reaches_the_engine_and_continues() {
        let (mut engine, _) = engine(true);
        let off = TapDemand {
            buttons: false,
            scroll: false,
            flags: false,
            keys: false,
        };
        assert_eq!(
            handle_control(&mut engine, Control::SetDemand(off)),
            Flow::Continue
        );
        assert_eq!(engine.snapshot().taps, TapSet::NONE);
    }

    #[test]
    fn config_bridge_sends_the_demand_and_wakes_the_input_thread() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let waker = Arc::new(CountingWaker::default());
        let handle = Arc::new(InputHandle::new(tx, waker.clone(), Arc::default(), None));
        let config = Config {
            version: 1,
            general: General {
                buttons_enabled: true,
                scroll_enabled: false,
                freeze_pointer_during_drag: true,
            },
            ..Config::default()
        };

        InputConfigBridge {
            handle,
            support: OutputSupport::NONE,
        }
        .config_changed(&config);

        assert_eq!(
            rx.try_recv(),
            Ok(Control::SetDemand(TapDemand {
                buttons: true,
                scroll: false,
                flags: true,
                keys: false
            }))
        );
        assert_eq!(
            rx.try_recv(),
            Ok(Control::SetRemaps(Box::new(compile_with(
                &config,
                OutputSupport::NONE
            ))))
        );
        assert_eq!(rx.try_recv(), Ok(Control::SetScroll(config.scroll)));
        assert_eq!(waker.0.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn set_scroll_reaches_the_engine_and_continues() {
        let (mut engine, _) = engine(true);
        let off = Scroll {
            smooth: false,
            invert_vertical: true,
            ..Scroll::default()
        };
        assert_eq!(
            handle_control(&mut engine, Control::SetScroll(off)),
            Flow::Continue
        );
        let wheel = ScrollFields {
            delta_axis1: 1,
            sender_id: 10,
            ..Default::default()
        };
        assert_eq!(
            engine.on_scroll(&wheel),
            ScrollVerdict::Invert {
                vertical: true,
                horizontal: false
            }
        );
    }

    #[test]
    fn set_remaps_reaches_the_engine_and_continues() {
        let (mut engine, _) = engine(true);
        assert_eq!(
            handle_control(
                &mut engine,
                Control::SetRemaps(Box::new(CompiledRemaps::empty()))
            ),
            Flow::Continue
        );
        let middle_down = ButtonEvent {
            kind: ButtonKind::Down,
            button: 2,
            sender_id: 10,
            user_data: 0,
            mouse_subtype: 0,
        };
        assert_eq!(engine.on_button(&middle_down), Verdict::Pass);
    }

    #[test]
    fn capture_controls_reach_the_engine_and_continue() {
        let sink = FakeSink::default();
        let (mut engine, _) = engine_with(true, sink.clone());
        let press = |engine: &mut InputEngine<FakeTapController>, kind, button| {
            engine.on_button(&ButtonEvent {
                kind,
                button,
                sender_id: 10,
                user_data: 0,
                mouse_subtype: 0,
            })
        };

        assert_eq!(
            handle_control(&mut engine, Control::StartCapture),
            Flow::Continue
        );
        press(&mut engine, ButtonKind::Down, 2);
        press(&mut engine, ButtonKind::Up, 2);
        handle_control(&mut engine, Control::StartCapture);
        assert_eq!(
            handle_control(&mut engine, Control::StopCapture),
            Flow::Continue
        );
        press(&mut engine, ButtonKind::Down, 3);

        assert_eq!(
            *sink.jobs.lock().unwrap(),
            vec![
                Job::Notice(Notice::ButtonCaptured { button: 3 }),
                Job::Perform(CompiledAction::Navigate(NavDirection::Back)),
            ]
        );
    }

    #[test]
    fn key_capture_controls_reach_the_engine_and_continue() {
        let (mut engine, _) = engine(true);
        assert_eq!(
            handle_control(&mut engine, Control::StartKeyCapture),
            Flow::Continue
        );
        assert!(engine.snapshot().taps.keys);
        assert_eq!(
            handle_control(&mut engine, Control::StopKeyCapture),
            Flow::Continue
        );
        assert!(!engine.snapshot().taps.keys);
    }

    #[test]
    fn shutdown_stops_the_loop() {
        let (mut engine, _) = engine(true);
        assert_eq!(handle_control(&mut engine, Control::Shutdown), Flow::Stop);
    }

    #[test]
    fn permission_bridge_forwards_and_wakes_the_input_thread() {
        let (tx, rx) = crossbeam_channel::unbounded();
        let waker = Arc::new(CountingWaker::default());
        let handle = Arc::new(InputHandle::new(tx, waker.clone(), Arc::default(), None));

        PermissionBridge(handle).permission_changed(true);

        assert!(matches!(rx.try_recv(), Ok(Control::SetTrusted(true))));
        assert_eq!(waker.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn store_snapshot_saves_then_forwards() {
        #[derive(Clone, Default)]
        struct Seen(Arc<Mutex<Vec<EngineSnapshot>>>);
        impl EngineObserver for Seen {
            fn snapshot_changed(&self, s: &EngineSnapshot) {
                self.0.lock().unwrap().push(s.clone());
            }
        }
        let shared = Arc::new(InputShared::default());
        let seen = Seen::default();
        let store = StoreSnapshot {
            shared: Arc::clone(&shared),
            ui: Box::new(seen.clone()),
        };
        let snapshot = EngineSnapshot {
            trusted: true,
            taps: TapSet::NONE,
            devices: vec![mouse(1)],
        };

        store.snapshot_changed(&snapshot);

        assert_eq!(**shared.snapshot.load(), snapshot);
        assert_eq!(*seen.0.lock().unwrap(), vec![snapshot]);
    }

    /// Real CFRunLoop thread. Without Accessibility (CI, dev) tap creation
    /// fails gracefully; the thread must still start, publish and stop.
    #[test]
    fn real_thread_starts_publishes_and_shuts_down() {
        struct Ignore2;
        impl EngineObserver for Ignore2 {
            fn snapshot_changed(&self, _: &EngineSnapshot) {}
        }
        let handle = spawn(
            false,
            TapDemand::ALL,
            CompiledRemaps::empty(),
            Scroll::default(),
            Box::new(crate::actions::queue::queue().0),
            Box::new(Ignore2),
        );
        assert!(!handle.shared().snapshot.load().trusted);
        let started = std::time::Instant::now();
        handle.shutdown();
        // Generous so a loaded CI runner doesn't flake; a hang still fails.
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}

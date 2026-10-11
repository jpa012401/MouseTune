//! The `mt-input` thread (spec §2, §4.4): a CFRunLoop that owns the input
//! engine, the event taps, the HID device monitor and the watchdog timer.
//! Other threads talk to it only through `InputHandle`.

use std::{
    cell::{OnceCell, RefCell},
    ffi::c_void,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError, atomic::AtomicBool, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

use arc_swap::ArcSwap;
use crossbeam_channel::{Receiver, Sender};
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopMode, CFRunLoopSource,
    CFRunLoopSourceContext, CFRunLoopTimer, CFRunLoopTimerContext, kCFRunLoopCommonModes,
};

use crate::{
    input::{
        engine::{EngineObserver, EngineSnapshot, InputEngine, ScrollCounters, TapHandler},
        guard::guarded,
        taps::{TapController, TapDemand},
    },
    permission_monitor::PermissionObserver,
    platform::{
        event_tap::{MacTapController, TapContext},
        hid::DeviceMonitor,
    },
};

/// How often the watchdog checks that the taps that should run are running.
const WATCHDOG_INTERVAL_SECS: f64 = 5.0;
/// `QOS_CLASS_USER_INTERACTIVE` from `<sys/qos.h>`.
const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;

/// Messages from other threads to `mt-input`.
#[derive(Debug, PartialEq, Eq)]
pub enum Control {
    SetTrusted(bool),
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

pub fn handle_control<C: TapController>(engine: &mut InputEngine<C>, msg: Control) -> Flow {
    match msg {
        Control::SetTrusted(trusted) => {
            engine.set_trusted(trusted);
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
pub fn spawn(trusted: bool, ui: Box<dyn EngineObserver + Send>) -> Arc<InputHandle> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let shared = Arc::new(InputShared::default());
    let (waker_tx, waker_rx) = mpsc::sync_channel::<Arc<dyn Waker>>(1);
    let thread_shared = Arc::clone(&shared);
    let join = thread::Builder::new()
        .name("mt-input".into())
        .spawn(move || run_input_thread(trusted, ui, rx, thread_shared, waker_tx))
        .expect("failed to spawn the mt-input thread");
    let waker = waker_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mt-input didn't start its run loop within 2 s");
    Arc::new(InputHandle::new(tx, waker, shared, Some(join)))
}

type Engine = Rc<RefCell<InputEngine<MacTapController>>>;

/// What the control source and watchdog timer callbacks reach through their
/// `info` pointer. Owned by `run_input_thread`, freed after the run loop stops.
struct LoopState {
    rx: Receiver<Control>,
    engine: Engine,
    run_loop: CFRetained<CFRunLoop>,
    degraded: Arc<AtomicBool>,
}

/// Wakes `mt-input` from any thread.
struct RunLoopWaker {
    run_loop: CFRetained<CFRunLoop>,
    source: CFRetained<CFRunLoopSource>,
}

// SAFETY: CFRunLoopSourceSignal and CFRunLoopWakeUp are documented thread-safe.
unsafe impl Send for RunLoopWaker {}
// SAFETY: CFRunLoopSourceSignal and CFRunLoopWakeUp are documented thread-safe.
unsafe impl Sync for RunLoopWaker {}

impl Waker for RunLoopWaker {
    fn wake(&self) {
        self.source.signal();
        self.run_loop.wake_up();
    }
}

fn common_modes() -> Option<&'static CFRunLoopMode> {
    // SAFETY: reading an immutable CoreFoundation constant.
    unsafe { kCFRunLoopCommonModes }
}

fn set_user_interactive_qos() {
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
    }
    // SAFETY: plain libc call on the current thread with a valid QoS class.
    let result = unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    if result != 0 {
        tracing::warn!(result, "couldn't raise mt-input to user-interactive QoS");
    }
}

fn run_input_thread(
    trusted: bool,
    ui: Box<dyn EngineObserver + Send>,
    rx: Receiver<Control>,
    shared: Arc<InputShared>,
    waker_tx: mpsc::SyncSender<Arc<dyn Waker>>,
) {
    set_user_interactive_qos();
    let run_loop = CFRunLoop::current().expect("mt-input has a run loop");

    let mut context = Rc::new(TapContext {
        handler: OnceCell::new(),
        degraded: Arc::clone(&shared.degraded),
    });
    let controller = MacTapController::new(Rc::as_ptr(&context), run_loop.clone());
    let engine: Engine = Rc::new(RefCell::new(InputEngine::new(
        controller,
        trusted,
        TapDemand::ALL,
        Arc::clone(&shared.counters),
        Box::new(StoreSnapshot {
            shared: Arc::clone(&shared),
            ui,
        }),
    )));
    let handler: Rc<RefCell<dyn TapHandler>> = engine.clone();
    if context.handler.set(handler).is_err() {
        unreachable!("the tap context is fresh");
    }

    let state = Box::into_raw(Box::new(LoopState {
        rx,
        engine: engine.clone(),
        run_loop: run_loop.clone(),
        degraded: Arc::clone(&shared.degraded),
    }));

    let mut source_context = CFRunLoopSourceContext {
        version: 0,
        info: state.cast(),
        retain: None,
        release: None,
        copyDescription: None,
        equal: None,
        hash: None,
        schedule: None,
        cancel: None,
        perform: Some(perform_control),
    };
    // SAFETY: the context is valid for the call (CoreFoundation copies it) and
    // `info` stays valid until the source is invalidated below.
    let source = unsafe { CFRunLoopSource::new(None, 0, &mut source_context) }
        .expect("CFRunLoopSourceCreate failed");
    run_loop.add_source(Some(&source), common_modes());

    let mut timer_context = CFRunLoopTimerContext {
        version: 0,
        info: state.cast(),
        retain: None,
        release: None,
        copyDescription: None,
    };
    // SAFETY: as for the source: the context is copied, `info` outlives the
    // timer, and `watchdog_tick` matches `CFRunLoopTimerCallBack`.
    let timer = unsafe {
        CFRunLoopTimer::new(
            None,
            CFAbsoluteTimeGetCurrent() + WATCHDOG_INTERVAL_SECS,
            WATCHDOG_INTERVAL_SECS,
            0,
            0,
            Some(watchdog_tick),
            &mut timer_context,
        )
    }
    .expect("CFRunLoopTimerCreate failed");
    run_loop.add_timer(Some(&timer), common_modes());

    let devices = DeviceMonitor::start(&run_loop, {
        let engine = engine.clone();
        move |event| engine.borrow_mut().apply_device_event(event)
    });

    engine.borrow_mut().start();

    let waker = Arc::new(RunLoopWaker {
        run_loop: run_loop.clone(),
        source: source.clone(),
    });
    if waker_tx.send(waker).is_err() {
        tracing::error!("input thread caller went away before startup finished");
    }

    CFRunLoop::run();

    // Teardown order matters: nothing may call back into state that is gone.
    // 1. Control source and watchdog timer, then the state they point to.
    timer.invalidate();
    source.invalidate();
    drop((timer, source));
    // SAFETY: `state` came from `Box::into_raw` above, and the only users of
    // the pointer (the source and timer) are invalidated.
    drop(unsafe { Box::from_raw(state) });
    // 2. The device monitor and its clone of the engine.
    drop(devices);
    // 3. The engine, which drops `MacTapController` and tears down the taps.
    //    The context's handler holds the last reference.
    drop(engine);
    let handler = Rc::get_mut(&mut context).and_then(|context| context.handler.take());
    debug_assert!(handler.is_some(), "the tap context is uniquely owned");
    drop(handler);
    // 4. Only now the context the taps pointed to.
    drop(context);
    tracing::info!("input thread stopped");
}

/// Control source callback: applies queued messages, stops on `Shutdown`.
unsafe extern "C-unwind" fn perform_control(info: *mut c_void) {
    // SAFETY: `info` is the `LoopState` pointer the source was created with;
    // it's freed only after the source is invalidated.
    let state = unsafe { &*info.cast_const().cast::<LoopState>() };
    guarded(&state.degraded, (), || {
        for msg in state.rx.try_iter() {
            if handle_control(&mut state.engine.borrow_mut(), msg) == Flow::Stop {
                state.run_loop.stop();
                return;
            }
        }
    });
}

/// Watchdog timer callback (every 5 s).
unsafe extern "C-unwind" fn watchdog_tick(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: as in `perform_control`; the timer is invalidated before `info` is freed.
    let state = unsafe { &*info.cast_const().cast::<LoopState>() };
    guarded(&state.degraded, (), || state.engine.borrow_mut().watchdog());
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::{
        input::{
            devices::DeviceEvent,
            engine::{EngineObserver, InputEngine},
            taps::{TapDemand, TapSet},
        },
        test_support::{FakeTapController, mouse},
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
        let taps = FakeTapController::default();
        let mut engine = InputEngine::new(
            taps.clone(),
            trusted,
            TapDemand::ALL,
            Arc::default(),
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
        let handle = spawn(false, Box::new(Ignore2));
        assert!(!handle.shared().snapshot.load().trusted);
        let started = std::time::Instant::now();
        handle.shutdown();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}

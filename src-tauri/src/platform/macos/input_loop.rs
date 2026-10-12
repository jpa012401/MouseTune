//! Run-loop adapter for the `mt-input` thread (spec §2, §4.4): creates the
//! thread, its CFRunLoop, the control source and the watchdog timer, and
//! hosts the engine, taps and device monitor on it. Humble object: every
//! decision lives in `input::thread::handle_control` and `InputEngine`.

use std::{
    cell::{OnceCell, RefCell},
    ffi::c_void,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::Receiver;
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopMode, CFRunLoopSource,
    CFRunLoopSourceContext, CFRunLoopTimer, CFRunLoopTimerContext, kCFRunLoopCommonModes,
};

use crate::{
    actions::ActionSink,
    config::schema::Scroll,
    input::{
        capture::SystemClock,
        drag::NoSwipe,
        engine::{ButtonSetup, EngineObserver, InputEngine, TapHandler},
        guard::guarded,
        intents::{GestureSetup, IntentSignal},
        remaps::CompiledRemaps,
        scroll::ScrollEngine,
        taps::TapDemand,
        thread::{Control, Flow, InputShared, Waker, handle_control},
    },
    platform::{
        click_timers::MacClickTimers,
        display_link::MacFrameDriver,
        event_poster::MacEventPoster,
        event_tap::{MacTapController, OnUnwind, TapContext},
        hid::DeviceMonitor,
        pointer::{self, MacPointerFreeze},
        scroll_poster::MacScrollPoster,
    },
};

/// How often the watchdog checks that the taps that should run are running.
const WATCHDOG_INTERVAL_SECS: f64 = 5.0;
/// `QOS_CLASS_USER_INTERACTIVE` from `<sys/qos.h>`.
const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;

/// Spawns `mt-input` and waits until its run loop can take messages.
/// `setup.observer` receives every engine snapshot; `rx` carries control
/// messages, which are applied after the returned waker is woken. The engine
/// owns `setup.sink`, so `mt-exec`'s queue closes when this thread stops.
pub fn start(
    setup: StartSetup,
    rx: Receiver<Control>,
    shared: Arc<InputShared>,
) -> (JoinHandle<()>, Arc<dyn Waker>) {
    let (waker_tx, waker_rx) = mpsc::sync_channel::<Arc<dyn Waker>>(1);
    let join = thread::Builder::new()
        .name("mt-input".into())
        .spawn(move || run_input_thread(setup, rx, shared, waker_tx))
        .expect("failed to spawn the mt-input thread");
    let waker = waker_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mt-input didn't start its run loop within 2 s");
    (join, waker)
}

type Engine = Rc<RefCell<InputEngine<MacTapController>>>;

/// What the engine starts from, moved onto `mt-input`.
pub(crate) struct StartSetup {
    pub(crate) trusted: bool,
    pub(crate) demand: TapDemand,
    pub(crate) remaps: CompiledRemaps,
    pub(crate) scroll: Scroll,
    pub(crate) sink: Box<dyn ActionSink + Send>,
    pub(crate) observer: Box<dyn EngineObserver + Send>,
}

/// What the control source and watchdog timer callbacks reach through their
/// `info` pointer. Owned by `run_input_thread`, freed after the run loop stops.
struct LoopState {
    rx: Receiver<Control>,
    engine: Engine,
    run_loop: CFRetained<CFRunLoop>,
    degraded: Arc<AtomicBool>,
}

/// Wakes `mt-input` from any thread.
pub(crate) struct RunLoopWaker {
    run_loop: CFRetained<CFRunLoop>,
    source: CFRetained<CFRunLoopSource>,
}

impl RunLoopWaker {
    pub(crate) fn new(
        run_loop: CFRetained<CFRunLoop>,
        source: CFRetained<CFRunLoopSource>,
    ) -> Self {
        Self { run_loop, source }
    }
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

/// Asks for the intents source to run once the current callout returns.
/// Only signals: callout-safe. The run loop is already awake (it is running
/// the callout), so no wake-up is needed.
struct MacIntentSignal {
    source: CFRetained<CFRunLoopSource>,
}

impl IntentSignal for MacIntentSignal {
    fn signal(&self) {
        self.source.signal();
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
    setup: StartSetup,
    rx: Receiver<Control>,
    shared: Arc<InputShared>,
    waker_tx: mpsc::SyncSender<Arc<dyn Waker>>,
) {
    set_user_interactive_qos();
    let run_loop = CFRunLoop::current().expect("mt-input has a run loop");

    let mut context = Rc::new(TapContext {
        handler: OnceCell::new(),
        degraded: Arc::clone(&shared.degraded),
        on_panic: pointer::release,
    });
    let controller = MacTapController::new(Rc::as_ptr(&context), run_loop.clone());
    // The engine owns the click timers, so they are invalidated when it drops,
    // before `context` (their `info`) is freed below.
    let timers = MacClickTimers::new(Rc::as_ptr(&context), &run_loop);
    // Owned by the engine (through `ScrollEngine`), so the frame source and
    // timer are invalidated when it drops, before `context` is freed below.
    let frames = MacFrameDriver::new(&run_loop, Arc::clone(&shared.degraded), {
        let context = Rc::as_ptr(&context);
        Box::new(move || frame_tick(context))
    });
    let scroll = ScrollEngine::new(
        setup.scroll,
        Box::new(frames),
        Box::new(MacScrollPoster::new()),
    );
    // Runs queued gesture intents outside every callout. Invalidated on
    // teardown before `context` (its `info`) is freed.
    let mut intents_context = CFRunLoopSourceContext {
        version: 0,
        info: Rc::as_ptr(&context).cast_mut().cast(),
        retain: None,
        release: None,
        copyDescription: None,
        equal: None,
        hash: None,
        schedule: None,
        cancel: None,
        perform: Some(perform_intents),
    };
    // SAFETY: CoreFoundation copies the context; `info` outlives the source
    // because the source is invalidated below before `context` is dropped.
    let intents = unsafe { CFRunLoopSource::new(None, 0, &mut intents_context) }
        .expect("CFRunLoopSourceCreate failed");
    run_loop.add_source(Some(&intents), common_modes());
    let gestures = GestureSetup {
        freeze: Box::new(MacPointerFreeze),
        poster: Box::new(MacEventPoster),
        signal: Box::new(MacIntentSignal {
            source: intents.clone(),
        }),
        // Task 9 replaces this once dock swipes are verified.
        swipe: Box::new(NoSwipe),
        scroll: Box::new(MacScrollPoster::new()),
    };
    let engine: Engine = Rc::new(RefCell::new(InputEngine::new(
        controller,
        setup.trusted,
        setup.demand,
        Arc::clone(&shared.counters),
        ButtonSetup {
            remaps: setup.remaps,
            timers: Box::new(timers),
            sink: setup.sink,
            counters: Arc::clone(&shared.actions),
            clock: Box::new(SystemClock),
        },
        scroll,
        gestures,
        setup.observer,
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
        move |event| {
            // The monitor catches a panic; the pointer is released first.
            let _release = OnUnwind(pointer::release);
            engine.borrow_mut().apply_device_event(event)
        }
    });

    engine.borrow_mut().start();

    let waker = Arc::new(RunLoopWaker::new(run_loop.clone(), source.clone()));
    if waker_tx.send(waker).is_err() {
        tracing::error!("input thread caller went away before startup finished");
    }

    CFRunLoop::run();

    // The run loop no longer services the taps, so switch them off before
    // anything else; a filtering tap left on but unserviced stalls input.
    // Its `refresh` also runs `release_everything`: a drag in flight
    // unfreezes the pointer and held hold-through buttons get their ups.
    engine.borrow_mut().set_trusted(false);

    // Teardown order matters: nothing may call back into state that is gone.
    // 1. Control source and watchdog timer, then the state they point to.
    //    The intents source too: it points at `context`.
    timer.invalidate();
    source.invalidate();
    intents.invalidate();
    drop((timer, source, intents));
    // SAFETY: `state` came from `Box::into_raw` above, and the only users of
    // the pointer (the source and timer) are invalidated.
    drop(unsafe { Box::from_raw(state) });
    // 2. The device monitor and its clone of the engine.
    drop(devices);
    // 3. The engine, which drops `MacTapController` (tearing down the taps),
    //    `MacClickTimers` (invalidating both click timers), `MacFrameDriver`
    //    (invalidating the frame source and timer) and the action sink
    //    (closing `mt-exec`'s queue). The context's handler holds the
    //    last reference.
    drop(engine);
    let handler = Rc::get_mut(&mut context).and_then(|context| context.handler.take());
    debug_assert!(handler.is_some(), "the tap context is uniquely owned");
    drop(handler);
    // 4. Only now the context the taps, click timers and frames pointed to.
    drop(context);
    tracing::info!("input thread stopped");
}

/// Control source callback: applies queued messages, stops on `Shutdown`.
unsafe extern "C-unwind" fn perform_control(info: *mut c_void) {
    // SAFETY: `info` is the `LoopState` pointer the source was created with;
    // it's freed only after the source is invalidated.
    let state = unsafe { &*info.cast_const().cast::<LoopState>() };
    guarded(&state.degraded, (), || {
        let _release = OnUnwind(pointer::release);
        for msg in state.rx.try_iter() {
            if handle_control(&mut state.engine.borrow_mut(), msg) == Flow::Stop {
                state.run_loop.stop();
                return;
            }
        }
    });
}

/// Frame callback body; the driver wraps it in `guarded`. Same borrow rule
/// as the click timers: a re-entrant frame is dropped, not waited for.
fn frame_tick(context: *const TapContext) {
    // SAFETY: the frame driver is owned by the engine, which drops (and so
    // invalidates the frame source and timer) before `context` is freed.
    let context = unsafe { &*context };
    let Some(handler) = context.handler.get() else {
        return;
    };
    let Ok(mut handler) = handler.try_borrow_mut() else {
        return;
    };
    // The driver's `guarded` catches a panic; the pointer is released first.
    let _release = OnUnwind(context.on_panic);
    handler.on_frame();
}

/// Intents source callback: carries out what callouts queued. Same borrow
/// rule as the frames: a re-entrant run is dropped; the intents stay queued
/// for the next signal.
unsafe extern "C-unwind" fn perform_intents(info: *mut c_void) {
    // SAFETY: `info` is the `TapContext` the source was created with; the
    // source is invalidated before `context` is freed.
    let context = unsafe { &*info.cast_const().cast::<TapContext>() };
    guarded(&context.degraded, (), || {
        let _release = OnUnwind(context.on_panic);
        let Some(handler) = context.handler.get() else {
            return;
        };
        let Ok(mut handler) = handler.try_borrow_mut() else {
            return;
        };
        handler.on_intents();
    });
}

/// Watchdog timer callback (every 5 s).
unsafe extern "C-unwind" fn watchdog_tick(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: as in `perform_control`; the timer is invalidated before `info` is freed.
    let state = unsafe { &*info.cast_const().cast::<LoopState>() };
    guarded(&state.degraded, (), || {
        let _release = OnUnwind(pointer::release);
        state.engine.borrow_mut().watchdog()
    });
}

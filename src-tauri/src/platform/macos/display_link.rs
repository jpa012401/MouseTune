//! Display-paced frames for the scroll animator on `mt-input` (Phase 5a
//! design §4, spec §8.1). CVDisplayLink calls back on CoreVideo's thread,
//! which only signals a run-loop source; the frame itself runs on
//! `mt-input`. A 120 Hz run-loop timer stands in when CVDisplayLink can't
//! be created or started. Humble object: starts, stops and forwards.
//!
//! CVDisplayLink has been deprecated since macOS 15 in favour of
//! `NSScreen.displayLink`, which is main-thread AppKit. `mt-input` isn't the
//! main thread, so the deprecated API is used on purpose.
#![allow(deprecated)]

use std::{
    cell::{Cell, OnceCell},
    ffi::c_void,
    ptr::{self, NonNull},
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopSource, CFRunLoopSourceContext,
    CFRunLoopTimer, CFRunLoopTimerContext, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{CGDirectDisplayID, CGError, CGEvent, CGGetDisplaysWithPoint};
use objc2_core_video::{CVDisplayLink, CVOptionFlags, CVReturn, CVTimeStamp, kCVReturnSuccess};

use crate::{
    input::{guard::guarded, scroll::FrameDriver, thread::Waker},
    platform::{click_timers::NEVER_SECS, input_loop::RunLoopWaker},
};

/// The fallback timer's period.
const FRAME_SECS: f64 = 1.0 / 120.0;
/// A running display link silent for longer than this is taken to have
/// stopped calling back (display unplugged, reconfigured or asleep) and is
/// restarted, which re-points it or falls back to the timer. Six frames at
/// 60 Hz is far beyond normal jitter.
const STALL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pacer {
    DisplayLink,
    Timer,
}

pub struct MacFrameDriver {
    inner: Rc<FrameInner>,
}

/// Reached by the frame source and the timer through their `info`
/// pointer. Every field is set once, during construction.
struct FrameInner {
    on_frame: Box<dyn Fn()>,
    degraded: Arc<AtomicBool>,
    link: OnceCell<CFRetained<CVDisplayLink>>,
    timer: OnceCell<CFRetained<CFRunLoopTimer>>,
    source: OnceCell<CFRetained<CFRunLoopSource>>,
    wanted: Cell<bool>,
    running: Cell<Option<Pacer>>,
    /// When the running pacer last delivered a frame (or was started).
    last_frame: Cell<Option<Instant>>,
}

impl MacFrameDriver {
    /// CVDisplayLink frames, with the timer as fallback.
    pub fn new(
        run_loop: &CFRetained<CFRunLoop>,
        degraded: Arc<AtomicBool>,
        on_frame: Box<dyn Fn()>,
    ) -> Self {
        let driver = Self::with_timer_only(run_loop, degraded, on_frame);
        let Some(source) = driver.inner.source.get() else {
            unreachable!("with_timer_only sets the source");
        };
        // Leaked on purpose: CoreVideo may call back once more after
        // `CVDisplayLinkStop`, so the waker must outlive the link. It's one
        // small allocation per input thread, and signalling an invalidated
        // source does nothing.
        let waker: &'static RunLoopWaker = Box::leak(Box::new(RunLoopWaker::new(
            run_loop.clone(),
            source.clone(),
        )));
        match create_display_link(waker) {
            Some(link) => {
                if driver.inner.link.set(link).is_err() {
                    unreachable!("the link cell is fresh");
                }
            }
            None => {
                tracing::warn!("CVDisplayLink unavailable: scroll frames come from a 120 Hz timer")
            }
        }
        driver
    }

    /// The frame source and the 120 Hz timer, without a display link.
    fn with_timer_only(
        run_loop: &CFRetained<CFRunLoop>,
        degraded: Arc<AtomicBool>,
        on_frame: Box<dyn Fn()>,
    ) -> Self {
        let inner = Rc::new(FrameInner {
            on_frame,
            degraded,
            link: OnceCell::new(),
            timer: OnceCell::new(),
            source: OnceCell::new(),
            wanted: Cell::new(false),
            running: Cell::new(None),
            last_frame: Cell::new(None),
        });
        let info: *mut c_void = Rc::as_ptr(&inner).cast_mut().cast();
        // SAFETY: reading an immutable CoreFoundation constant.
        let modes = unsafe { kCFRunLoopCommonModes };

        let mut source_context = CFRunLoopSourceContext {
            version: 0,
            info,
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: Some(perform_frame),
        };
        // SAFETY: CoreFoundation copies the context; `info` outlives the
        // source because `Drop` invalidates it before `inner` is freed.
        let source = unsafe { CFRunLoopSource::new(None, 0, &mut source_context) }
            .expect("CFRunLoopSourceCreate failed");
        run_loop.add_source(Some(&source), modes);

        let mut timer_context = CFRunLoopTimerContext {
            version: 0,
            info,
            retain: None,
            release: None,
            copyDescription: None,
        };
        // SAFETY: as for the source; `timer_fired` matches
        // `CFRunLoopTimerCallBack`. Parked until a frame is wanted.
        let timer = unsafe {
            CFRunLoopTimer::new(
                None,
                CFAbsoluteTimeGetCurrent() + NEVER_SECS,
                FRAME_SECS,
                0,
                0,
                Some(timer_fired),
                &mut timer_context,
            )
        }
        .expect("CFRunLoopTimerCreate failed");
        run_loop.add_timer(Some(&timer), modes);

        if inner.source.set(source).is_err() || inner.timer.set(timer).is_err() {
            unreachable!("the cells are fresh");
        }
        Self { inner }
    }
}

impl FrameDriver for MacFrameDriver {
    /// Called inside the scroll tap's callout: it only flags and signals.
    /// The first frame runs as soon as the callout returns.
    fn request(&mut self) {
        self.inner.wanted.set(true);
        if let Some(source) = self.inner.source.get() {
            source.signal();
        }
    }

    fn stop(&mut self) {
        self.inner.wanted.set(false);
        self.inner.halt();
    }
}

impl Drop for MacFrameDriver {
    /// After this, neither the source nor the timer can reach `inner`.
    fn drop(&mut self) {
        self.inner.halt();
        if let Some(timer) = self.inner.timer.get() {
            timer.invalidate();
        }
        if let Some(source) = self.inner.source.get() {
            source.invalidate();
        }
    }
}

impl FrameInner {
    /// One frame on `mt-input`. A panic stops the frames, so it can't
    /// repeat 120 times a second; the next tick asks again. A signal left
    /// over from before a stop, or a late CoreVideo callback, runs nothing.
    /// A display link that went silent is halted here and started again
    /// below, so the next tick's frame revives the pacing.
    fn frame(&self) {
        if !self.wanted.get() {
            self.halt();
            return;
        }
        let now = Instant::now();
        let stalled = self
            .last_frame
            .get()
            .is_none_or(|last| now.saturating_duration_since(last) > STALL);
        if self.running.get() == Some(Pacer::DisplayLink) && stalled {
            self.halt();
        }
        self.last_frame.set(Some(now));
        let healthy = guarded(&self.degraded, false, || {
            (self.on_frame)();
            true
        });
        if !healthy {
            self.wanted.set(false);
        }
        if self.wanted.get() {
            self.start();
        } else {
            self.halt();
        }
    }

    fn start(&self) {
        if self.running.get().is_some() {
            return;
        }
        if let Some(link) = self.link.get() {
            if let Some(display) = display_under_cursor() {
                link.set_current_cg_display(display);
            }
            if link.start() == kCVReturnSuccess {
                self.running.set(Some(Pacer::DisplayLink));
                return;
            }
        }
        if let Some(timer) = self.timer.get() {
            timer.set_next_fire_date(CFAbsoluteTimeGetCurrent() + FRAME_SECS);
            self.running.set(Some(Pacer::Timer));
        }
    }

    fn halt(&self) {
        match self.running.take() {
            Some(Pacer::DisplayLink) => {
                if let Some(link) = self.link.get() {
                    link.stop();
                }
            }
            Some(Pacer::Timer) => {
                if let Some(timer) = self.timer.get() {
                    timer.set_next_fire_date(CFAbsoluteTimeGetCurrent() + NEVER_SECS);
                }
            }
            None => {}
        }
    }
}

/// The display the pointer is on, so a 60 Hz and a 120 Hz screen each pace
/// their own frames. Runs outside any tap callout.
fn display_under_cursor() -> Option<CGDirectDisplayID> {
    let event = CGEvent::new(None)?;
    let location = CGEvent::location(Some(&event));
    let (mut display, mut count) = (0, 0);
    // SAFETY: both out-pointers are live locals, and at most one display is written.
    let status = unsafe { CGGetDisplaysWithPoint(location, 1, &mut display, &mut count) };
    (status == CGError::Success && count == 1).then_some(display)
}

fn create_display_link(waker: &'static RunLoopWaker) -> Option<CFRetained<CVDisplayLink>> {
    let mut link: *mut CVDisplayLink = ptr::null_mut();
    // SAFETY: `link` is a live out-pointer.
    let status = unsafe { CVDisplayLink::create_with_active_cg_displays(NonNull::from(&mut link)) };
    if status != kCVReturnSuccess {
        return None;
    }
    // SAFETY: on success CoreVideo handed us a +1 reference, which we now own.
    let link = unsafe { CFRetained::from_raw(NonNull::new(link)?) };
    let info: *mut c_void = ptr::from_ref(waker).cast_mut().cast();
    // SAFETY: `display_link_fired` matches `CVDisplayLinkOutputCallback`,
    // and `info` points to a leaked, never-freed waker.
    let status = unsafe { link.set_output_callback(Some(display_link_fired), info) };
    (status == kCVReturnSuccess).then_some(link)
}

/// On CoreVideo's thread: only wakes `mt-input`. Signals coalesce, so a
/// slow input thread skips frames rather than queueing them.
unsafe extern "C-unwind" fn display_link_fired(
    _link: NonNull<CVDisplayLink>,
    _now: NonNull<CVTimeStamp>,
    _output_time: NonNull<CVTimeStamp>,
    _flags_in: CVOptionFlags,
    _flags_out: NonNull<CVOptionFlags>,
    info: *mut c_void,
) -> CVReturn {
    // SAFETY: `info` is the leaked `RunLoopWaker` from `create_display_link`.
    let waker = unsafe { &*info.cast_const().cast::<RunLoopWaker>() };
    waker.wake();
    kCVReturnSuccess
}

unsafe extern "C-unwind" fn perform_frame(info: *mut c_void) {
    // SAFETY: `info` is the `FrameInner` the source was created with; the
    // source is invalidated before it is freed.
    let inner = unsafe { &*info.cast_const().cast::<FrameInner>() };
    inner.frame();
}

unsafe extern "C-unwind" fn timer_fired(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    // SAFETY: as in `perform_frame`; the timer is invalidated before `info` is freed.
    let inner = unsafe { &*info.cast_const().cast::<FrameInner>() };
    inner.frame();
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use objc2_core_foundation::kCFRunLoopDefaultMode;

    use super::*;

    fn run_for(secs: f64) {
        // SAFETY: reading an immutable CoreFoundation constant.
        let mode = unsafe { kCFRunLoopDefaultMode };
        CFRunLoop::run_in_mode(mode, secs, false);
    }

    fn counting() -> (Rc<Cell<u32>>, Box<dyn Fn()>) {
        let frames = Rc::new(Cell::new(0));
        let count = Rc::clone(&frames);
        (frames, Box::new(move || count.set(count.get() + 1)))
    }

    fn run_loop() -> CFRetained<CFRunLoop> {
        CFRunLoop::current().expect("test thread has a run loop")
    }

    #[test]
    fn the_timer_paces_frames_only_while_they_are_wanted() {
        let (frames, on_frame) = counting();
        let mut driver = MacFrameDriver::with_timer_only(&run_loop(), Arc::default(), on_frame);
        run_for(0.05);
        assert_eq!(frames.get(), 0, "nothing until requested");
        driver.request();
        run_for(0.1);
        let seen = frames.get();
        assert!(seen >= 3, "{seen} frames in 100 ms at 120 Hz");
        driver.stop();
        run_for(0.05);
        assert_eq!(frames.get(), seen, "no frames after stop");
    }

    #[test]
    fn a_stop_before_the_first_frame_runs_no_frame() {
        let (frames, on_frame) = counting();
        let mut driver = MacFrameDriver::with_timer_only(&run_loop(), Arc::default(), on_frame);
        driver.request();
        driver.stop();
        run_for(0.05);
        assert_eq!(frames.get(), 0, "the pending signal must not run a frame");
    }

    #[test]
    fn a_panicking_frame_marks_degraded_and_stops_the_frames() {
        let degraded = Arc::new(AtomicBool::new(false));
        let calls = Rc::new(Cell::new(0));
        let count = Rc::clone(&calls);
        let mut driver = MacFrameDriver::with_timer_only(
            &run_loop(),
            Arc::clone(&degraded),
            Box::new(move || {
                count.set(count.get() + 1);
                panic!("frame bug");
            }),
        );
        driver.request();
        run_for(0.1);
        assert_eq!(calls.get(), 1, "no frame after the panic");
        assert!(degraded.load(Ordering::SeqCst));
    }

    /// Fakes a display link that was started but has stopped calling back
    /// (display unplugged or asleep mid-coast): the driver believes the
    /// link is running, and its last frame was `since` ago. The timer-only
    /// driver has no link, so a restart falls back to the 120 Hz timer.
    fn stalled_link(driver: &MacFrameDriver, since: Duration) {
        driver.inner.running.set(Some(Pacer::DisplayLink));
        driver
            .inner
            .last_frame
            .set(Instant::now().checked_sub(since));
    }

    #[test]
    fn a_display_link_that_stopped_calling_back_is_restarted() {
        let (frames, on_frame) = counting();
        let mut driver = MacFrameDriver::with_timer_only(&run_loop(), Arc::default(), on_frame);
        stalled_link(&driver, Duration::from_millis(200));
        driver.request();
        run_for(0.1);
        // Without a restart only the tick's own signal runs a frame (1);
        // after one, the timer paces ~12 frames in 100 ms at 120 Hz.
        let seen = frames.get();
        assert!(
            seen >= 3,
            "{seen} frames: the stalled pacer wasn't replaced"
        );
        assert_eq!(driver.inner.running.get(), Some(Pacer::Timer));
        driver.stop();
    }

    #[test]
    fn a_display_link_between_frames_is_left_running() {
        let (frames, on_frame) = counting();
        let mut driver = MacFrameDriver::with_timer_only(&run_loop(), Arc::default(), on_frame);
        stalled_link(&driver, Duration::ZERO);
        driver.request();
        run_for(0.05);
        // A fresh link isn't restarted on a tick, so only the tick's own
        // signal runs a frame: 1.
        assert_eq!(frames.get(), 1);
        assert_eq!(driver.inner.running.get(), Some(Pacer::DisplayLink));
        driver.stop();
    }

    #[test]
    #[ignore = "needs a real display: cargo test display_link_delivers_frames -- --ignored --nocapture"]
    fn display_link_delivers_frames() {
        let (frames, on_frame) = counting();
        let mut driver = MacFrameDriver::new(&run_loop(), Arc::default(), on_frame);
        driver.request();
        run_for(0.2);
        let pacer = driver.inner.running.get();
        println!("{} frames in 200 ms, paced by {pacer:?}", frames.get());
        assert_eq!(pacer, Some(Pacer::DisplayLink));
        assert!(frames.get() >= 5);
        driver.stop();
    }
}

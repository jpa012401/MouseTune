//! The two click-cycle timers on `mt-input` (design §2). Created once and
//! re-armed by moving their next fire date, so the hot path never creates a
//! timer. They repeat with an effectively infinite interval because a
//! non-repeating CFRunLoopTimer is invalidated after it fires and can't be
//! re-armed.

use std::{ffi::c_void, time::Duration};

use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext,
    kCFRunLoopCommonModes,
};

use crate::{
    input::{
        guard::guarded,
        router::{ClickTimers, TimerKind},
    },
    platform::event_tap::{OnUnwind, TapContext},
};

/// "Not armed": a fire date about 31 years away, also used as the repeat
/// interval so a fired timer parks itself instead of firing again.
pub(crate) const NEVER_SECS: f64 = 1.0e9;

type TimerCallback = unsafe extern "C-unwind" fn(*mut CFRunLoopTimer, *mut c_void);

pub struct MacClickTimers {
    hold: CFRetained<CFRunLoopTimer>,
    expiry: CFRetained<CFRunLoopTimer>,
}

impl MacClickTimers {
    /// `context` must outlive the timers; dropping them invalidates both.
    pub fn new(context: *const TapContext, run_loop: &CFRunLoop) -> Self {
        Self {
            hold: parked_timer(context, run_loop, hold_fired),
            expiry: parked_timer(context, run_loop, expiry_fired),
        }
    }

    fn timer(&self, kind: TimerKind) -> &CFRunLoopTimer {
        match kind {
            TimerKind::Hold => &self.hold,
            TimerKind::Expiry => &self.expiry,
        }
    }
}

/// Creates a timer that won't fire until armed and adds it to `run_loop`.
fn parked_timer(
    context: *const TapContext,
    run_loop: &CFRunLoop,
    callback: TimerCallback,
) -> CFRetained<CFRunLoopTimer> {
    let mut timer_context = CFRunLoopTimerContext {
        version: 0,
        info: context.cast_mut().cast(),
        retain: None,
        release: None,
        copyDescription: None,
    };
    // SAFETY: CoreFoundation copies the context during the call; `info`
    // outlives the timer (the caller's contract, upheld by `Drop`
    // invalidating it); `callback` matches `CFRunLoopTimerCallBack`.
    let timer = unsafe {
        CFRunLoopTimer::new(
            None,
            CFAbsoluteTimeGetCurrent() + NEVER_SECS,
            NEVER_SECS,
            0,
            0,
            Some(callback),
            &mut timer_context,
        )
    }
    .expect("CFRunLoopTimerCreate failed");
    // SAFETY: reading an immutable CoreFoundation constant.
    let modes = unsafe { kCFRunLoopCommonModes };
    run_loop.add_timer(Some(&timer), modes);
    timer
}

impl ClickTimers for MacClickTimers {
    /// Moving the fire date replaces any pending fire.
    fn arm(&mut self, kind: TimerKind, after: Duration) {
        self.timer(kind)
            .set_next_fire_date(CFAbsoluteTimeGetCurrent() + after.as_secs_f64());
    }

    fn disarm(&mut self, kind: TimerKind) {
        self.timer(kind)
            .set_next_fire_date(CFAbsoluteTimeGetCurrent() + NEVER_SECS);
    }
}

impl Drop for MacClickTimers {
    /// Invalidating removes the timers from every run loop, so no callback
    /// can reach the context after this.
    fn drop(&mut self) {
        self.hold.invalidate();
        self.expiry.invalidate();
    }
}

unsafe extern "C-unwind" fn hold_fired(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    fire(info, TimerKind::Hold)
}

unsafe extern "C-unwind" fn expiry_fired(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    fire(info, TimerKind::Expiry)
}

/// Hot path: same rules as a tap callback. Re-entrant or early fires are
/// dropped rather than waiting.
fn fire(info: *mut c_void, kind: TimerKind) {
    // SAFETY: `info` is the `TapContext` pointer the timer was created with;
    // the timers are invalidated (when the engine that owns them drops)
    // before the context is freed.
    let context = unsafe { &*info.cast_const().cast::<TapContext>() };
    guarded(&context.degraded, (), || {
        let _release = OnUnwind(context.on_panic);
        let Some(handler) = context.handler.get() else {
            return;
        };
        // Unreachable today (nothing runs the run loop while the handler is
        // borrowed). If it ever happens, the dropped tick leaves the router's
        // armed flag set, so that timer stays inert until the next button
        // event re-arms or ends the cycle.
        let Ok(mut handler) = handler.try_borrow_mut() else {
            return;
        };
        handler.on_click_timer(kind);
    });
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{OnceCell, RefCell},
        rc::Rc,
        sync::{Arc, atomic::AtomicBool},
    };

    use objc2_core_foundation::kCFRunLoopDefaultMode;

    use super::*;
    use crate::input::{
        engine::TapHandler,
        filter::ScrollFields,
        guard::DisabledReason,
        key_recorder::KeyEvent,
        router::{ButtonEvent, Verdict},
        scroll::ScrollVerdict,
        taps::TapKind,
    };

    struct Recorder(Rc<RefCell<Vec<TimerKind>>>);
    impl TapHandler for Recorder {
        fn on_scroll(&mut self, _: &ScrollFields) -> ScrollVerdict {
            ScrollVerdict::Pass
        }
        fn on_button(&mut self, _: &ButtonEvent) -> Verdict {
            Verdict::Pass
        }
        fn on_click_timer(&mut self, kind: TimerKind) {
            self.0.borrow_mut().push(kind)
        }
        fn on_key(&mut self, _: &KeyEvent) -> Verdict {
            Verdict::Pass
        }
        fn on_tap_disabled(&mut self, _: TapKind, _: DisabledReason) {}
        fn on_frame(&mut self) {}
        fn on_motion(&mut self, _: &crate::input::router::MotionEvent) {}
        fn on_intents(&mut self) {}
    }

    fn run_for(secs: f64) {
        // SAFETY: reading an immutable CoreFoundation constant.
        let mode = unsafe { kCFRunLoopDefaultMode };
        CFRunLoop::run_in_mode(mode, secs, false);
    }

    /// Panics on every timer, like a router bug would.
    struct Panicker;
    impl TapHandler for Panicker {
        fn on_scroll(&mut self, _: &ScrollFields) -> ScrollVerdict {
            ScrollVerdict::Pass
        }
        fn on_button(&mut self, _: &ButtonEvent) -> Verdict {
            Verdict::Pass
        }
        fn on_click_timer(&mut self, _: TimerKind) {
            panic!("router bug");
        }
        fn on_key(&mut self, _: &KeyEvent) -> Verdict {
            Verdict::Pass
        }
        fn on_tap_disabled(&mut self, _: TapKind, _: DisabledReason) {}
        fn on_frame(&mut self) {}
        fn on_motion(&mut self, _: &crate::input::router::MotionEvent) {}
        fn on_intents(&mut self) {}
    }

    static TIMER_RELEASES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    #[test]
    fn a_panicking_timer_releases_the_pointer_and_marks_degraded() {
        let context = TapContext {
            handler: OnceCell::new(),
            degraded: Arc::new(AtomicBool::new(false)),
            on_panic: || {
                TIMER_RELEASES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            },
        };
        let handler: Rc<RefCell<dyn TapHandler>> = Rc::new(RefCell::new(Panicker));
        assert!(context.handler.set(handler).is_ok());
        let run_loop = CFRunLoop::current().expect("test thread has a run loop");
        let mut timers = MacClickTimers::new(&context, &run_loop);

        timers.arm(TimerKind::Hold, Duration::from_millis(10));
        run_for(0.1);

        assert_eq!(TIMER_RELEASES.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(context.degraded.load(std::sync::atomic::Ordering::SeqCst));
        timers.disarm(TimerKind::Hold);
        drop(timers); // invalidates both before `context` goes
    }

    #[test]
    fn an_armed_timer_fires_once_can_be_re_armed_and_a_disarmed_one_never_fires() {
        let fired = Rc::new(RefCell::new(Vec::new()));
        let context = TapContext {
            handler: OnceCell::new(),
            degraded: Arc::new(AtomicBool::new(false)),
            on_panic: || {},
        };
        let handler: Rc<RefCell<dyn TapHandler>> =
            Rc::new(RefCell::new(Recorder(Rc::clone(&fired))));
        assert!(context.handler.set(handler).is_ok());
        let run_loop = CFRunLoop::current().expect("test thread has a run loop");
        let mut timers = MacClickTimers::new(&context, &run_loop);

        timers.arm(TimerKind::Hold, Duration::from_millis(10));
        timers.arm(TimerKind::Expiry, Duration::from_millis(10));
        timers.disarm(TimerKind::Expiry);
        run_for(0.1);
        assert_eq!(*fired.borrow(), [TimerKind::Hold]);

        timers.arm(TimerKind::Hold, Duration::from_millis(10));
        run_for(0.1);
        assert_eq!(*fired.borrow(), [TimerKind::Hold, TimerKind::Hold]);

        // A later arm replaces a pending fire rather than adding one.
        timers.arm(TimerKind::Hold, Duration::from_millis(10));
        timers.arm(TimerKind::Hold, Duration::from_millis(500));
        run_for(0.1);
        assert_eq!(*fired.borrow(), [TimerKind::Hold, TimerKind::Hold]);
        timers.disarm(TimerKind::Hold);
        drop(timers); // invalidates both before `context` goes
    }
}

//! CGEventTap adapter (spec §4.1, §4.5). Humble object: creates taps,
//! copies fields out of events, forwards to a `TapHandler`. No decisions.

use std::{
    cell::{OnceCell, RefCell},
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{Arc, atomic::AtomicBool},
};

use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use crate::input::{
    engine::TapHandler,
    filter::ScrollFields,
    guard::{DisabledReason, guarded},
    taps::{TapController, TapKind},
};

/// Undocumented: IORegistry entry ID of the HID service that sent the event.
pub const SENDER_ID_FIELD: CGEventField = CGEventField(87);

pub fn event_mask(kind: TapKind) -> u64 {
    let bit = |t: CGEventType| 1u64 << t.0;
    match kind {
        TapKind::Buttons => bit(CGEventType::OtherMouseDown) | bit(CGEventType::OtherMouseUp),
        TapKind::Scroll => bit(CGEventType::ScrollWheel),
        TapKind::Flags => bit(CGEventType::FlagsChanged),
    }
}

pub fn tap_options(kind: TapKind) -> CGEventTapOptions {
    match kind {
        TapKind::Flags => CGEventTapOptions::ListenOnly,
        TapKind::Buttons | TapKind::Scroll => CGEventTapOptions::Default,
    }
}

pub fn scroll_fields(event: &CGEvent) -> ScrollFields {
    let get = |field| CGEvent::integer_value_field(Some(event), field);
    ScrollFields {
        is_continuous: get(CGEventField::ScrollWheelEventIsContinuous),
        scroll_phase: get(CGEventField::ScrollWheelEventScrollPhase),
        momentum_phase: get(CGEventField::ScrollWheelEventMomentumPhase),
        tablet_device_id: get(CGEventField::TabletEventDeviceID),
        delta_axis1: get(CGEventField::ScrollWheelEventDeltaAxis1),
        delta_axis2: get(CGEventField::ScrollWheelEventDeltaAxis2),
        sender_id: get(SENDER_ID_FIELD) as u64,
        user_data: get(CGEventField::EventSourceUserData),
    }
}

/// Shared by the input thread and the tap callbacks; must outlive the taps.
pub struct TapContext {
    pub handler: OnceCell<Rc<RefCell<dyn TapHandler>>>,
    pub degraded: Arc<AtomicBool>,
}

pub struct MacTapController {
    context: *const TapContext,
    run_loop: CFRetained<CFRunLoop>,
    ports: [Option<CFRetained<CFMachPort>>; 3],
}

impl MacTapController {
    /// `context` must outlive the controller. Dropping the controller disables
    /// and invalidates its taps, so no callback can fire after that.
    pub fn new(context: *const TapContext, run_loop: CFRetained<CFRunLoop>) -> Self {
        Self {
            context,
            run_loop,
            ports: [None, None, None],
        }
    }

    fn slot(kind: TapKind) -> usize {
        match kind {
            TapKind::Buttons => 0,
            TapKind::Scroll => 1,
            TapKind::Flags => 2,
        }
    }

    fn create(&mut self, kind: TapKind) -> bool {
        let callback = match kind {
            TapKind::Buttons => button_callback as _,
            TapKind::Scroll => scroll_callback as _,
            TapKind::Flags => flags_callback as _,
        };
        // SAFETY: the callbacks match `CGEventTapCallBack`; `context` outlives
        // the tap (owned by the input thread, which owns this controller).
        let port = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::HIDEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                tap_options(kind),
                event_mask(kind),
                Some(callback),
                self.context as *mut c_void,
            )
        };
        let Some(port) = port else {
            tracing::warn!(
                ?kind,
                "CGEventTapCreate failed (Accessibility not granted?)"
            );
            return false;
        };
        let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
            return false;
        };
        // SAFETY: reading an immutable CoreFoundation constant.
        let mode = unsafe { kCFRunLoopCommonModes };
        self.run_loop.add_source(Some(&source), mode);
        self.ports[Self::slot(kind)] = Some(port);
        true
    }

    /// Disables and invalidates a tap, which also removes its run-loop source.
    fn discard(&mut self, kind: TapKind) {
        if let Some(port) = self.ports[Self::slot(kind)].take() {
            CGEvent::tap_enable(&port, false);
            port.invalidate();
        }
    }
}

// Verified by review, not tests: creating a tap needs Accessibility, which tests don't have.
impl Drop for MacTapController {
    fn drop(&mut self) {
        // The run loop retains each source and each source retains its port, so
        // dropping `ports` alone would leave the taps live. Invalidating a port
        // also removes its run-loop sources.
        for kind in TapKind::ALL {
            self.discard(kind);
        }
    }
}

impl TapController for MacTapController {
    // Verified by review, not tests: creating or enabling a real tap needs
    // Accessibility, which test binaries don't have.
    fn set_enabled(&mut self, kind: TapKind, enabled: bool) -> bool {
        let slot = Self::slot(kind);
        if !enabled {
            if let Some(port) = &self.ports[slot] {
                CGEvent::tap_enable(port, false);
            }
            return true;
        }
        if let Some(port) = &self.ports[slot] {
            if port.is_valid() {
                CGEvent::tap_enable(port, true);
                if CGEvent::tap_is_enabled(port) {
                    return true;
                }
            }
            // macOS invalidates a tap when Accessibility is revoked, and a
            // dead port never comes back on: replace it with a fresh tap.
            tracing::info!(?kind, "event tap is dead; recreating it");
            self.discard(kind);
        }
        if !self.create(kind) {
            return false;
        }
        if let Some(port) = &self.ports[slot] {
            CGEvent::tap_enable(port, true);
        }
        self.is_enabled(kind)
    }

    fn is_enabled(&self, kind: TapKind) -> bool {
        self.ports[Self::slot(kind)]
            .as_ref()
            .is_some_and(|port| CGEvent::tap_is_enabled(port))
    }
}

/// Common callback body: always returns the original event (Phase 2 passes
/// everything through). Panics and re-entrant calls degrade to pass-through.
fn dispatch(
    kind: TapKind,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
    on_event: impl FnOnce(&mut dyn TapHandler, &CGEvent),
) -> *mut CGEvent {
    // SAFETY: `user_info` is the `TapContext` pointer given to
    // `CGEventTapCreate`, valid for the input thread's lifetime.
    let context = unsafe { &*(user_info as *const TapContext) };
    guarded(&context.degraded, (), || {
        let Some(handler) = context.handler.get() else {
            return;
        };
        let Ok(mut handler) = handler.try_borrow_mut() else {
            return;
        };
        if event_type == CGEventType::TapDisabledByTimeout {
            handler.on_tap_disabled(kind, DisabledReason::Timeout);
        } else if event_type == CGEventType::TapDisabledByUserInput {
            handler.on_tap_disabled(kind, DisabledReason::UserInput);
        } else {
            // SAFETY: CoreGraphics passes a valid event for the call's duration.
            on_event(&mut *handler, unsafe { event.as_ref() });
        }
    });
    event.as_ptr()
}

unsafe extern "C-unwind" fn scroll_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    dispatch(TapKind::Scroll, event_type, event, user_info, |h, e| {
        h.on_scroll(&scroll_fields(e))
    })
}

unsafe extern "C-unwind" fn button_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    dispatch(TapKind::Buttons, event_type, event, user_info, |h, e| {
        h.on_button(CGEvent::integer_value_field(Some(e), SENDER_ID_FIELD) as u64)
    })
}

unsafe extern "C-unwind" fn flags_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // Modifier state is consumed in Phase 5; Phase 2 only keeps the tap healthy.
    dispatch(TapKind::Flags, event_type, event, user_info, |_, _| {})
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, sync::atomic::Ordering};

    use objc2_core_graphics::CGScrollEventUnit;

    use super::*;

    /// Counts scrolls and can panic on them.
    struct Handler {
        scrolls: Rc<Cell<u32>>,
        panics: bool,
    }

    impl TapHandler for Handler {
        fn on_scroll(&mut self, _: &ScrollFields) {
            self.scrolls.set(self.scrolls.get() + 1);
            if self.panics {
                panic!("handler bug");
            }
        }
        fn on_button(&mut self, _: u64) {}
        fn on_tap_disabled(&mut self, _: TapKind, _: DisabledReason) {}
    }

    fn context(handler: Option<Rc<RefCell<Handler>>>) -> TapContext {
        let context = TapContext {
            handler: OnceCell::new(),
            degraded: Arc::new(AtomicBool::new(false)),
        };
        if let Some(handler) = handler {
            let handler: Rc<RefCell<dyn TapHandler>> = handler;
            assert!(context.handler.set(handler).is_ok());
        }
        context
    }

    fn handler(panics: bool) -> (Rc<RefCell<Handler>>, Rc<Cell<u32>>) {
        let scrolls = Rc::new(Cell::new(0));
        let handler = Rc::new(RefCell::new(Handler {
            scrolls: Rc::clone(&scrolls),
            panics,
        }));
        (handler, scrolls)
    }

    /// Feeds a real scroll event to the scroll tap callback.
    fn scroll_through(context: &TapContext) -> (*mut CGEvent, *mut CGEvent) {
        let event =
            CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Line, 1, -1, 0, 0).unwrap();
        let event = NonNull::from(&*event);
        // SAFETY: a real event and a live `TapContext`, as CoreGraphics would
        // pass them; the proxy is unused.
        let returned = unsafe {
            scroll_callback(
                std::ptr::null_mut(),
                CGEventType::ScrollWheel,
                event,
                (context as *const TapContext).cast_mut().cast(),
            )
        };
        (event.as_ptr(), returned)
    }

    #[test]
    fn a_panicking_handler_passes_the_event_through_and_marks_degraded() {
        let (handler, scrolls) = handler(true);
        let context = context(Some(handler));

        let (sent, returned) = scroll_through(&context);

        assert_eq!(returned, sent);
        assert_eq!(scrolls.get(), 1);
        assert!(context.degraded.load(Ordering::SeqCst));
    }

    #[test]
    fn a_re_entrant_call_passes_through_without_calling_the_handler() {
        let (handler, scrolls) = handler(false);
        let context = context(Some(Rc::clone(&handler)));
        let _busy = handler.borrow_mut();

        let (sent, returned) = scroll_through(&context);

        assert_eq!(returned, sent);
        assert_eq!(scrolls.get(), 0);
        assert!(!context.degraded.load(Ordering::SeqCst));
    }

    #[test]
    fn no_handler_yet_passes_the_event_through() {
        let context = context(None);

        let (sent, returned) = scroll_through(&context);

        assert_eq!(returned, sent);
        assert!(!context.degraded.load(Ordering::SeqCst));
    }

    #[test]
    fn a_healthy_handler_sees_the_scroll_and_the_event_passes_through() {
        let (handler, scrolls) = handler(false);
        let context = context(Some(handler));

        let (sent, returned) = scroll_through(&context);

        assert_eq!(returned, sent);
        assert_eq!(scrolls.get(), 1);
    }

    #[test]
    fn button_tap_listens_to_other_mouse_down_and_up_only() {
        assert_eq!(event_mask(TapKind::Buttons), (1 << 25) | (1 << 26));
    }

    #[test]
    fn scroll_tap_listens_to_scroll_wheel_only() {
        assert_eq!(event_mask(TapKind::Scroll), 1 << 22);
    }

    #[test]
    fn flags_tap_listens_to_flags_changed_only() {
        assert_eq!(event_mask(TapKind::Flags), 1 << 12);
    }

    #[test]
    fn only_the_flags_tap_is_listen_only() {
        assert_eq!(tap_options(TapKind::Flags), CGEventTapOptions::ListenOnly);
        assert_eq!(tap_options(TapKind::Buttons), CGEventTapOptions::Default);
        assert_eq!(tap_options(TapKind::Scroll), CGEventTapOptions::Default);
    }

    #[test]
    fn scroll_fields_are_copied_from_a_real_event() {
        let event =
            CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Line, 2, -3, 1, 0).unwrap();
        CGEvent::set_integer_value_field(Some(&event), CGEventField::EventSourceUserData, 42);
        CGEvent::set_integer_value_field(Some(&event), SENDER_ID_FIELD, 12345);

        let fields = scroll_fields(&event);

        assert_eq!(fields.delta_axis1, -3);
        assert_eq!(fields.delta_axis2, 1);
        assert_eq!(fields.user_data, 42);
        assert_eq!(fields.sender_id, 12345);
        assert_eq!(fields.is_continuous, 0, "line events are discrete");
    }

    /// Our synthetic smooth-scroll events (Phase 5) are pixel-based; the
    /// filter relies on them being continuous so the tap ignores them.
    #[test]
    fn pixel_events_are_continuous() {
        let event =
            CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 1, -10, 0, 0).unwrap();
        assert_eq!(scroll_fields(&event).is_continuous, 1);
    }
}

//! CGEventTap adapter (spec §4.1, §4.5). Humble object: creates taps,
//! copies fields out of events, forwards to a `TapHandler`. No decisions.
//! Phase 4a: the button tap may swallow; only after the router says the
//! event is ours (AGENTS.md footgun 2). Phase 4b: the keys tap, on only
//! while the shortcut recorder runs, may swallow when the recorder says so.

use std::{
    cell::{OnceCell, RefCell},
    ffi::{c_ulong, c_void},
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
    key_recorder::{KeyEvent, KeyKind},
    router::{ButtonEvent, ButtonKind, Verdict},
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
        TapKind::Keys => bit(CGEventType::KeyDown) | bit(CGEventType::KeyUp),
    }
}

pub fn tap_options(kind: TapKind) -> CGEventTapOptions {
    match kind {
        TapKind::Flags => CGEventTapOptions::ListenOnly,
        TapKind::Buttons | TapKind::Scroll | TapKind::Keys => CGEventTapOptions::Default,
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

/// Copies a button tap event's fields. A button number that doesn't fit a
/// `u8` saturates to 255, which the router passes as out of range.
pub fn button_event(event_type: CGEventType, event: &CGEvent) -> ButtonEvent {
    let get = |field| CGEvent::integer_value_field(Some(event), field);
    ButtonEvent {
        kind: if event_type == CGEventType::OtherMouseDown {
            ButtonKind::Down
        } else {
            ButtonKind::Up
        },
        button: u8::try_from(get(CGEventField::MouseEventButtonNumber)).unwrap_or(u8::MAX),
        sender_id: get(SENDER_ID_FIELD) as u64,
        user_data: get(CGEventField::EventSourceUserData),
        mouse_subtype: get(CGEventField::MouseEventSubtype),
    }
}

/// Copies a keys tap event's fields into a stack buffer: no allocation, so
/// it is safe on the hot path. A key code that doesn't fit a `u16`
/// saturates to `u16::MAX`, which the recorder passes like any key >= 128.
pub fn key_event(event_type: CGEventType, event: &CGEvent) -> KeyEvent {
    let get = |field| CGEvent::integer_value_field(Some(event), field);
    let mut chars = [0u16; 4];
    let mut len: c_ulong = 0;
    // SAFETY: both out-pointers point into live stack locals, and the max
    // length we pass is the buffer's length, so CoreGraphics can't overrun it.
    unsafe {
        CGEvent::keyboard_get_unicode_string(
            Some(event),
            chars.len() as c_ulong,
            &mut len,
            chars.as_mut_ptr(),
        );
    }
    KeyEvent {
        kind: if event_type == CGEventType::KeyDown {
            KeyKind::Down
        } else {
            KeyKind::Up
        },
        key_code: u16::try_from(get(CGEventField::KeyboardEventKeycode)).unwrap_or(u16::MAX),
        flags: CGEvent::flags(Some(event)).0,
        chars,
        // Clamped in case CoreGraphics ever reports more than it wrote.
        chars_len: len.min(chars.len() as c_ulong) as u8,
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
    ports: [Option<CFRetained<CFMachPort>>; 4],
}

impl MacTapController {
    /// `context` must outlive the controller. Dropping the controller disables
    /// and invalidates its taps, so no callback can fire after that.
    pub fn new(context: *const TapContext, run_loop: CFRetained<CFRunLoop>) -> Self {
        Self {
            context,
            run_loop,
            ports: [None, None, None, None],
        }
    }

    fn slot(kind: TapKind) -> usize {
        match kind {
            TapKind::Buttons => 0,
            TapKind::Scroll => 1,
            TapKind::Flags => 2,
            TapKind::Keys => 3,
        }
    }

    fn create(&mut self, kind: TapKind) -> bool {
        let callback = match kind {
            TapKind::Buttons => button_callback as _,
            TapKind::Scroll => scroll_callback as _,
            TapKind::Flags => flags_callback as _,
            TapKind::Keys => key_callback as _,
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

// Verified by review, not tests: creating, enabling or rebuilding a real tap
// (`set_enabled`, `revive`) needs Accessibility, which test binaries don't have.
impl TapController for MacTapController {
    /// Only called by the watchdog timer, outside every tap callout, so
    /// discarding a valid port here can't pull it out from under CoreGraphics.
    fn revive(&mut self, kind: TapKind) -> bool {
        if self.set_enabled(kind, true) {
            return true;
        }
        // `set_enabled` returns false for two reasons: the slot is empty or its
        // create just failed (nothing to rebuild), or a valid port won't re-enable.
        let stuck = matches!(&self.ports[Self::slot(kind)], Some(port) if port.is_valid());
        if !stuck {
            return false;
        }
        tracing::info!(?kind, "event tap won't re-enable; rebuilding it");
        self.discard(kind);
        if !self.create(kind) {
            return false;
        }
        if let Some(port) = &self.ports[Self::slot(kind)] {
            CGEvent::tap_enable(port, true);
        }
        self.is_enabled(kind)
    }

    /// May run inside the tap's own callout (`on_tap_disabled`), so it never
    /// discards a valid port. Only an already-invalid port is replaced: CF
    /// never runs callouts for an invalid port.
    fn set_enabled(&mut self, kind: TapKind, enabled: bool) -> bool {
        let slot = Self::slot(kind);
        if !enabled {
            if let Some(port) = &self.ports[slot] {
                CGEvent::tap_enable(port, false);
            }
            return true;
        }
        match &self.ports[slot] {
            Some(port) if port.is_valid() => {
                CGEvent::tap_enable(port, true);
                return self.is_enabled(kind);
            }
            Some(_) => {
                // macOS invalidates a tap when Accessibility is revoked, and a
                // dead port never comes back on: replace it with a fresh tap.
                tracing::info!(?kind, "event tap is invalid; recreating it");
                self.discard(kind);
            }
            None => {}
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

/// Common callback body: returns NULL (swallow) only when `on_event` says
/// `Swallow`; otherwise the original event. Panics, re-entrant calls, a
/// missing handler and tap-disabled notices all pass through.
fn dispatch(
    kind: TapKind,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
    on_event: impl FnOnce(&mut dyn TapHandler, &CGEvent) -> Verdict,
) -> *mut CGEvent {
    // SAFETY: `user_info` is the `TapContext` pointer given to
    // `CGEventTapCreate`, valid for the input thread's lifetime.
    let context = unsafe { &*(user_info as *const TapContext) };
    let verdict = guarded(&context.degraded, Verdict::Pass, || {
        let Some(handler) = context.handler.get() else {
            return Verdict::Pass;
        };
        let Ok(mut handler) = handler.try_borrow_mut() else {
            return Verdict::Pass;
        };
        if event_type == CGEventType::TapDisabledByTimeout {
            handler.on_tap_disabled(kind, DisabledReason::Timeout);
            Verdict::Pass
        } else if event_type == CGEventType::TapDisabledByUserInput {
            handler.on_tap_disabled(kind, DisabledReason::UserInput);
            Verdict::Pass
        } else {
            // SAFETY: CoreGraphics passes a valid event for the call's duration.
            on_event(&mut *handler, unsafe { event.as_ref() })
        }
    });
    match verdict {
        Verdict::Swallow => std::ptr::null_mut(),
        Verdict::Pass => event.as_ptr(),
    }
}

unsafe extern "C-unwind" fn scroll_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // Scroll events are only observed until Phase 5; they always pass.
    dispatch(TapKind::Scroll, event_type, event, user_info, |h, e| {
        h.on_scroll(&scroll_fields(e));
        Verdict::Pass
    })
}

unsafe extern "C-unwind" fn button_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    dispatch(TapKind::Buttons, event_type, event, user_info, |h, e| {
        h.on_button(&button_event(event_type, e))
    })
}

unsafe extern "C-unwind" fn flags_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // Modifier state is consumed in Phase 5; Phase 2 only keeps the tap healthy.
    dispatch(TapKind::Flags, event_type, event, user_info, |_, _| {
        Verdict::Pass
    })
}

unsafe extern "C-unwind" fn key_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // Only on while the shortcut recorder runs; the recorder decides what to swallow.
    dispatch(TapKind::Keys, event_type, event, user_info, |h, e| {
        h.on_key(&key_event(event_type, e))
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, sync::atomic::Ordering};

    use objc2_core_foundation::CGPoint;
    use objc2_core_graphics::{
        CGEventFlags, CGEventMouseSubtype, CGMouseButton, CGScrollEventUnit,
    };

    use crate::input::router::TimerKind;

    use super::*;

    /// Counts scrolls and can panic on them; answers buttons with `verdict`.
    struct Handler {
        scrolls: Rc<Cell<u32>>,
        panics: bool,
        verdict: Verdict,
        button_panics: bool,
        key_verdict: Verdict,
        key_panics: bool,
    }

    impl TapHandler for Handler {
        fn on_scroll(&mut self, _: &ScrollFields) {
            self.scrolls.set(self.scrolls.get() + 1);
            if self.panics {
                panic!("handler bug");
            }
        }
        fn on_button(&mut self, _: &ButtonEvent) -> Verdict {
            if self.button_panics {
                panic!("handler bug");
            }
            self.verdict
        }
        fn on_click_timer(&mut self, _: TimerKind) {}
        fn on_key(&mut self, _: &KeyEvent) -> Verdict {
            if self.key_panics {
                panic!("handler bug");
            }
            self.key_verdict
        }
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
            verdict: Verdict::Pass,
            button_panics: false,
            key_verdict: Verdict::Pass,
            key_panics: false,
        }));
        (handler, scrolls)
    }

    fn button_handler(verdict: Verdict, button_panics: bool) -> Rc<RefCell<Handler>> {
        Rc::new(RefCell::new(Handler {
            scrolls: Rc::new(Cell::new(0)),
            panics: false,
            verdict,
            button_panics,
            key_verdict: Verdict::Pass,
            key_panics: false,
        }))
    }

    fn key_handler(verdict: Verdict, panics: bool) -> Rc<RefCell<Handler>> {
        Rc::new(RefCell::new(Handler {
            scrolls: Rc::new(Cell::new(0)),
            panics: false,
            verdict: Verdict::Pass,
            button_panics: false,
            key_verdict: verdict,
            key_panics: panics,
        }))
    }

    /// Sets the characters a key event says it typed.
    fn set_unicode(event: &CGEvent, chars: &[u16]) {
        // SAFETY: `chars` is a live slice and we pass its exact length.
        unsafe {
            CGEvent::keyboard_set_unicode_string(Some(event), chars.len() as _, chars.as_ptr())
        };
    }

    /// Feeds a real key event to the keys tap callback.
    fn key_through(context: &TapContext, event_type: CGEventType) -> (*mut CGEvent, *mut CGEvent) {
        let event =
            CGEvent::new_keyboard_event(None, 0x01, event_type == CGEventType::KeyDown).unwrap();
        let ptr = NonNull::from(&*event);
        // SAFETY: a real event and a live `TapContext`, as CoreGraphics would pass them.
        let returned = unsafe {
            key_callback(
                std::ptr::null_mut(),
                event_type,
                ptr,
                (context as *const TapContext).cast_mut().cast(),
            )
        };
        (ptr.as_ptr(), returned) // `event` is still alive here, so `ptr` is valid
    }

    #[test]
    fn keys_tap_listens_to_key_down_and_up_only_and_can_swallow() {
        assert_eq!(event_mask(TapKind::Keys), (1 << 10) | (1 << 11));
        assert_eq!(tap_options(TapKind::Keys), CGEventTapOptions::Default);
    }

    #[test]
    fn key_event_fields_are_copied_from_a_real_event() {
        let e = CGEvent::new_keyboard_event(None, 0x01, true).unwrap();
        CGEvent::set_flags(Some(&e), CGEventFlags(0x120000));
        // Set the typed character explicitly so the test doesn't depend on the keyboard layout.
        set_unicode(&e, &[0x73]);
        let k = key_event(CGEventType::KeyDown, &e);
        assert_eq!(
            (
                k.kind,
                k.key_code,
                k.flags & 0x1E0000,
                k.chars[0],
                k.chars_len,
                k.user_data
            ),
            (KeyKind::Down, 0x01, 0x120000, 0x73, 1, 0)
        );
    }

    #[test]
    fn a_real_key_up_event_is_read_as_up() {
        let e = CGEvent::new_keyboard_event(None, 0x01, false).unwrap();
        assert_eq!(key_event(CGEventType::KeyUp, &e).kind, KeyKind::Up);
    }

    #[test]
    fn a_swallowing_key_handler_makes_the_key_callback_return_null() {
        let context = context(Some(key_handler(Verdict::Swallow, false)));
        let (_, returned) = key_through(&context, CGEventType::KeyDown);
        assert!(returned.is_null());
    }

    #[test]
    fn a_passing_key_handler_returns_the_key_event() {
        let context = context(Some(key_handler(Verdict::Pass, false)));
        let (sent, returned) = key_through(&context, CGEventType::KeyUp);
        assert_eq!(returned, sent);
    }

    #[test]
    fn a_panicking_key_handler_passes_the_key_through() {
        let context = context(Some(key_handler(Verdict::Swallow, true)));
        let (sent, returned) = key_through(&context, CGEventType::KeyDown);
        assert_eq!(returned, sent);
        assert!(context.degraded.load(Ordering::SeqCst));
    }

    /// Feeds a real middle-button event to the button tap callback.
    fn button_through(
        context: &TapContext,
        event_type: CGEventType,
    ) -> (*mut CGEvent, *mut CGEvent) {
        let event =
            CGEvent::new_mouse_event(None, event_type, CGPoint::ZERO, CGMouseButton(2)).unwrap();
        let sent = NonNull::from(&*event);
        // SAFETY: a real event (kept alive by `event` until we return) and a
        // live `TapContext`, as CoreGraphics would pass them; the proxy is unused.
        let returned = unsafe {
            button_callback(
                std::ptr::null_mut(),
                event_type,
                sent,
                (context as *const TapContext).cast_mut().cast(),
            )
        };
        (sent.as_ptr(), returned)
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
    fn a_swallowing_handler_makes_the_button_callback_return_null() {
        let context = context(Some(button_handler(Verdict::Swallow, false)));
        let (_, returned) = button_through(&context, CGEventType::OtherMouseDown);
        assert!(returned.is_null());
    }

    #[test]
    fn a_passing_handler_returns_the_button_event() {
        let context = context(Some(button_handler(Verdict::Pass, false)));
        let (sent, returned) = button_through(&context, CGEventType::OtherMouseDown);
        assert_eq!(returned, sent);
    }

    #[test]
    fn a_panicking_button_handler_passes_the_event_through() {
        let context = context(Some(button_handler(Verdict::Swallow, true)));
        let (sent, returned) = button_through(&context, CGEventType::OtherMouseDown);
        assert_eq!(returned, sent);
        assert!(context.degraded.load(Ordering::SeqCst));
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

    #[test]
    fn button_event_fields_are_copied_from_a_real_event() {
        let e = CGEvent::new_mouse_event(
            None,
            CGEventType::OtherMouseUp,
            CGPoint::ZERO,
            CGMouseButton(3),
        )
        .unwrap();
        CGEvent::set_integer_value_field(Some(&e), SENDER_ID_FIELD, 777);
        CGEvent::set_integer_value_field(
            Some(&e),
            CGEventField::MouseEventSubtype,
            CGEventMouseSubtype::TabletPoint.0.into(),
        );
        assert_eq!(
            button_event(CGEventType::OtherMouseUp, &e),
            ButtonEvent {
                kind: ButtonKind::Up,
                button: 3,
                sender_id: 777,
                user_data: 0,
                mouse_subtype: 1,
            }
        );
    }

    #[test]
    fn a_down_with_user_data_is_copied_as_a_down() {
        let e = CGEvent::new_mouse_event(
            None,
            CGEventType::OtherMouseDown,
            CGPoint::ZERO,
            CGMouseButton(2),
        )
        .unwrap();
        CGEvent::set_integer_value_field(Some(&e), CGEventField::EventSourceUserData, 42);
        let fields = button_event(CGEventType::OtherMouseDown, &e);
        assert_eq!(
            (fields.kind, fields.button, fields.user_data),
            (ButtonKind::Down, 2, 42)
        );
    }

    /// CoreGraphics stores the button number as a signed byte: setting 258
    /// reads back 2, and 255 reads back -1. So the out-of-range values a real
    /// event can carry are negative, and they must not wrap into a valid CG
    /// button number.
    #[test]
    fn a_button_number_that_doesnt_fit_u8_saturates_instead_of_wrapping() {
        let e = CGEvent::new_mouse_event(
            None,
            CGEventType::OtherMouseDown,
            CGPoint::ZERO,
            CGMouseButton(3),
        )
        .unwrap();
        CGEvent::set_integer_value_field(Some(&e), CGEventField::MouseEventButtonNumber, -126);
        // `as u8` would give 130.
        assert_eq!(button_event(CGEventType::OtherMouseDown, &e).button, 255);
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

//! CGEventTap adapter (spec §4.1, §4.5). Humble object: creates taps,
//! copies fields out of events, forwards to a `TapHandler`. No decisions.

// Wired into the input thread in Task 8, which removes this allow.
#![allow(dead_code)]

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
    /// `context` must stay valid for as long as this controller's taps exist.
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
}

impl TapController for MacTapController {
    fn set_enabled(&mut self, kind: TapKind, enabled: bool) -> bool {
        if enabled && self.ports[Self::slot(kind)].is_none() && !self.create(kind) {
            return false;
        }
        if let Some(port) = &self.ports[Self::slot(kind)] {
            CGEvent::tap_enable(port, enabled);
        }
        true
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
    use objc2_core_graphics::CGScrollEventUnit;

    use super::*;

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

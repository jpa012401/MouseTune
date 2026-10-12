//! Synthetic key combos and mouse clicks for the action executor (spec §7.2,
//! §7.4). Every event is stamped with `MOUSETUNE_EVENT_TAG` and posted at the
//! session tap, downstream of our HID taps, so they never re-enter them
//! (spec §4.1). Humble object: no decisions beyond mapping a failed
//! constructor to an error.

use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    CGEventType, CGMouseButton,
};

use crate::{
    actions::{OutputError, keys::KeyStroke, perform::EventPoster},
    input::filter::MOUSETUNE_EVENT_TAG,
};

pub struct MacEventPoster;

impl MacEventPoster {
    /// Fresh per call: jobs are rare, and a local source keeps the poster
    /// free of non-`Send` state.
    fn source() -> Result<CFRetained<CGEventSource>, OutputError> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .ok_or(OutputError::Os("CGEventSourceCreate"))
    }

    fn post_all(events: &[CFRetained<CGEvent>]) {
        for event in events {
            CGEvent::set_integer_value_field(
                Some(event),
                CGEventField::EventSourceUserData,
                MOUSETUNE_EVENT_TAG,
            );
            CGEvent::post(CGEventTapLocation::SessionEventTap, Some(event));
        }
    }
}

impl EventPoster for MacEventPoster {
    /// Posts the strokes in order. Every event is created before any is
    /// posted: if one constructor fails, nothing is posted and this returns
    /// `Err`, so a failure can never leave a modifier key held down.
    fn post_keys(&self, strokes: &[KeyStroke]) -> Result<(), OutputError> {
        let source = Self::source()?;
        let events = strokes
            .iter()
            .map(|stroke| {
                let event =
                    CGEvent::new_keyboard_event(Some(&source), stroke.key_code, stroke.down)
                        .ok_or(OutputError::Os("CGEventCreateKeyboardEvent"))?;
                CGEvent::set_flags(Some(&event), CGEventFlags(stroke.flags.0));
                Ok(event)
            })
            .collect::<Result<Vec<_>, OutputError>>()?;
        Self::post_all(&events);
        Ok(())
    }

    /// Clicks `cg_button` at the cursor. Both the down and the up are created
    /// before either is posted, so a failure can't leave the button held.
    fn click(&self, cg_button: u8) -> Result<(), OutputError> {
        let source = Self::source()?;
        let here = CGEvent::new(None).ok_or(OutputError::Os("CGEventCreate"))?;
        let location = CGEvent::location(Some(&here));
        let make = |kind| {
            let event = CGEvent::new_mouse_event(
                Some(&source),
                kind,
                location,
                CGMouseButton(u32::from(cg_button)),
            )
            .ok_or(OutputError::Os("CGEventCreateMouseEvent"))?;
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::MouseEventButtonNumber,
                i64::from(cg_button),
            );
            Ok(event)
        };
        let down = make(CGEventType::OtherMouseDown)?;
        let up = make(CGEventType::OtherMouseUp)?;
        Self::post_all(&[down, up]);
        Ok(())
    }
}

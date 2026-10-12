//! Synthetic key combos, mouse clicks and media keys for the action executor
//! (spec §7.2, §7.4). Every event is stamped with `MOUSETUNE_EVENT_TAG` and
//! posted at the session tap, downstream of our HID taps, so they never
//! re-enter them (spec §4.1). Humble object: no decisions beyond mapping a failed
//! constructor to an error.

use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_core_foundation::{CFRetained, CGPoint};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    CGEventType, CGMouseButton,
};

use crate::{
    actions::{OutputError, keys::KeyStroke, perform::EventPoster},
    config::schema::SystemKey,
    input::filter::MOUSETUNE_EVENT_TAG,
};

/// Builds one button event, untagged and unposted, so tests can read it back.
fn mouse_event(
    source: Option<&CGEventSource>,
    cg_button: u8,
    down: bool,
    click_state: i64,
    at: CGPoint,
) -> Result<CFRetained<CGEvent>, OutputError> {
    // Left and right have their own event types; apps ignore an "other"
    // event for buttons 0 and 1.
    let kind = match (cg_button, down) {
        (0, true) => CGEventType::LeftMouseDown,
        (0, false) => CGEventType::LeftMouseUp,
        (1, true) => CGEventType::RightMouseDown,
        (1, false) => CGEventType::RightMouseUp,
        (_, true) => CGEventType::OtherMouseDown,
        (_, false) => CGEventType::OtherMouseUp,
    };
    let event = CGEvent::new_mouse_event(source, kind, at, CGMouseButton(u32::from(cg_button)))
        .ok_or(OutputError::Os("CGEventCreateMouseEvent"))?;
    CGEvent::set_integer_value_field(
        Some(&event),
        CGEventField::MouseEventButtonNumber,
        i64::from(cg_button),
    );
    CGEvent::set_integer_value_field(
        Some(&event),
        CGEventField::MouseEventClickState,
        click_state,
    );
    Ok(event)
}

/// Down/up pairs with click state 1, 2, … so apps see a real multi-click.
pub fn click_events(
    source: Option<&CGEventSource>,
    cg_button: u8,
    clicks: u8,
    at: CGPoint,
) -> Result<Vec<CFRetained<CGEvent>>, OutputError> {
    let mut events = Vec::with_capacity(usize::from(clicks) * 2);
    for state in 1..=i64::from(clicks.max(1)) {
        events.push(mouse_event(source, cg_button, true, state, at)?);
        events.push(mouse_event(source, cg_button, false, state, at)?);
    }
    Ok(events)
}

/// `NX_KEYTYPE_*` from `<IOKit/hidsystem/ev_keymap.h>`.
fn nx_key_type(key: SystemKey) -> isize {
    match key {
        SystemKey::VolumeUp => 0,
        SystemKey::VolumeDown => 1,
        SystemKey::BrightnessUp => 2,
        SystemKey::BrightnessDown => 3,
        SystemKey::Mute => 7,
        SystemKey::PlayPause => 16,
        SystemKey::Next => 17,
        SystemKey::Previous => 18,
    }
}

/// `NX_SUBTYPE_AUX_CONTROL_BUTTONS`.
const AUX_CONTROL_BUTTONS: i16 = 8;

/// A media/brightness key as the system-defined event macOS's own keys send
/// (spec §7.4), untagged and unposted.
///
/// `mt-exec` never drains an autorelease pool of its own, so AppKit's
/// autoreleased temporaries are released here instead of accumulating for
/// the app's lifetime. The returned event is owned and outlives the pool.
pub fn system_key_event(key: SystemKey, down: bool) -> Option<Retained<CGEvent>> {
    let state: isize = if down { 0xA } else { 0xB };
    objc2::rc::autoreleasepool(|_| {
        let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::SystemDefined,
            CGPoint::new(0.0, 0.0),
            NSEventModifierFlags::from_bits_retain((state << 8) as usize),
            0.0,
            0,
            None,
            AUX_CONTROL_BUTTONS,
            (nx_key_type(key) << 16) | (state << 8),
            -1,
        )?;
        event.CGEvent()
    })
}

pub struct MacEventPoster;

impl MacEventPoster {
    /// Fresh per call: jobs are rare, and a local source keeps the poster
    /// free of non-`Send` state.
    fn source() -> Result<CFRetained<CGEventSource>, OutputError> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .ok_or(OutputError::Os("CGEventSourceCreate"))
    }

    fn post_all(events: &[&CGEvent]) {
        for &event in events {
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
        Self::post_all(&events.iter().map(|e| &**e).collect::<Vec<_>>());
        Ok(())
    }

    /// Clicks `cg_button` at the cursor. Every down and up is created before
    /// any is posted, so a failure can't leave the button held.
    fn click(&self, cg_button: u8, clicks: u8) -> Result<(), OutputError> {
        let source = Self::source()?;
        let here = CGEvent::new(None).ok_or(OutputError::Os("CGEventCreate"))?;
        let location = CGEvent::location(Some(&here));
        let events = click_events(Some(&source), cg_button, clicks, location)?;
        Self::post_all(&events.iter().map(|e| &**e).collect::<Vec<_>>());
        Ok(())
    }

    /// Both the down and the up are created before either is posted, so a
    /// failure can't leave the key held.
    fn post_system_key(&self, key: SystemKey) -> Result<(), OutputError> {
        let make =
            |down| system_key_event(key, down).ok_or(OutputError::Os("NSEvent systemDefined"));
        let down = make(true)?;
        let up = make(false)?;
        Self::post_all(&[&down, &up]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use objc2_app_kit::{NSEvent, NSEventType};
    use objc2_core_foundation::CGPoint;
    use objc2_core_graphics::{CGEvent, CGEventField, CGEventType};

    use super::*;
    use crate::config::schema::SystemKey;

    fn kinds_and_states(events: &[CFRetained<CGEvent>]) -> Vec<(CGEventType, i64)> {
        events
            .iter()
            .map(|e| {
                (
                    CGEvent::r#type(Some(e)),
                    CGEvent::integer_value_field(Some(e), CGEventField::MouseEventClickState),
                )
            })
            .collect()
    }

    #[test]
    fn a_double_middle_click_is_two_clicks_with_rising_click_state() {
        let events = click_events(None, 2, 2, CGPoint::ZERO).unwrap();
        assert_eq!(
            kinds_and_states(&events),
            [
                (CGEventType::OtherMouseDown, 1),
                (CGEventType::OtherMouseUp, 1),
                (CGEventType::OtherMouseDown, 2),
                (CGEventType::OtherMouseUp, 2),
            ]
        );
        for e in &events {
            assert_eq!(
                CGEvent::integer_value_field(Some(e), CGEventField::MouseEventButtonNumber),
                2
            );
        }
    }

    #[test]
    fn a_left_click_uses_left_mouse_events() {
        let events = click_events(None, 0, 1, CGPoint::ZERO).unwrap();
        assert_eq!(
            kinds_and_states(&events),
            [
                (CGEventType::LeftMouseDown, 1),
                (CGEventType::LeftMouseUp, 1)
            ]
        );
    }

    #[test]
    fn a_right_click_uses_right_mouse_events() {
        let events = click_events(None, 1, 1, CGPoint::ZERO).unwrap();
        assert_eq!(
            kinds_and_states(&events),
            [
                (CGEventType::RightMouseDown, 1),
                (CGEventType::RightMouseUp, 1)
            ]
        );
    }

    #[test]
    fn play_down_is_an_aux_control_button_system_event() {
        let cg = system_key_event(SystemKey::PlayPause, true).expect("event");
        let ns = NSEvent::eventWithCGEvent(&cg).expect("back to an NSEvent");
        assert_eq!(ns.r#type(), NSEventType::SystemDefined);
        assert_eq!(ns.subtype().0 as i64, 8, "NX_SUBTYPE_AUX_CONTROL_BUTTONS");
        // NX_KEYTYPE_PLAY = 16; key down = 0xA in bits 8-15
        assert_eq!(ns.data1() as i64, (16 << 16) | 0xA00);
    }

    #[test]
    fn volume_up_release_carries_the_up_state() {
        let cg = system_key_event(SystemKey::VolumeUp, false).expect("event");
        let ns = NSEvent::eventWithCGEvent(&cg).expect("back to an NSEvent");
        // NX_KEYTYPE_SOUND_UP = 0; key up = 0xB
        assert_eq!(ns.data1() as i64, 0xB00);
    }
}

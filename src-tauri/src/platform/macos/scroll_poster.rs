//! Posts the scroll animator's events (spec §8.4).
//! Every event is stamped with `MOUSETUNE_EVENT_TAG` and posted at the
//! session tap, downstream of our HID taps (spec §4.1). Humble object: maps
//! `ScrollOut` onto CGEvent fields, no decisions. Runs on display frames,
//! so it never logs.

use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    CGMomentumScrollPhase, CGScrollEventUnit, CGScrollPhase,
};

use crate::input::{
    filter::MOUSETUNE_EVENT_TAG,
    scroll::{
        ScrollPoster,
        animator::{GesturePhase, MomentumPhase, ScrollOut},
    },
};

pub struct MacScrollPoster {
    /// Created once on `mt-input` and reused for every frame.
    source: Option<CFRetained<CGEventSource>>,
}

impl MacScrollPoster {
    pub fn new() -> Self {
        Self {
            source: CGEventSource::new(CGEventSourceStateID::HIDSystemState),
        }
    }
}

impl Default for MacScrollPoster {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollPoster for MacScrollPoster {
    /// A failed create skips the event: the frame path can't log.
    fn post(&mut self, event: &ScrollOut) {
        if let Some(e) = scroll_event(self.source.as_deref(), event) {
            CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&e));
        }
    }
}

/// Builds the event without posting it, so tests can read it back. Setting
/// a line delta (DeltaAxisN) makes CoreGraphics rewrite that axis's
/// fixed-point delta (to the line value) and point delta (to an accelerated
/// value); setting fixed or point changes nothing else. So each axis is
/// written line, then fixed, then point.
pub fn scroll_event(
    source: Option<&CGEventSource>,
    out: &ScrollOut,
) -> Option<CFRetained<CGEvent>> {
    let wheel = |v: i64| i32::try_from(v).unwrap_or(0);
    let event = CGEvent::new_scroll_wheel_event2(
        source,
        CGScrollEventUnit::Pixel,
        2,
        wheel(out.dy),
        wheel(out.dx),
        0,
    )?;
    let set = |field, value| CGEvent::set_integer_value_field(Some(&event), field, value);
    let set_fine = |field, value| CGEvent::set_double_value_field(Some(&event), field, value);
    set(CGEventField::ScrollWheelEventIsContinuous, 1);
    set(CGEventField::ScrollWheelEventDeltaAxis1, out.dy_lines);
    set(CGEventField::ScrollWheelEventDeltaAxis2, out.dx_lines);
    set_fine(CGEventField::ScrollWheelEventFixedPtDeltaAxis1, out.dy_fine);
    set_fine(CGEventField::ScrollWheelEventFixedPtDeltaAxis2, out.dx_fine);
    set(CGEventField::ScrollWheelEventPointDeltaAxis1, out.dy);
    set(CGEventField::ScrollWheelEventPointDeltaAxis2, out.dx);
    set(
        CGEventField::ScrollWheelEventScrollPhase,
        scroll_phase(out.phase),
    );
    set(
        CGEventField::ScrollWheelEventMomentumPhase,
        momentum_phase(out.momentum),
    );
    set(CGEventField::EventSourceUserData, MOUSETUNE_EVENT_TAG);
    // A modifier pressed mid-coast must not turn the coast into a zoom.
    CGEvent::set_flags(Some(&event), CGEventFlags(0));
    Some(event)
}

fn scroll_phase(phase: GesturePhase) -> i64 {
    let value = match phase {
        GesturePhase::None => 0,
        GesturePhase::Began => CGScrollPhase::Began.0,
        GesturePhase::Changed => CGScrollPhase::Changed.0,
        GesturePhase::Ended => CGScrollPhase::Ended.0,
    };
    i64::from(value)
}

fn momentum_phase(momentum: MomentumPhase) -> i64 {
    let value = match momentum {
        MomentumPhase::None => CGMomentumScrollPhase::None.0,
        MomentumPhase::Begin => CGMomentumScrollPhase::Begin.0,
        MomentumPhase::Continue => CGMomentumScrollPhase::Continue.0,
        MomentumPhase::End => CGMomentumScrollPhase::End.0,
    };
    i64::from(value)
}

#[cfg(test)]
mod tests {
    use objc2_core_graphics::{CGEvent, CGEventField};

    use super::*;
    use crate::input::scroll::{
        MODIFIER_FLAGS,
        animator::{GesturePhase, MomentumPhase, ScrollOut},
    };

    fn out(phase: GesturePhase, momentum: MomentumPhase) -> ScrollOut {
        ScrollOut {
            dy: -7,
            dx: 0,
            dy_fine: -6.5691,
            dx_fine: 0.0,
            dy_lines: -1,
            dx_lines: 0,
            phase,
            momentum,
        }
    }

    fn int(e: &CGEvent, field: CGEventField) -> i64 {
        CGEvent::integer_value_field(Some(e), field)
    }

    #[test]
    fn a_scroll_out_becomes_a_tagged_continuous_pixel_event() {
        let e = scroll_event(None, &out(GesturePhase::None, MomentumPhase::Begin)).expect("event");
        assert_eq!(int(&e, CGEventField::ScrollWheelEventIsContinuous), 1);
        assert_eq!(int(&e, CGEventField::ScrollWheelEventPointDeltaAxis1), -7);
        assert_eq!(int(&e, CGEventField::ScrollWheelEventPointDeltaAxis2), 0);
        assert_eq!(int(&e, CGEventField::ScrollWheelEventDeltaAxis1), -1);
        let fine =
            CGEvent::double_value_field(Some(&e), CGEventField::ScrollWheelEventFixedPtDeltaAxis1);
        assert!(
            (fine + 6.5691).abs() < 1e-3,
            "fixed point keeps 16 fraction bits: {fine}"
        );
        assert_eq!(int(&e, CGEventField::ScrollWheelEventScrollPhase), 0);
        assert_eq!(int(&e, CGEventField::ScrollWheelEventMomentumPhase), 1);
        assert_eq!(
            int(&e, CGEventField::EventSourceUserData),
            MOUSETUNE_EVENT_TAG
        );
        assert_eq!(
            CGEvent::flags(Some(&e)).0 & MODIFIER_FLAGS,
            0,
            "no modifiers"
        );
    }

    #[test]
    fn phases_map_to_their_core_graphics_values() {
        // kCGScrollPhaseBegan 1, Changed 2, Ended 4; kCGMomentumScrollPhaseContinue 2, End 3
        let cases = [
            (GesturePhase::Began, MomentumPhase::None, 1, 0),
            (GesturePhase::Changed, MomentumPhase::None, 2, 0),
            (GesturePhase::Ended, MomentumPhase::None, 4, 0),
            (GesturePhase::None, MomentumPhase::Continue, 0, 2),
            (GesturePhase::None, MomentumPhase::End, 0, 3),
        ];
        for (phase, momentum, scroll, momentum_value) in cases {
            let e = scroll_event(None, &out(phase, momentum)).expect("event");
            assert_eq!(
                int(&e, CGEventField::ScrollWheelEventScrollPhase),
                scroll,
                "{phase:?}"
            );
            assert_eq!(
                int(&e, CGEventField::ScrollWheelEventMomentumPhase),
                momentum_value,
                "{momentum:?}"
            );
        }
    }
}

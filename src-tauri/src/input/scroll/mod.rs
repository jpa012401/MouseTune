//! Smooth scrolling (Phase 5a design §2–§5). Decides each mouse-wheel tick
//! in the scroll tap callback, then animates swallowed ticks on
//! display-link frames. Pure core: frames and posting are ports, with
//! adapters in `platform::macos::{display_link, scroll_poster}`.

pub mod acceleration;
pub mod animator;
pub mod curves;
pub mod modifiers;
pub mod preview;

use std::time::Instant;

use acceleration::Tick;
use animator::{Animator, FrameOut, ScrollOut};
use modifiers::ModifierKeys;

use crate::{config::schema::Scroll, input::filter::ScrollFields};

/// Shift, Control, Option and Command in `CGEventFlags`. A held key that isn't bound to a scroll effect passes the tick through (Phase 5b design §2).
pub const MODIFIER_FLAGS: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;

/// What the scroll tap does with a wheel event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollVerdict {
    Pass,
    Swallow,
    /// Negate these axes' deltas on the event itself, then pass it.
    Invert {
        vertical: bool,
        horizontal: bool,
    },
}

/// Port: display-paced frames on `mt-input`. `request` is called inside the
/// scroll tap's callout, so an adapter must only flag and signal there.
pub trait FrameDriver {
    fn request(&mut self);
    fn stop(&mut self);
}

/// Port: posts one synthetic scroll event at the session tap, tagged as ours.
pub trait ScrollPoster {
    fn post(&mut self, event: &ScrollOut);
}

pub struct ScrollEngine {
    settings: Scroll,
    animator: Animator,
    keys: ModifierKeys,
    frames: Box<dyn FrameDriver>,
    poster: Box<dyn ScrollPoster>,
}

impl ScrollEngine {
    pub fn new(
        settings: Scroll,
        frames: Box<dyn FrameDriver>,
        poster: Box<dyn ScrollPoster>,
    ) -> Self {
        Self {
            settings,
            animator: Animator::new(settings),
            keys: ModifierKeys::compile(&settings.keyboard_modifiers),
            frames,
            poster,
        }
    }

    /// A tick `classify_scroll` called a mouse wheel. Hot path: O(1), no
    /// allocation, nothing posted (Phase 5b design §2).
    pub fn on_wheel(&mut self, fields: &ScrollFields, now: Instant) -> ScrollVerdict {
        self.wheel(fields, now, false)
    }

    /// `force_horizontal` swaps the tick's axis after it's read, as the
    /// horizontal key does.
    fn wheel(
        &mut self,
        fields: &ScrollFields,
        now: Instant,
        force_horizontal: bool,
    ) -> ScrollVerdict {
        if !self.settings.smooth {
            return self.inverted_in_place();
        }
        let held = fields.flags & MODIFIER_FLAGS;
        if held & !self.keys.bound != 0 {
            // A key with no scroll effect: leave the tick to macOS and apps
            // (⌃ + scroll is Accessibility Zoom).
            if self.animator.end() {
                self.frames.request();
            }
            return self.inverted_in_place();
        }
        let Some(mut tick) = Tick::from_fields(fields) else {
            return ScrollVerdict::Pass;
        };
        if force_horizontal || held & self.keys.horizontal != 0 {
            tick = tick.swapped();
        }
        tick.precise = held & self.keys.precise != 0;
        tick.swift = held & self.keys.swift != 0;
        self.animator.tick(tick, now);
        self.frames.request();
        ScrollVerdict::Swallow
    }

    /// The key bound to zoom is held. Hot path: O(1).
    pub fn zoom_key_held(&self, flags: u64) -> bool {
        self.keys.zoom != 0 && flags & self.keys.zoom != 0
    }

    /// A tick taken by a held button's Horizontal binding: smoothed sideways.
    /// Needs smoothing: with it off the tick passes untouched (spec §8.5).
    pub fn on_wheel_horizontal(&mut self, fields: &ScrollFields, now: Instant) -> ScrollVerdict {
        if !self.settings.smooth {
            return ScrollVerdict::Pass;
        }
        self.wheel(fields, now, true)
    }

    /// One display frame: post its events, and stop frames once idle.
    pub fn on_frame(&mut self, now: Instant) {
        let out = self.animator.frame(now);
        self.post(&out);
        if !out.wants_frames {
            self.frames.stop();
        }
    }

    /// Ends any gesture now, so no app is left mid-gesture. Posts, so never
    /// from inside a tap callout.
    pub fn cancel(&mut self) {
        if !self.animator.is_active() {
            return;
        }
        let out = self.animator.cancel();
        self.post(&out);
        self.frames.stop();
    }

    pub fn set_settings(&mut self, settings: Scroll) {
        self.cancel();
        self.settings = settings;
        self.animator.set_settings(settings);
        self.keys = ModifierKeys::compile(&settings.keyboard_modifiers);
    }

    fn inverted_in_place(&self) -> ScrollVerdict {
        let s = &self.settings;
        if s.invert_vertical || s.invert_horizontal {
            ScrollVerdict::Invert {
                vertical: s.invert_vertical,
                horizontal: s.invert_horizontal,
            }
        } else {
            ScrollVerdict::Pass
        }
    }

    fn post(&mut self, out: &FrameOut) {
        for event in out.events() {
            self.poster.post(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        config::schema::Smoothness,
        test_support::{FakeFrames, FakeScrollPoster},
    };

    const CONTROL: u64 = 0x0004_0000;

    const SHIFT: u64 = 0x2_0000;
    const OPTION: u64 = 0x8_0000;

    fn held(flags: u64) -> ScrollFields {
        ScrollFields { flags, ..wheel(1) }
    }

    fn after_one_tick(r: &mut Rig, fields: ScrollFields) -> Vec<String> {
        assert_eq!(r.scroll.on_wheel(&fields, r.ms(0)), ScrollVerdict::Swallow);
        r.scroll.on_frame(r.ms(0));
        r.scroll.on_frame(r.ms(90));
        r.posted.log()
    }

    #[test]
    fn shift_scrolls_horizontally_and_obeys_invert_horizontal() {
        let mut r = rig(Scroll {
            invert_horizontal: true,
            ..low()
        });
        assert_eq!(
            after_one_tick(&mut r, held(SHIFT)),
            ["Began", "Changed dx -90", "Ended"]
        );
    }

    #[test]
    fn shift_and_option_combine_horizontal_and_precise() {
        // A first tick is the slowest (x = 0): precise 1 + 19·0 = 1 px.
        let mut r = rig(low());
        assert_eq!(
            after_one_tick(&mut r, held(SHIFT | OPTION)),
            ["Began", "Changed dx 1", "Ended"]
        );
    }

    #[test]
    fn an_unbound_key_passes_even_with_bound_keys_held() {
        let mut r = rig(low());
        assert_eq!(
            r.scroll.on_wheel(&held(SHIFT | CONTROL), r.ms(0)),
            ScrollVerdict::Pass
        );
        assert!(r.frames.calls().is_empty());
    }

    #[test]
    fn with_smoothing_off_modifiers_do_nothing() {
        let mut r = rig(Scroll {
            smooth: false,
            ..low()
        });
        assert_eq!(
            r.scroll.on_wheel(&held(SHIFT), r.ms(0)),
            ScrollVerdict::Pass
        );
        let mut r = rig(Scroll {
            smooth: false,
            invert_vertical: true,
            ..low()
        });
        assert_eq!(
            r.scroll.on_wheel(&held(SHIFT), r.ms(0)),
            ScrollVerdict::Invert {
                vertical: true,
                horizontal: false
            },
            "inverted as delivered, not swapped"
        );
    }

    #[test]
    fn a_bound_swift_key_doubles_the_distance() {
        // 90 px × max(2.0, 1.0) = 180
        let mut s = low();
        s.keyboard_modifiers.swift = Some(crate::config::schema::KeyModifier::Control);
        let mut r = rig(s);
        assert_eq!(
            after_one_tick(&mut r, held(CONTROL)),
            ["Began", "Changed dy 180", "Ended"]
        );
    }

    #[test]
    fn the_precise_setting_makes_a_slow_tick_fine() {
        // 1 + (180 − 1)·0² = 1 px
        let mut r = rig(Scroll {
            precise: true,
            ..low()
        });
        assert_eq!(
            after_one_tick(&mut r, wheel(1)),
            ["Began", "Changed dy 1", "Ended"]
        );
    }

    #[test]
    fn pressing_shift_mid_gesture_closes_it_and_begins_a_horizontal_one() {
        // The axis change starts over at 90 px; 10 ms into the new curve:
        // 90·(1 − (8/9)³) = 26.79 → 27.
        let mut r = rig(low());
        r.scroll.on_wheel(&wheel(1), r.ms(0));
        r.scroll.on_frame(r.ms(0));
        assert_eq!(
            r.scroll.on_wheel(&held(SHIFT), r.ms(20)),
            ScrollVerdict::Swallow
        );
        r.scroll.on_frame(r.ms(30));
        assert_eq!(r.posted.log(), ["Began", "Ended", "Began dx 27"]);
    }

    #[test]
    fn new_settings_rebind_the_keys() {
        let mut r = rig(low());
        let mut s = low();
        s.keyboard_modifiers.horizontal = None;
        r.scroll.set_settings(s);
        assert_eq!(
            r.scroll.on_wheel(&held(SHIFT), r.ms(0)),
            ScrollVerdict::Pass,
            "Shift is unbound now, so it passes"
        );
    }

    struct Rig {
        scroll: ScrollEngine,
        frames: FakeFrames,
        posted: FakeScrollPoster,
        t0: Instant,
    }

    impl Rig {
        fn ms(&self, ms: u64) -> Instant {
            self.t0 + Duration::from_millis(ms)
        }
    }

    fn rig(settings: Scroll) -> Rig {
        let (frames, posted) = (FakeFrames::default(), FakeScrollPoster::default());
        Rig {
            scroll: ScrollEngine::new(settings, Box::new(frames.clone()), Box::new(posted.clone())),
            frames,
            posted,
            t0: Instant::now(),
        }
    }

    /// Low smoothness: a 90 px tick is done 90 ms later, with no tail.
    fn low() -> Scroll {
        Scroll {
            smoothness: Smoothness::Low,
            ..Scroll::default()
        }
    }

    fn wheel(delta: i64) -> ScrollFields {
        ScrollFields {
            delta_axis1: delta,
            sender_id: 10,
            ..Default::default()
        }
    }

    #[test]
    fn a_wheel_tick_is_swallowed_and_asks_for_frames_without_posting() {
        let mut r = rig(low());
        assert_eq!(
            r.scroll.on_wheel(&wheel(1), r.ms(0)),
            ScrollVerdict::Swallow
        );
        assert_eq!(r.frames.calls(), ["request"]);
        assert!(
            r.posted.log().is_empty(),
            "nothing is posted inside the callout"
        );
    }

    #[test]
    fn frames_post_the_animation_and_stop_when_it_is_done() {
        let mut r = rig(low());
        r.scroll.on_wheel(&wheel(1), r.ms(0));
        r.scroll.on_frame(r.ms(0));
        r.scroll.on_frame(r.ms(90));
        assert_eq!(r.posted.log(), ["Began", "Changed dy 90", "Ended"]);
        assert_eq!(r.frames.calls(), ["request", "stop"]);
    }

    #[test]
    fn smoothing_off_passes_ticks_or_inverts_them_in_place() {
        let mut r = rig(Scroll {
            smooth: false,
            ..low()
        });
        assert_eq!(r.scroll.on_wheel(&wheel(1), r.ms(0)), ScrollVerdict::Pass);
        let mut r = rig(Scroll {
            smooth: false,
            invert_vertical: true,
            ..low()
        });
        assert_eq!(
            r.scroll.on_wheel(&wheel(1), r.ms(0)),
            ScrollVerdict::Invert {
                vertical: true,
                horizontal: false
            }
        );
        assert!(r.frames.calls().is_empty());
        assert!(r.posted.log().is_empty());
    }

    #[test]
    fn a_modifier_tick_with_nothing_in_flight_just_passes() {
        let mut r = rig(low());
        let ctrl = ScrollFields {
            flags: CONTROL,
            ..wheel(1)
        };
        assert_eq!(r.scroll.on_wheel(&ctrl, r.ms(0)), ScrollVerdict::Pass);
        assert!(r.frames.calls().is_empty());
        assert!(r.posted.log().is_empty());
    }

    #[test]
    fn a_modifier_tick_with_inversion_is_inverted_in_place_not_smoothed() {
        let mut r = rig(Scroll {
            invert_horizontal: true,
            ..low()
        });
        let cmd = ScrollFields {
            flags: 0x0010_0000,
            ..wheel(1)
        };
        assert_eq!(
            r.scroll.on_wheel(&cmd, r.ms(0)),
            ScrollVerdict::Invert {
                vertical: false,
                horizontal: true
            }
        );
    }

    #[test]
    fn a_modifier_tick_mid_gesture_ends_it_on_the_next_frame() {
        let mut r = rig(low());
        r.scroll.on_wheel(&wheel(1), r.ms(0));
        r.scroll.on_frame(r.ms(0));
        let ctrl = ScrollFields {
            flags: CONTROL,
            ..wheel(1)
        };
        assert_eq!(r.scroll.on_wheel(&ctrl, r.ms(20)), ScrollVerdict::Pass);
        assert_eq!(r.posted.log(), ["Began"], "not posted inside the callout");
        r.scroll.on_frame(r.ms(30));
        assert_eq!(r.posted.log(), ["Began", "Ended"]);
        assert_eq!(r.frames.calls(), ["request", "request", "stop"]);
    }

    #[test]
    fn a_tick_with_no_delta_passes_and_starts_nothing() {
        let mut r = rig(low());
        assert_eq!(r.scroll.on_wheel(&wheel(0), r.ms(0)), ScrollVerdict::Pass);
        assert!(r.frames.calls().is_empty());
    }

    #[test]
    fn cancel_mid_gesture_posts_the_close_and_stops_frames() {
        let mut r = rig(low());
        r.scroll.on_wheel(&wheel(1), r.ms(0));
        r.scroll.on_frame(r.ms(0));
        r.scroll.cancel();
        assert_eq!(r.posted.log(), ["Began", "Ended"]);
        assert_eq!(r.frames.calls(), ["request", "stop"]);
    }

    #[test]
    fn cancel_with_nothing_in_flight_does_nothing() {
        let mut r = rig(low());
        r.scroll.cancel();
        assert!(r.posted.log().is_empty());
        assert!(r.frames.calls().is_empty());
    }

    #[test]
    fn new_settings_close_the_gesture_and_apply_from_the_next_tick() {
        let mut r = rig(low());
        r.scroll.on_wheel(&wheel(1), r.ms(0));
        r.scroll.on_frame(r.ms(0));
        r.scroll.set_settings(Scroll {
            invert_vertical: true,
            ..low()
        });
        assert_eq!(r.posted.log(), ["Began", "Ended"]);
        r.scroll.on_wheel(&wheel(1), r.ms(500));
        r.scroll.on_frame(r.ms(500));
        r.scroll.on_frame(r.ms(590));
        assert_eq!(r.posted.log()[2..], ["Began", "Changed dy -90", "Ended"]);
    }
}

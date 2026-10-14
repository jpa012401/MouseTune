//! What tap callouts ask for but must not do themselves (spec §4.5):
//! posting, pointer freeze, tap changes. Callouts push `Copy` intents into a
//! fixed ring; `InputEngine::run_intents`, run from `TapHandler::on_intents`
//! by a run-loop source outside any callout, carries them out.
//! Allocation-free.

use crate::{
    actions::keys::KeyStroke,
    input::{drag::SwipeOutput, remaps::SwipeKind, router::DragAxis, scroll::ScrollPoster},
};

/// Port: stops the pointer moving during a drag. Must be idempotent.
pub trait PointerFreeze {
    fn set_frozen(&mut self, frozen: bool);
}

/// Port: posts real-looking button and key events, tagged, at the session tap.
pub trait InputPoster {
    fn button(&mut self, cg_button: u8, down: bool);
    fn keys(&mut self, strokes: &[KeyStroke]);
}

/// Port: one zoom step per call: +1 in, −1 out.
pub trait MagnifyOutput {
    fn step(&mut self, amount: f64);
}

/// Port: asks for `TapHandler::on_intents` to run once the callout returns.
/// Callout-safe: only signals.
pub trait IntentSignal {
    fn signal(&self);
}

pub struct GestureSetup {
    pub freeze: Box<dyn PointerFreeze>,
    pub poster: Box<dyn InputPoster>,
    pub signal: Box<dyn IntentSignal>,
    pub swipe: Box<dyn SwipeOutput>,
    /// A second poster, for the two-finger swipe.
    pub scroll: Box<dyn ScrollPoster>,
    pub magnify: Box<dyn MagnifyOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppSwitchStep {
    /// ⌘ down, then Tab (or ⇧Tab when `backward`).
    Start { backward: bool },
    /// Tab or ⇧Tab.
    Next { backward: bool },
    /// ⌘ up: the highlighted app is chosen.
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Intent {
    /// Turn the Drag tap on or off.
    DragTap(bool),
    DragBegin {
        axis: DragAxis,
        swipe: Option<SwipeKind>,
    },
    DragMove {
        dx: i32,
        dy: i32,
    },
    DragEnd,
    HoldThrough {
        cg_button: u8,
        down: bool,
    },
    AppSwitch(AppSwitchStep),
    /// One zoom step: +1 in, −1 out.
    Magnify(f64),
}

const CAPACITY: usize = 64;
/// Only the critical intents (`Intent::is_critical`: a drag end, a
/// hold-through down or up, the app switcher's ⌘ up, the Drag tap off) may
/// fill the ring past this point. Everything else is refused and counted
/// once this many are queued, so the releases always fit.
const MOVE_LIMIT: usize = 48;

#[derive(Debug)]
pub struct IntentRing {
    items: [Option<Intent>; CAPACITY],
    head: usize,
    len: usize,
    dropped: u32,
}

impl Default for IntentRing {
    fn default() -> Self {
        Self {
            items: [None; CAPACITY],
            head: 0,
            len: 0,
            dropped: 0,
        }
    }
}

impl Intent {
    /// Ends or releases something: dropping it could leave the pointer
    /// frozen, a button or ⌘ held, or a swipe open. Hold-through downs are
    /// kept with their ups, so the target button sees whole presses.
    fn is_critical(&self) -> bool {
        matches!(
            self,
            Intent::DragEnd
                | Intent::HoldThrough { .. }
                | Intent::AppSwitch(AppSwitchStep::End)
                | Intent::DragTap(false)
        )
    }
}

impl IntentRing {
    /// False when the intent was dropped (counted in `dropped`).
    pub fn push(&mut self, intent: Intent) -> bool {
        if let Intent::DragMove { dx, dy } = intent
            && self.len > 0
        {
            let back = (self.head + self.len - 1) % CAPACITY;
            if let Some(Intent::DragMove { dx: bx, dy: by }) = &mut self.items[back] {
                *bx = bx.saturating_add(dx);
                *by = by.saturating_add(dy);
                return true;
            }
        }
        if self.len >= MOVE_LIMIT && !intent.is_critical() {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        if self.len == CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.items[(self.head + self.len) % CAPACITY] = Some(intent);
        self.len += 1;
        true
    }

    pub fn pop(&mut self) -> Option<Intent> {
        if self.len == 0 {
            return None;
        }
        let item = self.items[self.head].take();
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        item
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no counter reads it yet; tests pin the drops")
    )]
    pub fn dropped(&self) -> u32 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::router::DragAxis;

    #[test]
    fn intents_come_out_in_order() {
        let mut r = IntentRing::default();
        assert!(r.push(Intent::DragTap(true)));
        assert!(r.push(Intent::HoldThrough {
            cg_button: 2,
            down: true
        }));
        assert_eq!(r.pop(), Some(Intent::DragTap(true)));
        assert_eq!(
            r.pop(),
            Some(Intent::HoldThrough {
                cg_button: 2,
                down: true
            })
        );
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn back_to_back_moves_merge() {
        let mut r = IntentRing::default();
        r.push(Intent::DragBegin {
            axis: DragAxis::Horizontal,
            swipe: None,
        });
        r.push(Intent::DragMove { dx: 3, dy: 1 });
        r.push(Intent::DragMove { dx: 4, dy: -2 });
        r.pop();
        assert_eq!(r.pop(), Some(Intent::DragMove { dx: 7, dy: -1 }));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn a_full_ring_still_takes_the_critical_intents() {
        let mut r = IntentRing::default();
        // 48 non-move intents: the move limit is reached.
        for _ in 0..48 {
            assert!(r.push(Intent::Magnify(1.0)));
        }
        assert!(
            !r.push(Intent::DragMove { dx: 1, dy: 0 }),
            "moves may not use the reserve"
        );
        assert_eq!(r.dropped(), 1);
        assert!(r.push(Intent::DragEnd));
        assert!(r.push(Intent::HoldThrough {
            cg_button: 2,
            down: false
        }));
        assert!(r.push(Intent::AppSwitch(AppSwitchStep::End)));
        for _ in 0..48 {
            r.pop();
        }
        assert_eq!(r.pop(), Some(Intent::DragEnd));
        assert_eq!(
            r.pop(),
            Some(Intent::HoldThrough {
                cg_button: 2,
                down: false
            })
        );
        assert_eq!(r.pop(), Some(Intent::AppSwitch(AppSwitchStep::End)));
    }

    #[test]
    fn a_completely_full_ring_refuses_and_counts() {
        let mut r = IntentRing::default();
        for _ in 0..64 {
            assert!(r.push(Intent::DragTap(false)));
        }
        assert!(!r.push(Intent::DragEnd));
        assert_eq!(r.dropped(), 1);
    }

    #[test]
    fn non_critical_intents_cannot_use_the_reserve() {
        let mut r = IntentRing::default();
        for _ in 0..48 {
            assert!(r.push(Intent::DragEnd));
        }
        assert!(!r.push(Intent::Magnify(1.0)));
        assert!(!r.push(Intent::DragBegin {
            axis: DragAxis::Horizontal,
            swipe: None,
        }));
        assert!(!r.push(Intent::DragTap(true)));
        assert!(!r.push(Intent::AppSwitch(AppSwitchStep::Start { backward: false })));
        assert_eq!(r.dropped(), 4);
        assert!(r.push(Intent::HoldThrough {
            cg_button: 2,
            down: true
        }));
        assert!(r.push(Intent::DragTap(false)));
    }

    #[test]
    fn the_ring_wraps_around() {
        let mut r = IntentRing::default();
        for _ in 0..60 {
            assert!(r.push(Intent::DragEnd));
            assert_eq!(r.pop(), Some(Intent::DragEnd));
        }
        assert!(r.push(Intent::DragMove { dx: 1, dy: 0 }));
        assert!(r.push(Intent::DragMove { dx: 2, dy: 3 }));
        assert_eq!(r.pop(), Some(Intent::DragMove { dx: 3, dy: 3 }));
        assert_eq!(r.pop(), None);
    }
}

//! What tap callouts ask for but must not do themselves (Phase 6 design §2):
//! posting, pointer freeze, tap changes. Callouts push `Copy` intents into a
//! fixed ring; `InputEngine::run_intents`, a run-loop source handler outside
//! any callout, carries them out. Allocation-free.

#![expect(
    dead_code,
    reason = "the engine queues and runs intents from Phase 6 Task 5"
)]

use crate::input::{remaps::SwipeKind, router::DragAxis};

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
/// Moves may fill the ring only this far, so a drag end, a button or key
/// release, or an unfreeze always fits (Phase 6 design §2).
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

impl IntentRing {
    /// False when the intent was dropped (counted in `dropped`).
    pub fn push(&mut self, intent: Intent) -> bool {
        if let Intent::DragMove { dx, dy } = intent {
            if self.len > 0 {
                let back = (self.head + self.len - 1) % CAPACITY;
                if let Some(Intent::DragMove { dx: bx, dy: by }) = &mut self.items[back] {
                    *bx = bx.saturating_add(dx);
                    *by = by.saturating_add(dy);
                    return true;
                }
            }
            if self.len >= MOVE_LIMIT {
                self.dropped = self.dropped.saturating_add(1);
                return false;
            }
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
}

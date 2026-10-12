//! Click levels (spec §5.1): repeated presses of one button on one mouse
//! within the level-expiry window count up 1, 2, 3 and wrap at the button's
//! highest bound level. Timing comes from the router's timers, not a clock.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Press {
    pub button: u8,
    pub sender_id: u64,
    pub level: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycle {
    pub press: Press,
    pub down: bool,
    pub hold_fired: bool,
    pub click_fired: bool,
    pub expired: bool,
}

#[derive(Debug, Default)]
pub struct ClickCycle {
    current: Option<Cycle>,
}

impl ClickCycle {
    pub fn press(&mut self, button: u8, sender_id: u64, max_level: u8) -> Press {
        let level = match self.current {
            Some(c)
                if c.press.button == button
                    && c.press.sender_id == sender_id
                    && c.press.level < max_level =>
            {
                c.press.level + 1
            }
            _ => 1,
        };
        let press = Press {
            button,
            sender_id,
            level,
        };
        self.current = Some(Cycle {
            press,
            down: true,
            hold_fired: false,
            click_fired: false,
            expired: false,
        });
        press
    }

    pub fn current(&self) -> Option<Cycle> {
        self.current
    }

    fn update(&mut self, f: impl FnOnce(&mut Cycle)) {
        if let Some(c) = self.current.as_mut() {
            f(c);
        }
    }

    pub fn release(&mut self) {
        self.update(|c| c.down = false);
    }

    pub fn mark_hold_fired(&mut self) {
        self.update(|c| c.hold_fired = true);
    }

    pub fn mark_click_fired(&mut self) {
        self.update(|c| c.click_fired = true);
    }

    pub fn mark_expired(&mut self) {
        self.update(|c| c.expired = true);
    }

    pub fn end(&mut self) {
        self.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_press_is_level_1() {
        let mut c = ClickCycle::default();
        assert_eq!(c.press(2, 7, 3).level, 1);
        assert_eq!(c.current().map(|c| c.down), Some(true));
    }

    #[test]
    fn a_second_press_of_the_same_button_in_the_same_cycle_is_level_2() {
        let mut c = ClickCycle::default();
        c.press(2, 7, 3);
        c.release();
        assert_eq!(c.press(2, 7, 3).level, 2);
    }

    #[test]
    fn levels_wrap_at_the_highest_bound_level() {
        let mut c = ClickCycle::default();
        c.press(2, 7, 2);
        c.release();
        c.press(2, 7, 2);
        c.release();
        assert_eq!(c.press(2, 7, 2).level, 1);
    }

    #[test]
    fn a_different_button_starts_at_level_1() {
        let mut c = ClickCycle::default();
        c.press(2, 7, 3);
        c.release();
        assert_eq!(c.press(3, 7, 3).level, 1);
    }

    #[test]
    fn a_different_mouse_starts_at_level_1() {
        let mut c = ClickCycle::default();
        c.press(2, 7, 3);
        c.release();
        assert_eq!(c.press(2, 8, 3).level, 1);
    }

    #[test]
    fn after_the_cycle_ends_the_next_press_is_level_1() {
        let mut c = ClickCycle::default();
        c.press(2, 7, 3);
        c.release();
        c.end();
        assert_eq!(c.current(), None);
        assert_eq!(c.press(2, 7, 3).level, 1);
    }

    #[test]
    fn a_new_press_clears_the_previous_press_flags() {
        let mut c = ClickCycle::default();
        c.press(2, 7, 3);
        c.mark_hold_fired();
        c.mark_click_fired();
        c.mark_expired();
        c.release();
        c.press(2, 7, 3);
        let cycle = c.current().unwrap();
        assert!(!cycle.hold_fired && !cycle.click_fired && !cycle.expired && cycle.down);
    }
}

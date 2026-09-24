//! From controller states to actions: a press is one action, a held direction repeats like a key.

use std::time::{Duration, Instant};

/// XInput's `XINPUT_GAMEPAD_*` bits.
const UP: u16 = 0x0001;
const DOWN: u16 = 0x0002;
const LEFT: u16 = 0x0004;
const RIGHT: u16 = 0x0008;
const SHOULDER_LEFT: u16 = 0x0100;
const SHOULDER_RIGHT: u16 = 0x0200;
const A: u16 = 0x1000;
const B: u16 = 0x2000;
const DIRECTIONS: u16 = UP | DOWN | LEFT | RIGHT;

/// How far the stick must lean to count as a direction (of 32767).
const STICK_THRESHOLD: i16 = 16_000;
/// How long a direction is held before it starts repeating, and how often it then repeats.
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    /// Press the focused row.
    Activate,
    Back,
    NextPage,
    PreviousPage,
}

const PRESSES: [(u16, Action); 8] = [
    (UP, Action::Up),
    (LEFT, Action::Left),
    (DOWN, Action::Down),
    (RIGHT, Action::Right),
    (A, Action::Activate),
    (B, Action::Back),
    (SHOULDER_LEFT, Action::PreviousPage),
    (SHOULDER_RIGHT, Action::NextPage),
];

#[derive(Debug, Default)]
pub struct Navigator {
    previous: u16,
    repeat_at: Option<Instant>,
}

impl Navigator {
    /// Forgets what is held, e.g. while another window is in front.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// What changed since the last call. `state` is buttons and left stick, `None` without a
    /// controller.
    pub fn update(&mut self, state: Option<(u16, i16, i16)>, now: Instant) -> Vec<Action> {
        let current = state.map_or(0, |(buttons, x, y)| buttons | stick_directions(x, y));
        let pressed = current & !self.previous;
        let mut actions: Vec<Action> = PRESSES
            .iter()
            .filter(|(bit, _)| pressed & bit != 0)
            .map(|(_, action)| *action)
            .collect();

        let held = current & DIRECTIONS;
        if held == 0 {
            self.repeat_at = None;
        } else if held != self.previous & DIRECTIONS {
            self.repeat_at = Some(now + REPEAT_DELAY);
        } else if self.repeat_at.is_some_and(|at| now >= at) {
            actions.extend(
                PRESSES
                    .iter()
                    .filter(|(bit, _)| held & bit != 0)
                    .map(|(_, action)| *action),
            );
            self.repeat_at = Some(now + REPEAT_EVERY);
        }
        self.previous = current;
        actions
    }
}

/// The left stick as D-pad bits. Screen up is positive Y in XInput.
fn stick_directions(x: i16, y: i16) -> u16 {
    let mut bits = 0;
    if y > STICK_THRESHOLD {
        bits |= UP;
    } else if y < -STICK_THRESHOLD {
        bits |= DOWN;
    }
    if x > STICK_THRESHOLD {
        bits |= RIGHT;
    } else if x < -STICK_THRESHOLD {
        bits |= LEFT;
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like `Navigator::update`'s argument, which is `None` without a controller.
    #[allow(clippy::unnecessary_wraps)]
    fn pad(buttons: u16) -> Option<(u16, i16, i16)> {
        Some((buttons, 0, 0))
    }

    #[test]
    fn a_press_is_one_action_until_released() {
        let mut navigator = Navigator::default();
        let now = Instant::now();
        assert_eq!(navigator.update(pad(A), now), [Action::Activate]);
        assert!(
            navigator
                .update(pad(A), now + Duration::from_secs(2))
                .is_empty()
        );
        assert!(navigator.update(pad(0), now).is_empty());
        assert_eq!(
            navigator.update(pad(A | B), now),
            [Action::Activate, Action::Back]
        );
    }

    #[test]
    fn a_held_direction_repeats_after_a_delay() {
        let mut navigator = Navigator::default();
        let start = Instant::now();
        assert_eq!(navigator.update(pad(DOWN), start), [Action::Down]);
        let early = start + Duration::from_millis(300);
        assert!(navigator.update(pad(DOWN), early).is_empty());
        let later = start + REPEAT_DELAY;
        assert_eq!(navigator.update(pad(DOWN), later), [Action::Down]);
        assert!(
            navigator
                .update(pad(DOWN), later + Duration::from_millis(50))
                .is_empty()
        );
        assert_eq!(
            navigator.update(pad(DOWN), later + REPEAT_EVERY),
            [Action::Down]
        );
        assert!(
            navigator
                .update(pad(0), later + REPEAT_EVERY * 2)
                .is_empty()
        );
    }

    #[test]
    fn the_stick_works_like_the_d_pad() {
        let mut navigator = Navigator::default();
        let now = Instant::now();
        assert_eq!(navigator.update(Some((0, 0, 30_000)), now), [Action::Up]);
        assert!(
            navigator.update(Some((0, 0, 8_000)), now).is_empty(),
            "back to rest"
        );
        assert_eq!(navigator.update(Some((0, 30_000, 0)), now), [Action::Right]);
    }

    #[test]
    fn shoulders_turn_pages_and_nothing_is_held_after_a_reset() {
        let mut navigator = Navigator::default();
        let now = Instant::now();
        assert_eq!(
            navigator.update(pad(SHOULDER_RIGHT), now),
            [Action::NextPage]
        );
        navigator.reset();
        assert_eq!(
            navigator.update(pad(SHOULDER_RIGHT), now),
            [Action::NextPage]
        );
        assert!(navigator.update(None, now).is_empty());
    }
}

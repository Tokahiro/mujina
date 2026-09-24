//! Device buttons that arrive as keyboard chords. [`ChordSetMatcher`] runs inside a low-level
//! keyboard hook, which Windows removes if it answers too slowly: no allocation, constant time.

use core::fmt;

use crate::button::ButtonId;
use crate::keys::{ChordParseError, KeyChord, MAX_CHORD_KEYS, VirtualKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Physical,
    /// Synthesized by another program.
    Injected,
    /// Synthesized by Mujina itself; never interpreted again.
    Own,
    /// Sent on again by Mujina after the matcher held it back ([`Verdict::SwallowAndReplay`]);
    /// never interpreted again, and its return tells the matcher the replay went through.
    Replayed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: VirtualKey,
    pub direction: Direction,
    pub origin: Origin,
    /// Carried so a held-back event is sent on exactly as it came; the matcher never reads it.
    pub scan: ScanCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanCode {
    /// The hardware scan code; for a `VK_PACKET` character, the UTF-16 unit it carries.
    pub code: u16,
    /// An `0xE0`-prefixed scan code; without it a right Ctrl reads as the left one.
    pub extended: bool,
}

/// A device button's chord: the last key (the trigger) goes down while the others are held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerChord {
    /// One to four different keys. Firmware may press the held ones in any order.
    pub keys: KeyChord,
    /// Only react to synthesized input, so the same chord on a real keyboard keeps working.
    pub injected_only: bool,
}

impl TriggerChord {
    /// Parses `LWIN+D` or `LCTRL+LWIN+LALT`: keys joined by `+`, each named once.
    pub fn parse(text: &str, injected_only: bool) -> Result<Self, ChordParseError> {
        let keys = KeyChord::parse(text)?;
        let pressed = keys.keys();
        if (1..pressed.len()).any(|index| pressed[..index].contains(&pressed[index])) {
            return Err(ChordParseError::RepeatedKey);
        }
        Ok(Self {
            keys,
            injected_only,
        })
    }

    pub fn trigger(&self) -> VirtualKey {
        self.keys.keys().last().copied().unwrap_or(VirtualKey(0))
    }

    /// The keys held while the trigger goes down, in the order given.
    pub fn held(&self) -> &[VirtualKey] {
        let keys = self.keys.keys();
        &keys[..keys.len().saturating_sub(1)]
    }

    fn admits(&self, origin: Origin) -> bool {
        match origin {
            Origin::Injected => true,
            Origin::Physical => !self.injected_only,
            Origin::Own | Origin::Replayed => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Swallow,
    SwallowAndFire(ButtonId),
    /// Hide the event; it and everything held back before it are to be sent on in order
    /// ([`ChordSetMatcher::take_replay`]), so no input is lost when keys start no chord after all.
    SwallowAndReplay,
}

/// Far more events than arrive in one turn of the hook thread's message loop. Overflow passes at
/// once, ahead of the queue: reordered (a key may stay down), but not lost.
pub const REPLAY_CAPACITY: usize = 64;

/// How long replayed events may take to come back through the hook. One may never return: a hook
/// installed later ran first and took it, or `SendInput` dropped it silently.
pub const REPLAY_GIVE_UP_MS: u32 = 1000;

/// Events to send on again, oldest first.
#[derive(Debug, Clone, Copy)]
pub struct Replay {
    events: [KeyEvent; REPLAY_CAPACITY],
    len: usize,
}

impl Replay {
    pub fn events(&self) -> &[KeyEvent] {
        &self.events[..self.len]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyButtons {
    pub limit: usize,
}

impl fmt::Display for TooManyButtons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "more than {} buttons", self.limit)
    }
}

impl core::error::Error for TooManyButtons {}

/// Fills unused slots; never looked at.
const NO_EVENT: KeyEvent = KeyEvent {
    key: VirtualKey(0),
    direction: Direction::Up,
    origin: Origin::Own,
    scan: ScanCode {
        code: 0,
        extended: false,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    /// Keys that may start a chord were swallowed (`prefix`); waiting to see what follows.
    Prefix,
    /// The chord at `chord` fired; its keys are swallowed until they are released. `down` has
    /// a bit per key of the chord, by position, for those still down.
    Fired {
        chord: usize,
        down: u8,
    },
    /// Swallowed keys started no chord and are being sent on. Until all have come back through the
    /// hook, later events are held back too, or they would overtake the replay.
    Replaying,
}

/// Recognises the chords of up to `N` (at most 32) buttons. A whole chord is swallowed, since a
/// lone `LWIN` release would open Start; where one chord begins another, the shorter fires.
#[derive(Debug)]
pub struct ChordSetMatcher<const N: usize> {
    buttons: [Option<(ButtonId, TriggerChord)>; N],
    state: State,
    prefix: [KeyEvent; MAX_CHORD_KEYS],
    prefix_len: usize,
    /// The chords the prefix may still become, a bit per slot of `buttons`.
    candidates: u32,
    /// Held back, to be sent on: taken by [`take_replay`](Self::take_replay).
    queue: [KeyEvent; REPLAY_CAPACITY],
    queued: usize,
    /// Sent on and not yet seen back.
    in_flight: usize,
    /// When `in_flight` last rose from 0, in the caller's milliseconds. Not the latest batch, so a
    /// stream of keys cannot put off giving up on one that never returns.
    in_flight_since: u32,
}

impl<const N: usize> Default for ChordSetMatcher<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ChordSetMatcher<N> {
    pub const fn new() -> Self {
        const { assert!(N <= 32, "the candidates of a prefix are the bits of a u32") };
        Self {
            buttons: [None; N],
            state: State::Idle,
            prefix: [NO_EVENT; MAX_CHORD_KEYS],
            prefix_len: 0,
            candidates: 0,
            queue: [NO_EVENT; REPLAY_CAPACITY],
            queued: 0,
            in_flight: 0,
            in_flight_since: 0,
        }
    }

    /// The buttons to recognise from now on. A chord under way is given up: what was held back
    /// of it is sent on ([`take_replay`](Self::take_replay)).
    pub fn set(&mut self, buttons: &[(ButtonId, TriggerChord)]) -> Result<(), TooManyButtons> {
        if buttons.len() > N {
            return Err(TooManyButtons { limit: N });
        }
        self.give_up_chord();
        self.buttons = [None; N];
        for (slot, button) in self.buttons.iter_mut().zip(buttons) {
            *slot = Some(*button);
        }
        Ok(())
    }

    /// For after the hook was installed again: gives up a chord under way as [`set`](Self::set)
    /// does, and stops waiting for replays, which may have passed while no hook saw them.
    pub fn reset(&mut self) {
        self.give_up_chord();
        self.forget_in_flight();
    }

    pub fn feed(&mut self, event: KeyEvent) -> Verdict {
        match event.origin {
            Origin::Own => return Verdict::Pass,
            Origin::Replayed => {
                self.in_flight = self.in_flight.saturating_sub(1);
                self.settle();
                return Verdict::Pass;
            }
            Origin::Physical | Origin::Injected => {}
        }
        // Chords that take only injected input leave a real keyboard alone, even while
        // something is held back.
        let concerned = self.admitting(event.origin);
        if concerned == 0 {
            return Verdict::Pass;
        }
        match self.state {
            State::Idle => self.start(event, concerned),
            State::Prefix => match self.candidates & concerned {
                0 => Verdict::Pass,
                candidates => self.extend(event, candidates),
            },
            State::Fired { chord, down } => self.finish(event, chord, down),
            State::Replaying => self.hold_back(event),
        }
    }

    pub fn replay_pending(&self) -> bool {
        self.queued > 0
    }

    /// The events to send on, oldest first; `now` in milliseconds, on a clock that wraps like
    /// `GetTickCount`. They are waited for until fed back as [`Origin::Replayed`].
    pub fn take_replay(&mut self, now: u32) -> Replay {
        let replay = Replay {
            events: self.queue,
            len: self.queued,
        };
        if self.in_flight == 0 && self.queued > 0 {
            self.in_flight_since = now;
        }
        self.in_flight += self.queued;
        self.queued = 0;
        self.settle();
        replay
    }

    /// Stops waiting for replays out longer than [`REPLAY_GIVE_UP_MS`]; whether it gave up. Call it
    /// before each [`feed`](Self::feed), on the clock of [`take_replay`](Self::take_replay).
    pub fn give_up_if_late(&mut self, now: u32) -> bool {
        if self.in_flight > 0 && now.wrapping_sub(self.in_flight_since) > REPLAY_GIVE_UP_MS {
            self.forget_in_flight();
            return true;
        }
        false
    }

    /// `count` events of a replay could not be sent, so they will not come back.
    pub fn replay_lost(&mut self, count: usize) {
        self.in_flight = self.in_flight.saturating_sub(count);
        self.settle();
    }

    /// Stops waiting for events sent on, for when they cannot come back through this hook.
    pub fn forget_in_flight(&mut self) {
        self.in_flight = 0;
        self.settle();
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    fn start(&mut self, event: KeyEvent, concerned: u32) -> Verdict {
        if event.direction == Direction::Up {
            return Verdict::Pass;
        }
        if let Some(index) = self.first(concerned, |chord| {
            chord.held().is_empty() && chord.trigger() == event.key
        }) {
            return self.fire(index);
        }
        let candidates = self.having(concerned, |chord| chord.held().contains(&event.key));
        if candidates == 0 {
            return Verdict::Pass;
        }
        self.prefix[0] = event;
        self.prefix_len = 1;
        self.candidates = candidates;
        self.state = State::Prefix;
        Verdict::Swallow
    }

    fn extend(&mut self, event: KeyEvent, candidates: u32) -> Verdict {
        if event.direction == Direction::Down {
            let prefix = &self.prefix[..self.prefix_len];
            // Held down long enough to repeat.
            if prefix.iter().any(|held| held.key == event.key) {
                return Verdict::Swallow;
            }
            let completes = |chord: &TriggerChord| {
                chord.trigger() == event.key
                    && chord.held().len() == prefix.len()
                    && prefix.iter().all(|held| chord.held().contains(&held.key))
            };
            if let Some(index) = self.first(candidates, completes) {
                return self.fire(index);
            }
            let grown = self.having(candidates, |chord| chord.held().contains(&event.key));
            if grown != 0 && self.prefix_len < self.prefix.len() {
                self.prefix[self.prefix_len] = event;
                self.prefix_len += 1;
                self.candidates = grown;
                return Verdict::Swallow;
            }
        }
        // Another key, or a held one let go: no chord is coming.
        for index in 0..self.prefix_len {
            self.push(self.prefix[index]);
        }
        self.push(event);
        self.prefix_len = 0;
        self.candidates = 0;
        self.state = State::Replaying;
        Verdict::SwallowAndReplay
    }

    fn finish(&mut self, event: KeyEvent, chord: usize, down: u8) -> Verdict {
        let Some(Some((_, trigger))) = self.buttons.get(chord).copied() else {
            self.state = State::Idle;
            return Verdict::Pass;
        };
        let position = trigger.keys.keys().iter().position(|key| *key == event.key);
        let Some(position) = position.filter(|_| trigger.admits(event.origin)) else {
            return Verdict::Pass;
        };
        let bit = 1u8 << position;
        let down = match event.direction {
            Direction::Down => down | bit,
            Direction::Up => down & !bit,
        };
        // Over once the held keys are up: a trigger whose release went missing must not keep
        // the next press from firing. A chord of one key is over with its release.
        let held = (1u8 << trigger.held().len()) - 1;
        let lasting = if held == 0 { down } else { down & held };
        self.state = if lasting == 0 {
            State::Idle
        } else {
            State::Fired { chord, down }
        };
        Verdict::Swallow
    }

    fn hold_back(&mut self, event: KeyEvent) -> Verdict {
        if self.push(event) {
            Verdict::SwallowAndReplay
        } else {
            Verdict::Pass
        }
    }

    fn fire(&mut self, index: usize) -> Verdict {
        let Some(Some((id, chord))) = self.buttons.get(index).copied() else {
            return Verdict::Pass;
        };
        let keys = chord.keys.keys().len();
        let down = u8::try_from((1u16 << keys) - 1).unwrap_or(u8::MAX);
        self.state = State::Fired { chord: index, down };
        self.prefix_len = 0;
        self.candidates = 0;
        Verdict::SwallowAndFire(id)
    }

    /// The prefix is to be sent on; a fired chord's remaining releases pass.
    fn give_up_chord(&mut self) {
        match self.state {
            State::Prefix => {
                for index in 0..self.prefix_len {
                    self.push(self.prefix[index]);
                }
                self.prefix_len = 0;
                self.candidates = 0;
                self.state = State::Replaying;
            }
            State::Fired { .. } => self.state = State::Idle,
            State::Idle | State::Replaying => {}
        }
    }

    /// Queues `event` to be sent on; `false` when the queue is full.
    fn push(&mut self, event: KeyEvent) -> bool {
        match self.queue.get_mut(self.queued) {
            Some(slot) => {
                *slot = event;
                self.queued += 1;
                true
            }
            None => false,
        }
    }

    fn settle(&mut self) {
        if self.state == State::Replaying && self.queued == 0 && self.in_flight == 0 {
            self.state = State::Idle;
        }
    }

    fn admitting(&self, origin: Origin) -> u32 {
        self.having(u32::MAX, |chord| chord.admits(origin))
    }

    /// The buttons among `among` whose chord is `wanted`.
    fn having(&self, among: u32, wanted: impl Fn(&TriggerChord) -> bool) -> u32 {
        self.buttons
            .iter()
            .enumerate()
            .filter(|(index, _)| among & (1 << index) != 0)
            .filter_map(|(index, button)| button.as_ref().map(|(_, chord)| (index, chord)))
            .filter(|(_, chord)| wanted(chord))
            .fold(0, |bits, (index, _)| bits | (1 << index))
    }

    fn first(&self, among: u32, wanted: impl Fn(&TriggerChord) -> bool) -> Option<usize> {
        let found = self.having(among, wanted);
        (found != 0)
            .then(|| usize::try_from(found.trailing_zeros()).ok())
            .flatten()
    }
}

/// Reads a device button out of observed key events: the keys pressed until the first release,
/// when at least two. Mujina's own keystrokes and single keys on their own are ignored.
pub fn suggest_trigger(events: &[KeyEvent]) -> Option<TriggerChord> {
    let mut held = [VirtualKey(0); MAX_CHORD_KEYS];
    let mut len = 0;
    let mut injected = true;
    let theirs = events
        .iter()
        .filter(|event| matches!(event.origin, Origin::Physical | Origin::Injected));
    for event in theirs {
        let pressed = held[..len].contains(&event.key);
        match event.direction {
            Direction::Down if !pressed && len < MAX_CHORD_KEYS => {
                held[len] = event.key;
                len += 1;
                injected &= event.origin == Origin::Injected;
            }
            Direction::Up if pressed && len >= 2 => break,
            Direction::Up if pressed => {
                len = 0;
                injected = true;
            }
            _ => {}
        }
    }
    if len < 2 {
        return None;
    }
    KeyChord::from_keys(&held[..len]).map(|keys| TriggerChord {
        keys,
        injected_only: injected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use Direction::{Down, Up};
    use Verdict::{Pass, Swallow, SwallowAndFire, SwallowAndReplay};

    const E: VirtualKey = VirtualKey(0x45);
    const O: VirtualKey = VirtualKey(0x4F);
    const RCTRL: VirtualKey = VirtualKey(0xA3);
    const LWIN: VirtualKey = VirtualKey::LWIN;
    const D: VirtualKey = VirtualKey::D;
    const LCTRL: VirtualKey = VirtualKey::LCONTROL;
    const LALT: VirtualKey = VirtualKey::LMENU;

    fn injected(key: VirtualKey, direction: Direction) -> KeyEvent {
        KeyEvent {
            key,
            direction,
            origin: Origin::Injected,
            scan: ScanCode::default(),
        }
    }

    fn physical(key: VirtualKey, direction: Direction) -> KeyEvent {
        KeyEvent {
            origin: Origin::Physical,
            ..injected(key, direction)
        }
    }

    fn own(key: VirtualKey, direction: Direction) -> KeyEvent {
        KeyEvent {
            origin: Origin::Own,
            ..injected(key, direction)
        }
    }

    fn replayed(event: KeyEvent) -> KeyEvent {
        KeyEvent {
            origin: Origin::Replayed,
            ..event
        }
    }

    fn chord(text: &str, injected_only: bool) -> TriggerChord {
        TriggerChord::parse(text, injected_only).unwrap()
    }

    /// A matcher for `buttons`: (id, chord, injected only).
    fn matcher(buttons: &[(u8, &str, bool)]) -> ChordSetMatcher<4> {
        let mut list = [(ButtonId(0), chord("F24", true)); 4];
        for (slot, (id, text, injected_only)) in list.iter_mut().zip(buttons) {
            *slot = (ButtonId(*id), chord(text, *injected_only));
        }
        let mut matcher = ChordSetMatcher::new();
        matcher.set(&list[..buttons.len()]).unwrap();
        matcher
    }

    fn onexplayer() -> ChordSetMatcher<4> {
        matcher(&[(0, "LWIN+D", true)])
    }

    fn table<const N: usize>(matcher: &mut ChordSetMatcher<N>, steps: &[(KeyEvent, Verdict)]) {
        for (step, (event, verdict)) in steps.iter().enumerate() {
            assert_eq!(matcher.feed(*event), *verdict, "step {step}: {event:?}");
        }
    }

    #[test]
    fn swallows_the_whole_chord_and_fires_once() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
                (injected(D, Down), Swallow),
                (injected(D, Up), Swallow),
                (injected(LWIN, Up), Swallow),
                (injected(E, Down), Pass),
            ],
        );
    }

    #[test]
    fn an_unrelated_key_passes_while_the_chord_is_held() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
                (injected(E, Down), Pass),
                (injected(E, Up), Pass),
                // The held key's release ends it, even with the trigger's gone missing.
                (injected(LWIN, Up), Swallow),
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
    }

    #[test]
    fn physical_chord_is_left_alone() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (physical(LWIN, Down), Pass),
                (physical(D, Down), Pass),
                (physical(D, Up), Pass),
                (physical(LWIN, Up), Pass),
            ],
        );
    }

    #[test]
    fn own_input_is_never_interpreted() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (own(LWIN, Down), Pass),
                (own(D, Down), Pass),
                (own(D, Up), Pass),
            ],
        );
    }

    #[test]
    fn a_repeated_held_key_is_swallowed_and_the_chord_still_fires() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
    }

    #[test]
    fn lone_modifier_tap_is_replayed() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(LWIN, Down), Swallow),
                (injected(LWIN, Up), SwallowAndReplay),
            ],
        );
        let replay = m.take_replay(0);
        assert_eq!(
            replay.events(),
            [injected(LWIN, Down), injected(LWIN, Up)],
            "one press is enough; the repeat was not kept"
        );
    }

    #[test]
    fn a_burst_after_a_mismatch_is_replayed_in_order() {
        // Another program's LWIN+E sent in one go: the releases arrive before the replay is sent.
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
                (injected(E, Up), SwallowAndReplay),
            ],
        );
        let first = m.take_replay(0);
        assert_eq!(
            first.events(),
            [injected(LWIN, Down), injected(E, Down), injected(E, Up)]
        );
        table(&mut m, &[(injected(LWIN, Up), SwallowAndReplay)]);
        let second = m.take_replay(0);
        assert_eq!(second.events(), [injected(LWIN, Up)]);
        assert_eq!(m.in_flight(), 4);

        for event in first.events().iter().chain(second.events()) {
            assert_eq!(m.feed(replayed(*event)), Pass);
        }
        assert_eq!(m.in_flight(), 0);
        table(
            &mut m,
            &[
                (injected(E, Down), Pass),
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
    }

    #[test]
    fn a_real_keyboard_is_not_held_back_during_a_replay() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
                (physical(E, Down), Pass),
                (own(D, Down), Pass),
            ],
        );
        assert_eq!(m.take_replay(0).events().len(), 2);
    }

    #[test]
    fn a_replay_that_never_comes_back_is_given_up() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        let _ = m.take_replay(0);
        // Windows took one of the two.
        m.replay_lost(1);
        assert_eq!(m.in_flight(), 1);
        table(&mut m, &[(injected(E, Up), SwallowAndReplay)]);
        let _ = m.take_replay(0);
        m.forget_in_flight();
        assert_eq!(m.in_flight(), 0);
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
    }

    #[test]
    fn a_burst_longer_than_the_replay_passes_out_of_order_rather_than_being_lost() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        for _ in 2..REPLAY_CAPACITY {
            assert_eq!(m.feed(injected(E, Down)), SwallowAndReplay);
        }
        assert_eq!(m.feed(injected(E, Up)), Pass);
        assert_eq!(m.take_replay(0).events().len(), REPLAY_CAPACITY);
    }

    #[test]
    fn what_never_comes_back_is_given_up_a_second_after_it_went_out() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        let first = m.take_replay(0);
        // LWIN comes back; E never does (a hook installed later took it).
        assert_eq!(m.feed(replayed(first.events()[0])), Pass);
        assert_eq!(m.in_flight(), 1);

        // Another program's keys, every half second, each come back; they do not put off E.
        for now in [500, 1000] {
            assert!(!m.give_up_if_late(now), "{now} ms");
            assert_eq!(m.feed(injected(E, Down)), SwallowAndReplay, "{now} ms");
            let later = m.take_replay(now);
            assert_eq!(m.feed(replayed(later.events()[0])), Pass);
        }
        assert_eq!(m.in_flight(), 1);

        assert!(m.give_up_if_late(1001));
        assert_eq!(m.in_flight(), 0);
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
        assert!(!m.give_up_if_late(5000), "nothing is waited for");
    }

    #[test]
    fn the_wait_is_timed_from_the_oldest_replay_still_out_even_as_the_clock_wraps() {
        let mut m = onexplayer();
        let start = u32::MAX - 100;
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        let _ = m.take_replay(start);
        table(&mut m, &[(injected(E, Up), SwallowAndReplay)]);
        let _ = m.take_replay(start.wrapping_add(900));
        assert!(!m.give_up_if_late(start.wrapping_add(1000)));
        assert!(m.give_up_if_late(start.wrapping_add(1001)));

        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        let replay = m.take_replay(5000);
        for event in replay.events() {
            assert_eq!(m.feed(replayed(*event)), Pass);
        }
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        let _ = m.take_replay(9000);
        assert!(!m.give_up_if_late(9500));
        assert!(m.give_up_if_late(10_001));
    }

    #[test]
    fn a_three_key_chord_fires_on_its_last_key() {
        let mut m = matcher(&[(0, "LCTRL+LWIN+LALT", true)]);
        table(
            &mut m,
            &[
                (injected(LCTRL, Down), Swallow),
                (injected(LWIN, Down), Swallow),
                (injected(LALT, Down), SwallowAndFire(ButtonId(0))),
                (injected(LALT, Up), Swallow),
                (injected(LWIN, Up), Swallow),
                (injected(LCTRL, Up), Swallow),
                (injected(LCTRL, Down), Swallow),
            ],
        );
        // The held keys in another order fire as well.
        let mut m = matcher(&[(0, "LCTRL+LWIN+LALT", true)]);
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(LCTRL, Down), Swallow),
                (injected(LALT, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
    }

    #[test]
    fn a_three_key_chord_that_goes_elsewhere_is_replayed_whole() {
        let mut m = matcher(&[(0, "LCTRL+LWIN+LALT", true)]);
        table(
            &mut m,
            &[
                (injected(LCTRL, Down), Swallow),
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        assert_eq!(
            m.take_replay(0).events(),
            [
                injected(LCTRL, Down),
                injected(LWIN, Down),
                injected(E, Down)
            ]
        );
    }

    #[test]
    fn two_buttons_sharing_a_modifier_stay_undecided_until_they_part() {
        // The OneXPlayer Mini: LWIN+D, and LWIN+RCTRL+O for its keyboard button.
        let mut m = matcher(&[(0, "LWIN+D", true), (1, "LWIN+0xA3+O", true)]);
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(RCTRL, Down), Swallow),
                (injected(O, Down), SwallowAndFire(ButtonId(1))),
                (injected(O, Up), Swallow),
                (injected(RCTRL, Up), Swallow),
                (injected(LWIN, Up), Swallow),
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
                (injected(D, Up), Swallow),
                (injected(LWIN, Up), Swallow),
            ],
        );
    }

    #[test]
    fn only_the_buttons_that_take_a_real_keyboard_see_it() {
        let mut m = matcher(&[(0, "LWIN+D", true), (1, "LCTRL+F24", false)]);
        table(
            &mut m,
            &[
                (physical(LWIN, Down), Pass),
                (physical(D, Down), Pass),
                (physical(LCTRL, Down), Swallow),
                (
                    physical(VirtualKey(0x87), Down),
                    SwallowAndFire(ButtonId(1)),
                ),
                (injected(LWIN, Down), Pass),
                (physical(LCTRL, Up), Swallow),
                (injected(LWIN, Down), Swallow),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
    }

    #[test]
    fn a_key_from_elsewhere_during_a_prefix_is_left_to_its_stream() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (physical(E, Down), Pass),
                (injected(D, Down), SwallowAndFire(ButtonId(0))),
            ],
        );
    }

    #[test]
    fn a_button_of_one_key_fires_on_it() {
        let mut m = matcher(&[(3, "F24", true)]);
        table(
            &mut m,
            &[
                (
                    injected(VirtualKey(0x87), Down),
                    SwallowAndFire(ButtonId(3)),
                ),
                (injected(VirtualKey(0x87), Up), Swallow),
                (injected(VirtualKey(0x87), Up), Pass),
            ],
        );
    }

    #[test]
    fn new_buttons_give_up_a_chord_under_way_without_losing_it() {
        let mut m = onexplayer();
        table(&mut m, &[(injected(LWIN, Down), Swallow)]);
        m.set(&[(ButtonId(0), chord("LCTRL+F24", true))]).unwrap();
        assert!(m.replay_pending());
        assert_eq!(m.take_replay(0).events(), [injected(LWIN, Down)]);

        let mut m = matcher(&[]);
        assert_eq!(
            m.feed(injected(LWIN, Down)),
            Pass,
            "no buttons, nothing held"
        );
        let many = [(ButtonId(0), chord("F24", true)); 5];
        assert_eq!(m.set(&many), Err(TooManyButtons { limit: 4 }));
    }

    #[test]
    fn a_reset_sends_on_the_prefix_and_stops_waiting() {
        let mut m = onexplayer();
        table(
            &mut m,
            &[
                (injected(LWIN, Down), Swallow),
                (injected(E, Down), SwallowAndReplay),
            ],
        );
        let _ = m.take_replay(0);
        m.reset();
        assert_eq!(m.in_flight(), 0);
        table(&mut m, &[(injected(LWIN, Down), Swallow)]);
        m.reset();
        assert_eq!(m.take_replay(0).events(), [injected(LWIN, Down)]);
    }

    #[test]
    fn a_chord_names_each_key_once() {
        assert_eq!(
            TriggerChord::parse("LWIN+LWIN", true),
            Err(ChordParseError::RepeatedKey)
        );
        let three = chord("LCTRL+LWIN+LALT", true);
        assert_eq!(three.trigger(), LALT);
        assert_eq!(three.held(), [LCTRL, LWIN]);
    }

    #[test]
    fn a_button_is_recognised_in_observed_events() {
        let events = [
            injected(E, Down),
            injected(E, Up),
            injected(LWIN, Down),
            injected(LWIN, Down),
            injected(D, Down),
            injected(D, Up),
            injected(LWIN, Up),
        ];
        assert_eq!(suggest_trigger(&events), Some(chord("LWIN+D", true)));
        assert_eq!(
            suggest_trigger(&events[..2]),
            None,
            "single keys are no chord"
        );
    }

    #[test]
    fn every_key_held_at_once_is_part_of_the_chord() {
        let events = [
            own(E, Down),
            injected(LCTRL, Down),
            injected(LWIN, Down),
            injected(LALT, Down),
            injected(LALT, Up),
            injected(LWIN, Up),
            injected(LCTRL, Up),
        ];
        assert_eq!(
            suggest_trigger(&events),
            Some(chord("LCTRL+LWIN+LALT", true))
        );
    }

    #[test]
    fn a_chord_from_a_real_keyboard_is_not_marked_injected() {
        let events = [physical(LCTRL, Down), physical(E, Down)];
        assert!(!suggest_trigger(&events).unwrap().injected_only);
    }
}

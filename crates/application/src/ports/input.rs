//! Sending keys: the launcher's shortcuts, whatever device the button comes from.

use mujina_domain::keys::{HoldTiming, KeyChord};

pub trait KeySender {
    /// Presses and releases `chord`. Must not block.
    fn send_chord(&self, chord: KeyChord, timing: HoldTiming);
}

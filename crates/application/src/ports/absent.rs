//! A stand-in for ports whose adapter could not be started.

use mujina_domain::button::ButtonId;
use mujina_domain::keys::{HoldTiming, KeyChord};

use super::{DeviceButtons, KeySender};
use crate::device::DeviceSelection;

/// Does nothing, so the rest keeps working; the caller logs that the adapter is missing.
#[derive(Debug, Default, Clone, Copy)]
pub struct Absent;

impl DeviceButtons for Absent {
    fn reconfigure(&self, _device: &DeviceSelection) -> bool {
        false
    }

    fn pass_on(&self, _button: ButtonId) {}
}

impl KeySender for Absent {
    fn send_chord(&self, _chord: KeyChord, _timing: HoldTiming) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use mujina_domain::keys::VirtualKey;

    #[test]
    fn stands_in_for_each_port_and_does_nothing() {
        let buttons: &dyn DeviceButtons = &Absent;
        let device = DeviceSelection {
            id: Some("onexplayer".into()),
            ..DeviceSelection::none()
        };
        assert!(!buttons.reconfigure(&device));
        buttons.pass_on(ButtonId(0));

        let keys: &dyn KeySender = &Absent;
        keys.send_chord(
            KeyChord::pair(VirtualKey::LCONTROL, VirtualKey::DIGIT_1),
            HoldTiming::default(),
        );
    }
}

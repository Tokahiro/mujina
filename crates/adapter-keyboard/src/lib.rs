//! Devices whose buttons arrive as key chords (`profiles/devices/*.toml`, `[device.button]`), the
//! low-level hook that catches them (ADR-0005), and the thread that sends Mujina's keys.

mod custom;
mod profile;

#[cfg(windows)]
mod hook;
#[cfg(windows)]
pub mod probe;
#[cfg(windows)]
mod runtime;
#[cfg(windows)]
mod sender;

use mujina_application::device::{ButtonId, DeviceDescriptor, DeviceSelection};
use mujina_domain::chord::TriggerChord;

pub use custom::{OWN_BUTTON, OWN_ID, OwnButton, own_chord};
pub use profile::{ChordProfile, builtin, wildcard_match};
#[cfg(windows)]
pub use runtime::{KeyboardRuntime, KeyboardSender, RUNTIME, plugins};

/// How many buttons of one device the hook catches; a profile with more is refused.
pub const MAX_BUTTONS: usize = 8;

/// The built-in profiles by file name, then the button of one's own.
pub fn descriptors() -> Vec<&'static dyn DeviceDescriptor> {
    let mut all: Vec<&'static dyn DeviceDescriptor> = builtin()
        .iter()
        .map(|profile| profile as &dyn DeviceDescriptor)
        .collect();
    all.push(&OWN_BUTTON);
    all
}

/// Empty for no device or an unusable own button; `None` for a device this crate does not serve.
pub fn chords_for(device: &DeviceSelection) -> Option<Vec<(ButtonId, TriggerChord)>> {
    let Some(id) = device.id.as_deref() else {
        return Some(Vec::new());
    };
    if id == OWN_ID {
        let chord = own_chord(&device.options).ok().flatten();
        return Some(
            chord
                .map(|chord| (ButtonId(0), chord))
                .into_iter()
                .collect(),
        );
    }
    builtin()
        .iter()
        .find(|profile| profile.id() == id)
        .map(ChordProfile::chords)
}

#[cfg(test)]
mod tests {
    use mujina_application::launcher::OptionTable;
    use mujina_application::settings::SettingValue;
    use mujina_domain::keys::VirtualKey;

    use super::*;

    fn device(id: &str, options: OptionTable) -> DeviceSelection {
        DeviceSelection {
            id: Some(id.to_string()),
            options,
        }
    }

    #[test]
    fn every_device_is_there_once_and_keeps_the_rules() {
        let all = descriptors();
        for (index, descriptor) in all.iter().enumerate() {
            mujina_application::testing::device_conformance(*descriptor);
            assert!(
                all[..index]
                    .iter()
                    .all(|other| other.id() != descriptor.id()),
                "{} is there twice",
                descriptor.id()
            );
        }
        assert_eq!(all.last().map(|last| last.id()), Some(OWN_ID));
    }

    #[test]
    fn the_chords_follow_the_device_chosen() {
        assert_eq!(chords_for(&DeviceSelection::none()), Some(Vec::new()));
        assert_eq!(
            chords_for(&device("no such device", OptionTable::new())),
            None
        );

        let oxp = chords_for(&device("onexplayer", OptionTable::new())).unwrap();
        assert_eq!(oxp[0].1.keys.keys(), [VirtualKey::LWIN, VirtualKey::D]);

        let own = OptionTable::from([
            ("modifier".to_string(), SettingValue::Text("LCTRL".into())),
            ("key".to_string(), SettingValue::Text("F24".into())),
        ]);
        let chords = chords_for(&device(OWN_ID, own)).unwrap();
        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].1.trigger(), VirtualKey(0x87));
        let half = OptionTable::from([("key".to_string(), SettingValue::Text("F24".into()))]);
        assert_eq!(chords_for(&device(OWN_ID, half)), Some(Vec::new()));
    }
}

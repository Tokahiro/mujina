//! `[device.button]`, a modifier and a key; while it names a button, it wins over any profile.

use mujina_application::device::{
    ButtonId, ButtonSpec, DeviceDescriptor, Suppression, SystemIdentity,
};
use mujina_application::launcher::OptionTable;
use mujina_application::settings::SettingValue;
use mujina_application::settings::schema::SettingSpec;
use mujina_domain::chord::TriggerChord;
use mujina_domain::keys::{KeyChord, VirtualKey};

/// Never in a configuration file; shown only in `mujinactl doctor` and the log.
pub const OWN_ID: &str = "custom";

#[derive(Debug)]
pub struct OwnButton;

pub static OWN_BUTTON: OwnButton = OwnButton;

impl DeviceDescriptor for OwnButton {
    fn id(&self) -> &str {
        OWN_ID
    }

    fn name(&self) -> String {
        "Your own button (config.toml)".to_string()
    }

    /// Chosen by its section, never by the machine.
    fn matches(&self, _identity: &SystemIdentity) -> bool {
        false
    }

    fn buttons(&self) -> Vec<ButtonSpec> {
        vec![ButtonSpec {
            id: ButtonId(0),
            key: "button".to_string(),
            label: "Your own button".to_string(),
            suppression: Suppression::Swallowed,
        }]
    }

    /// Its options are `[device.button]`, one of Mujina's own sections, not `[device.custom]`.
    fn settings(&self) -> &[SettingSpec] {
        &[]
    }

    fn validate(&self, options: &OptionTable, notes: &mut Vec<String>) {
        if let Err(problem) = own_chord(options) {
            notes.push(problem);
        }
    }
}

/// The chord `[device.button]` names: `Ok(None)` for none, `Err` with what is wrong otherwise.
pub fn own_chord(options: &OptionTable) -> Result<Option<TriggerChord>, String> {
    let text = |key: &str| match options.get(key) {
        Some(SettingValue::Text(text)) => Some(text.as_str()),
        _ => None,
    };
    let (modifier, key) = match (text("modifier"), text("key")) {
        (None, None) => return Ok(None),
        (Some(modifier), Some(key)) => (modifier, key),
        _ => return Err("it needs both modifier and key".to_string()),
    };
    let named = |name: &str| {
        VirtualKey::from_name(name).ok_or_else(|| format!("unknown key name \"{name}\""))
    };
    let (modifier, key) = (named(modifier)?, named(key)?);
    let injected_only = !matches!(
        options.get("injected_only"),
        Some(SettingValue::Bool(false))
    );
    Ok(Some(TriggerChord {
        keys: KeyChord::pair(modifier, key),
        injected_only,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(pairs: &[(&str, SettingValue)]) -> OptionTable {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    fn text(value: &str) -> SettingValue {
        SettingValue::Text(value.to_string())
    }

    #[test]
    fn the_section_is_read_as_a_chord_of_two_keys() {
        let chord = own_chord(&options(&[
            ("modifier", text("LCTRL")),
            ("key", text("F24")),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(chord.keys.keys(), [VirtualKey::LCONTROL, VirtualKey(0x87)]);
        assert!(chord.injected_only, "as in a profile");
        let physical = own_chord(&options(&[
            ("modifier", text("LWIN")),
            ("key", text("D")),
            ("injected_only", SettingValue::Bool(false)),
        ]));
        assert!(!physical.unwrap().unwrap().injected_only);
    }

    #[test]
    fn what_cannot_be_used_is_said_and_nothing_is_no_mistake() {
        assert_eq!(own_chord(&OptionTable::new()), Ok(None));
        let mut notes = Vec::new();
        OWN_BUTTON.validate(&OptionTable::new(), &mut notes);
        assert!(notes.is_empty());

        OWN_BUTTON.validate(&options(&[("modifier", text("LCTRL"))]), &mut notes);
        OWN_BUTTON.validate(
            &options(&[("modifier", text("LCTRL")), ("key", text("NOPE"))]),
            &mut notes,
        );
        assert_eq!(
            notes,
            [
                "it needs both modifier and key",
                "unknown key name \"NOPE\""
            ]
        );
    }

    #[test]
    fn it_keeps_the_rules_of_a_device() {
        mujina_application::testing::device_conformance(&OWN_BUTTON);
    }
}

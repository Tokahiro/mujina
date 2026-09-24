//! Device button capture: `mujinactl capture` watches, this stores what it saw.
//! `[device.button]` holds a modifier and a key; a longer chord needs a device profile.

use std::sync::atomic::AtomicBool;

use mujina_app::tool;
use mujina_application::settings::{SettingChange, SettingValue};
use mujina_domain::chord::TriggerChord;

/// Waits for a two-key chord: `Ok(None)` on timeout or `cancel`, `Err` on failure or a longer
/// chord.
pub fn capture(cancel: &AtomicBool) -> Result<Option<TriggerChord>, String> {
    storable(tool::capture(cancel))
}

/// What `[device.button]` can hold of what the watcher saw.
fn storable(seen: Result<Option<TriggerChord>, String>) -> Result<Option<TriggerChord>, String> {
    match seen? {
        Some(button) if button.keys.keys().len() != 2 => Err(format!(
            "mujinactl capture printed \"{}\"",
            tool::capture_line(button)
        )),
        seen => Ok(seen),
    }
}

/// `[device.button]` for a captured chord of two keys ([`capture`] gives no other).
pub fn changes(button: TriggerChord) -> Vec<SettingChange> {
    let modifier = button.held().first().map(ToString::to_string);
    vec![
        SettingChange::set(
            "device.button.modifier",
            SettingValue::Text(modifier.unwrap_or_default()),
        ),
        SettingChange::set(
            "device.button.key",
            SettingValue::Text(button.trigger().to_string()),
        ),
        SettingChange::set(
            "device.button.injected_only",
            SettingValue::Bool(button.injected_only),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_chord_of_two_keys_is_stored() {
        let two = TriggerChord::parse("LWIN+D", true).unwrap();
        assert_eq!(storable(Ok(Some(two))), Ok(Some(two)));
        assert_eq!(storable(Ok(None)), Ok(None));
        let three = TriggerChord::parse("LCTRL+LWIN+LALT", true).unwrap();
        assert_eq!(
            storable(Ok(Some(three))),
            Err("mujinactl capture printed \"LCTRL+LWIN+LALT injected\"".to_string())
        );
        assert_eq!(storable(Err("no hook".into())), Err("no hook".to_string()));
    }

    #[test]
    fn a_captured_button_is_stored_whole() {
        let changes = changes(TriggerChord::parse("LWIN+D", false).unwrap());
        assert_eq!(
            changes[..2],
            [
                SettingChange::set("device.button.modifier", SettingValue::Text("LWIN".into())),
                SettingChange::set("device.button.key", SettingValue::Text("D".into())),
            ]
        );
        assert_eq!(
            changes[2],
            SettingChange::set("device.button.injected_only", SettingValue::Bool(false))
        );
    }
}

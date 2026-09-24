//! Steam's keyboard shortcuts. The overlay's is set per account in `localconfig.vdf`, as
//! `"InGameOverlayShortcutKey"  "Shift\tKEY_TAB"`; absent while the default is in effect.

use std::path::Path;

use mujina_domain::keys::{KeyChord, VirtualKey};

const SETTING: &str = "\"InGameOverlayShortcutKey\"";

pub fn menu() -> KeyChord {
    KeyChord::pair(VirtualKey::LCONTROL, VirtualKey::DIGIT_1)
}

pub fn default_overlay() -> KeyChord {
    KeyChord::pair(VirtualKey::LSHIFT, VirtualKey::TAB)
}

/// The overlay shortcut of the given Steam account, if it was changed and is expressible.
pub fn configured_overlay(steam_directory: &Path, account_id: u32) -> Option<KeyChord> {
    let path = steam_directory
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("localconfig.vdf");
    let text = std::fs::read(path).ok()?;
    parse_overlay(&String::from_utf8_lossy(&text))
}

pub fn parse_overlay(vdf: &str) -> Option<KeyChord> {
    let line = vdf
        .lines()
        .find(|line| line.trim_start().starts_with(SETTING))?;
    let value = line
        .trim_start()
        .strip_prefix(SETTING)?
        .trim()
        .trim_matches('"');
    let keys: Option<Vec<VirtualKey>> = value
        // VDF escapes a tab inside a value as the two characters `\` and `t`.
        .replace(r"\t", " ")
        .split_whitespace()
        .map(translate)
        .collect();
    KeyChord::from_keys(&keys?)
}

fn translate(token: &str) -> Option<VirtualKey> {
    match token.to_ascii_uppercase().as_str() {
        "SHIFT" => Some(VirtualKey::LSHIFT),
        "CTRL" | "CONTROL" => Some(VirtualKey::LCONTROL),
        "ALT" => Some(VirtualKey::LMENU),
        key => VirtualKey::from_name(key.strip_prefix("KEY_")?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_changed_shortcut() {
        let vdf = "\"UserLocalConfigStore\"\n{\n\t\"system\"\n\t{\n\t\t\"InGameOverlayShortcutKey\"\t\t\"Shift\\tKEY_F12\"\n\t}\n}\n";
        let expected = KeyChord::from_keys(&[VirtualKey::LSHIFT, VirtualKey(0x7B)]);
        assert_eq!(parse_overlay(vdf), expected);
    }

    #[test]
    fn single_key_and_real_tab_separator() {
        assert_eq!(
            parse_overlay("\t\"InGameOverlayShortcutKey\"\t\t\"KEY_INSERT\""),
            KeyChord::from_keys(&[VirtualKey::INSERT])
        );
        assert_eq!(
            parse_overlay("\"InGameOverlayShortcutKey\" \"Ctrl\tAlt\tKEY_O\""),
            KeyChord::from_keys(&[VirtualKey::LCONTROL, VirtualKey::LMENU, VirtualKey(0x4F)])
        );
    }

    #[test]
    fn absent_or_unknown_means_default() {
        assert_eq!(
            parse_overlay("\"InGameOverlayShowFPSCorner\"\t\t\"0\""),
            None
        );
        assert_eq!(
            parse_overlay("\"InGameOverlayShortcutKey\"\t\t\"KEY_WHATEVER\""),
            None
        );
        assert_eq!(parse_overlay("\"InGameOverlayShortcutKey\"\t\t\"\""), None);
    }
}

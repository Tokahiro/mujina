//! What the user (or a device profile) can decide about Mujina's behaviour.

pub mod schema;

use mujina_domain::keys::{HoldTiming, KeyChord};
use mujina_domain::supervision::ExitPolicy;

use crate::device::DeviceSelection;
use crate::launcher::LauncherSelection;

// Independent switches a user flips one at a time, not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub launcher: LauncherSelection,
    pub button_remap: bool,
    /// As `[device]` chose it; none when the device is unknown.
    pub device: DeviceSelection,
    /// Overrides the launcher's own menu shortcut.
    pub menu: Option<KeyChord>,
    /// Overrides the launcher's own overlay shortcut.
    pub overlay: Option<KeyChord>,
    pub exit_policy: ExitPolicy,
    pub timing: HoldTiming,
    /// Keep the launcher on its "game is starting" screen until the game shows itself.
    pub game_start_screen: bool,
    /// A black window while the launcher starts, instead of Xbox mode's own backdrop.
    pub launch_screen: bool,
    /// Log every event, not only what a user would want to read.
    pub detailed_log: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launcher: LauncherSelection::default(),
            button_remap: true,
            device: DeviceSelection::none(),
            menu: None,
            overlay: None,
            exit_policy: ExitPolicy::default(),
            timing: HoldTiming::default(),
            game_start_screen: true,
            launch_screen: false,
            detailed_log: false,
        }
    }
}

impl Settings {
    /// The device as the agent should drive it: none while the button is switched off.
    pub fn active_device(&self) -> DeviceSelection {
        if self.button_remap {
            self.device.clone()
        } else {
            DeviceSelection::none()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadedSettings {
    pub settings: Settings,
    /// Human-readable remarks: ignored entries, fallbacks, the profile chosen.
    pub notes: Vec<String>,
}

pub trait SettingsSource {
    /// Never fails: a broken configuration yields defaults and a note saying so.
    fn load(&self) -> LoadedSettings;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
    Bool(bool),
    Integer(i64),
    Text(String),
    TextList(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChange {
    /// Dotted path as in `config.toml`, e.g. `features.launch_screen`.
    pub key: String,
    /// `None` removes the entry, which restores the default.
    pub value: Option<SettingValue>,
}

impl SettingChange {
    pub fn set(key: &str, value: SettingValue) -> Self {
        Self {
            key: key.to_string(),
            value: Some(value),
        }
    }

    pub fn unset(key: &str) -> Self {
        Self {
            key: key.to_string(),
            value: None,
        }
    }
}

pub trait SettingsStore {
    /// All or none; refuses what the configuration would ignore or report, giving the reason.
    fn apply(&self, changes: &[SettingChange]) -> crate::ports::PortResult<()>;

    /// `None` where `key` relies on the default.
    fn stored(&self, key: &str) -> Option<SettingValue>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_device_is_only_driven_while_remapping_is_on() {
        let device = DeviceSelection {
            id: Some("onexplayer".to_string()),
            ..DeviceSelection::none()
        };
        let mut settings = Settings {
            device: device.clone(),
            ..Settings::default()
        };
        assert_eq!(settings.active_device(), device);
        settings.button_remap = false;
        assert!(settings.active_device().is_none());
    }
}

//! Settings from `config.toml`. Launcher and device sections are read against their descriptors;
//! this crate names no launcher or device itself.

#[cfg(test)]
mod fakes;
mod file;
mod store;

use std::fs::{OpenOptions, TryLockError};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use mujina_application::device::{Devices, SystemIdentity};
use mujina_application::launcher::Launchers;
use mujina_application::settings::{LoadedSettings, SettingsSource};

pub use store::{StoredSnapshot, parse_value};

const FILE_NAME: &str = "config.toml";

/// The template up to the launchers' own sections, which follow `[launcher]`.
const TEMPLATE_HEAD: &str = r#"# Mujina configuration. Everything is optional; remove the leading '#' to change a value,
# or use `mujinactl config set section.name value`, which checks the value first and applies
# it at once. Changes made here by hand take effect the next time Xbox mode is entered.

[features]
# button_remap = true       # device button -> launcher menu / overlay
# game_start_screen = true  # keep the launcher on its "game is starting" screen while a game loads
# launch_screen = false     # black screen while the launcher starts, instead of Xbox mode's own

[launcher]
# kind = "steam"                  # or another launcher below, e.g. "generic" with its section
# on_exit = "relaunch_on_crash"   # or "relaunch_always", "nothing"
# menu = "LCTRL+1"                # default: the launcher's own shortcut
# overlay = "LSHIFT+TAB"          # default: read from the launcher's settings
"#;

/// The template after the launchers' own sections.
const TEMPLATE_TAIL: &str = r#"
[device]
# profile = "auto"          # "auto", "none", or the id of a device Mujina has

# A button of your own instead of a profile (see `mujinactl doctor` for the device strings):
# [device.button]
# modifier = "LWIN"
# key = "D"
# injected_only = true

[timing]
# modifier_gap_ms = 20
# key_hold_ms = 50

[logging]
# level = "info"            # "debug" also logs every event the agent sees (for bug reports)

[interface]
# language = "auto"         # of Mujina Settings: "auto" follows Windows, or "en", "de"
"#;

/// The commented template of `config.toml`: Mujina's own settings, with each launcher's
/// section after `[launcher]`.
pub fn template(launchers: &Launchers) -> String {
    let mut text = TEMPLATE_HEAD.to_string();
    for launcher in launchers.all {
        text.push('\n');
        text.push_str(launcher.template());
    }
    text.push_str(TEMPLATE_TAIL);
    text
}

pub struct ConfigFile {
    path: PathBuf,
    system: SystemIdentity,
    /// The launchers compiled in, whose sections the file may hold.
    launchers: Launchers,
    /// The devices compiled in, which `[device]` chooses among and whose sections it may hold.
    devices: Devices,
}

impl ConfigFile {
    /// `config.toml` in `directory`.
    pub fn for_system(
        directory: &Path,
        system: SystemIdentity,
        launchers: Launchers,
        devices: Devices,
    ) -> Self {
        Self {
            path: directory.join(FILE_NAME),
            system,
            launchers,
            devices,
        }
    }

    /// Writes a commented template unless a configuration already exists.
    pub fn ensure_template(&self) {
        // A change holding the lock writes the file anyway. Holding it here keeps a change from
        // reading a half-written template.
        let lock = store::lock_file(&self.path);
        if let Ok(lock) = &lock
            && matches!(lock.try_lock(), Err(TryLockError::WouldBlock))
        {
            return;
        }
        // `create_new` never replaces a file another process has just written. Errors are
        // ignored: without a template the defaults still apply.
        if let Ok(mut file) = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
        {
            let _ = file.write_all(template(&self.launchers).as_bytes());
        }
    }

    pub fn system(&self) -> &SystemIdentity {
        &self.system
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file read once, for asking many keys; each
    /// [`SettingsStore::stored`](mujina_application::settings::SettingsStore::stored) reads it
    /// again.
    pub fn snapshot(&self) -> StoredSnapshot {
        StoredSnapshot::read(&self.path)
    }
}

impl SettingsSource for ConfigFile {
    fn load(&self) -> LoadedSettings {
        let mut notes = Vec::new();
        let parsed = match std::fs::read_to_string(&self.path) {
            // A key that is wrong costs that key alone; text that is no TOML costs the file.
            Ok(text) => {
                file::parse(&text, &self.launchers, &self.devices).unwrap_or_else(|error| {
                    notes.push(format!(
                        "{} is not valid and was ignored: {}",
                        self.path.display(),
                        error.message()
                    ));
                    file::Parsed::default()
                })
            }
            Err(_) => file::Parsed::default(),
        };
        let settings = parsed.resolve(&self.system, &self.devices, &self.launchers, &mut notes);
        LoadedSettings { settings, notes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn onexplayer() -> SystemIdentity {
        SystemIdentity {
            manufacturer: "ONE-NETBOOK".to_string(),
            product: "ONEXPLAYER 3".to_string(),
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mujina-config-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_template_is_valid_and_changes_nothing() {
        let dir = temp_dir("template");
        let config = ConfigFile::for_system(&dir, onexplayer(), fakes::LAUNCHERS, fakes::DEVICES);
        let before = config.load();
        config.ensure_template();
        let after = config.load();
        assert_eq!(before.settings, after.settings);
        assert!(after.notes.is_empty(), "{:?}", after.notes);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_template_never_replaces_a_file() {
        let dir = temp_dir("keep");
        std::fs::write(dir.join(FILE_NAME), "[timing]\nkey_hold_ms = 80\n").unwrap();
        let config = ConfigFile::for_system(&dir, onexplayer(), fakes::LAUNCHERS, fakes::DEVICES);
        config.ensure_template();
        let text = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        assert_eq!(text, "[timing]\nkey_hold_ms = 80\n");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn text_that_is_no_toml_falls_back_to_defaults_with_a_note() {
        let dir = temp_dir("broken");
        std::fs::write(dir.join(FILE_NAME), "[features\nbutton_remap = maybe").unwrap();
        let loaded =
            ConfigFile::for_system(&dir, onexplayer(), fakes::LAUNCHERS, fakes::DEVICES).load();
        assert!(loaded.settings.button_remap);
        assert!(loaded.notes.iter().any(|note| note.contains("not valid")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_wrong_key_costs_that_key_alone() {
        let dir = temp_dir("typo");
        let text = "[featurs]\nlaunch_screen = true\n[features]\nbutton_remap = \"no\"\n\
                    [launcher]\non_exit = \"nothing\"\n";
        std::fs::write(dir.join(FILE_NAME), text).unwrap();
        let loaded =
            ConfigFile::for_system(&dir, onexplayer(), fakes::LAUNCHERS, fakes::DEVICES).load();
        assert_eq!(loaded.notes.len(), 2, "{:?}", loaded.notes);
        assert!(loaded.notes.iter().all(|note| !note.contains("not valid")));
        assert_eq!(
            loaded.settings.exit_policy,
            mujina_domain::supervision::ExitPolicy::Nothing
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_template_names_every_setting_and_nothing_else() {
        use mujina_application::settings::schema::CORE;
        use mujina_application::testing::template_keys;

        let mut named: Vec<String> = template_keys(&template(&fakes::LAUNCHERS))
            .into_iter()
            .map(|(section, key)| format!("{section}.{key}"))
            .collect();
        let mut described: Vec<String> = CORE
            .iter()
            .flat_map(|section| {
                section
                    .settings
                    .iter()
                    .map(|spec| format!("{}.{}", section.name, spec.key))
            })
            .collect();
        for launcher in fakes::LAUNCHERS.all {
            described.extend(
                launcher
                    .settings()
                    .iter()
                    .map(|spec| format!("launcher.{}.{}", launcher.id(), spec.key)),
            );
        }
        // Sorted, so that a key named twice shows as well as one missing.
        named.sort();
        described.sort();
        assert_eq!(named, described);
    }

    #[test]
    fn the_launchers_sections_follow_the_launcher_section() {
        let text = template(&fakes::LAUNCHERS);
        let launcher = text.find("\n[launcher]\n").unwrap();
        let steam = text.find("[launcher.steam]").unwrap();
        let generic = text.find("# [launcher.generic]").unwrap();
        let device = text.find("\n[device]\n").unwrap();
        assert!(
            launcher < steam && steam < generic && generic < device,
            "{text}"
        );
    }
}

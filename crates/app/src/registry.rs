//! The launchers and devices compiled into Mujina: the lists a new one is added to (ADR-0013,
//! `docs/new-launcher.md`, `docs/new-device.md`). Nothing else in the composition root names a
//! launcher or a device.

use std::sync::LazyLock;

use mujina_adapter_kit::plugin::{DevicePlugin, DeviceRuntime, LauncherPlugin};
use mujina_application::device::{DeviceDescriptor, DeviceSelection, Devices};
use mujina_application::launcher::{LauncherDescriptor, Launchers};

/// Every launcher, in the order Mujina Settings lists them. One line each; its crate is also
/// named once in `[workspace.dependencies]` and once in this crate's `Cargo.toml`.
pub static LAUNCHERS: &[&LauncherPlugin] = &[
    &mujina_adapter_steam::PLUGIN,
    &mujina_adapter_generic::PLUGIN,
];

/// What applies where the configuration names no launcher, or one that cannot be used: Steam,
/// which Mujina is built around.
pub static FALLBACK: &LauncherPlugin = &mujina_adapter_steam::PLUGIN;

/// Devices whose buttons are not key chords (a vendor HID report, a WMI event), one line each;
/// their crate is also named in `[workspace.dependencies]` and this crate's `Cargo.toml`.
pub static DEVICE_PLUGINS: &[&DevicePlugin] = &[];

static DESCRIPTORS: LazyLock<Vec<&'static dyn LauncherDescriptor>> =
    LazyLock::new(|| LAUNCHERS.iter().map(|plugin| plugin.descriptor).collect());

/// Every device: those of their own crates, then the key-chord devices. `profile = "auto"` takes
/// the first that matches, so a device crate is not hidden by a profile for a whole family.
static DEVICES: LazyLock<Vec<DevicePlugin>> = LazyLock::new(|| {
    DEVICE_PLUGINS
        .iter()
        .map(|plugin| **plugin)
        .chain(mujina_adapter_keyboard::plugins().iter().copied())
        .collect()
});

static DEVICE_DESCRIPTORS: LazyLock<Vec<&'static dyn DeviceDescriptor>> =
    LazyLock::new(|| DEVICES.iter().map(|plugin| plugin.descriptor).collect());

/// The launchers as the configuration and the tools see them.
pub fn launchers() -> Launchers {
    Launchers {
        all: DESCRIPTORS.as_slice(),
        fallback: FALLBACK.descriptor,
    }
}

/// The launcher `id` names, or the fallback.
pub fn plugin(id: &str) -> &'static LauncherPlugin {
    LAUNCHERS
        .iter()
        .copied()
        .find(|plugin| plugin.descriptor.id() == id)
        .unwrap_or(FALLBACK)
}

/// The devices as the configuration and the tools see them.
pub fn devices() -> Devices {
    Devices {
        all: DEVICE_DESCRIPTORS.as_slice(),
        own: &mujina_adapter_keyboard::OWN_BUTTON,
    }
}

/// Every device with what runs it.
pub fn device_plugins() -> &'static [DevicePlugin] {
    &DEVICES
}

/// What runs the device `id` names. With none, the key-chord devices' runtime, so that a button
/// captured or a profile chosen while the agent runs applies at once.
pub fn device_runtime(id: Option<&str>) -> &'static dyn DeviceRuntime {
    id.and_then(|id| DEVICES.iter().find(|plugin| plugin.descriptor.id() == id))
        .map_or(KEYBOARD, |plugin| plugin.runtime)
}

/// The runtime of every key-chord device.
static KEYBOARD: &dyn DeviceRuntime = &mujina_adapter_keyboard::RUNTIME;

/// Whether an agent on runtime `running` must wait for the next session to take over the device
/// `next` names, which is when another runtime runs it. `None` never waits.
pub fn device_waits(running: &dyn DeviceRuntime, next: Option<&str>) -> bool {
    next.is_some() && !std::ptr::addr_eq(device_runtime(next), running)
}

/// The keys the buttons of `device` arrive as, for the tools to show, e.g. `LWIN+D`; empty for a
/// device whose buttons are no key chords, and for none.
pub fn button_keys(device: &DeviceSelection) -> String {
    let chords = mujina_adapter_keyboard::chords_for(device).unwrap_or_default();
    let keys: Vec<String> = chords
        .iter()
        .map(|(_, chord)| chord.keys.to_string())
        .collect();
    keys.join(", ")
}

#[cfg(test)]
mod tests {
    use mujina_adapter_kit::plugin::DeviceParts;
    use mujina_application::device::SystemIdentity;
    use mujina_application::launcher::OptionTable;
    use mujina_application::ports::{PortError, PortResult};
    use mujina_application::testing::{conformance, device_conformance, template_keys};

    use super::*;

    #[test]
    fn every_launcher_keeps_the_rules_under_an_id_of_its_own() {
        for (index, plugin) in LAUNCHERS.iter().enumerate() {
            conformance(plugin.descriptor);
            let id = plugin.descriptor.id();
            assert!(
                LAUNCHERS[..index]
                    .iter()
                    .all(|other| other.descriptor.id() != id),
                "{id} is listed twice"
            );
        }
        assert!(
            LAUNCHERS
                .iter()
                .any(|plugin| std::ptr::eq(*plugin, FALLBACK)),
            "the fallback is one of the launchers"
        );
    }

    #[test]
    fn every_device_keeps_the_rules_under_an_id_of_its_own() {
        let all = devices().all;
        for (index, device) in all.iter().enumerate() {
            device_conformance(*device);
            let id = device.id();
            assert!(
                all[..index].iter().all(|other| other.id() != id),
                "{id} is listed twice"
            );
        }
        let own = devices().own;
        assert!(
            all.iter().any(|device| device.id() == own.id()),
            "the button of one's own is one of the devices"
        );
        assert!(all.iter().any(|device| device.id() == "onexplayer"));
    }

    #[test]
    fn a_device_is_run_by_its_own_runtime_and_none_by_the_keyboards() {
        let keyboard: &dyn DeviceRuntime = &mujina_adapter_keyboard::RUNTIME;
        for plugin in device_plugins() {
            let runtime = device_runtime(Some(plugin.descriptor.id()));
            assert!(std::ptr::addr_eq(runtime, plugin.runtime));
        }
        assert!(std::ptr::addr_eq(device_runtime(None), keyboard));
        // An id no device has, whichever devices are listed.
        assert!(std::ptr::addr_eq(
            device_runtime(Some("no such device")),
            keyboard
        ));
    }

    /// A runtime of a crate of its own, for [`device_waits`].
    struct Elsewhere;

    impl DeviceRuntime for Elsewhere {
        fn start(&self, _device: &DeviceSelection) -> PortResult<DeviceParts> {
            Err(PortError::Failed("not started in this test".into()))
        }
    }

    #[test]
    fn only_a_device_of_another_runtime_waits_for_the_next_session() {
        let keyboard: &dyn DeviceRuntime = &mujina_adapter_keyboard::RUNTIME;
        let onexplayer = Some("onexplayer");
        assert!(!device_waits(keyboard, onexplayer));
        assert!(!device_waits(
            keyboard,
            Some(mujina_adapter_keyboard::OWN_ID)
        ));
        assert!(!device_waits(keyboard, None), "no device is taken over");
        // An agent running a device crate's runtime keeps it for a profile until the next
        // session, but switches the button off at once.
        assert!(device_waits(&Elsewhere, onexplayer));
        assert!(!device_waits(&Elsewhere, None));
    }

    #[test]
    fn a_launcher_is_found_by_its_id_or_else_the_fallback_applies() {
        assert_eq!(plugin("generic").descriptor.id(), "generic");
        assert_eq!(plugin("").descriptor.id(), FALLBACK.descriptor.id());
        assert_eq!(plugin("heroic").descriptor.id(), FALLBACK.descriptor.id());
        assert_eq!(launchers().all.len(), LAUNCHERS.len());
    }

    #[test]
    fn the_template_names_each_launchers_options() {
        let named = template_keys(&mujina_adapter_config::template(&launchers()));
        for plugin in LAUNCHERS {
            let section = format!("launcher.{}", plugin.descriptor.id());
            let keys: Vec<&str> = named
                .iter()
                .filter(|(under, _)| *under == section)
                .map(|(_, key)| key.as_str())
                .collect();
            let settings: Vec<&str> = plugin
                .descriptor
                .settings()
                .iter()
                .map(|spec| spec.key)
                .collect();
            assert_eq!(keys, settings, "[{section}]");
        }
    }

    fn onexplayer() -> SystemIdentity {
        SystemIdentity {
            manufacturer: "ONE-NETBOOK".to_string(),
            product: "ONEXPLAYER 3".to_string(),
        }
    }

    #[test]
    fn the_template_loads_without_a_note_and_takes_steams_options_in_place() {
        use mujina_adapter_config::ConfigFile;
        use mujina_application::settings::{
            SettingChange, SettingValue, SettingsSource, SettingsStore,
        };

        let dir = std::env::temp_dir().join(format!("mujina-registry-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = ConfigFile::for_system(&dir, onexplayer(), launchers(), devices());
        config.ensure_template();
        let loaded = config.load();
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(loaded.settings.launcher.id, FALLBACK.descriptor.id());
        assert_eq!(loaded.settings.device.id.as_deref(), Some("onexplayer"));

        config
            .apply(&[SettingChange::set(
                "launcher.steam.ui_link",
                SettingValue::Bool(false),
            )])
            .unwrap();
        assert_eq!(
            config.stored("launcher.steam.ui_link"),
            Some(SettingValue::Bool(false))
        );
        // Uncommented under the template's own header, not added as a second section.
        let text = std::fs::read_to_string(config.path()).unwrap();
        assert_eq!(text.matches("[launcher.steam]").count(), 1, "{text}");
        assert!(config.load().notes.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_button_of_ones_own_is_checked_by_the_keyboard_device_and_wins() {
        use mujina_adapter_config::ConfigFile;
        use mujina_application::settings::{
            SettingChange, SettingValue, SettingsSource, SettingsStore,
        };

        let dir = std::env::temp_dir().join(format!("mujina-own-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = ConfigFile::for_system(&dir, onexplayer(), launchers(), devices());
        config.ensure_template();
        let key = |name: &str| SettingValue::Text(name.to_string());
        // A key name the keyboard device does not know is refused, whatever the reader thinks.
        let refused = config.apply(&[
            SettingChange::set("device.button.modifier", key("LCTRL")),
            SettingChange::set("device.button.key", key("NOPE")),
        ]);
        assert!(
            refused
                .unwrap_err()
                .to_string()
                .contains("[device.button] ignored: unknown key name \"NOPE\"")
        );
        config
            .apply(&[
                SettingChange::set("device.button.modifier", key("LCTRL")),
                SettingChange::set("device.button.key", key("F24")),
            ])
            .unwrap();
        let loaded = config.load();
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        let device = &loaded.settings.device;
        assert_eq!(device.id.as_deref(), Some(mujina_adapter_keyboard::OWN_ID));
        assert_eq!(button_keys(device), "LCTRL+F24");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_launcher_comes_up_for_the_home_role_with_its_defaults() {
        for plugin in LAUNCHERS {
            let home = plugin.runtime.home(&OptionTable::new());
            assert!(!home.display_name().is_empty());
        }
    }
}

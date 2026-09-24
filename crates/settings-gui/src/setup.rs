//! The Setup page's data and list changes. Nothing here names a specific launcher or device.

use mujina_app::tool::{self, Configuration};
use mujina_application::device::DeviceDescriptor;
use mujina_application::launcher::{LauncherDescriptor, OptionTable};
use mujina_application::settings::schema::{self, SettingKind, SettingSpec};
use mujina_application::settings::{SettingChange, SettingValue, Settings};

use crate::form::{self, PROFILE_CHOICES};
use crate::rows::{self, Owner, Rows};
use crate::ui::Config;

pub struct Page {
    pub config: Config,
    pub launcher: Rows,
    pub device: Rows,
    /// In the registry's order, translated.
    pub launcher_names: Vec<String>,
    /// Automatic, None, then the devices a profile names.
    pub profiles: Vec<String>,
    /// The keys the button in effect arrives as, e.g. `LWIN+D`; empty without one.
    pub button_in_use: String,
}

/// `draft`: a launcher chosen but not stored yet, shown as chosen with its rows.
pub fn page(configuration: &Configuration, draft: Option<&'static str>) -> Page {
    let settings = &configuration.loaded.settings;
    let launchers = tool::launchers();
    let devices = tool::devices();
    let known: Vec<&dyn DeviceDescriptor> = devices.profiles().collect();
    let shown = draft.map_or_else(
        || shown_launcher(configuration.stored("launcher.kind"), &settings.launcher.id),
        |id| launchers.get(id),
    );
    let options = options_of(configuration, shown);
    let launcher = rows::build(
        Owner::Launcher,
        &format!("launcher.{}", shown.id()),
        shown.settings(),
        &options,
        draft.is_some(),
        &crate::texts::launcher_words(shown),
    );
    // A button of one's own is a core setting, not a device's rows.
    let device = settings
        .device
        .id
        .as_deref()
        .and_then(|id| devices.find(id))
        .map_or_else(Rows::default, |device| {
            rows::build(
                Owner::Device,
                &format!("device.{}", device.id()),
                device.settings(),
                &settings.device.options,
                false,
                &crate::texts::device_words(device),
            )
        });
    let found = known
        .iter()
        .find(|device| device.matches(&configuration.system));
    let automatic = match found {
        Some(device) => crate::texts::t_with(&crate::texts::AUTOMATIC_WITH, &button_label(*device)),
        None => crate::texts::t(&crate::texts::AUTOMATIC),
    };
    let mut profiles = vec![automatic, crate::texts::t(&crate::texts::NONE)];
    profiles.extend(
        known
            .iter()
            .map(|device| crate::texts::device_words(*device)(&device.name())),
    );
    Page {
        config: config(configuration, &known, shown, &options),
        launcher,
        device,
        launcher_names: launchers
            .all
            .iter()
            .map(|launcher| crate::texts::launcher_words(*launcher)(launcher.name()))
            .collect(),
        profiles,
        button_in_use: tool::button_keys(&settings.device),
    }
}

/// The launcher the page shows: the one `config.toml` names if Mujina has it, even if unusable
/// as stored, so its rows can fix it; otherwise `in_effect`.
pub fn shown_launcher(
    stored: Option<SettingValue>,
    in_effect: &str,
) -> &'static dyn LauncherDescriptor {
    let named = form::as_text(stored);
    let launchers = tool::launchers();
    launchers.find(&named).unwrap_or(launchers.get(in_effect))
}

/// The options of `launcher`: as in effect for the running one (an old setting may have decided
/// them, see adapter-config), as stored for another.
fn options_of(configuration: &Configuration, launcher: &dyn LauncherDescriptor) -> OptionTable {
    let settings = &configuration.loaded.settings;
    if settings.launcher.id == launcher.id() {
        return settings.launcher.options.clone();
    }
    launcher
        .settings()
        .iter()
        .filter_map(|spec| {
            let stored = configuration.stored(&format!("launcher.{}.{}", launcher.id(), spec.key));
            Some((spec.key.to_string(), stored?))
        })
        .collect()
}

/// The required settings of `launcher` that `configuration` does not store.
pub fn missing(
    configuration: &Configuration,
    launcher: &dyn LauncherDescriptor,
) -> Vec<&'static SettingSpec> {
    launcher
        .settings()
        .iter()
        .filter(|spec| spec.required)
        .filter(|spec| {
            let key = format!("launcher.{}.{}", launcher.id(), spec.key);
            configuration.stored(&key).is_none()
        })
        .collect()
}

/// The device's button labels, or its name if it has none.
pub fn button_label(device: &dyn DeviceDescriptor) -> String {
    let words = crate::texts::device_words(device);
    let labels: Vec<String> = device
        .buttons()
        .iter()
        .map(|button| words(&button.label))
        .collect();
    if labels.is_empty() {
        words(&device.name())
    } else {
        labels.join(", ")
    }
}

/// `known`: the profile devices, listed after `PROFILE_CHOICES`.
fn config(
    store: &Configuration,
    known: &[&dyn DeviceDescriptor],
    shown: &dyn LauncherDescriptor,
    options: &OptionTable,
) -> Config {
    let text = |key: &str| form::as_text(store.stored(key));
    let flag = |key: &str, default: bool| match store.stored(key) {
        Some(SettingValue::Bool(on)) => on,
        _ => default,
    };
    let number = |key: &str, default: u16| match store.stored(key) {
        Some(SettingValue::Integer(value)) => i32::try_from(value).unwrap_or(i32::from(default)),
        _ => i32::from(default),
    };
    let defaults = Settings::default();
    let profile = text("device.profile");
    let device_profile = match PROFILE_CHOICES.iter().position(|choice| *choice == profile) {
        Some(index) => i32::try_from(index).unwrap_or(0),
        None => known
            .iter()
            .position(|known| known.id() == profile)
            .and_then(|index| i32::try_from(index + PROFILE_CHOICES.len()).ok())
            .unwrap_or(0),
    };
    let caps = shown.capabilities(options);
    // Its starting screen goes to the launcher's page, which takes navigation.
    let start_screen_off = caps.game_detection && !caps.navigation;
    Config {
        launcher_kind: launcher_index(shown.id()),
        game_detection: caps.game_detection,
        start_screen_off,
        start_screen_needs: if start_screen_off {
            navigation_needs(shown, options)
        } else {
            String::new()
        }
        .into(),
        on_exit: form::index_of(form::choices("launcher.on_exit"), &text("launcher.on_exit")),
        menu: text("launcher.menu").into(),
        overlay: text("launcher.overlay").into(),
        device_profile,
        custom_button: form::button_text(
            &text("device.button.modifier"),
            &text("device.button.key"),
        )
        .into(),
        modifier_gap_ms: number("timing.modifier_gap_ms", defaults.timing.modifier_gap_ms),
        key_hold_ms: number("timing.key_hold_ms", defaults.timing.key_hold_ms),
        button_remap: flag("features.button_remap", defaults.button_remap),
        game_start_screen: flag("features.game_start_screen", defaults.game_start_screen),
        launch_screen: flag("features.launch_screen", defaults.launch_screen),
        debug_log: text("logging.level") == "debug",
        language: form::index_of(
            form::choices("interface.language"),
            &text("interface.language"),
        ),
    }
}

/// The translated title of the one switch that would give `launcher` navigation; empty if none
/// does.
fn navigation_needs(launcher: &dyn LauncherDescriptor, options: &OptionTable) -> String {
    launcher
        .settings()
        .iter()
        .filter(|spec| matches!(spec.kind, SettingKind::Toggle { .. }))
        .filter(|spec| !schema::flag(launcher.settings(), options, spec.key))
        .find(|spec| {
            let mut on = options.clone();
            on.insert(spec.key.to_string(), SettingValue::Bool(true));
            launcher.capabilities(&on).navigation
        })
        .map(|spec| crate::texts::launcher_words(launcher)(spec.title))
        .unwrap_or_default()
}

/// The position of launcher `id` in the page's list; the fallback's for one Mujina lacks.
pub fn launcher_index(id: &str) -> i32 {
    let launchers = tool::launchers();
    let id = launchers.get(id).id();
    launchers
        .all
        .iter()
        .position(|launcher| launcher.id() == id)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or(0)
}

pub fn launcher_at(index: i32) -> Option<&'static dyn LauncherDescriptor> {
    let index = usize::try_from(index).ok()?;
    tool::launchers().all.get(index).copied()
}

/// A list entry's change. The default is stored by leaving the key out; in the core lists it is
/// the first entry (for launchers, the fallback).
pub fn choice_change(key: &str, index: i32) -> SettingChange {
    let index = usize::try_from(index).unwrap_or(0);
    let value = match key {
        "launcher.kind" => {
            let launchers = tool::launchers();
            let id = launchers
                .all
                .get(index)
                .map(|launcher| launcher.id())
                .filter(|id| *id != launchers.fallback.id());
            return form::choice(key, id);
        }
        "device.profile" if index == 1 => Some(PROFILE_CHOICES[1].to_string()),
        "device.profile" => index
            .checked_sub(PROFILE_CHOICES.len())
            .and_then(|profile| {
                tool::devices()
                    .profiles()
                    .nth(profile)
                    .map(|known| known.id().to_string())
            }),
        _ => {
            let Some(SettingKind::Choice { values, default }) = spec(key).map(|spec| spec.kind)
            else {
                return form::choice(key, None);
            };
            let value = values.get(index).copied().filter(|value| *value != default);
            return form::choice(key, value);
        }
    };
    form::choice(key, value.filter(|_| index != 0).as_deref())
}

pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    schema::find(key, tool::launchers().all, tool::devices().all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_shows_the_launcher_the_file_names_where_mujina_has_it() {
        let named = |id: &str| Some(SettingValue::Text(id.to_string()));
        let shown = |stored, in_effect| shown_launcher(stored, in_effect).id();
        assert_eq!(shown(named("generic"), "steam"), "generic");
        assert_eq!(shown(named("heroic"), "steam"), "steam");
        assert_eq!(shown(None, "generic"), "generic");
        assert_eq!(shown(None, "steam"), "steam");
    }

    #[test]
    fn the_starting_screen_names_the_switch_that_lets_the_launcher_show_its_pages() {
        let steam = tool::launchers().get("steam");
        let off = OptionTable::from([("ui_link".to_string(), SettingValue::Bool(false))]);
        assert!(!steam.capabilities(&off).navigation);
        assert_eq!(navigation_needs(steam, &off), "Use Steam's debugging port");
        let generic = tool::launchers().get("generic");
        assert_eq!(navigation_needs(generic, &OptionTable::new()), "");
    }

    #[test]
    fn list_entries_become_changes_and_the_first_is_the_default() {
        assert_eq!(
            choice_change("launcher.on_exit", 2),
            SettingChange::set("launcher.on_exit", SettingValue::Text("nothing".into()))
        );
        assert_eq!(
            choice_change("launcher.on_exit", 0),
            SettingChange::unset("launcher.on_exit")
        );
        assert_eq!(
            choice_change("interface.language", 2),
            SettingChange::set("interface.language", SettingValue::Text("de".into()))
        );
        assert_eq!(
            choice_change("device.profile", 1),
            SettingChange::set("device.profile", SettingValue::Text("none".into()))
        );
        // Not a fixed index: a device crate in `DEVICE_PLUGINS` comes before the profiles.
        let onexplayer = tool::devices()
            .profiles()
            .position(|device| device.id() == "onexplayer")
            .unwrap();
        assert_eq!(
            choice_change(
                "device.profile",
                i32::try_from(PROFILE_CHOICES.len() + onexplayer).unwrap()
            ),
            SettingChange::set("device.profile", SettingValue::Text("onexplayer".into()))
        );
        assert_eq!(
            choice_change("launcher.kind", 1),
            SettingChange::set("launcher.kind", SettingValue::Text("generic".into()))
        );
        assert_eq!(
            choice_change("launcher.kind", 0),
            SettingChange::unset("launcher.kind")
        );
    }
}

//! The shape of `config.toml` and how it becomes [`Settings`], key by key.

use std::collections::BTreeMap;

use mujina_application::device::{self, DeviceChoice, Devices, SystemIdentity};
use mujina_application::launcher::{LauncherDescriptor, LauncherSelection, Launchers, OptionTable};
use mujina_application::settings::schema::SettingSpec;
use mujina_application::settings::{SettingValue, Settings};
use mujina_domain::keys::{HoldTiming, KeyChord};
use mujina_domain::supervision::ExitPolicy;
use toml::{Table, Value};

/// What the file says; `None` where it says nothing usable.
#[derive(Debug, Default)]
pub struct RawConfig {
    features: RawFeatures,
    launcher: RawLauncher,
    device: RawDevice,
    timing: RawTiming,
    logging: RawLogging,
    interface: RawInterface,
}

#[derive(Debug, Default)]
struct RawFeatures {
    button_remap: Option<bool>,
    game_start_screen: Option<bool>,
    launch_screen: Option<bool>,
}

#[derive(Debug, Default)]
struct RawLauncher {
    kind: Option<String>,
    on_exit: Option<String>,
    menu: Option<String>,
    overlay: Option<String>,
    /// `[launcher.<id>]` of each launcher compiled in, read against its settings.
    tables: BTreeMap<String, OptionTable>,
}

#[derive(Debug, Default)]
struct RawDevice {
    profile: Option<String>,
    button: Option<RawOwnButton>,
    /// `[device.<id>]` of every device compiled in that has options, as read against them.
    sections: BTreeMap<String, OptionTable>,
}

/// `[device.button]`. Each key is optional: a missing one is the device's note, not a parse error.
#[derive(Debug, Default)]
struct RawOwnButton {
    modifier: Option<String>,
    key: Option<String>,
    injected_only: Option<bool>,
}

impl RawOwnButton {
    /// Empty when neither key could be read: `injected_only` alone describes no button and must
    /// not take the profile's place.
    fn options(&self) -> OptionTable {
        let mut options = OptionTable::new();
        if self.modifier.is_none() && self.key.is_none() {
            return options;
        }
        let text = |value: &Option<String>| value.clone().map(SettingValue::Text);
        for (key, value) in [
            ("modifier", text(&self.modifier)),
            ("key", text(&self.key)),
            ("injected_only", self.injected_only.map(SettingValue::Bool)),
        ] {
            if let Some(value) = value {
                options.insert(key.to_string(), value);
            }
        }
        options
    }
}

#[derive(Debug, Default)]
struct RawTiming {
    modifier_gap_ms: Option<u16>,
    key_hold_ms: Option<u16>,
}

#[derive(Debug, Default)]
struct RawLogging {
    level: Option<String>,
}

/// Only Mujina Settings reads this; checked here so a wrong value is refused like any other.
#[derive(Debug, Default)]
struct RawInterface {
    language: Option<String>,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub raw: RawConfig,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug)]
pub struct Skipped {
    /// Dotted name, e.g. `features.surprise`, or `featurs` for a whole section.
    pub key: String,
    pub note: String,
}

impl Skipped {
    /// Whether the setting `key` lies in what was skipped.
    pub fn covers(&self, key: &str) -> bool {
        key.strip_prefix(self.key.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
    }
}

impl Parsed {
    /// The notes on what was skipped come first.
    pub fn resolve(
        self,
        system: &SystemIdentity,
        devices: &Devices,
        launchers: &Launchers,
        notes: &mut Vec<String>,
    ) -> Settings {
        notes.extend(self.skipped.into_iter().map(|skipped| skipped.note));
        resolve(&self.raw, system, devices, launchers, notes)
    }
}

/// Fails only on text that is no TOML; anything else that is wrong is skipped with a note.
pub fn parse(
    text: &str,
    launchers: &Launchers,
    devices: &Devices,
) -> Result<Parsed, toml::de::Error> {
    let mut top = Section {
        path: String::new(),
        table: text.parse::<Table>()?,
    };
    let mut skipped = Vec::new();
    let raw = RawConfig::read_with(&mut top, launchers.all, devices, &mut skipped);
    top.finish(&mut skipped);
    Ok(Parsed { raw, skipped })
}

/// One table of the file. Each read takes its key out; what is left at the end is no setting.
struct Section {
    /// Dotted name; empty for the top of the file.
    path: String,
    table: Table,
}

impl Section {
    fn name(&self, key: &str) -> String {
        if self.path.is_empty() {
            key.to_string()
        } else {
            format!("{}.{key}", self.path)
        }
    }

    /// Takes out `key`; a value of another kind is skipped with a note.
    fn value<T: Kind>(&mut self, key: &str, skipped: &mut Vec<Skipped>) -> Option<T> {
        let value = self.table.remove(key)?;
        let read = T::from_value(&value);
        if read.is_none() {
            let name = self.name(key);
            let note = format!(
                "{name} = {} ignored: expected {}",
                shown(&value),
                T::EXPECTED
            );
            skipped.push(Skipped { key: name, note });
        }
        read
    }

    /// Takes out the table `key` as a `T`; `None` when absent or not a table (with a note).
    fn section<T: Read>(&mut self, key: &str, skipped: &mut Vec<Skipped>) -> Option<T> {
        self.section_with(key, skipped, T::read)
    }

    /// As [`section`](Self::section), reading the table with `read`.
    fn section_with<T>(
        &mut self,
        key: &str,
        skipped: &mut Vec<Skipped>,
        read: impl FnOnce(&mut Section, &mut Vec<Skipped>) -> T,
    ) -> Option<T> {
        let name = self.name(key);
        match self.table.remove(key)? {
            Value::Table(table) => {
                let mut section = Section { path: name, table };
                let read = read(&mut section, skipped);
                section.finish(skipped);
                Some(read)
            }
            other => {
                let note = format!(
                    "{name} = {} ignored: expected a section, [{name}]",
                    shown(&other)
                );
                skipped.push(Skipped { key: name, note });
                None
            }
        }
    }

    /// Takes out each of `specs`' keys; a value of another kind is skipped with a note.
    fn options(&mut self, specs: &[SettingSpec], skipped: &mut Vec<Skipped>) -> OptionTable {
        let mut options = OptionTable::new();
        for spec in specs {
            let Some(value) = self.table.remove(spec.key) else {
                continue;
            };
            if let Some(read) = setting_value(&value).filter(|read| spec.kind.admits(read)) {
                options.insert(spec.key.to_string(), read);
            } else {
                let name = self.name(spec.key);
                let note = format!(
                    "{name} = {} ignored: expected {}",
                    shown(&value),
                    spec.kind.expected()
                );
                skipped.push(Skipped { key: name, note });
            }
        }
        options
    }

    /// Notes each key no read took out.
    fn finish(self, skipped: &mut Vec<Skipped>) {
        for (key, value) in &self.table {
            let name = self.name(key);
            let note = if value.is_table() {
                format!("[{name}] ignored: unknown section")
            } else {
                format!("{name} = {} ignored: not a setting", shown(value))
            };
            skipped.push(Skipped { key: name, note });
        }
    }
}

/// A value for a note, on one line; what would not fit or has no plain spelling is named by kind.
fn shown(value: &Value) -> String {
    match value {
        Value::String(text) if text.contains(['\n', '\r']) => "text over several lines".to_string(),
        Value::Array(_) => "a list".to_string(),
        Value::Table(_) => "a table".to_string(),
        _ => value.to_string(),
    }
}

fn setting_value(value: &Value) -> Option<SettingValue> {
    match value {
        Value::Boolean(flag) => Some(SettingValue::Bool(*flag)),
        Value::Integer(number) => Some(SettingValue::Integer(*number)),
        Value::String(text) => Some(SettingValue::Text(text.clone())),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .map(SettingValue::TextList),
        _ => None,
    }
}

/// A table of the file with a fixed set of keys.
trait Read: Default {
    fn read(section: &mut Section, skipped: &mut Vec<Skipped>) -> Self;
}

impl RawConfig {
    fn read_with(
        section: &mut Section,
        launchers: &[&dyn LauncherDescriptor],
        devices: &Devices,
        skipped: &mut Vec<Skipped>,
    ) -> Self {
        Self {
            features: section.section("features", skipped).unwrap_or_default(),
            launcher: section
                .section_with("launcher", skipped, |launcher, skipped| {
                    RawLauncher::read_with(launcher, launchers, skipped)
                })
                .unwrap_or_default(),
            device: section
                .section_with("device", skipped, |device, skipped| {
                    RawDevice::read_with(device, devices, skipped)
                })
                .unwrap_or_default(),
            timing: section.section("timing", skipped).unwrap_or_default(),
            logging: section.section("logging", skipped).unwrap_or_default(),
            interface: section.section("interface", skipped).unwrap_or_default(),
        }
    }
}

impl Read for RawFeatures {
    fn read(section: &mut Section, skipped: &mut Vec<Skipped>) -> Self {
        Self {
            button_remap: section.value("button_remap", skipped),
            game_start_screen: section.value("game_start_screen", skipped),
            launch_screen: section.value("launch_screen", skipped),
        }
    }
}

impl RawLauncher {
    /// Reads each of `launchers`' sections against its settings; any other is noted as unknown.
    fn read_with(
        section: &mut Section,
        launchers: &[&dyn LauncherDescriptor],
        skipped: &mut Vec<Skipped>,
    ) -> Self {
        let kind = section.value("kind", skipped);
        let on_exit = section.value("on_exit", skipped);
        let menu = section.value("menu", skipped);
        let overlay = section.value("overlay", skipped);
        let mut tables = BTreeMap::new();
        for launcher in launchers {
            let options = section.section_with(launcher.id(), skipped, |table, skipped| {
                table.options(launcher.settings(), skipped)
            });
            if let Some(options) = options {
                tables.insert(launcher.id().to_string(), options);
            }
        }
        Self {
            kind,
            on_exit,
            menu,
            overlay,
            tables,
        }
    }
}

impl RawDevice {
    /// Reads the section of each device that has options; any other is noted as unknown.
    fn read_with(section: &mut Section, devices: &Devices, skipped: &mut Vec<Skipped>) -> Self {
        let profile = section.value("profile", skipped);
        let button = section.section("button", skipped);
        let mut sections = BTreeMap::new();
        for device in devices.profiles() {
            let settings = device.settings();
            if settings.is_empty() {
                continue;
            }
            let options = section.section_with(device.id(), skipped, |table, skipped| {
                table.options(settings, skipped)
            });
            if let Some(options) = options {
                sections.insert(device.id().to_string(), options);
            }
        }
        Self {
            profile,
            button,
            sections,
        }
    }
}

impl Read for RawOwnButton {
    fn read(section: &mut Section, skipped: &mut Vec<Skipped>) -> Self {
        // Named at all, even with a value of the wrong kind (which gets its own note).
        let names_a_key =
            section.table.contains_key("modifier") || section.table.contains_key("key");
        let modifier = section.value("modifier", skipped);
        let key = section.value("key", skipped);
        let mut injected_only = section.value("injected_only", skipped);
        // Alone it describes no button, so it would change nothing.
        if !names_a_key && let Some(flag) = injected_only.take() {
            let name = section.name("injected_only");
            let note = format!("{name} = {flag} ignored: [device.button] has no modifier and key");
            skipped.push(Skipped { key: name, note });
        }
        Self {
            modifier,
            key,
            injected_only,
        }
    }
}

impl Read for RawTiming {
    fn read(section: &mut Section, skipped: &mut Vec<Skipped>) -> Self {
        Self {
            modifier_gap_ms: section.value("modifier_gap_ms", skipped),
            key_hold_ms: section.value("key_hold_ms", skipped),
        }
    }
}

impl Read for RawLogging {
    fn read(section: &mut Section, skipped: &mut Vec<Skipped>) -> Self {
        Self {
            level: section.value("level", skipped),
        }
    }
}

impl Read for RawInterface {
    fn read(section: &mut Section, skipped: &mut Vec<Skipped>) -> Self {
        Self {
            language: section.value("language", skipped),
        }
    }
}

/// A kind of value settings hold, and how a note asks for it.
trait Kind: Sized {
    const EXPECTED: &'static str;
    fn from_value(value: &Value) -> Option<Self>;
}

impl Kind for bool {
    const EXPECTED: &'static str = "true or false";
    fn from_value(value: &Value) -> Option<Self> {
        value.as_bool()
    }
}

impl Kind for u16 {
    const EXPECTED: &'static str = "a whole number from 0 to 65535";
    fn from_value(value: &Value) -> Option<Self> {
        value
            .as_integer()
            .and_then(|number| u16::try_from(number).ok())
    }
}

impl Kind for String {
    const EXPECTED: &'static str = "text in quotes";
    fn from_value(value: &Value) -> Option<Self> {
        value.as_str().map(str::to_string)
    }
}

fn chord(text: Option<&str>, what: &str, notes: &mut Vec<String>) -> Option<KeyChord> {
    let text = text?;
    match KeyChord::parse(text) {
        Ok(chord) => Some(chord),
        Err(error) => {
            notes.push(format!("{what} = \"{text}\" ignored: {error}"));
            None
        }
    }
}

/// The launcher `kind` names, with its options, when it can be used with them; anything less
/// falls back to the default launcher with a note saying why.
fn launcher(raw: &RawConfig, launchers: &Launchers, notes: &mut Vec<String>) -> LauncherSelection {
    let fallback = launchers.fallback;
    let named = match raw.launcher.kind.as_deref() {
        None => fallback,
        Some(id) => launchers.find(id).unwrap_or_else(|| {
            notes.push(format!("kind = \"{id}\" ignored: unknown launcher"));
            fallback
        }),
    };
    match usable(named, &raw.launcher.tables) {
        Ok(options) => LauncherSelection {
            id: named.id().to_string(),
            options,
        },
        Err(problems) => {
            for problem in problems {
                notes.push(format!("{problem}; using \"{}\"", fallback.id()));
            }
            LauncherSelection {
                id: fallback.id().to_string(),
                options: options_of(fallback, &raw.launcher.tables),
            }
        }
    }
}

fn options_of(
    launcher: &dyn LauncherDescriptor,
    tables: &BTreeMap<String, OptionTable>,
) -> OptionTable {
    tables.get(launcher.id()).cloned().unwrap_or_default()
}

/// The options of `launcher`, when it can be used with them: every required setting set, and
/// the launcher's own rules kept. Otherwise what is wrong.
fn usable(
    launcher: &dyn LauncherDescriptor,
    tables: &BTreeMap<String, OptionTable>,
) -> Result<OptionTable, Vec<String>> {
    let id = launcher.id();
    let options = options_of(launcher, tables);
    let unset = |value: Option<&SettingValue>| match value {
        None => true,
        Some(SettingValue::Text(text)) => text.is_empty(),
        Some(SettingValue::TextList(items)) => items.is_empty(),
        Some(_) => false,
    };
    let missing: Vec<String> = launcher
        .settings()
        .iter()
        .filter(|spec| spec.required && unset(options.get(spec.key)))
        .map(|spec| format!("kind = \"{id}\" needs launcher.{id}.{}", spec.key))
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let mut broken = Vec::new();
    launcher.validate(&options, &mut broken);
    if broken.is_empty() {
        Ok(options)
    } else {
        Err(broken)
    }
}

pub fn resolve(
    raw: &RawConfig,
    system: &SystemIdentity,
    devices: &Devices,
    launchers: &Launchers,
    notes: &mut Vec<String>,
) -> Settings {
    let defaults = Settings::default();
    let default_timing = HoldTiming::default();

    let launcher = launcher(raw, launchers, notes);
    let exit_policy = match raw.launcher.on_exit.as_deref() {
        None => defaults.exit_policy,
        Some(name) => ExitPolicy::from_name(name).unwrap_or_else(|| {
            notes.push(format!("on_exit = \"{name}\" ignored: unknown policy"));
            defaults.exit_policy
        }),
    };
    let detailed_log = match raw.logging.level.as_deref() {
        None | Some("info") => defaults.detailed_log,
        Some("debug") => true,
        Some(level) => {
            notes.push(format!(
                "level = \"{level}\" ignored: use \"info\" or \"debug\""
            ));
            defaults.detailed_log
        }
    };
    if let Some(language) = raw.interface.language.as_deref()
        && !matches!(language, "auto" | "en" | "de")
    {
        notes.push(format!(
            "language = \"{language}\" ignored: use \"auto\", \"en\" or \"de\""
        ));
    }
    let own = raw
        .device
        .button
        .as_ref()
        .map(RawOwnButton::options)
        .unwrap_or_default();
    let choice = DeviceChoice::from_profile(raw.device.profile.as_deref());
    let device = device::select(devices, system, &choice, &own, &raw.device.sections, notes);

    Settings {
        launcher,
        button_remap: raw.features.button_remap.unwrap_or(defaults.button_remap),
        game_start_screen: raw
            .features
            .game_start_screen
            .unwrap_or(defaults.game_start_screen),
        launch_screen: raw.features.launch_screen.unwrap_or(defaults.launch_screen),
        device,
        menu: chord(raw.launcher.menu.as_deref(), "menu", notes),
        overlay: chord(raw.launcher.overlay.as_deref(), "overlay", notes),
        exit_policy,
        timing: HoldTiming {
            modifier_gap_ms: raw
                .timing
                .modifier_gap_ms
                .unwrap_or(default_timing.modifier_gap_ms),
            key_hold_ms: raw.timing.key_hold_ms.unwrap_or(default_timing.key_hold_ms),
        },
        detailed_log,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::{DEVICES, LAUNCHERS};

    fn resolve_text(text: &str, manufacturer: &str) -> (Settings, Vec<String>) {
        let system = SystemIdentity {
            manufacturer: manufacturer.to_string(),
            product: "whatever".to_string(),
        };
        let mut notes = Vec::new();
        let settings = parse(text, &LAUNCHERS, &DEVICES)
            .unwrap()
            .resolve(&system, &DEVICES, &LAUNCHERS, &mut notes);
        (settings, notes)
    }

    fn words(value: &str) -> SettingValue {
        SettingValue::Text(value.to_string())
    }

    #[test]
    fn known_device_gets_its_button_without_any_configuration() {
        let (settings, notes) = resolve_text("", "ONE-NETBOOK");
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));
        assert!(settings.device.options.is_empty());
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn unknown_device_has_no_button_and_says_so() {
        let (settings, notes) = resolve_text("", "Contoso");
        assert!(settings.device.is_none());
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn explicit_button_and_shortcuts_win() {
        let text = r#"
            [features]
            launch_screen = true
            [launcher]
            on_exit = "nothing"
            overlay = "F12"
            [launcher.steam]
            wifi_indicator = false
            [device.button]
            modifier = "LCTRL"
            key = "F24"
            injected_only = false
            [timing]
            key_hold_ms = 80
        "#;
        let (settings, notes) = resolve_text(text, "Contoso");
        assert!(notes.is_empty(), "{notes:?}");
        assert!(settings.launch_screen);
        assert_eq!(settings.launcher.id, "steam");
        assert_eq!(
            settings.launcher.options,
            OptionTable::from([("wifi_indicator".to_string(), SettingValue::Bool(false))])
        );
        assert_eq!(settings.exit_policy, ExitPolicy::Nothing);
        assert_eq!(settings.overlay, KeyChord::parse("F12").ok());
        assert_eq!(settings.menu, None);
        assert_eq!(settings.device.id.as_deref(), Some("custom"));
        assert_eq!(
            settings.device.options,
            OptionTable::from([
                ("injected_only".to_string(), SettingValue::Bool(false)),
                ("key".to_string(), words("F24")),
                ("modifier".to_string(), words("LCTRL")),
            ])
        );
        assert_eq!(settings.timing.key_hold_ms, 80);
        assert_eq!(settings.timing.modifier_gap_ms, 20);
    }

    #[test]
    fn mistakes_are_reported_not_fatal() {
        let text = r#"
            [launcher]
            on_exit = "explode"
            menu = "LCTRL+NOPE"
            [device]
            profile = "steam-deck"
        "#;
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(settings.exit_policy, ExitPolicy::RelaunchOnCrash);
        assert_eq!(settings.menu, None);
        assert_eq!(notes.len(), 3, "{notes:?}");
    }

    #[test]
    fn detailed_logging_is_opt_in() {
        assert!(!resolve_text("", "ONE-NETBOOK").0.detailed_log);
        let (settings, notes) = resolve_text("[logging]\nlevel = \"debug\"", "ONE-NETBOOK");
        assert!(settings.detailed_log);
        assert!(notes.is_empty(), "{notes:?}");
        let (settings, notes) = resolve_text("[logging]\nlevel = \"loud\"", "ONE-NETBOOK");
        assert!(!settings.detailed_log);
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn the_launcher_kind_is_steam_by_default_and_by_name() {
        let (settings, notes) = resolve_text("", "ONE-NETBOOK");
        assert_eq!(settings.launcher.id, "steam");
        assert!(settings.launcher.options.is_empty());
        assert!(notes.is_empty(), "{notes:?}");
        let (settings, notes) = resolve_text("[launcher]\nkind = \"steam\"", "ONE-NETBOOK");
        assert_eq!(settings.launcher.id, "steam");
        assert!(notes.is_empty(), "{notes:?}");
    }

    #[test]
    fn an_unknown_launcher_kind_falls_back_to_steam_with_a_note() {
        let (settings, notes) = resolve_text("[launcher]\nkind = \"heroic\"", "ONE-NETBOOK");
        assert_eq!(settings.launcher.id, "steam");
        assert_eq!(notes, ["kind = \"heroic\" ignored: unknown launcher"]);
    }

    #[test]
    fn only_known_languages_are_accepted() {
        let (_, notes) = resolve_text("[interface]\nlanguage = \"de\"", "ONE-NETBOOK");
        assert!(notes.is_empty(), "{notes:?}");
        let (_, notes) = resolve_text("[interface]\nlanguage = \"fr\"", "ONE-NETBOOK");
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    #[test]
    fn a_generic_launcher_carries_its_configuration() {
        let text = r#"
            [launcher]
            kind = "generic"
            [launcher.generic]
            executable = 'C:\Frontend\frontend.exe'
            arguments = ["--fullscreen"]
            window_class = "FrontendWindow"
        "#;
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(settings.launcher.id, "generic");
        assert_eq!(
            settings.launcher.options,
            OptionTable::from([
                (
                    "arguments".to_string(),
                    SettingValue::TextList(vec!["--fullscreen".to_string()])
                ),
                ("executable".to_string(), words(r"C:\Frontend\frontend.exe")),
                ("window_class".to_string(), words("FrontendWindow")),
            ])
        );
    }

    #[test]
    fn a_launcher_without_a_required_setting_falls_back_to_steam() {
        let (settings, notes) = resolve_text("[launcher]\nkind = \"generic\"", "ONE-NETBOOK");
        assert_eq!(settings.launcher.id, "steam");
        assert_eq!(
            notes,
            ["kind = \"generic\" needs launcher.generic.executable; using \"steam\""]
        );

        let text = "[launcher]\nkind = \"generic\"\n[launcher.generic]\nexecutable = \"\"\n\
                    process = \"x.exe\"";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(settings.launcher.id, "steam");
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    #[test]
    fn a_launcher_whose_own_rules_are_broken_falls_back_to_steam() {
        let text = "[launcher]\nkind = \"generic\"\n[launcher.generic]\nexecutable = 'C:\\'\n\
                    [launcher.steam]\nui_link = false";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            ["launcher.generic.executable = \"C:\\\" cannot be used; using \"steam\""]
        );
        assert_eq!(settings.launcher.id, "steam");
        assert_eq!(
            settings.launcher.options,
            OptionTable::from([("ui_link".to_string(), SettingValue::Bool(false))])
        );
    }

    #[test]
    fn a_launchers_section_is_checked_key_by_key() {
        let text = "[launcher.steam]\nui_link = \"no\"\nwifi_indicator = false\nsurprise = 1\n";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            [
                "launcher.steam.ui_link = \"no\" ignored: expected true or false",
                "launcher.steam.surprise = 1 ignored: not a setting",
            ]
        );
        assert_eq!(
            settings.launcher.options,
            OptionTable::from([("wifi_indicator".to_string(), SettingValue::Bool(false))])
        );

        let text = "[launcher.generic]\narguments = \"--fullscreen\"\n";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            [
                "launcher.generic.arguments = \"--fullscreen\" ignored: expected a list of texts in \
              quotes"
            ]
        );
        assert_eq!(settings.launcher.id, "steam");
    }

    #[test]
    fn a_removed_key_is_an_unknown_one_and_carries_nothing_over() {
        // No migration of removed keys (ADR-0013).
        let (settings, notes) = resolve_text("[features]\nwifi_indicator = false", "ONE-NETBOOK");
        assert_eq!(
            notes,
            ["features.wifi_indicator = false ignored: not a setting"]
        );
        assert!(settings.launcher.options.is_empty());
    }

    #[test]
    fn a_devices_section_is_read_against_its_options() {
        let text = "[device]\nprofile = \"pad\"\n[device.pad]\nmode = true\nsurprise = 1\n";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(notes, ["device.pad.surprise = 1 ignored: not a setting"]);
        assert_eq!(settings.device.id.as_deref(), Some("pad"));
        assert_eq!(
            settings.device.options,
            OptionTable::from([("mode".to_string(), SettingValue::Bool(true))])
        );

        // A device Mujina does not have, or one without options, has no section.
        let text = "[device.rog-ally]\nmode = true\n[device.onexplayer]\nmode = true\n";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            [
                "[device.onexplayer] ignored: unknown section",
                "[device.rog-ally] ignored: unknown section",
            ]
        );
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));
    }

    #[test]
    fn profile_none_switches_the_button_off() {
        let (settings, notes) = resolve_text("[device]\nprofile = \"none\"", "ONE-NETBOOK");
        assert!(settings.device.is_none());
        assert!(notes.is_empty());
    }

    #[test]
    fn a_typo_is_one_note_and_every_other_setting_stays() {
        let text = r#"
            [features]
            butten_remap = false
            launch_screen = true
            [launcher]
            on_exit = "nothing"
            [timing]
            key_hold_ms = 80
        "#;
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            ["features.butten_remap = false ignored: not a setting"]
        );
        assert!(settings.button_remap);
        assert!(settings.launch_screen);
        assert_eq!(settings.exit_policy, ExitPolicy::Nothing);
        assert_eq!(settings.timing.key_hold_ms, 80);
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));
    }

    #[test]
    fn an_unknown_section_is_one_note() {
        let text = "[featurs]\nbutton_remap = false\n[launcher]\non_exit = \"nothing\"";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(notes, ["[featurs] ignored: unknown section"]);
        assert!(settings.button_remap);
        assert_eq!(settings.exit_policy, ExitPolicy::Nothing);

        let text = "[launcher]\nkind = \"steam\"\n[launcher.playnite]\npath = 'C:\\P.exe'";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(notes, ["[launcher.playnite] ignored: unknown section"]);
        assert_eq!(settings.launcher.id, "steam");
    }

    #[test]
    fn a_value_of_the_wrong_kind_is_a_note_for_that_key_only() {
        let text = r#"
            [features]
            button_remap = "yes"
            launch_screen = true
            [timing]
            key_hold_ms = 70000
            modifier_gap_ms = 30
        "#;
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            [
                "features.button_remap = \"yes\" ignored: expected true or false",
                "timing.key_hold_ms = 70000 ignored: expected a whole number from 0 to 65535",
            ]
        );
        assert!(settings.button_remap);
        assert!(settings.launch_screen);
        assert_eq!(settings.timing.key_hold_ms, 50);
        assert_eq!(settings.timing.modifier_gap_ms, 30);
    }

    #[test]
    fn a_note_shows_the_value_on_one_line_or_names_its_kind() {
        let text = r#"
            [features]
            button_remap = 1979-05-27
            extras = [true]
            launch_screen = """on
            please"""
            surprise = 1.5
            [features.game_start_screen]
        "#;
        let (_, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            [
                "features.button_remap = 1979-05-27 ignored: expected true or false",
                "features.game_start_screen = a table ignored: expected true or false",
                "features.launch_screen = text over several lines ignored: expected true or false",
                "features.extras = a list ignored: not a setting",
                "features.surprise = 1.5 ignored: not a setting",
            ]
        );
    }

    #[test]
    fn a_section_of_the_wrong_kind_falls_back_alone() {
        let text = "features = 5\n[timing]\nkey_hold_ms = 80";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            ["features = 5 ignored: expected a section, [features]"]
        );
        assert_eq!(settings.timing.key_hold_ms, 80);
        assert!(settings.button_remap);
    }

    #[test]
    fn a_mistake_in_the_generic_launcher_is_named_and_falls_back_to_steam() {
        let text = r#"
            [launcher]
            kind = "generic"
            [launcher.generic]
            executible = 'C:\Frontend\frontend.exe'
            arguments = "--fullscreen"
        "#;
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(settings.launcher.id, "steam");
        assert_eq!(notes.len(), 3, "{notes:?}");
        assert!(
            notes[0].starts_with("launcher.generic.arguments ="),
            "{notes:?}"
        );
        assert!(
            notes[1].starts_with("launcher.generic.executible ="),
            "{notes:?}"
        );
        assert!(
            notes[2].contains("needs launcher.generic.executable"),
            "{notes:?}"
        );
    }

    #[test]
    fn a_button_of_ones_own_needs_both_keys() {
        let (settings, notes) =
            resolve_text("[device.button]\nmodifier = \"LCTRL\"", "ONE-NETBOOK");
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("[device.button] ignored"), "{notes:?}");
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));

        // A section left empty by unsetting both keys says nothing.
        let (settings, notes) =
            resolve_text("[device.button]\n# modifier = \"LCTRL\"", "ONE-NETBOOK");
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));

        let text = "[device.button]\n# modifier = \"LCTRL\"\ninjected_only = false";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(
            notes,
            [
                "device.button.injected_only = false ignored: [device.button] has no modifier \
                 and key"
            ]
        );
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));
    }

    #[test]
    fn a_button_of_ones_own_whose_keys_cannot_be_read_leaves_the_profile_in_charge() {
        let text = "[device.button]\nmodifier = 5\nkey = 5\ninjected_only = false";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[0].starts_with("device.button.modifier = 5 ignored"));
        assert!(notes[1].starts_with("device.button.key = 5 ignored"));

        let (settings, notes) = resolve_text(
            "[device.button]\nkey = 5\ninjected_only = false",
            "ONE-NETBOOK",
        );
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));
        assert_eq!(notes.len(), 1, "{notes:?}");

        let text = "[device.button]\nmodifier = 5\nkey = \"D\"\ninjected_only = false";
        let (settings, notes) = resolve_text(text, "ONE-NETBOOK");
        assert_eq!(settings.device.id.as_deref(), Some("onexplayer"));
        assert_eq!(notes.len(), 2, "{notes:?}");
        assert!(notes[1].starts_with("[device.button] ignored"), "{notes:?}");
    }

    #[test]
    fn only_text_that_is_no_toml_loses_the_whole_file() {
        assert!(parse("[features\nbutton_remap = maybe", &LAUNCHERS, &DEVICES).is_err());
        assert!(parse("surprise = 1", &LAUNCHERS, &DEVICES).is_ok());
    }

    #[test]
    fn a_skipped_part_covers_the_settings_inside_it() {
        let skipped = Skipped {
            key: "featurs".to_string(),
            note: String::new(),
        };
        assert!(skipped.covers("featurs"));
        assert!(skipped.covers("featurs.button_remap"));
        assert!(!skipped.covers("featurs_old.button_remap"));
        assert!(!skipped.covers("features.button_remap"));
    }
}

//! In-memory ports for tests, and the conformance checks every launcher and device must pass.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use mujina_domain::activation::HomeDestination;
use mujina_domain::button::{ButtonId, WindowShape};
use mujina_domain::keys::{HoldTiming, KeyChord, VirtualKey};
use mujina_i18n::Catalog;

use crate::agent::AgentEvent;
use crate::device::{ButtonSpec, DeviceDescriptor, DeviceSelection, Suppression, SystemIdentity};
use crate::launcher::{LauncherCaps, LauncherDescriptor, OptionTable};
use crate::ports::{
    AgentControl, DeviceButtons, Direct, ForegroundProbe, FseState, FullScreenExperience,
    GameWhereabouts, HomeActivator, HomeAppRegistry, HomeLauncher, KeySender, LaunchScreen,
    LauncherInstall, LauncherState, PackageIdentity, PortError, PortResult, SessionLauncher,
};
use crate::settings::schema::{CORE, SettingKind, SettingSpec};
use crate::settings::{LoadedSettings, SettingValue, Settings, SettingsSource};

pub struct FakeFse(pub FseState);

impl FullScreenExperience for FakeFse {
    fn state(&self) -> FseState {
        self.0
    }
}

pub struct FakeIdentity(Option<String>);

impl FakeIdentity {
    pub fn packaged(app_user_model_id: &str) -> Self {
        Self(Some(app_user_model_id.to_string()))
    }

    pub fn unpackaged() -> Self {
        Self(None)
    }
}

impl PackageIdentity for FakeIdentity {
    fn app_user_model_id(&self) -> Option<String> {
        self.0.clone()
    }
}

#[derive(Default)]
pub struct FakeHomeAppRegistry {
    current: RefCell<Option<String>>,
    backup: RefCell<Option<String>>,
}

impl FakeHomeAppRegistry {
    pub fn with_current(current: Option<&str>) -> Self {
        Self {
            current: RefCell::new(current.map(str::to_string)),
            backup: RefCell::default(),
        }
    }
}

impl HomeAppRegistry for FakeHomeAppRegistry {
    fn current(&self) -> PortResult<Option<String>> {
        Ok(self.current.borrow().clone())
    }

    fn set(&self, app_user_model_id: &str) -> PortResult<()> {
        *self.current.borrow_mut() = Some(app_user_model_id.to_string());
        Ok(())
    }

    fn clear(&self) -> PortResult<()> {
        *self.current.borrow_mut() = None;
        Ok(())
    }

    fn backup(&self) -> PortResult<Option<String>> {
        Ok(self.backup.borrow().clone())
    }

    fn set_backup(&self, app_user_model_id: Option<&str>) -> PortResult<()> {
        *self.backup.borrow_mut() = app_user_model_id.map(str::to_string);
        Ok(())
    }
}

/// A launcher that records which operations were requested, in both of its roles.
pub struct FakeLauncher {
    install: Option<LauncherInstall>,
    state: Cell<LauncherState>,
    pub fail_prepare: bool,
    calls: RefCell<Vec<&'static str>>,
    game_running: Cell<bool>,
    /// The game's window: `Some(true)` in front, `Some(false)` behind, `None` not found.
    pub game_in_front: Cell<Option<bool>>,
    /// Whether the launcher knows the game's processes.
    pub game_known: Cell<bool>,
    pub game_gone: Cell<bool>,
    pub direct_menu: Cell<bool>,
    pub direct_overlay: Cell<bool>,
    /// [`FakeLauncher::MENU`] unless the test says otherwise.
    pub menu_shortcut: Cell<Option<KeyChord>>,
    /// [`FakeLauncher::OVERLAY`] unless the test says otherwise.
    pub overlay_shortcut: Cell<Option<KeyChord>>,
    reconfigured: RefCell<Vec<OptionTable>>,
    observed: RefCell<Vec<AgentEvent>>,
}

impl FakeLauncher {
    pub const MENU: KeyChord = KeyChord::pair(VirtualKey::LCONTROL, VirtualKey::DIGIT_1);
    pub const OVERLAY: KeyChord = KeyChord::pair(VirtualKey::LSHIFT, VirtualKey::TAB);

    pub fn installed(state: LauncherState) -> Self {
        let install = LauncherInstall {
            executable: PathBuf::from("/opt/fake/launcher"),
            directory: PathBuf::from("/opt/fake"),
        };
        Self::with_install(Some(install), state)
    }

    pub fn missing() -> Self {
        Self::with_install(None, LauncherState::NotRunning)
    }

    fn with_install(install: Option<LauncherInstall>, state: LauncherState) -> Self {
        Self {
            install,
            state: Cell::new(state),
            fail_prepare: false,
            calls: RefCell::default(),
            game_running: Cell::new(false),
            game_in_front: Cell::new(None),
            game_known: Cell::new(false),
            game_gone: Cell::new(false),
            direct_menu: Cell::new(false),
            direct_overlay: Cell::new(false),
            menu_shortcut: Cell::new(Some(Self::MENU)),
            overlay_shortcut: Cell::new(Some(Self::OVERLAY)),
            reconfigured: RefCell::default(),
            observed: RefCell::default(),
        }
    }

    pub fn set_game_running(&self, running: bool) {
        self.game_running.set(running);
    }

    pub fn observed(&self) -> Vec<AgentEvent> {
        self.observed.borrow().clone()
    }

    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }

    pub fn reconfigured(&self) -> Vec<OptionTable> {
        self.reconfigured.borrow().clone()
    }

    fn record(&self, call: &'static str) {
        self.calls.borrow_mut().push(call);
    }

    fn direct(taken: bool) -> Direct {
        if taken {
            Direct::Taken
        } else {
            Direct::NotTaken
        }
    }
}

impl HomeLauncher for FakeLauncher {
    fn display_name(&self) -> String {
        "Fake Launcher".to_string()
    }

    fn locate(&self) -> PortResult<LauncherInstall> {
        self.install
            .clone()
            .ok_or_else(|| PortError::NotFound("fake launcher".to_string()))
    }

    fn state(&self) -> LauncherState {
        self.state.get()
    }

    fn prepare(&self, _install: &LauncherInstall) -> PortResult<()> {
        self.record("prepare");
        if self.fail_prepare {
            return Err(PortError::Failed("fake preparation failure".to_string()));
        }
        Ok(())
    }

    fn start_ui(&self, _install: &LauncherInstall) -> PortResult<()> {
        self.record("start_ui");
        self.state.set(LauncherState::UiVisible);
        Ok(())
    }

    fn switch_to_ui(&self, _install: &LauncherInstall) -> PortResult<()> {
        self.record("switch_to_ui");
        self.state.set(LauncherState::UiVisible);
        Ok(())
    }

    fn focus_ui(&self) -> PortResult<()> {
        self.record("focus_ui");
        Ok(())
    }

    fn navigate(&self, _install: &LauncherInstall, destination: HomeDestination) -> PortResult<()> {
        self.record(match destination {
            HomeDestination::Home => "navigate home",
            HomeDestination::Library => "navigate library",
            HomeDestination::Game => "navigate game",
        });
        Ok(())
    }

    fn focus_game(&self) -> PortResult<()> {
        match self.game_in_front.get() {
            Some(_) => {
                self.record("focus_game");
                Ok(())
            }
            None => Err(PortError::NotFound("fake game window".to_string())),
        }
    }
}

impl SessionLauncher for FakeLauncher {
    fn owns_process(&self, process_name: &str) -> bool {
        process_name == "fakelauncher.exe"
    }

    fn process_id(&self) -> Option<u32> {
        None
    }

    fn game_running(&self) -> bool {
        self.game_running.get()
    }

    fn game_in_front(&self) -> bool {
        self.game_in_front.get() == Some(true)
    }

    fn game_findable(&self) -> bool {
        self.game_in_front.get().is_some()
    }

    fn game_whereabouts(&self) -> GameWhereabouts {
        if self.game_gone.get() {
            return GameWhereabouts::Gone;
        }
        match self.game_in_front.get() {
            Some(true) => GameWhereabouts::InFront,
            Some(false) => GameWhereabouts::Behind,
            None => GameWhereabouts::NoWindow {
                known: self.game_known.get(),
            },
        }
    }

    fn game_started(&self) {
        self.record("game_started");
    }

    fn game_ended(&self) {
        self.record("game_ended");
    }

    fn menu_shortcut(&self) -> Option<KeyChord> {
        self.menu_shortcut.get()
    }

    fn overlay_shortcut(&self) -> Option<KeyChord> {
        self.overlay_shortcut.get()
    }

    fn open_menu(&self) -> Direct {
        self.record("open_menu");
        Self::direct(self.direct_menu.get())
    }

    fn open_overlay(&self) -> Direct {
        self.record("open_overlay");
        Self::direct(self.direct_overlay.get())
    }

    fn reconfigure(&self, live: &OptionTable) {
        self.reconfigured.borrow_mut().push(live.clone());
    }

    fn observe(&self, event: &AgentEvent) {
        self.observed.borrow_mut().push(event.clone());
    }
}

/// Build it in a `static` from [`FakeLauncherDescriptor::named`] with struct update syntax.
pub struct FakeLauncherDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub settings: &'static [SettingSpec],
    pub template: &'static str,
    pub caps: LauncherCaps,
    /// For `validate`: the text option named first must not hold the second.
    pub refuses: Option<(&'static str, &'static str)>,
    pub conflicting: &'static [&'static str],
    pub catalogs: &'static [(&'static str, &'static str)],
}

impl FakeLauncherDescriptor {
    /// Without options, rules or conflicting programs.
    pub const fn named(id: &'static str, name: &'static str, caps: LauncherCaps) -> Self {
        Self {
            id,
            name,
            settings: &[],
            template: "",
            caps,
            refuses: None,
            conflicting: &[],
            catalogs: &[],
        }
    }
}

impl LauncherDescriptor for FakeLauncherDescriptor {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn settings(&self) -> &'static [SettingSpec] {
        self.settings
    }

    fn template(&self) -> &'static str {
        self.template
    }

    fn capabilities(&self, _options: &OptionTable) -> LauncherCaps {
        self.caps
    }

    fn validate(&self, options: &OptionTable, notes: &mut Vec<String>) {
        if let Some((key, refused)) = self.refuses
            && options.get(key) == Some(&SettingValue::Text(refused.to_string()))
        {
            notes.push(format!(
                "launcher.{}.{key} = \"{refused}\" cannot be used",
                self.id
            ));
        }
    }

    fn conflicting_processes(&self) -> &'static [&'static str] {
        self.conflicting
    }

    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        self.catalogs
    }
}

/// Checks the rules every launcher descriptor must keep. Call it from a test in the launcher's
/// crate, with `mujina-application`'s `test-util` feature as a dev-dependency.
///
/// # Panics
///
/// At the first rule the descriptor breaks, saying which.
pub fn conformance(descriptor: &dyn LauncherDescriptor) {
    let id = descriptor.id();
    assert!(
        plain(id),
        "launcher id \"{id}\": lower case, digits and _ only"
    );
    // `[launcher]` keys are read before the launchers' sections, so no id may be one of them.
    let taken = CORE
        .iter()
        .filter(|section| section.name == "launcher")
        .flat_map(|section| section.settings)
        .any(|spec| spec.key == id);
    assert!(!taken, "launcher id \"{id}\" is a key of [launcher] itself");
    assert!(!descriptor.name().is_empty(), "launcher {id}: no name");
    let section = format!("launcher.{id}");
    let settings = descriptor.settings();
    check_settings(&section, settings);

    let named: Vec<String> = template_keys(descriptor.template())
        .into_iter()
        .filter(|(under, _)| *under == section)
        .map(|(_, key)| key)
        .collect();
    let keys: Vec<&str> = settings.iter().map(|spec| spec.key).collect();
    assert_eq!(
        named, keys,
        "launcher {id}: the template names each option under [{section}], in order"
    );

    let mut notes = Vec::new();
    descriptor.validate(&OptionTable::new(), &mut notes);
    let defaults: OptionTable = settings
        .iter()
        .filter_map(|spec| Some((spec.key.to_string(), spec.kind.default_value()?)))
        .collect();
    descriptor.validate(&defaults, &mut notes);
    assert!(
        notes.is_empty(),
        "launcher {id}: its defaults break its own rules: {notes:?}"
    );

    for program in descriptor.conflicting_processes() {
        assert!(
            !program.is_empty() && *program == program.to_lowercase() && !program.contains('\\'),
            "launcher {id}: conflicting program \"{program}\" is no file name in lower case"
        );
    }

    let mut texts = vec![descriptor.name()];
    add_setting_texts(settings, &mut texts);
    check_catalogs(&format!("launcher {id}"), descriptor.catalogs(), &texts);
}

/// The rules for the settings of one section, `[section]`; see [`conformance`].
///
/// # Panics
///
/// At the first setting that breaks one, naming it.
pub fn check_settings(section: &str, settings: &[SettingSpec]) {
    for (index, spec) in settings.iter().enumerate() {
        let name = format!("{section}.{}", spec.key);
        assert!(plain(spec.key), "{name}: lower case, digits and _ only");
        assert!(
            settings[..index].iter().all(|other| other.key != spec.key),
            "{name} is there twice"
        );
        assert!(!spec.title.is_empty(), "{name}: no title");
        match spec.kind {
            SettingKind::Choice { values, default } => assert!(
                values.contains(&default),
                "{name}: the default \"{default}\" is none of its values"
            ),
            SettingKind::Number { min, max, default } => assert!(
                min <= default && default <= max,
                "{name}: the default {default} lies outside {min} to {max}"
            ),
            _ => {}
        }
        assert!(
            !spec.required || spec.kind.default_value().is_none(),
            "{name}: required, yet it has a default"
        );
        if let Some(needed) = spec.requires {
            let toggle = settings.iter().any(|other| {
                other.key == needed && matches!(other.kind, SettingKind::Toggle { .. })
            });
            assert!(
                toggle,
                "{name} requires {needed}, which is no toggle of [{section}]"
            );
        }
    }
}

/// The (section, key) pairs a configuration template names, in order, commented out or not.
pub fn template_keys(template: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut keys = Vec::new();
    for line in template.lines() {
        let line = line.trim();
        let body = line.strip_prefix('#').map_or(line, str::trim_start);
        if let Some((name, _)) = body.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
            section = name.trim().to_string();
        } else if let Some((key, _)) = body.split_once('=')
            && plain(key.trim())
        {
            keys.push((section.clone(), key.trim().to_string()));
        }
    }
    keys
}

/// What `[device]` itself gives a meaning to: `profile`'s own values, and its keys and sections.
const RESERVED_DEVICE_IDS: &[&str] = &["auto", "none", "button", "profile"];

/// Checks the rules every device descriptor must keep. Call it from a test in the device's
/// crate, with `mujina-application`'s `test-util` feature as a dev-dependency.
///
/// # Panics
///
/// At the first rule the descriptor breaks, saying which.
pub fn device_conformance(descriptor: &dyn DeviceDescriptor) {
    let id = descriptor.id();
    let spelt = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    assert!(
        spelt,
        "device id \"{id}\": lower case, digits, _ and - only"
    );
    assert!(
        !RESERVED_DEVICE_IDS.contains(&id),
        "device id \"{id}\" has a meaning of its own in [device]"
    );
    assert!(!descriptor.name().is_empty(), "device {id}: no name");
    let buttons = descriptor.buttons();
    assert!(!buttons.is_empty(), "device {id}: no buttons");
    for (index, button) in buttons.iter().enumerate() {
        let key = &button.key;
        assert!(
            plain(key),
            "device {id}: button \"{key}\": lower case, digits and _ only"
        );
        assert!(
            !button.label.is_empty(),
            "device {id}: button {key} has no label"
        );
        let before = &buttons[..index];
        assert!(
            before.iter().all(|other| other.id != button.id),
            "device {id}: button id {} is there twice",
            button.id.0
        );
        assert!(
            before.iter().all(|other| other.key != *key),
            "device {id}: button {key} is there twice"
        );
    }
    let settings = descriptor.settings();
    check_settings(&format!("device.{id}"), settings);
    let defaults: OptionTable = settings
        .iter()
        .filter_map(|spec| Some((spec.key.to_string(), spec.kind.default_value()?)))
        .collect();
    let mut notes = Vec::new();
    descriptor.validate(&defaults, &mut notes);
    assert!(
        notes.is_empty(),
        "device {id}: its defaults break its own rules: {notes:?}"
    );

    let name = descriptor.name();
    let mut texts: Vec<&str> = vec![&name];
    texts.extend(buttons.iter().map(|button| button.label.as_str()));
    add_setting_texts(settings, &mut texts);
    check_catalogs(&format!("device {id}"), descriptor.catalogs(), &texts);
}

fn add_setting_texts(settings: &[SettingSpec], texts: &mut Vec<&str>) {
    for spec in settings {
        texts.push(spec.title);
        if !spec.help.is_empty() {
            texts.push(spec.help);
        }
        if let SettingKind::Choice { values, .. } = spec.kind {
            texts.extend(values);
        }
    }
}

/// Each of `catalogs` parses and translates every one of `texts`.
fn check_catalogs(owner: &str, catalogs: &[(&str, &str)], texts: &[&str]) {
    for (language, po) in catalogs {
        let catalog = Catalog::parse(po).unwrap_or_else(|error| {
            panic!("{owner}: the \"{language}\" catalog does not read: {error}")
        });
        for text in texts {
            assert!(
                catalog.texts().any(|(english, _)| english == *text),
                "{owner}: the \"{language}\" catalog does not translate \"{text}\""
            );
        }
    }
}

/// Build it in a `static` from [`FakeDevice::named`] with struct update syntax.
pub struct FakeDevice {
    pub id: &'static str,
    pub name: &'static str,
    /// It is every machine of this manufacturer; "" is none.
    pub manufacturer: &'static str,
    /// Its buttons: id, key (also the label) and suppression.
    pub buttons: &'static [(u8, &'static str, Suppression)],
    pub settings: &'static [SettingSpec],
    /// A rule for `validate`: this option must be set.
    pub needs: Option<&'static str>,
    /// A rule for `validate`: both options or neither, as for a button of one's own.
    pub needs_both: Option<(&'static str, &'static str)>,
    pub catalogs: &'static [(&'static str, &'static str)],
}

impl FakeDevice {
    /// One swallowed button, `button` (0); no machine, options or rules.
    pub const fn named(id: &'static str, name: &'static str) -> Self {
        Self {
            id,
            name,
            manufacturer: "",
            buttons: &[(0, "button", Suppression::Swallowed)],
            settings: &[],
            needs: None,
            needs_both: None,
            catalogs: &[],
        }
    }
}

impl DeviceDescriptor for FakeDevice {
    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> String {
        self.name.to_string()
    }

    fn matches(&self, identity: &SystemIdentity) -> bool {
        !self.manufacturer.is_empty() && identity.manufacturer == self.manufacturer
    }

    fn buttons(&self) -> Vec<ButtonSpec> {
        self.buttons
            .iter()
            .map(|(id, key, suppression)| ButtonSpec {
                id: ButtonId(*id),
                key: (*key).to_string(),
                label: (*key).to_string(),
                suppression: *suppression,
            })
            .collect()
    }

    fn settings(&self) -> &[SettingSpec] {
        self.settings
    }

    fn validate(&self, options: &OptionTable, notes: &mut Vec<String>) {
        if let Some(key) = self.needs
            && !options.contains_key(key)
        {
            notes.push(format!("needs {key}"));
        }
        if let Some((first, second)) = self.needs_both
            && options.contains_key(first) != options.contains_key(second)
        {
            notes.push(format!("it needs both {first} and {second}"));
        }
    }

    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        self.catalogs
    }
}

/// A key as `config.toml` spells them: lower case, digits and `_`.
fn plain(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Records what the agent asks of the keys it sends and of the device's buttons.
#[derive(Default)]
pub struct FakeInput {
    reconfigured: RefCell<Vec<DeviceSelection>>,
    sent: RefCell<Vec<KeyChord>>,
    passed_on: RefCell<Vec<ButtonId>>,
    /// Devices another adapter runs: `reconfigure` does not take them over.
    pub foreign: RefCell<Vec<&'static str>>,
}

impl FakeInput {
    pub fn reconfigured(&self) -> Vec<DeviceSelection> {
        self.reconfigured.borrow().clone()
    }

    pub fn sent(&self) -> Vec<KeyChord> {
        self.sent.borrow().clone()
    }

    pub fn passed_on(&self) -> Vec<ButtonId> {
        self.passed_on.borrow().clone()
    }
}

impl DeviceButtons for FakeInput {
    fn reconfigure(&self, device: &DeviceSelection) -> bool {
        self.reconfigured.borrow_mut().push(device.clone());
        let foreign = self.foreign.borrow();
        device.id.as_deref().is_none_or(|id| !foreign.contains(&id))
    }

    fn pass_on(&self, button: ButtonId) {
        self.passed_on.borrow_mut().push(button);
    }
}

#[derive(Default)]
pub struct FakeSettingsSource(RefCell<Settings>);

impl FakeSettingsSource {
    pub fn set(&self, settings: Settings) {
        *self.0.borrow_mut() = settings;
    }
}

impl SettingsSource for FakeSettingsSource {
    fn load(&self) -> LoadedSettings {
        LoadedSettings {
            settings: self.0.borrow().clone(),
            notes: Vec::new(),
        }
    }
}

impl KeySender for FakeInput {
    fn send_chord(&self, chord: KeyChord, _timing: HoldTiming) {
        self.sent.borrow_mut().push(chord);
    }
}

#[derive(Default)]
pub struct FakeForeground {
    process: RefCell<Option<String>>,
    /// `None` (the default) as if it could not be read.
    pub shape: Cell<Option<WindowShape>>,
    pub shape_asked: Cell<u32>,
}

impl FakeForeground {
    pub fn set(&self, process_name: Option<&str>) {
        *self.process.borrow_mut() = process_name.map(str::to_string);
    }
}

impl ForegroundProbe for FakeForeground {
    fn foreground_process(&self) -> Option<String> {
        self.process.borrow().clone()
    }

    fn foreground_shape(&self) -> Option<WindowShape> {
        self.shape_asked.set(self.shape_asked.get() + 1);
        self.shape.get()
    }
}

#[derive(Default)]
pub struct FakeHomeActivator {
    pub activations: Cell<u32>,
    pub game_activations: Cell<u32>,
    pub refuse_game: Cell<bool>,
}

impl HomeActivator for FakeHomeActivator {
    fn activate_home(&self) -> PortResult<()> {
        self.activations.set(self.activations.get() + 1);
        Ok(())
    }

    fn activate_game(&self) -> PortResult<()> {
        self.game_activations.set(self.game_activations.get() + 1);
        if self.refuse_game.get() {
            return Err(PortError::Failed("fake activation refused".to_string()));
        }
        Ok(())
    }
}

/// Records its calls and never waits.
#[derive(Default)]
pub struct FakeLaunchScreen {
    pub log: RefCell<Vec<&'static str>>,
}

impl LaunchScreen for FakeLaunchScreen {
    fn show(&self) {
        self.log.borrow_mut().push("show");
    }

    fn hold_until(&self, ready: &dyn Fn() -> bool, _timeout: std::time::Duration) -> bool {
        self.log.borrow_mut().push("hold");
        ready()
    }

    fn raise(&self) {
        self.log.borrow_mut().push("raise");
    }

    fn close(&self) {
        self.log.borrow_mut().push("close");
    }
}

#[derive(Default)]
pub struct FakeAgentControl {
    pub requests: Cell<u32>,
    pub launcher_starts: Cell<u32>,
    pub fail: bool,
}

impl AgentControl for FakeAgentControl {
    fn ensure_running(&self) -> PortResult<()> {
        self.requests.set(self.requests.get() + 1);
        if self.fail {
            return Err(PortError::Failed("fake agent failure".to_string()));
        }
        Ok(())
    }

    fn launcher_started(&self) {
        self.launcher_starts.set(self.launcher_starts.get() + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::schema::Applies;

    const SOUND_SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "program",
            kind: SettingKind::Text {
                format: crate::settings::schema::TextFormat::Path,
            },
            title: "Program",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: true,
        },
        SettingSpec {
            key: "link",
            kind: SettingKind::Toggle { default: true },
            title: "Link",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: false,
        },
        SettingSpec {
            key: "icon",
            kind: SettingKind::Toggle { default: true },
            title: "Icon",
            help: "",
            applies: Applies::Live,
            requires: Some("link"),
            required: false,
        },
    ];

    static SOUND: FakeLauncherDescriptor = FakeLauncherDescriptor {
        settings: SOUND_SETTINGS,
        template: "# The sound launcher, with kind = \"sound\" above:\n\
                   # [launcher.sound]\n\
                   # program = 'C:\\Sound.exe'   # its program\n\
                   # link = true\n\
                   # icon = true              # needs link\n",
        refuses: Some(("program", "")),
        conflicting: &["soundhelper.exe"],
        ..FakeLauncherDescriptor::named("sound", "Sound", LauncherCaps::ALL)
    };

    #[test]
    fn a_sound_descriptor_passes() {
        conformance(&SOUND);
    }

    #[test]
    fn a_catalog_with_every_text_passes() {
        static TRANSLATED: FakeLauncherDescriptor = FakeLauncherDescriptor {
            catalogs: &[(
                "de",
                "msgid \"Sound\"\nmsgstr \"Klang\"\n\nmsgid \"Program\"\nmsgstr \"Programm\"\n\n\
                 msgid \"Link\"\nmsgstr \"Verbindung\"\n\nmsgid \"Icon\"\nmsgstr \"Symbol\"\n",
            )],
            ..SOUND
        };
        conformance(&TRANSLATED);
    }

    #[test]
    #[should_panic(expected = "the \"de\" catalog does not translate \"Program\"")]
    fn a_catalog_without_a_title_fails() {
        static HALF: FakeLauncherDescriptor = FakeLauncherDescriptor {
            catalogs: &[("de", "msgid \"Sound\"\nmsgstr \"Klang\"\n")],
            ..SOUND
        };
        conformance(&HALF);
    }

    #[test]
    #[should_panic(expected = "the \"de\" catalog does not translate \"loud\"")]
    fn a_catalog_without_a_choice_value_fails() {
        static CHOOSY: FakeLauncherDescriptor = FakeLauncherDescriptor {
            settings: &[SettingSpec {
                key: "volume",
                kind: SettingKind::Choice {
                    values: &["quiet", "loud"],
                    default: "quiet",
                },
                title: "Volume",
                help: "",
                applies: Applies::Live,
                requires: None,
                required: false,
            }],
            template: "# [launcher.choosy]\n# volume = \"quiet\"\n",
            catalogs: &[(
                "de",
                "msgid \"Choosy\"\nmsgstr \"Wählerisch\"\n\nmsgid \"Volume\"\nmsgstr \"Lautstärke\"\n\n\
                 msgid \"quiet\"\nmsgstr \"leise\"\n",
            )],
            ..FakeLauncherDescriptor::named("choosy", "Choosy", LauncherCaps::ALL)
        };
        conformance(&CHOOSY);
    }

    #[test]
    #[should_panic(expected = "the template names each option")]
    fn a_template_missing_an_option_fails() {
        static MISSING: FakeLauncherDescriptor = FakeLauncherDescriptor {
            settings: SOUND_SETTINGS,
            template: "# [launcher.sound]\n# program = ''\n# link = true\n",
            ..FakeLauncherDescriptor::named("sound", "Sound", LauncherCaps::ALL)
        };
        conformance(&MISSING);
    }

    #[test]
    #[should_panic(expected = "is a key of [launcher] itself")]
    fn an_id_that_is_a_key_of_the_launcher_section_fails() {
        static MENU: FakeLauncherDescriptor =
            FakeLauncherDescriptor::named("menu", "Menu", LauncherCaps::ALL);
        conformance(&MENU);
    }

    #[test]
    #[should_panic(expected = "its defaults break")]
    fn defaults_that_break_the_rules_fail() {
        static STRICT: FakeLauncherDescriptor = FakeLauncherDescriptor {
            settings: &[SettingSpec {
                key: "mode",
                kind: SettingKind::Choice {
                    values: &["a", "b"],
                    default: "a",
                },
                title: "Mode",
                help: "",
                applies: Applies::Live,
                requires: None,
                required: false,
            }],
            template: "# [launcher.strict]\n# mode = \"a\"\n",
            refuses: Some(("mode", "a")),
            ..FakeLauncherDescriptor::named("strict", "Strict", LauncherCaps::ALL)
        };
        conformance(&STRICT);
    }

    #[test]
    #[should_panic(expected = "which is no toggle")]
    fn requiring_what_is_no_toggle_fails() {
        check_settings(
            "launcher.x",
            &[SettingSpec {
                key: "icon",
                kind: SettingKind::Toggle { default: true },
                title: "Icon",
                help: "",
                applies: Applies::Live,
                requires: Some("link"),
                required: false,
            }],
        );
    }

    #[test]
    #[should_panic(expected = "required, yet it has a default")]
    fn a_required_toggle_fails() {
        check_settings(
            "launcher.x",
            &[SettingSpec {
                key: "link",
                kind: SettingKind::Toggle { default: true },
                title: "Link",
                help: "",
                applies: Applies::Live,
                requires: None,
                required: true,
            }],
        );
    }

    #[test]
    fn a_sound_device_passes() {
        static PAD: FakeDevice = FakeDevice {
            buttons: &[
                (0, "home", Suppression::Swallowed),
                (1, "quick_access", Suppression::Observed),
            ],
            settings: SOUND_SETTINGS,
            ..FakeDevice::named("my-pad_2", "My Pad")
        };
        // Its required program has no default, so its defaults are checked without it.
        device_conformance(&PAD);
    }

    #[test]
    #[should_panic(expected = "has a meaning of its own in [device]")]
    fn a_device_named_like_a_profile_value_fails() {
        static AUTO: FakeDevice = FakeDevice::named("auto", "Auto");
        device_conformance(&AUTO);
    }

    #[test]
    #[should_panic(expected = "no buttons")]
    fn a_device_without_buttons_fails() {
        static EMPTY: FakeDevice = FakeDevice {
            buttons: &[],
            ..FakeDevice::named("empty", "Empty")
        };
        device_conformance(&EMPTY);
    }

    #[test]
    #[should_panic(expected = "button id 0 is there twice")]
    fn two_buttons_of_one_id_fail() {
        static TWICE: FakeDevice = FakeDevice {
            buttons: &[
                (0, "home", Suppression::Swallowed),
                (0, "menu", Suppression::Swallowed),
            ],
            ..FakeDevice::named("twice", "Twice")
        };
        device_conformance(&TWICE);
    }

    #[test]
    #[should_panic(expected = "its defaults break")]
    fn a_device_whose_defaults_break_its_rules_fails() {
        static STRICT: FakeDevice = FakeDevice {
            needs: Some("mode"),
            ..FakeDevice::named("strict", "Strict")
        };
        device_conformance(&STRICT);
    }

    #[test]
    fn a_template_is_read_for_its_keys_only() {
        let template = "# Explained, with kind = \"x\" here:\n[features]\n\
                        # button_remap = true   # a comment = not a key\n\
                        launch_screen = false\n# [device.button]\n# key = \"D\"\n";
        let keys: Vec<String> = template_keys(template)
            .iter()
            .map(|(section, key)| format!("{section}.{key}"))
            .collect();
        assert_eq!(
            keys,
            [
                "features.button_remap",
                "features.launch_screen",
                "device.button.key"
            ]
        );
    }
}

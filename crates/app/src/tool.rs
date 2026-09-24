//! What `mujinactl` and Mujina Settings share. Mujina Settings reaches the rest of Mujina only
//! through here, so that it names no adapter (docs/architecture.md); a test there enforces it.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use mujina_adapter_config::StoredSnapshot;
use mujina_adapter_windows::agent_control::{self, AgentInstance};
use mujina_adapter_windows::fse::WindowsFse;
use mujina_adapter_windows::paths;
use mujina_application::device::{DeviceSelection, Devices, SystemIdentity};
use mujina_application::doctor::{Doctor, Finding};
use mujina_application::launcher::Launchers;
use mujina_application::ports::{
    FseState, FullScreenExperience, HomeAppRegistry, PackageIdentity, PortError,
};
use mujina_application::register::{
    HomeAppRegistration, RegisterError, RegisterOutcome, UnregisterOutcome,
};
use mujina_application::settings::schema::takes_effect_live;
use mujina_application::settings::{
    LoadedSettings, SettingChange, SettingValue, SettingsSource, SettingsStore,
};
use mujina_domain::chord::TriggerChord;

use crate::compose::{self, Adapters, Role};
use crate::{capture, registry};

pub fn data_dir() -> PathBuf {
    paths::data_dir()
}

pub fn in_xbox_mode() -> bool {
    WindowsFse::bind().state() == FseState::Active
}

/// Mujina Settings' claim to be the one open; held for as long as its window is.
pub struct SettingsInstance {
    _claim: AgentInstance,
}

/// The claim, unless another Mujina Settings holds it already.
pub fn claim_settings_instance() -> Option<SettingsInstance> {
    AgentInstance::claim_settings_app().map(|claim| SettingsInstance { _claim: claim })
}

/// Starts the agent as `mujina.exe` next to the running program would be started in Xbox mode.
pub fn start_agent() -> std::io::Result<()> {
    let program = std::env::current_exe()?.with_file_name("mujina.exe");
    std::process::Command::new(program)
        .arg(agent_control::AGENT_ARGUMENT)
        .spawn()
        .map(drop)
}

/// The launchers compiled in, in the order Mujina Settings lists them.
pub fn launchers() -> Launchers {
    registry::launchers()
}

pub fn devices() -> Devices {
    registry::devices()
}

/// E.g. `LWIN+D`; empty for none.
pub fn button_keys(device: &DeviceSelection) -> String {
    registry::button_keys(device)
}

/// Translations, by language, of the texts Mujina Settings shows from the rest of Mujina.
pub fn catalogs() -> Vec<(&'static str, &'static str)> {
    let launchers = launchers()
        .all
        .iter()
        .flat_map(|launcher| launcher.catalogs());
    let devices = devices().all.iter().flat_map(|device| device.catalogs());
    mujina_application::CATALOGS
        .iter()
        .chain(mujina_adapter_windows::CATALOGS)
        .chain(launchers)
        .chain(devices)
        .copied()
        .collect()
}

/// `config.toml`, read once: what it stores and what that amounts to.
pub struct Configuration {
    stored: StoredSnapshot,
    pub loaded: LoadedSettings,
    /// What the firmware says this machine is; `profile = "auto"` goes by it.
    pub system: SystemIdentity,
}

impl Configuration {
    /// What the file says for `key` itself; `None` where it relies on the default.
    pub fn stored(&self, key: &str) -> Option<SettingValue> {
        self.stored.stored(key)
    }
}

/// Writes the commented template first where there is no file.
pub fn configuration() -> Configuration {
    let config = compose::config();
    config.ensure_template();
    Configuration {
        stored: config.snapshot(),
        loaded: config.load(),
        system: config.system().clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// The running agent took it over.
    Now,
    /// It is read the next time Xbox mode is entered.
    NextSession,
}

/// Stores `changes` together and tells a running agent.
pub fn change(changes: &[SettingChange]) -> Result<Applied, PortError> {
    let config = compose::config();
    // The file's device before the change stands in for the one the agent runs; if an earlier
    // change is still waiting, this errs towards "next session".
    let device_before = changes
        .iter()
        .any(|change| change.key.starts_with("device."))
        .then(|| config.load().settings.device.id);
    config.apply(changes)?;
    mujina_adapter_windows::settings_signal::notify();
    let (launchers, devices) = (registry::launchers(), registry::devices());
    let now = config.load().settings;
    // The running launcher unless changed during the session; the agent follows only its options.
    let named = now.launcher.id;
    let device_waits = device_before.is_some_and(|before| {
        let running = registry::device_runtime(before.as_deref());
        registry::device_waits(running, now.device.id.as_deref())
    });
    let live = !device_waits
        && changes
            .iter()
            .all(|change| takes_effect_live(&change.key, launchers.all, devices.all, &named));
    Ok(if live && agent_control::agent_is_running() {
        Applied::Now
    } else {
        Applied::NextSession
    })
}

/// The doctor's checks of what Windows allows Mujina, in the order Mujina Settings' System page
/// lists them. The location permission is checked only while Steam's Wi-Fi fix is on.
pub const SYSTEM_CHECKS: [&str; 4] = [
    mujina_application::doctor::FULL_SCREEN_EXPERIENCE,
    mujina_adapter_windows::checks::DEVELOPER_MODE,
    mujina_adapter_steam::checks::LOCATION_PERMISSION,
    mujina_adapter_windows::checks::AGENT,
];

// `Doctor::examine` is tied to a doctor's lifetime, which the one built in `ask_doctor` lacks.
#[allow(clippy::redundant_closure_for_method_calls)]
pub(crate) fn examine(adapters: &Adapters) -> Vec<Finding> {
    ask_doctor(adapters, |doctor| doctor.examine())
}

fn ask_doctor<T>(adapters: &Adapters, ask: impl FnOnce(&Doctor<'_>) -> T) -> T {
    let checks = adapters.checks();
    ask(&Doctor {
        checks: &checks,
        fse: &adapters.fse,
        identity: &adapters.identity,
        registry: &adapters.home_registry,
        launcher: adapters.launcher.as_ref(),
    })
}

/// Build and use it on one thread: the launcher's adapter cannot move between threads.
pub struct Diagnostics {
    adapters: Adapters,
    /// What [`system`](Self::system) found, so that [`examine`](Self::examine) skips those checks.
    known: Vec<Finding>,
}

/// Quick to find, so a page can show them before the doctor is done.
pub struct SystemFacts {
    pub launcher: String,
    /// Mujina's own app ID; `None` when it runs unpackaged.
    pub ours: Option<String>,
    /// The home app Windows starts in Xbox mode; `None` for its own.
    pub home_app: Option<String>,
    pub findings: Vec<Finding>,
}

pub struct Diagnosis {
    pub findings: Vec<Finding>,
    pub settings: LoadedSettings,
    pub system: SystemIdentity,
    pub launcher: String,
    /// `device.profile` as `config.toml` says it; `None` where it relies on the default.
    pub stored_profile: Option<SettingValue>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self {
            adapters: Adapters::new(Role::Tool),
            known: Vec::new(),
        }
    }

    /// The facts, with the findings of the checks `ids` names alone, in that order.
    pub fn system(&mut self, ids: &[&str]) -> SystemFacts {
        let adapters = &self.adapters;
        let mut findings = ask_doctor(adapters, |doctor| {
            doctor.examine_only(|id| ids.contains(&id))
        });
        findings.sort_by_key(|finding| ids.iter().position(|id| *id == finding.id));
        self.known.clone_from(&findings);
        SystemFacts {
            launcher: adapters.launcher.display_name(),
            ours: adapters.identity.app_user_model_id(),
            home_app: adapters.home_registry.current().ok().flatten(),
            findings,
        }
    }

    /// Every check, reusing what [`system`](Self::system) found.
    pub fn examine(self) -> Diagnosis {
        let findings = ask_doctor(&self.adapters, |doctor| doctor.examine_knowing(self.known));
        let config = compose::config_for(self.adapters.system.clone());
        Diagnosis {
            findings,
            stored_profile: config.stored("device.profile"),
            launcher: self.adapters.launcher.display_name(),
            settings: self.adapters.settings,
            system: self.adapters.system,
        }
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self::new()
    }
}

/// Makes Mujina the home app, remembering the one before.
pub fn make_home_app() -> Result<RegisterOutcome, RegisterError> {
    let adapters = Adapters::new(Role::Tool);
    HomeAppRegistration::new(&adapters.identity, &adapters.home_registry).register()
}

pub fn give_home_app_back() -> Result<UnregisterOutcome, RegisterError> {
    let adapters = Adapters::new(Role::Tool);
    HomeAppRegistration::new(&adapters.identity, &adapters.home_registry).unregister()
}

/// The Mujina Setup this package's installation kept, which removes Mujina in the right order.
/// `None` when the program is unpackaged, or its package was installed some other way.
pub fn retained_setup() -> Option<PathBuf> {
    let family = mujina_winutil::package::family_name()?;
    paths::retained_setup(&family).filter(|path| path.is_file())
}

/// Starts the kept Mujina Setup outside the package and does not wait. The removal stops the
/// package's processes, so the caller should end.
pub fn start_removal() -> Result<(), String> {
    let setup = retained_setup().ok_or("Mujina Setup is not kept on this device")?;
    mujina_winutil::process::spawn_outside_package(&setup, &["--uninstall"])
        .map_err(|error| format!("{}: {error}", setup.display()))
}

pub const CAPTURE_TIME: Duration = Duration::from_secs(10);

/// Blocks up to [`CAPTURE_TIME`]; `Ok(None)` if no chord came in time or `cancel` was set.
pub fn capture(cancel: &AtomicBool) -> Result<Option<TriggerChord>, String> {
    capture::watch(CAPTURE_TIME, cancel)
}

/// A chord as `mujinactl capture` prints it, e.g. `LWIN+D injected`.
pub fn capture_line(chord: TriggerChord) -> String {
    capture::line(chord)
}

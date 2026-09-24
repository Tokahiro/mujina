//! Wiring of adapters. The only place that names concrete adapter types; the launchers and
//! devices are listed in [`registry`].

use mujina_adapter_config::ConfigFile;
use mujina_adapter_kit::plugin::{DeviceParts, DeviceRuntime, LauncherPlugin, SessionParts};
use mujina_adapter_windows::agent_control::WindowsAgentControl;
use mujina_adapter_windows::fse::WindowsFse;
use mujina_adapter_windows::home_registry::WindowsHomeAppRegistry;
use mujina_adapter_windows::identity::WindowsPackageIdentity;
use mujina_adapter_windows::{paths, smbios};
use mujina_application::device::SystemIdentity;
use mujina_application::doctor::Check;
use mujina_application::ports::{FullScreenExperience, HomeLauncher};
use mujina_application::settings::{LoadedSettings, SettingsSource};

use crate::registry;

// Re-exported for the entry points and Mujina Settings, which ask for their adapters here.
pub use mujina_application::Role;

/// The adapters every entry point needs.
pub struct Adapters {
    pub fse: WindowsFse,
    pub identity: WindowsPackageIdentity,
    pub home_registry: WindowsHomeAppRegistry,
    pub agent_control: WindowsAgentControl,
    /// The launcher the configuration names.
    pub plugin: &'static LauncherPlugin,
    /// What the home role and the tools need of it.
    pub launcher: Box<dyn HomeLauncher>,
    /// What the agent needs of it. Built for the agent role only: it starts whatever the
    /// launcher keeps running in the background.
    pub session: Option<SessionParts>,
    /// What runs the device the configuration names.
    pub device_runtime: &'static dyn DeviceRuntime,
    /// The device's buttons as the agent runs them. Built for the agent role only, and only if
    /// the device could be started: without, the agent runs without the button.
    pub device: Option<DeviceParts>,
    pub settings: LoadedSettings,
    /// What the firmware says this machine is; decides the device.
    pub system: SystemIdentity,
}

impl Adapters {
    pub fn new(role: Role) -> Self {
        let system = smbios::identity();
        let config = config_for(system.clone());
        config.ensure_template();
        let settings = config.load();
        let plugin = registry::plugin(&settings.settings.launcher.id);
        let options = &settings.settings.launcher.options;
        let device_runtime = registry::device_runtime(settings.settings.device.id.as_deref());
        Self {
            fse: WindowsFse::bind(),
            identity: WindowsPackageIdentity,
            home_registry: WindowsHomeAppRegistry,
            agent_control: WindowsAgentControl,
            plugin,
            launcher: plugin.runtime.home(options),
            session: (role == Role::Agent).then(|| plugin.runtime.session(options)),
            device_runtime,
            device: (role == Role::Agent)
                .then(|| start_device(device_runtime, &settings))
                .flatten(),
            system,
            settings,
        }
    }

    /// What `doctor` looks at beyond the ports: Windows' side, then the launcher's own, then the
    /// device's.
    pub fn checks(&self) -> Vec<Box<dyn Check>> {
        let descriptor = self.plugin.descriptor;
        let mut checks = mujina_adapter_windows::checks::all(
            descriptor.conflicting_processes(),
            self.fse.state(),
        );
        checks.extend(
            self.plugin
                .runtime
                .checks(&self.settings.settings.launcher.options),
        );
        checks.extend(self.device_runtime.checks());
        checks
    }
}

/// Starts the device's buttons for the agent, as switched on or off: off, its runtime runs
/// without one, so that switching it on applies at once.
fn start_device(runtime: &dyn DeviceRuntime, settings: &LoadedSettings) -> Option<DeviceParts> {
    if let Err(error) = runtime.prepare() {
        log::warn!("the device could not be readied: {error}");
    }
    let device = settings.settings.active_device();
    runtime
        .start(&device)
        .inspect_err(|error| {
            if device.is_none() {
                // Every runtime must start so (DeviceRuntime::start); one that does not leaves a
                // button switched on later unmapped until the next session.
                log::warn!(
                    "the device's runtime did not start with the button switched off ({error}); \
                     switching it on applies the next time Xbox mode is entered"
                );
            } else {
                log::error!("the device button could not be started ({error}); it stays unmapped");
            }
        })
        .ok()
}

/// `config.toml` in Mujina's data folder, read with the launchers and devices compiled in, for
/// this machine. Every reader and writer of the configuration comes from here, so that each
/// knows the same launchers and devices.
pub fn config() -> ConfigFile {
    config_for(smbios::identity())
}

/// As [`config`], for a machine whose identity was read already.
pub fn config_for(system: SystemIdentity) -> ConfigFile {
    ConfigFile::for_system(
        &paths::data_dir(),
        system,
        registry::launchers(),
        registry::devices(),
    )
}

//! Plug-ins: a portable descriptor and its Windows side, listed in `crates/app/src/registry.rs`.

use mujina_application::agent::AgentEvent;
use mujina_application::device::{DeviceDescriptor, DeviceSelection};
use mujina_application::doctor::Check;
use mujina_application::launcher::{LauncherDescriptor, OptionTable};
use mujina_application::ports::{DeviceButtons, HomeLauncher, PortResult, SessionLauncher};
use mujina_winutil::wait::WaitSource;

/// `home` and `session` must not block: the home role calls `home` on every activation.
// TODO: an undo hook for what a runtime changes outside Mujina (Steam's debugging marker), for
// when a feature is switched off, another launcher is chosen or Mujina is removed.
pub trait LauncherRuntime: Sync {
    /// For the short-lived home role, and for the tools: starts nothing that outlives the call.
    fn home(&self, options: &OptionTable) -> Box<dyn HomeLauncher>;

    /// For the resident agent: the launcher, with whatever it keeps running in the background.
    fn session(&self, options: &OptionTable) -> SessionParts;

    /// What `doctor` should look at for this launcher.
    fn checks(&self, _options: &OptionTable) -> Vec<Box<dyn Check>> {
        Vec::new()
    }
}

pub struct SessionParts {
    pub launcher: Box<dyn SessionLauncher>,
    /// E.g. one reporting [`AgentEvent::LauncherStateChanged`]; may be empty. A source must not
    /// block, and owns what it waits on.
    pub sources: Vec<Box<dyn WaitSource<AgentEvent>>>,
}

pub struct LauncherPlugin {
    pub descriptor: &'static dyn LauncherDescriptor,
    pub runtime: &'static dyn LauncherRuntime,
}

/// A device's Windows side, for the resident agent. One runtime may serve several devices, so
/// that switching between them applies at once ([`DeviceButtons::reconfigure`]).
pub trait DeviceRuntime: Sync {
    /// On the main thread, also with `device.id` `None` (off); stops when the parts drop.
    fn start(&self, device: &DeviceSelection) -> PortResult<DeviceParts>;

    /// What `doctor` should look at, e.g. the device's own software that also reacts to a button.
    fn checks(&self) -> Vec<Box<dyn Check>> {
        Vec::new()
    }

    /// Runs when the agent starts, before [`start`](Self::start); a failure is only logged.
    fn prepare(&self) -> PortResult<()> {
        Ok(())
    }
}

pub struct DeviceParts {
    pub buttons: Box<dyn DeviceButtons>,
    /// Wait sources for this device's presses, each reporting [`AgentEvent::ButtonPressed`]; one
    /// per way its buttons reach Windows. A source must not block, and owns what it waits on.
    pub sources: Vec<Box<dyn WaitSource<AgentEvent>>>,
}

/// A descriptor made at run time (a profile file) must be kept for the rest of the program.
#[derive(Clone, Copy)]
pub struct DevicePlugin {
    pub descriptor: &'static dyn DeviceDescriptor,
    pub runtime: &'static dyn DeviceRuntime,
}

/// For a device crate's tests: the runtime must start switched off, then reconfigure to off, to
/// `device` and to off again. `device` should need no hardware.
pub fn runtime_conformance(
    runtime: &dyn DeviceRuntime,
    device: &DeviceSelection,
) -> Result<(), String> {
    let off = DeviceSelection::none();
    let parts = runtime
        .start(&off)
        .map_err(|error| format!("it does not start with the button switched off: {error}"))?;
    let id = device.id.as_deref().unwrap_or("no device");
    for (step, selection) in [
        ("switched off", &off),
        (id, device),
        ("switched off again", &off),
    ] {
        if !parts.buttons.reconfigure(selection) {
            return Err(format!("its buttons do not take over {step}"));
        }
    }
    Ok(())
}

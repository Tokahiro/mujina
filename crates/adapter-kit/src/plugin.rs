//! Launcher and device plug-ins: each pairs a portable descriptor with its Windows side.
//! `crates/app/src/registry.rs` lists them (ADR-0013).

use mujina_application::agent::AgentEvent;
use mujina_application::device::{DeviceDescriptor, DeviceSelection};
use mujina_application::doctor::Check;
use mujina_application::launcher::{LauncherDescriptor, OptionTable};
use mujina_application::ports::{DeviceButtons, HomeLauncher, PortResult, SessionLauncher};
use mujina_winutil::wait::WaitSource;

/// A launcher's Windows side, built from its configured options. Building must not block: the
/// home role builds its part on every activation.
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
    /// The launcher's own wait sources for the agent's event loop, e.g. one reporting
    /// [`AgentEvent::LauncherStateChanged`] (Steam: its registry key). May be empty; the agent
    /// still sees the process come and go. A source must not block, and owns what it waits on.
    pub sources: Vec<Box<dyn WaitSource<AgentEvent>>>,
}

/// One launcher, as `registry.rs` lists it.
pub struct LauncherPlugin {
    pub descriptor: &'static dyn LauncherDescriptor,
    pub runtime: &'static dyn LauncherRuntime,
}

/// A device's Windows side, for the resident agent. One runtime may serve several devices (all
/// key-chord devices share the keyboard crate's), so that switching between them applies at
/// once ([`DeviceButtons::reconfigure`]).
pub trait DeviceRuntime: Sync {
    /// Starts the buttons of `device` on the agent's main thread. Its id is `None` while no
    /// button is mapped (switched off); a later reconfigure may map one. What it starts lasts as
    /// long as the parts. On failure the agent runs without the button and logs why.
    fn start(&self, device: &DeviceSelection) -> PortResult<DeviceParts>;

    /// What `doctor` should look at for the devices it serves, e.g. the device's own software
    /// that also reacts to a button.
    fn checks(&self) -> Vec<Box<dyn Check>> {
        Vec::new()
    }

    /// Readies the device when the agent starts, before [`start`](Self::start): a controller
    /// mode, say. A failure is only logged.
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

/// One device, as `registry.rs` lists it. A descriptor may be data made at run time (a profile
/// file).
#[derive(Clone, Copy)]
pub struct DevicePlugin {
    pub descriptor: &'static dyn DeviceDescriptor,
    pub runtime: &'static dyn DeviceRuntime,
}

/// Conformance check for a device crate's tests: the runtime must start switched off (as the
/// agent starts it; refusing would leave the button unmapped all session), then reconfigure to
/// off, to `device` and to off again. Returns what failed. `device` should need no hardware.
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

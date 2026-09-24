//! Launchers and devices as the composition root plugs them in: each one's portable description
//! and its Windows side, paired. The root lists these pairs in `crates/app/src/registry.rs`
//! (ADR-0013).

use mujina_application::agent::AgentEvent;
use mujina_application::device::{DeviceDescriptor, DeviceSelection};
use mujina_application::doctor::Check;
use mujina_application::launcher::{LauncherDescriptor, OptionTable};
use mujina_application::ports::{DeviceButtons, HomeLauncher, PortResult, SessionLauncher};
use mujina_winutil::wait::WaitSource;

/// What runs of a launcher on Windows, built from its options as the configuration has them.
/// Building must not block: the home role builds its part on every activation.
// TODO(leftover-1): an undo hook for what a runtime changes outside Mujina (Steam's debugging
// marker), for when a feature is switched off, another launcher chosen or Mujina removed.
pub trait LauncherRuntime: Sync {
    /// For the short-lived home role, and for the tools: starts nothing that outlives the call.
    fn home(&self, options: &OptionTable) -> Box<dyn HomeLauncher>;

    /// For the resident agent: the launcher as it runs beside it, with whatever it keeps running
    /// in the background.
    fn session(&self, options: &OptionTable) -> SessionParts;

    /// What `doctor` should look at for this launcher.
    fn checks(&self, _options: &OptionTable) -> Vec<Box<dyn Check>> {
        Vec::new()
    }
}

/// What the agent gets of a launcher.
pub struct SessionParts {
    pub launcher: Box<dyn SessionLauncher>,
    /// Kernel objects of the launcher's own that the agent's event loop waits on beside its
    /// own, each with what it means when signalled. A launcher that can signal its state brings
    /// a source that reports [`AgentEvent::LauncherStateChanged`] (Steam: a change notification
    /// on its registry key); one that cannot brings none, and the agent still sees its process
    /// come and go. A source must not block, and owns whatever it waits on.
    pub sources: Vec<Box<dyn WaitSource<AgentEvent>>>,
}

/// One launcher, as `registry.rs` lists it.
pub struct LauncherPlugin {
    pub descriptor: &'static dyn LauncherDescriptor,
    pub runtime: &'static dyn LauncherRuntime,
}

/// What runs of a device on Windows, for the resident agent. One runtime may serve several
/// devices: every key-chord device is served by the keyboard crate's, so that a switch between
/// them applies at once ([`DeviceButtons::reconfigure`]).
pub trait DeviceRuntime: Sync {
    /// Starts the buttons of `device`, one this runtime serves, on the agent's main thread; its
    /// id is `None` where no button is mapped yet (switched off), and one may be later. What it
    /// starts lasts as long as the parts. Failing leaves the agent without the button, and says
    /// why in the log.
    fn start(&self, device: &DeviceSelection) -> PortResult<DeviceParts>;

    /// What `doctor` should look at for the devices it serves: the device's own software that
    /// reacts to a button too, for one.
    fn checks(&self) -> Vec<Box<dyn Check>> {
        Vec::new()
    }

    /// Readies the device when the agent starts, before [`start`](Self::start): a controller
    /// mode, say. A failure is only logged.
    fn prepare(&self) -> PortResult<()> {
        Ok(())
    }
}

/// What the agent gets of a device.
pub struct DeviceParts {
    pub buttons: Box<dyn DeviceButtons>,
    /// Kernel objects the agent's event loop waits on for this device's presses, each reporting
    /// [`AgentEvent::ButtonPressed`] with the button's id. A device may bring several, one per
    /// way its buttons reach Windows. A source must not block, and owns whatever it waits on.
    pub sources: Vec<Box<dyn WaitSource<AgentEvent>>>,
}

/// One device, as `registry.rs` lists it. Descriptors are made at run time where they are data
/// (a profile file), so the pair holds whatever outlives the program.
#[derive(Clone, Copy)]
pub struct DevicePlugin {
    pub descriptor: &'static dyn DeviceDescriptor,
    pub runtime: &'static dyn DeviceRuntime,
}

/// What every device runtime has to do beyond its own tests, for a device crate to check in
/// them: start with the button switched off (the agent starts it so, and a later change may
/// switch it on; a runtime that refused would leave the button unmapped for the whole session),
/// then take over being switched off, `device` (one of its own), and being switched off again.
/// What it fails at, otherwise. `device` should need no hardware, like a selection whose options
/// map no button.
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

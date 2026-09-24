//! What the agent asks of the device's buttons. Presses arrive the other way, as
//! [`AgentEvent::ButtonPressed`](crate::agent::AgentEvent::ButtonPressed).

use mujina_domain::button::ButtonId;

use crate::device::DeviceSelection;

pub trait DeviceButtons {
    /// Takes `device` over now if it can (none, one it serves, new options) and returns true;
    /// otherwise the change waits until Xbox mode is next entered. Must not block.
    fn reconfigure(&self, device: &DeviceSelection) -> bool;

    /// Replays a swallowed press of `button` as if Mujina were not there. Must not block.
    fn pass_on(&self, button: ButtonId);
}

//! What the agent asks of the device's buttons. Presses arrive the other way, as
//! [`AgentEvent::ButtonPressed`](crate::agent::AgentEvent::ButtonPressed).

use mujina_domain::button::ButtonId;

use crate::device::DeviceSelection;

pub trait DeviceButtons {
    /// Takes `device` over at once where it can (no button, a device this adapter serves, or new
    /// options of the running one) and returns true; otherwise the change waits until Xbox mode
    /// is next entered. Must not block.
    fn reconfigure(&self, device: &DeviceSelection) -> bool;

    /// Replays a swallowed press of `button`, so it does what it does without Mujina. Must not
    /// block.
    fn pass_on(&self, button: ButtonId);
}

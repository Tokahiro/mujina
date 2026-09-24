//! The device's buttons while the agent runs. Presses arrive as
//! [`AgentEvent::ButtonPressed`](crate::agent::AgentEvent::ButtonPressed) from the device's own
//! wait sources; this is what the agent asks of the device in return.

use mujina_domain::button::ButtonId;

use crate::device::DeviceSelection;

pub trait DeviceButtons {
    /// The configuration now chooses `device`; `None` as its id means no button, as with the
    /// button switched off, which every device takes over by catching nothing. Takes it over at
    /// once where it can (no button, a device it serves, or new options of the one running) and
    /// says so; otherwise it keeps running as it is, and the device changes the next time Xbox
    /// mode is entered. Must not block.
    fn reconfigure(&self, device: &DeviceSelection) -> bool;

    /// Lets `button` do what it does without Mujina: sends on what was swallowed of it. Only
    /// asked for a button whose presses are swallowed. Must not block.
    fn pass_on(&self, button: ButtonId);
}

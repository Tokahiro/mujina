//! The key-chord devices as the agent runs them, and the keys it sends.

use std::cell::RefCell;
use std::sync::LazyLock;

use mujina_adapter_kit::plugin::{DeviceParts, DevicePlugin, DeviceRuntime};
use mujina_application::device::{ButtonId, DeviceSelection};
use mujina_application::ports::{DeviceButtons, KeySender, PortError, PortResult};
use mujina_domain::chord::TriggerChord;
use mujina_domain::keys::{HoldTiming, KeyChord};

use crate::hook::KeyboardHook;
use crate::sender::{self, Job};
use crate::{chords_for, descriptors};

#[derive(Debug)]
pub struct KeyboardRuntime;

/// The one runtime of every device of this crate, so that a switch between them applies at once.
pub static RUNTIME: KeyboardRuntime = KeyboardRuntime;

pub fn plugins() -> &'static [DevicePlugin] {
    static PLUGINS: LazyLock<Vec<DevicePlugin>> = LazyLock::new(|| {
        descriptors()
            .into_iter()
            .map(|descriptor| DevicePlugin {
                descriptor,
                runtime: &RUNTIME,
            })
            .collect()
    });
    &PLUGINS
}

impl DeviceRuntime for KeyboardRuntime {
    fn start(&self, device: &DeviceSelection) -> PortResult<DeviceParts> {
        let Some(chords) = chords_for(device) else {
            let id = device.id.as_deref().unwrap_or_default();
            return Err(PortError::Failed(format!("{id} is no key-chord device")));
        };
        // Only passing on and sending on held-back keys need the sender, so this is not fatal.
        if !sender::start() {
            log::error!("no sender thread; held-back keys cannot be sent on");
        }
        let hook = KeyboardHook::spawn().map_err(PortError::Failed)?;
        describe(&chords);
        hook.set_buttons(chords.clone());
        let source = hook.source();
        Ok(DeviceParts {
            buttons: Box::new(KeyboardButtons {
                hook,
                chords: RefCell::new(chords),
            }),
            sources: vec![Box::new(source)],
        })
    }
}

/// The buttons of the key-chord device in use, for the agent's main thread.
struct KeyboardButtons {
    hook: KeyboardHook,
    /// What the hook was last told to catch.
    chords: RefCell<Vec<(ButtonId, TriggerChord)>>,
}

impl DeviceButtons for KeyboardButtons {
    /// Takes over any device of this crate at once: only the chords the hook catches change.
    fn reconfigure(&self, device: &DeviceSelection) -> bool {
        let Some(chords) = chords_for(device) else {
            return false;
        };
        if *self.chords.borrow() != chords {
            describe(&chords);
            if !self.hook.set_buttons(chords.clone()) {
                log::error!("the keyboard hook thread is gone; the device button stays as it was");
            }
            *self.chords.borrow_mut() = chords;
        }
        true
    }

    fn pass_on(&self, button: ButtonId) {
        let chord = self
            .chords
            .borrow()
            .iter()
            .find(|(id, _)| *id == button)
            .map(|(_, chord)| chord.keys);
        if let Some(keys) = chord {
            // Tagged as ours, so the hook lets it through to whoever handled it before Mujina.
            sender::send(Job::Chord(keys, HoldTiming::default()));
        }
    }
}

fn describe(chords: &[(ButtonId, TriggerChord)]) {
    if chords.is_empty() {
        log::info!("device button: not mapped");
        return;
    }
    let named: Vec<String> = chords
        .iter()
        .map(|(_, chord)| chord.keys.to_string())
        .collect();
    log::info!("device button: {}", named.join(", "));
}

/// Sends the launcher's shortcuts on the sender thread, so that each proves the hook alive.
#[derive(Debug)]
pub struct KeyboardSender(());

impl KeyboardSender {
    /// Starts the sender thread, if it does not run yet; `None` if it cannot.
    pub fn start() -> Option<Self> {
        sender::start().then_some(Self(()))
    }
}

impl KeySender for KeyboardSender {
    fn send_chord(&self, chord: KeyChord, timing: HoldTiming) {
        sender::send(Job::Chord(chord, timing));
    }
}

#[cfg(test)]
mod tests {
    use mujina_adapter_kit::plugin::runtime_conformance;

    use super::*;
    use crate::OWN_ID;

    #[test]
    fn the_runtime_keeps_the_rules_of_every_device_runtime() {
        // Without keys no hook is installed, so the test needs no desktop.
        let own = DeviceSelection {
            id: Some(OWN_ID.to_string()),
            ..DeviceSelection::none()
        };
        runtime_conformance(&RUNTIME, &own).unwrap();
        let buttons = RUNTIME.start(&DeviceSelection::none()).unwrap().buttons;
        let other = DeviceSelection {
            id: Some("no such device".to_string()),
            ..DeviceSelection::none()
        };
        assert!(!buttons.reconfigure(&other));
    }
}

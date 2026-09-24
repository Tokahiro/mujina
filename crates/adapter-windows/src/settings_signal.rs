//! Tells a running agent that `config.toml` changed. A named event in the session's namespace, so
//! a settings tool and the agent of the same session find each other without any IPC, and the
//! agent only ever waits on it.

use std::os::windows::io::{AsHandle, BorrowedHandle};

use mujina_application::agent::AgentEvent;
use mujina_winutil::event::Event;
use mujina_winutil::wait::WaitSource;

const NAME: &str = r"Local\Mujina.settings";

/// For the agent: the event to wait on.
pub fn listen() -> Option<SettingsChangedSource> {
    SettingsChangedSource::named(NAME)
}

/// For whoever changed the configuration. Does nothing noticeable without a running agent; the
/// next agent reads the file when it starts anyway.
pub fn notify() {
    if let Ok(event) = Event::named_auto_reset(NAME) {
        event.set();
    }
}

/// The configuration changed, as a wait source of the agent's event loop.
pub struct SettingsChangedSource(Event);

impl SettingsChangedSource {
    pub(crate) fn named(name: &str) -> Option<Self> {
        Event::named_auto_reset(name).ok().map(Self)
    }
}

impl WaitSource<AgentEvent> for SettingsChangedSource {
    fn name(&self) -> &'static str {
        "configuration changed"
    }

    fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
        Some(self.0.as_handle())
    }

    /// Auto-reset: the wait has taken the signal, and signals that came together are one.
    fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
        out.push(AgentEvent::SettingsChanged);
    }
}

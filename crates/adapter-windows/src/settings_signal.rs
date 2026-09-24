//! Tells a running agent that `config.toml` changed, through a named event (ADR-0010).

use std::os::windows::io::{AsHandle, BorrowedHandle};

use mujina_application::agent::AgentEvent;
use mujina_winutil::event::Event;
use mujina_winutil::wait::WaitSource;

const NAME: &str = r"Local\Mujina.settings";

/// For the agent: the event to wait on.
pub fn listen() -> Option<SettingsChangedSource> {
    SettingsChangedSource::named(NAME)
}

/// Without a running agent this does nothing; the next agent reads the file when it starts.
pub fn notify() {
    if let Ok(event) = Event::named_auto_reset(NAME) {
        event.set();
    }
}

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

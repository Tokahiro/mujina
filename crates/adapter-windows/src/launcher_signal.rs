//! Tells a running agent that the home role has just started the launcher, or switched it to its
//! console UI, so that the agent looks for the launcher's process again. The same pattern as
//! `settings_signal` (ADR-0010): a named auto-reset event in the session's namespace, no IPC, and
//! the agent only ever waits on it.
//!
//! Without it the agent would find a launcher it did not see start only once the launcher's
//! window comes to the front, and one started while the agent was still getting ready not at all.

use std::os::windows::io::{AsHandle, BorrowedHandle};

use mujina_application::agent::AgentEvent;
use mujina_winutil::event::Event;
use mujina_winutil::wait::WaitSource;

const NAME: &str = r"Local\Mujina.launcher-started";

/// For the agent: the event to wait on.
pub fn listen() -> Option<LauncherStartedSource> {
    LauncherStartedSource::named(NAME)
}

/// For the home role, once the launcher has been started or switched to its console UI. Does
/// nothing noticeable without a running agent. Should the event not exist yet, it is gone again
/// with this call's handle (a named event lives as long as a handle to it, CreateEventW), and an
/// agent that opens it later looks for the launcher once it is ready anyway.
pub fn notify() {
    if let Ok(event) = Event::named_auto_reset(NAME) {
        event.set();
    }
}

/// The launcher was started by the home role, as a wait source of the agent's event loop.
pub struct LauncherStartedSource(Event);

impl LauncherStartedSource {
    fn named(name: &str) -> Option<Self> {
        Event::named_auto_reset(name).ok().map(Self)
    }
}

impl WaitSource<AgentEvent> for LauncherStartedSource {
    fn name(&self) -> &'static str {
        "launcher started"
    }

    fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
        Some(self.0.as_handle())
    }

    /// Auto-reset: the wait has taken the signal, and starts that came together are one.
    fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
        out.push(AgentEvent::LauncherStarted);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    use mujina_winutil::wait::{EventLoop, Flow};

    use super::*;
    use crate::settings_signal::SettingsChangedSource;

    /// A name no other test run uses, and never the agent's own: a running agent on the
    /// developer's machine must not be told anything.
    fn unique_name() -> String {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        format!(
            r"Local\MujinaTests.signal.{}.{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn signal(name: &str) {
        Event::named_auto_reset(name).unwrap().set();
    }

    #[test]
    fn each_named_event_says_what_it_means() {
        let (started, changed) = (unique_name(), unique_name());
        let mut events = EventLoop::new();
        events
            .add(Box::new(LauncherStartedSource::named(&started).unwrap()))
            .unwrap();
        events
            .add(Box::new(SettingsChangedSource::named(&changed).unwrap()))
            .unwrap();
        events.wake_at(
            Instant::now() + Duration::from_secs(10),
            AgentEvent::SessionEnding,
        );

        // Twice before anyone waits: an auto-reset event reports it once.
        signal(&started);
        signal(&started);
        let mut handled = Vec::new();
        events
            .run(&mut |_| {}, &mut |event| {
                let flow = if event == AgentEvent::LauncherStarted {
                    signal(&changed);
                    Flow::Continue
                } else {
                    Flow::Exit
                };
                handled.push(event);
                flow
            })
            .unwrap();
        assert_eq!(
            handled,
            [AgentEvent::LauncherStarted, AgentEvent::SettingsChanged]
        );
    }
}

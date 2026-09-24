//! Tells a running agent that the home role started the launcher or switched it to its console
//! UI, so that the agent looks for the launcher's process again (ADR-0010).

use std::os::windows::io::{AsHandle, BorrowedHandle};

use mujina_application::agent::AgentEvent;
use mujina_winutil::event::Event;
use mujina_winutil::wait::WaitSource;

const NAME: &str = r"Local\Mujina.launcher-started";

/// For the agent: the event to wait on.
pub fn listen() -> Option<LauncherStartedSource> {
    LauncherStartedSource::named(NAME)
}

/// For the home role. Without a running agent the event dies with this call's handle
/// (CreateEventW); an agent that starts later looks for the launcher anyway.
pub fn notify() {
    if let Ok(event) = Event::named_auto_reset(NAME) {
        event.set();
    }
}

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

    /// Never the agent's own name: a running agent on the developer's machine must not hear it.
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

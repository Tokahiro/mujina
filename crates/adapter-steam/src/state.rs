//! Steam writes a game's start and end, and its own process id, under its registry key; a change
//! notification on that key wakes the agent's event loop.

use std::os::windows::io::{AsHandle, BorrowedHandle};

use mujina_application::agent::AgentEvent;
use mujina_winutil::registry::{Hive, RegistryWatch};
use mujina_winutil::wait::WaitSource;

use crate::registry_keys;

pub struct SteamState {
    /// Withdrawn once it cannot be armed again: such a watch never signals again.
    watch: Option<RegistryWatch>,
}

impl SteamState {
    /// `None`, logged, where Steam's key cannot be watched (Steam never ran for this user, say).
    pub fn watch() -> Option<Self> {
        Self::watching(Hive::CurrentUser, registry_keys::STEAM)
            .inspect_err(|error| {
                log::warn!(
                    "Steam's state cannot be watched ({error}); the button will not notice games"
                );
            })
            .ok()
    }

    fn watching(hive: Hive, key: &str) -> mujina_winutil::error::Win32Result<Self> {
        RegistryWatch::open(hive, key).map(|watch| Self { watch: Some(watch) })
    }
}

impl WaitSource<AgentEvent> for SteamState {
    fn name(&self) -> &'static str {
        "Steam's registry key"
    }

    fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
        self.watch.as_ref().map(|watch| watch.event().as_handle())
    }

    fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
        // One notification per arming (RegNotifyChangeKeyValue), so armed again before the
        // change is looked at: whatever Steam writes meanwhile signals once more.
        if let Some(watch) = &self.watch
            && let Err(error) = watch.rearm()
        {
            log::warn!(
                "Steam's state is no longer watched ({error}); the button will not notice games \
                 any more"
            );
            self.watch = None;
        }
        out.push(AgentEvent::LauncherStateChanged);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    use mujina_winutil::registry::{delete_key, write_u32};
    use mujina_winutil::wait::{EventLoop, Flow};

    use super::*;

    /// Per process, so that runs of several checkouts do not see each other's writes.
    fn unique_key() -> String {
        format!(r"Software\MujinaTests\steam-state.{}", std::process::id())
    }

    #[test]
    fn a_change_is_reported_and_the_watch_armed_again() {
        let key = unique_key();
        write_u32(Hive::CurrentUser, &key, "RunningAppID", 0).unwrap();
        let state = SteamState::watching(Hive::CurrentUser, &key).unwrap();

        let mut events = EventLoop::new();
        events.add(Box::new(state)).unwrap();
        // Ends a test whose watch was not armed again, which would otherwise wait for ever.
        events.wake_at(
            Instant::now() + Duration::from_secs(10),
            AgentEvent::SessionEnding,
        );

        write_u32(Hive::CurrentUser, &key, "RunningAppID", 1).unwrap();
        let changes = Cell::new(0);
        events
            .run(&mut |_| {}, &mut |event| {
                assert_eq!(event, AgentEvent::LauncherStateChanged, "no change in time");
                changes.set(changes.get() + 1);
                if changes.get() == 3 {
                    return Flow::Exit;
                }
                write_u32(Hive::CurrentUser, &key, "RunningAppID", changes.get() + 1).unwrap();
                Flow::Continue
            })
            .unwrap();
        assert_eq!(changes.get(), 3);
        delete_key(Hive::CurrentUser, &key).unwrap();
    }

    #[test]
    fn a_key_that_is_not_there_is_not_watched() {
        let missing = SteamState::watching(Hive::CurrentUser, r"Software\MujinaTests\no-such-key");
        assert!(missing.is_err());
    }
}

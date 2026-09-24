//! The launcher as the home role and the agent use it; descriptors are in [`crate::launcher`].

use std::path::PathBuf;

use mujina_domain::activation::HomeDestination;
use mujina_domain::keys::KeyChord;

use super::{PortError, PortResult};
use crate::agent::AgentEvent;
use crate::launcher::OptionTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherInstall {
    pub executable: PathBuf,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherState {
    NotRunning,
    /// The process is up but its console UI is not on screen (tray, desktop client, …).
    RunningWithoutUi,
    /// The console UI window exists.
    UiVisible,
}

/// Where the launcher's running game is at a button press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameWhereabouts {
    InFront,
    Behind,
    /// No window found. `known`: the launcher knows the game's processes, so the window in front
    /// is not the game's.
    NoWindow {
        known: bool,
    },
    /// The launcher counts it as running, but nothing of it runs (it waits for a browser, say).
    Gone,
}

/// What the home role needs of a launcher: find it, and put its console UI in front of the user.
pub trait HomeLauncher {
    /// For logs and pages, e.g. "Steam Big Picture" or the program's name.
    fn display_name(&self) -> String;

    fn locate(&self) -> PortResult<LauncherInstall>;

    fn state(&self) -> LauncherState;

    /// Called on every home activation, so idempotent and cheap; a failure is only a warning.
    fn prepare(&self, _install: &LauncherInstall) -> PortResult<()> {
        Ok(())
    }

    fn start_ui(&self, install: &LauncherInstall) -> PortResult<()>;

    fn switch_to_ui(&self, install: &LauncherInstall) -> PortResult<()>;

    fn focus_ui(&self) -> PortResult<()>;

    /// Opens `destination` in the console UI, which is already up.
    fn navigate(
        &self,
        _install: &LauncherInstall,
        _destination: HomeDestination,
    ) -> PortResult<()> {
        Ok(())
    }

    /// Brings the running game to the front; works only from the freshly activated home role.
    fn focus_game(&self) -> PortResult<()> {
        Err(PortError::NotFound("the game's window".into()))
    }
}

/// Whether the launcher took a request on itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direct {
    Taken,
    /// It did not, or cannot tell: the shortcut is next.
    NotTaken,
}

/// What the resident agent needs of a running launcher.
pub trait SessionLauncher {
    /// Whether this image name is the launcher's: its window in front counts as the launcher UI.
    fn owns_process(&self, process_name: &str) -> bool;

    fn process_id(&self) -> Option<u32>;

    /// `false` for a launcher that cannot tell.
    fn game_running(&self) -> bool {
        false
    }

    /// Whether the window in front is a game this launcher started; may look things up.
    fn game_in_front(&self) -> bool {
        false
    }

    fn game_findable(&self) -> bool {
        false
    }

    /// Asked on a button press while [`game_running`](Self::game_running), so it may look things
    /// up. Override it where the launcher knows the game's processes.
    fn game_whereabouts(&self) -> GameWhereabouts {
        if self.game_in_front() {
            GameWhereabouts::InFront
        } else if self.game_findable() {
            GameWhereabouts::Behind
        } else {
            GameWhereabouts::NoWindow { known: false }
        }
    }

    /// The launcher may tidy up what it shows while the game loads. Must not block.
    fn game_started(&self) {}

    /// Must not block.
    fn game_ended(&self) {}

    /// Opens the main menu while its UI has the focus. Read on each press, so it may change.
    fn menu_shortcut(&self) -> Option<KeyChord> {
        None
    }

    /// Opens the overlay while a game has the focus. May read the launcher's files.
    fn overlay_shortcut(&self) -> Option<KeyChord> {
        None
    }

    /// Toggles the menu without a shortcut; [`Direct::Taken`] once it went through. Must not block.
    fn open_menu(&self) -> Direct {
        Direct::NotTaken
    }

    /// As [`open_menu`](Self::open_menu), for the overlay; asked only while the game is in front.
    fn open_overlay(&self) -> Direct {
        Direct::NotTaken
    }

    /// Called only when `live` changed. It holds only the options set: get defaults with
    /// [`SettingSpec::value_in`](crate::settings::schema::SettingSpec::value_in) or
    /// [`schema::flag`](crate::settings::schema::flag). Must not block.
    fn reconfigure(&self, _live: &OptionTable) {}

    /// Every event, after the agent handled it. Must not block: hand work to the launcher's thread.
    fn observe(&self, _event: &AgentEvent) {}
}

//! The launcher as the home role (bringing it up) and the agent (living beside it) need it. What
//! it is and what it offers is its descriptor's business ([`crate::launcher`]).

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
    /// Its window is the one in front.
    InFront,
    /// Its window was found behind the one in front.
    Behind,
    /// No window of it was found. `known`: the launcher knows the game's processes, so the window
    /// in front is not the game's; otherwise it may be (a game started through its own launcher).
    NoWindow { known: bool },
    /// The launcher still counts it as running, but nothing of it runs: a launcher may wait for
    /// what the game started (a browser, say) to end too.
    Gone,
}

/// What the home role needs of a launcher: find it, and put its console UI in front of the user.
pub trait HomeLauncher {
    /// For logs and pages, e.g. "Steam Big Picture" or the program's name.
    fn display_name(&self) -> String;

    fn locate(&self) -> PortResult<LauncherInstall>;

    fn state(&self) -> LauncherState;

    /// Puts in place what the launcher needs, however it gets started. Called on every home
    /// activation, so it must be idempotent and cheap; a failure is only a warning.
    fn prepare(&self, _install: &LauncherInstall) -> PortResult<()> {
        Ok(())
    }

    fn start_ui(&self, install: &LauncherInstall) -> PortResult<()>;

    fn switch_to_ui(&self, install: &LauncherInstall) -> PortResult<()>;

    fn focus_ui(&self) -> PortResult<()>;

    /// Opens `destination` in the console UI, which is already up. Launchers without such pages
    /// stay where they are.
    fn navigate(
        &self,
        _install: &LauncherInstall,
        _destination: HomeDestination,
    ) -> PortResult<()> {
        Ok(())
    }

    /// Brings the running game back to the front. Works from the freshly activated home role
    /// only; see [`HomeActivator::activate_game`](super::HomeActivator::activate_game).
    fn focus_game(&self) -> PortResult<()> {
        Err(PortError::NotFound("the game's window".into()))
    }
}

/// Whether the launcher took a request on itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direct {
    /// It did; nothing else is to be done.
    Taken,
    /// It did not, or cannot tell that it did: the shortcut is next.
    NotTaken,
}

/// What the resident agent needs of a running launcher.
pub trait SessionLauncher {
    /// Whether a process of this image name is the launcher's own: its window in front is the
    /// launcher UI, and supervision may watch it (a reused process id names someone else's).
    fn owns_process(&self, process_name: &str) -> bool;

    fn process_id(&self) -> Option<u32>;

    /// `false` for a launcher that cannot tell; the button then opens the menu in the launcher UI
    /// and leads home elsewhere.
    fn game_running(&self) -> bool {
        false
    }

    /// Whether the window in front belongs to a game this launcher started. Asked on a button
    /// press only, so it may look things up.
    fn game_in_front(&self) -> bool {
        false
    }

    /// Whether the running game's window can be found at all.
    fn game_findable(&self) -> bool {
        false
    }

    /// Asked on a button press only, while [`game_running`](Self::game_running) is true, so it
    /// may look things up. Override it where the launcher knows the game's processes.
    fn game_whereabouts(&self) -> GameWhereabouts {
        if self.game_in_front() {
            GameWhereabouts::InFront
        } else if self.game_findable() {
            GameWhereabouts::Behind
        } else {
            GameWhereabouts::NoWindow { known: false }
        }
    }

    /// A game was just started; the launcher may tidy up what it shows while it loads. Must not
    /// block.
    fn game_started(&self) {}

    /// The game has ended just now. Must not block.
    fn game_ended(&self) {}

    /// The chord that opens its main menu while its UI has the focus. Read on each press: the
    /// user may change it at any time.
    fn menu_shortcut(&self) -> Option<KeyChord> {
        None
    }

    /// The chord that opens its overlay while a game has the focus. Separate from the menu's,
    /// since finding it may mean reading the launcher's files.
    fn overlay_shortcut(&self) -> Option<KeyChord> {
        None
    }

    /// Toggles the menu without a shortcut (a shortcut only works while the right part of the
    /// launcher has the focus). [`Direct::Taken`] once the request went through. Must not block.
    fn open_menu(&self) -> Direct {
        Direct::NotTaken
    }

    /// As [`open_menu`](Self::open_menu), for the in-game overlay; asked only while the game is
    /// in front.
    fn open_overlay(&self) -> Direct {
        Direct::NotTaken
    }

    /// The options that apply at once changed. `live` holds only those set, so read them with
    /// [`SettingSpec::value_in`](crate::settings::schema::SettingSpec::value_in) or
    /// [`schema::flag`](crate::settings::schema::flag) to get defaults. Called only when `live`
    /// differs from last time. Must not block.
    fn reconfigure(&self, _live: &OptionTable) {}

    /// Sees every event after the agent has handled it, for work beside these ports (Steam keeps
    /// its Big Picture link up this way). Must not block: hand work to the launcher's own thread.
    fn observe(&self, _event: &AgentEvent) {}
}

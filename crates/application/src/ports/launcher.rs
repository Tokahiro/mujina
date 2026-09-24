//! The launcher, in the two parts Mujina's roles need of it: bringing it up (the short-lived
//! home role) and living beside it (the resident agent). Which launcher it is, and what it
//! offers, is its descriptor's business ([`crate::launcher`]).

use std::path::PathBuf;

use mujina_domain::activation::HomeDestination;
use mujina_domain::keys::KeyChord;

use super::{PortError, PortResult};
use crate::agent::AgentEvent;
use crate::launcher::OptionTable;

/// Where a launcher is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherInstall {
    pub executable: PathBuf,
    pub directory: PathBuf,
}

/// What the launcher is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherState {
    NotRunning,
    /// The process is up but its console UI is not on screen (tray, desktop client, …).
    RunningWithoutUi,
    /// The console UI window exists.
    UiVisible,
}

/// Where the launcher's running game is, as a button press finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameWhereabouts {
    /// Its window is the one in front.
    InFront,
    /// Its window was found, behind the one in front.
    Behind,
    /// No window of it was found. `known`: the launcher knows which processes are the game's
    /// (by the folder it installed the game in, say), so the window in front is surely not the
    /// game's. Otherwise it may be: a game started through a launcher of its own may be tied to
    /// nothing the launcher can see.
    NoWindow { known: bool },
    /// The launcher still counts it as running, but nothing of it runs any more: a launcher may
    /// wait for something the game started to end as well.
    Gone,
}

/// What the home role needs of a launcher: find it, and put its console UI in front of the user.
pub trait HomeLauncher {
    /// What to call it in logs and on the pages, e.g. "Steam Big Picture" or the program's name.
    fn display_name(&self) -> String;

    fn locate(&self) -> PortResult<LauncherInstall>;

    fn state(&self) -> LauncherState;

    /// Whatever has to be in place before the launcher runs, however it gets started. Called on
    /// every home activation, before the launcher is started, switched to or focused, so it
    /// must be idempotent and cheap. A failure is only a warning.
    fn prepare(&self, _install: &LauncherInstall) -> PortResult<()> {
        Ok(())
    }

    /// Starts the launcher directly into its console UI.
    fn start_ui(&self, install: &LauncherInstall) -> PortResult<()>;

    /// Asks the already running launcher to show its console UI.
    fn switch_to_ui(&self, install: &LauncherInstall) -> PortResult<()>;

    /// Brings the existing console UI to the foreground.
    fn focus_ui(&self) -> PortResult<()>;

    /// Shows a page of the console UI, which is up. Launchers without such pages stay where
    /// they are.
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
    /// It did, after trying: nothing else is to be done.
    Taken,
    /// It did not, or cannot tell that it did: the shortcut is next.
    NotTaken,
}

/// What the resident agent needs of a launcher while it runs: to tell its windows and games
/// apart, and to have it open its menus.
pub trait SessionLauncher {
    /// Whether a process with this image name is the launcher's own. It answers two questions:
    /// whether a foreground window of that process means "the launcher UI is in front", and
    /// whether supervision may watch that process as the launcher (a stale or reused process id
    /// names someone else's).
    fn owns_process(&self, process_name: &str) -> bool;

    /// Process id of the running launcher, for supervision.
    fn process_id(&self) -> Option<u32>;

    /// Whether the launcher currently runs a game. A launcher that cannot tell answers `false`;
    /// the device button then opens the menu in the launcher UI and leads home elsewhere.
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

    /// Where the running game is. Asked on a button press only, so it may look things up, and
    /// only while [`game_running`](Self::game_running) says a game runs. By default from
    /// [`game_in_front`](Self::game_in_front) and [`game_findable`](Self::game_findable); a
    /// launcher that knows more (which processes are the game's, whether any still runs) says
    /// so here.
    fn game_whereabouts(&self) -> GameWhereabouts {
        if self.game_in_front() {
            GameWhereabouts::InFront
        } else if self.game_findable() {
            GameWhereabouts::Behind
        } else {
            GameWhereabouts::NoWindow { known: false }
        }
    }

    /// A game was started just now. The launcher may tidy up what it shows while the game
    /// loads; it must not block.
    fn game_started(&self) {}

    /// The game has ended just now. Must not block.
    fn game_ended(&self) {}

    /// The chord that opens its main menu while its UI has the focus; `None` without one. Read
    /// when a press needs it, as is the overlay's: a user may change them at any time.
    fn menu_shortcut(&self) -> Option<KeyChord> {
        None
    }

    /// The chord that opens its overlay while a game has the focus; `None` without one. Apart
    /// from the menu's, since finding it may take a look into the launcher's files.
    fn overlay_shortcut(&self) -> Option<KeyChord> {
        None
    }

    /// Opens or closes the launcher's menu without a keyboard shortcut, where the launcher offers
    /// a way. A shortcut only reaches the menu while the right part of the launcher has the
    /// keyboard focus. [`Direct::Taken`] only once the request went through; must not block.
    fn open_menu(&self) -> Direct {
        Direct::NotTaken
    }

    /// Opens or closes the launcher's in-game overlay without a keyboard shortcut, where the
    /// launcher offers a way; asked only while the game is in front. As for
    /// [`open_menu`](Self::open_menu).
    fn open_overlay(&self) -> Direct {
        Direct::NotTaken
    }

    /// The configuration changed; `live` holds the launcher's options that apply at once, as far
    /// as the configuration sets them: one missing is back to its default, so read them with
    /// [`SettingSpec::value_in`](crate::settings::schema::SettingSpec::value_in) or
    /// [`schema::flag`](crate::settings::schema::flag). The rest waits for the next session.
    /// Called only when `live` differs from what the launcher was last given. It takes `&self`:
    /// what it changes lives behind a `Cell` or a lock. Must not block.
    fn reconfigure(&self, _live: &OptionTable) {}

    /// Every event the agent handles, after it has handled it: for what the launcher does beside
    /// these ports, reacting to the launcher's own signs of life, say. Steam keeps its link to
    /// Big Picture up this way, for the Wi-Fi icon and the menus. It takes `&self`, as
    /// [`reconfigure`](Self::reconfigure) does, and must not block: hand the work to a thread
    /// of the launcher's own that is otherwise blocked.
    fn observe(&self, _event: &AgentEvent) {}
}

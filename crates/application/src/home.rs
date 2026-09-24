//! The home activation use case: Windows activated us as the console home, so put the launcher's
//! console UI in front of the user.

use std::time::Duration;

use mujina_domain::activation::HomeDestination;

use crate::ports::{AgentControl, HomeLauncher, LaunchScreen, LauncherState, PortError};

/// How long the launch screen waits for the console UI. A cold start with an update check takes
/// a while; after this the user gets to see whatever the launcher is showing.
const UI_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the launch screen stays above the console UI once that has appeared. Long enough
/// for the first frame, short enough not to swallow the launcher's own start-up animation.
const PAINT_GRACE: Duration = Duration::from_millis(400);

/// How the launcher UI was brought up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeOutcome {
    /// The UI was already there and only needed the foreground.
    Focused,
    /// The launcher was running without its console UI and was asked to show it.
    SwitchedToUi,
    /// The launcher was started.
    Started,
    /// The running game was brought back to the front; the launcher was left alone.
    ReturnedToGame,
}

/// Result of a successful activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeReport {
    pub outcome: HomeOutcome,
    /// Problems that did not stop the launcher from coming up.
    pub warnings: Vec<String>,
}

/// Why the launcher could not be brought up. The message names the port's error too, so that
/// error is not also given as the source: whoever prints the chain would print it twice.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HomeError {
    #[error("launcher is not installed: {0}")]
    LauncherMissing(PortError),
    #[error("launcher could not be brought up: {0}")]
    LaunchFailed(PortError),
}

pub struct HomeActivation<'a> {
    launcher: &'a dyn HomeLauncher,
    agent: &'a dyn AgentControl,
    screen: Option<&'a dyn LaunchScreen>,
    destination: HomeDestination,
}

impl<'a> HomeActivation<'a> {
    pub fn new(launcher: &'a dyn HomeLauncher, agent: &'a dyn AgentControl) -> Self {
        Self {
            launcher,
            agent,
            screen: None,
            destination: HomeDestination::Home,
        }
    }

    /// Covers the desktop while the launcher starts and waits for its console UI. The screen is
    /// an ordinary window behind the launcher's, so the launcher's own start-up (update progress,
    /// intro video) stays fully visible.
    #[must_use]
    pub fn with_screen(mut self, screen: &'a dyn LaunchScreen) -> Self {
        self.screen = Some(screen);
        self
    }

    /// The page Windows asked for; the launcher's start page unless told otherwise.
    #[must_use]
    pub fn showing(mut self, destination: HomeDestination) -> Self {
        self.destination = destination;
        self
    }

    pub fn run(&self) -> Result<HomeReport, HomeError> {
        let install = self.launcher.locate().map_err(HomeError::LauncherMissing)?;
        let mut warnings = Vec::new();

        // The agent only adds comfort; the launcher comes up with or without it.
        if let Err(error) = self.agent.ensure_running() {
            warnings.push(format!("agent not started: {error}"));
        }

        // On every activation, not only before a start: the launcher may have been started by
        // something else, and it may be restarted by anything. Never blocks bringing it up.
        if let Err(error) = self.launcher.prepare(&install) {
            warnings.push(format!("preparation skipped: {error}"));
        }

        // Asked for by the agent when the device button is pressed with the game behind another
        // window. Without a game window to be found, the launcher is the next best place.
        let mut destination = self.destination;
        if destination == HomeDestination::Game {
            match self.launcher.focus_game() {
                Ok(()) => {
                    return Ok(HomeReport {
                        outcome: HomeOutcome::ReturnedToGame,
                        warnings,
                    });
                }
                Err(error) => {
                    warnings.push(format!("game not brought to the front: {error}"));
                    destination = HomeDestination::Home;
                }
            }
        }

        let outcome = match self.launcher.state() {
            LauncherState::UiVisible => {
                self.launcher.focus_ui().map_err(HomeError::LaunchFailed)?;
                HomeOutcome::Focused
            }
            LauncherState::RunningWithoutUi => {
                self.launcher
                    .switch_to_ui(&install)
                    .map_err(HomeError::LaunchFailed)?;
                HomeOutcome::SwitchedToUi
            }
            LauncherState::NotRunning => {
                self.launcher
                    .start_ui(&install)
                    .map_err(HomeError::LaunchFailed)?;
                HomeOutcome::Started
            }
        };
        // A launcher the agent watched may be gone, and one it never saw may have come: it looks
        // for the launcher's process now, not only once its window comes to the front.
        if outcome != HomeOutcome::Focused {
            self.agent.launcher_started();
        }

        if outcome != HomeOutcome::Focused
            && let Some(screen) = self.screen
        {
            // Only now: the launcher is already on its way, so whatever the screen needs to
            // get in front costs no start-up time. The launcher shows nothing for a second
            // or more, so the screen is up long before there is anything to cover.
            screen.show();
            let ready = || self.launcher.state() == LauncherState::UiVisible;
            let appeared = screen.hold_until(&ready, UI_TIMEOUT);
            if appeared {
                // The console UI's window exists before it has painted, and it takes the
                // foreground by itself, so the screen ends up behind an empty window and
                // what lies further back shows through (seen on a device as a flash). The
                // screen goes above it until it has had time to paint. Short: the launcher's
                // own start-up belongs to the user, not behind a black window.
                screen.raise();
                // Make sure the console UI ends up in front even if something else grabbed
                // the focus while the launcher was starting.
                if let Err(error) = self.launcher.focus_ui() {
                    warnings.push(format!("console UI not focused: {error}"));
                }
                screen.hold_until(&|| false, PAINT_GRACE);
            } else {
                warnings.push("console UI did not appear in time".to_string());
            }
            screen.close();
        }

        // A launcher that was just started comes up on its home page by itself. One that was
        // already showing something else is taken there, as the home button of a console does.
        let wanted = destination != HomeDestination::Home || outcome == HomeOutcome::Focused;
        if wanted
            && self.launcher.state() == LauncherState::UiVisible
            && let Err(error) = self.launcher.navigate(&install, destination)
        {
            warnings.push(format!("{destination:?} not opened: {error}"));
        }

        Ok(HomeReport { outcome, warnings })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeAgentControl, FakeLaunchScreen, FakeLauncher};

    #[test]
    fn cold_start_prepares_then_starts() {
        let launcher = FakeLauncher::installed(LauncherState::NotRunning);
        let agent = FakeAgentControl::default();
        let report = HomeActivation::new(&launcher, &agent).run().unwrap();
        assert_eq!(report.outcome, HomeOutcome::Started);
        assert!(report.warnings.is_empty());
        assert_eq!(launcher.calls(), ["prepare", "start_ui"]);
        assert_eq!(agent.launcher_starts.get(), 1, "the agent is told");
    }

    #[test]
    fn failed_preparation_is_only_a_warning() {
        let mut launcher = FakeLauncher::installed(LauncherState::NotRunning);
        launcher.fail_prepare = true;
        let report = HomeActivation::new(&launcher, &FakeAgentControl::default())
            .run()
            .unwrap();
        assert_eq!(report.outcome, HomeOutcome::Started);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(launcher.calls(), ["prepare", "start_ui"]);
    }

    #[test]
    fn running_launcher_is_switched_not_restarted() {
        let launcher = FakeLauncher::installed(LauncherState::RunningWithoutUi);
        let agent = FakeAgentControl::default();
        let report = HomeActivation::new(&launcher, &agent).run().unwrap();
        assert_eq!(report.outcome, HomeOutcome::SwitchedToUi);
        assert_eq!(launcher.calls(), ["prepare", "switch_to_ui"]);
        assert_eq!(agent.launcher_starts.get(), 1, "the agent is told");
    }

    #[test]
    fn visible_ui_is_focused_and_taken_home() {
        let launcher = FakeLauncher::installed(LauncherState::UiVisible);
        let agent = FakeAgentControl::default();
        let report = HomeActivation::new(&launcher, &agent).run().unwrap();
        assert_eq!(report.outcome, HomeOutcome::Focused);
        assert_eq!(launcher.calls(), ["prepare", "focus_ui", "navigate home"]);
        assert_eq!(agent.launcher_starts.get(), 0, "nothing new to look for");
    }

    #[test]
    fn failed_preparation_does_not_stop_a_running_launcher_either() {
        let mut launcher = FakeLauncher::installed(LauncherState::UiVisible);
        launcher.fail_prepare = true;
        let report = HomeActivation::new(&launcher, &FakeAgentControl::default())
            .run()
            .unwrap();
        assert_eq!(report.outcome, HomeOutcome::Focused);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(launcher.calls(), ["prepare", "focus_ui", "navigate home"]);
    }

    #[test]
    fn the_game_is_brought_back_and_the_launcher_left_alone() {
        let launcher = FakeLauncher::installed(LauncherState::UiVisible);
        launcher.game_in_front.set(Some(false));
        let report = HomeActivation::new(&launcher, &FakeAgentControl::default())
            .showing(HomeDestination::Game)
            .run()
            .unwrap();
        assert_eq!(report.outcome, HomeOutcome::ReturnedToGame);
        assert_eq!(launcher.calls(), ["prepare", "focus_game"]);
    }

    #[test]
    fn without_a_game_window_the_launcher_is_the_next_best_place() {
        let launcher = FakeLauncher::installed(LauncherState::UiVisible);
        let report = HomeActivation::new(&launcher, &FakeAgentControl::default())
            .showing(HomeDestination::Game)
            .run()
            .unwrap();
        assert_eq!(report.outcome, HomeOutcome::Focused);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(launcher.calls(), ["prepare", "focus_ui", "navigate home"]);
    }

    #[test]
    fn the_library_is_opened_once_the_ui_is_there() {
        let launcher = FakeLauncher::installed(LauncherState::UiVisible);
        HomeActivation::new(&launcher, &FakeAgentControl::default())
            .showing(HomeDestination::Library)
            .run()
            .unwrap();
        assert_eq!(
            launcher.calls(),
            ["prepare", "focus_ui", "navigate library"]
        );

        let launcher = FakeLauncher::installed(LauncherState::NotRunning);
        HomeActivation::new(&launcher, &FakeAgentControl::default())
            .showing(HomeDestination::Library)
            .run()
            .unwrap();
        assert_eq!(
            launcher.calls(),
            ["prepare", "start_ui", "navigate library"]
        );
    }

    #[test]
    fn launch_screen_covers_the_start_and_the_ui_is_focused_when_it_appears() {
        let launcher = FakeLauncher::installed(LauncherState::NotRunning);
        let screen = FakeLaunchScreen::default();
        let report = HomeActivation::new(&launcher, &FakeAgentControl::default())
            .with_screen(&screen)
            .run()
            .unwrap();
        assert_eq!(report.outcome, HomeOutcome::Started);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(
            *screen.log.borrow(),
            ["show", "hold", "raise", "hold", "close"]
        );
        assert_eq!(launcher.calls(), ["prepare", "start_ui", "focus_ui"]);
    }

    #[test]
    fn launch_screen_is_not_used_when_the_ui_is_already_there() {
        let launcher = FakeLauncher::installed(LauncherState::UiVisible);
        let screen = FakeLaunchScreen::default();
        HomeActivation::new(&launcher, &FakeAgentControl::default())
            .with_screen(&screen)
            .run()
            .unwrap();
        assert!(screen.log.borrow().is_empty());
    }

    #[test]
    fn agent_is_requested_and_its_failure_is_only_a_warning() {
        let launcher = FakeLauncher::installed(LauncherState::UiVisible);
        let agent = FakeAgentControl {
            fail: true,
            ..FakeAgentControl::default()
        };
        let report = HomeActivation::new(&launcher, &agent).run().unwrap();
        assert_eq!(agent.requests.get(), 1);
        assert_eq!(report.outcome, HomeOutcome::Focused);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn missing_launcher_is_reported() {
        let launcher = FakeLauncher::missing();
        let error = HomeActivation::new(&launcher, &FakeAgentControl::default())
            .run()
            .unwrap_err();
        assert!(matches!(error, HomeError::LauncherMissing(_)));
        assert!(launcher.calls().is_empty());
    }

    #[test]
    fn an_error_names_the_ports_error_once() {
        let error = HomeActivation::new(&FakeLauncher::missing(), &FakeAgentControl::default())
            .run()
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "launcher is not installed: not found: fake launcher"
        );
        // In the message, so not the source as well: a printed chain would say it twice.
        assert!(std::error::Error::source(&error).is_none());

        let error = HomeError::LaunchFailed(PortError::Failed("no window".to_string()));
        assert_eq!(
            error.to_string(),
            "launcher could not be brought up: no window"
        );
        assert!(std::error::Error::source(&error).is_none());
    }
}

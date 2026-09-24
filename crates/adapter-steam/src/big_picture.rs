//! Steam Big Picture as a [`HomeLauncher`] and a [`SessionLauncher`].

use std::time::Duration;

use mujina_adapter_kit::launcher;
use mujina_application::agent::AgentEvent;
use mujina_application::ports::{
    Direct, GameWhereabouts, HomeLauncher, LauncherInstall, LauncherState, PortError, PortResult,
    SessionLauncher,
};
use mujina_domain::activation::HomeDestination;
use mujina_domain::keys::KeyChord;
use mujina_winutil::process;

use crate::descriptor::SteamOptions;
use crate::indicator::{STEAM_DEBUG_PORT, SteamWifiIndicator};
use crate::navigation::{DirectMenus, MenuHost};
use crate::{games, marker, navigation, registry_keys, shortcuts, window_rule, wlan};

/// Starts Steam straight into the gamepad UI, without the desktop client flashing up first.
const START_ARGUMENT: &str = "-gamepadui";
const SWITCH_ARGUMENT: &str = "steam://open/bigpicture";

pub struct SteamBigPicture {
    ui_link: bool,
    indicator: Option<SteamWifiIndicator>,
    /// Only with the indicator, whose worker keeps the link the menus need.
    menus: Option<DirectMenus>,
}

impl SteamBigPicture {
    /// For the short-lived home role and the tools: launches Steam, keeps nothing running.
    pub fn for_home(options: SteamOptions) -> Self {
        Self {
            ui_link: options.ui_link,
            indicator: None,
            menus: None,
        }
    }

    /// For the resident agent: with the Wi-Fi fix on, also starts the Steam UI worker and the
    /// WLAN reader, the only part that makes Windows ask for the location permission.
    pub fn for_agent(options: SteamOptions) -> Self {
        // TODO: the direct menus should need only `ui_link`. They need the Wi-Fi fix, whose
        // worker keeps the link; let that worker run without the hook, with no extra thread.
        let indicator = options
            .wifi_fix()
            .then(|| SteamWifiIndicator::start(STEAM_DEBUG_PORT))
            .flatten();
        if let Some(indicator) = &indicator {
            // The earlier in Steam's start-up the link is up, the less it disturbs.
            indicator.connect();
            if !wlan::follow(indicator.clone()) {
                log::warn!(
                    "the Wi-Fi reader could not be started; the Wi-Fi icon stays as Steam draws it"
                );
            }
        }
        let menus = indicator
            .as_ref()
            .map(|indicator| DirectMenus::new(indicator.link()));
        Self {
            ui_link: options.ui_link,
            indicator,
            menus,
        }
    }

    fn toggle(&self, host: MenuHost) -> Direct {
        let Some(menus) = &self.menus else {
            return Direct::NotTaken;
        };
        if window_rule::find_big_picture().is_some() && menus.toggle(host) {
            Direct::Taken
        } else {
            Direct::NotTaken
        }
    }

    fn overlay(&self) -> KeyChord {
        let configured = self
            .locate()
            .ok()
            .zip(registry_keys::active_user())
            .and_then(|(install, account)| {
                shortcuts::configured_overlay(&install.directory, account)
            });
        configured.unwrap_or_else(shortcuts::default_overlay)
    }
}

impl HomeLauncher for SteamBigPicture {
    fn display_name(&self) -> String {
        "Steam Big Picture".to_string()
    }

    fn locate(&self) -> PortResult<LauncherInstall> {
        let executable = registry_keys::executable()
            .ok_or_else(|| PortError::NotFound("Steam is not installed for this user".into()))?;
        launcher::install_from_executable(executable)
    }

    fn state(&self) -> LauncherState {
        if window_rule::find_big_picture().is_some() {
            LauncherState::UiVisible
        } else if registry_keys::client_pid().is_some_and(process::is_running) {
            LauncherState::RunningWithoutUi
        } else {
            LauncherState::NotRunning
        }
    }

    /// Not in `start_ui`: anything may start Steam, which reads the marker only at start-up.
    fn prepare(&self, install: &LauncherInstall) -> PortResult<()> {
        if !self.ui_link {
            return Ok(());
        }
        match marker::ensure(&install.directory) {
            Ok(true) => {
                log::info!("created Steam UI debugging marker");
                // The marker works without the record; uninstalling then leaves it behind.
                if let Err(error) = marker::record(&install.directory) {
                    log::warn!("Steam UI debugging marker not recorded for removal: {error}");
                }
                if registry_keys::client_pid().is_some_and(process::is_running) {
                    log::info!(
                        "Steam is already running: it has to be restarted once before Mujina can \
                         use its UI link"
                    );
                }
            }
            Ok(false) => {}
            Err(error) => return Err(PortError::Failed(format!("debugging marker: {error}"))),
        }
        Ok(())
    }

    fn start_ui(&self, install: &LauncherInstall) -> PortResult<()> {
        launcher::launch(install, &[START_ARGUMENT])
    }

    fn switch_to_ui(&self, install: &LauncherInstall) -> PortResult<()> {
        launcher::launch(install, &[SWITCH_ARGUMENT])
    }

    fn focus_ui(&self) -> PortResult<()> {
        let handle = window_rule::find_big_picture()
            .ok_or_else(|| PortError::NotFound("Big Picture window".into()))?;
        launcher::focus_ui(handle, "Big Picture")
    }

    fn navigate(&self, _install: &LauncherInstall, destination: HomeDestination) -> PortResult<()> {
        let (route, patience) = match destination {
            // The game is a window, not a page; the home role handles it before it gets here.
            HomeDestination::Game => return Ok(()),
            // No UI link, no pages: neither wait for a port that will not answer nor complain.
            _ if !self.ui_link => return Ok(()),
            HomeDestination::Home => (navigation::HOME_ROUTE, Duration::ZERO),
            HomeDestination::Library => (navigation::LIBRARY_ROUTE, navigation::START_PATIENCE),
        };
        navigation::show(STEAM_DEBUG_PORT, route, patience).map_err(PortError::Failed)
    }

    fn focus_game(&self) -> PortResult<()> {
        let handle =
            games::window().ok_or_else(|| PortError::NotFound("the game's window".into()))?;
        launcher::focus_game(handle)
    }
}

impl SessionLauncher for SteamBigPicture {
    fn owns_process(&self, process_name: &str) -> bool {
        ["steamwebhelper.exe", "steam.exe"]
            .iter()
            .any(|name| name.eq_ignore_ascii_case(process_name))
    }

    fn process_id(&self) -> Option<u32> {
        registry_keys::client_pid()
    }

    fn game_running(&self) -> bool {
        registry_keys::running_app_id().is_some()
    }

    fn game_in_front(&self) -> bool {
        games::in_front()
    }

    fn game_findable(&self) -> bool {
        games::window().is_some()
    }

    /// By the game's install folder, so it also knows when nothing of the game runs any more:
    /// Steam may count a game as running while something it started, like a browser, is open.
    fn game_whereabouts(&self) -> GameWhereabouts {
        games::whereabouts()
    }

    fn game_started(&self) {
        if self.ui_link {
            navigation::show_running_game(STEAM_DEBUG_PORT);
        }
    }

    fn game_ended(&self) {
        if self.ui_link {
            navigation::log_state_after_game(STEAM_DEBUG_PORT);
        }
    }

    fn menu_shortcut(&self) -> Option<KeyChord> {
        Some(shortcuts::menu())
    }

    /// Read on every press: presses are rare and the user may change the setting any time.
    fn overlay_shortcut(&self) -> Option<KeyChord> {
        Some(self.overlay())
    }

    fn open_menu(&self) -> Direct {
        self.toggle(MenuHost::BigPicture)
    }

    /// The overlay only exists while Big Picture is the client's UI.
    fn open_overlay(&self) -> Direct {
        self.toggle(MenuHost::GameOverlay)
    }

    /// Every sign of life from Steam (re)establishes the link; cheap when it is up. Foreground
    /// changes alone are not enough: behind the Windows welcome screen none arrive.
    fn observe(&self, event: &AgentEvent) {
        let sign_of_life = match event {
            AgentEvent::ForegroundChanged {
                process_name: Some(name),
            } => self.owns_process(name),
            AgentEvent::LauncherStateChanged | AgentEvent::LauncherStarted => true,
            _ => false,
        };
        if sign_of_life && let Some(indicator) = &self.indicator {
            indicator.connect();
        }
    }
}

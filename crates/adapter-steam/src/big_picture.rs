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
/// Asks a running Steam to switch to Big Picture.
const SWITCH_ARGUMENT: &str = "steam://open/bigpicture";

pub struct SteamBigPicture {
    /// Steam's UI link, `[launcher.steam] ui_link`: the debugging-port marker is kept in place,
    /// and Big Picture's pages are reached through the port.
    ui_link: bool,
    indicator: Option<SteamWifiIndicator>,
    /// The direct way to the menus, only where the indicator runs: its worker keeps the link
    /// this needs, and knows whether it is up.
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

    /// For the resident agent: additionally runs, with the Wi-Fi fix on, the worker that keeps
    /// the link to Big Picture and corrects its Wi-Fi icon, and the worker that reads the Wi-Fi
    /// from the WLAN service for it. Nothing asks the WLAN service otherwise, so Windows asks
    /// for the location permission only for the Wi-Fi fix.
    pub fn for_agent(options: SteamOptions) -> Self {
        // The worker is what keeps a link up for the direct menus too, so without the Wi-Fi fix
        // the menus go by their shortcuts for now.
        // TODO(rings-4): the direct menus should need only `ui_link`, once the steam-ui worker
        // can run without the Wi-Fi hook, and without a thread more than today.
        let indicator = options
            .wifi_fix()
            .then(|| SteamWifiIndicator::start(STEAM_DEBUG_PORT))
            .flatten();
        if let Some(indicator) = &indicator {
            // As early as possible: the earlier in Steam's start-up the link is up, the less it
            // disturbs.
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

    /// Opens or closes a menu directly while the link to Big Picture is known to be up;
    /// [`Direct::NotTaken`] means "use the shortcut". Only the indicator's worker keeps that link,
    /// so where it does not run (the home and tool roles, the Wi-Fi fix off) there is no live
    /// link and the shortcut is all there is.
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

    /// Steam's overlay shortcut: the user's own from Steam's settings, or Steam's default.
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

    /// Makes sure of the marker on every activation, not only before a start: Steam may have been
    /// started by anything, and it reads the marker when it starts. Once the marker is there,
    /// this is one file creation that fails.
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
            // Without the UI link no page can be reached: no waiting for a port that will not
            // answer, and no complaint on every press of the home button.
            _ if !self.ui_link => return Ok(()),
            // Only asked for while Big Picture is up and running.
            HomeDestination::Home => (navigation::HOME_ROUTE, Duration::ZERO),
            HomeDestination::Library => (navigation::LIBRARY_ROUTE, navigation::START_PATIENCE),
        };
        navigation::show(STEAM_DEBUG_PORT, route, patience).map_err(PortError::Failed)
    }

    /// For the home role only. Never for the agent: the synthetic key tap this may need passes
    /// through the agent's own keyboard hook, whose thread would be the one waiting here.
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

    /// Knows the game's processes by the folder Steam installed it in, and so also when nothing
    /// of it runs any more: Steam may keep counting a game as running while something it started
    /// is still open (reported for a browser opened from a link in the game; not verified).
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

    /// `Ctrl+1` opens the Steam menu in Big Picture.
    fn menu_shortcut(&self) -> Option<KeyChord> {
        Some(shortcuts::menu())
    }

    /// Read from Steam's settings when a press needs it: a button press is rare, and the user may
    /// change the setting any time. Big Picture's own shortcuts do not reach a focused game; the
    /// overlay hotkey does.
    fn overlay_shortcut(&self) -> Option<KeyChord> {
        Some(self.overlay())
    }

    /// Through Big Picture's menu store, which works wherever the keyboard focus is. Falls back
    /// to the shortcut while the link to Big Picture is not known to be up, without Big Picture
    /// on screen, and for the rest of a Steam run in which the menu store turned out to be
    /// missing.
    fn open_menu(&self) -> Direct {
        self.toggle(MenuHost::BigPicture)
    }

    /// Through the game overlay's menu store, the same way; the overlay only exists while Big
    /// Picture is the client's UI.
    fn open_overlay(&self) -> Direct {
        self.toggle(MenuHost::GameOverlay)
    }

    /// Every sign of life from Steam is a chance to (re)establish the link to Big Picture: the
    /// earlier in Steam's start-up the link is up, the less it disturbs, and Steam may restart
    /// at any time. Cheap when the link is already up. Foreground changes alone are not enough;
    /// behind the Windows welcome screen none arrive.
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

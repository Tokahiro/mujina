//! What runs of Steam Big Picture, for each of Mujina's roles.

use mujina_adapter_kit::plugin::{LauncherRuntime, SessionParts};
use mujina_application::agent::AgentEvent;
use mujina_application::doctor::Check;
use mujina_application::launcher::OptionTable;
use mujina_application::ports::HomeLauncher;
use mujina_winutil::wait::WaitSource;

use crate::SteamBigPicture;
use crate::checks::{LocationPermission, OverlayShortcut, UiDebugging};
use crate::descriptor::SteamOptions;
use crate::indicator::STEAM_DEBUG_PORT;
use crate::state::SteamState;

pub struct SteamRuntime;

pub static RUNTIME: SteamRuntime = SteamRuntime;

impl LauncherRuntime for SteamRuntime {
    fn home(&self, options: &OptionTable) -> Box<dyn HomeLauncher> {
        Box::new(SteamBigPicture::for_home(SteamOptions::read(options)))
    }

    fn session(&self, options: &OptionTable) -> SessionParts {
        SessionParts {
            launcher: Box::new(SteamBigPicture::for_agent(SteamOptions::read(options))),
            // Steam writes game starts and its own pid under its registry key.
            sources: SteamState::watch()
                .into_iter()
                .map(|state| Box::new(state) as Box<dyn WaitSource<AgentEvent>>)
                .collect(),
        }
    }

    /// The UI link only where it is wanted, the location permission only for the Wi-Fi fix,
    /// which alone reads what it guards; the overlay shortcut always.
    fn checks(&self, options: &OptionTable) -> Vec<Box<dyn Check>> {
        let options = SteamOptions::read(options);
        let steam_directory = SteamBigPicture::for_home(options)
            .locate()
            .ok()
            .map(|install| install.directory);
        let mut checks: Vec<Box<dyn Check>> = Vec::new();
        if options.ui_link {
            checks.push(Box::new(UiDebugging {
                steam_directory: steam_directory.clone(),
                port: STEAM_DEBUG_PORT,
            }));
        }
        if options.wifi_fix() {
            checks.push(Box::new(LocationPermission));
        }
        checks.push(Box::new(OverlayShortcut { steam_directory }));
        checks
    }
}

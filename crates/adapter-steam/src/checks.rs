//! What `mujinactl doctor` looks at on the Steam side.

use std::path::PathBuf;

use mujina_application::Msg;
use mujina_application::doctor::{Check, Finding, Remedy, Severity};
use mujina_winutil::location::{self, LocationConsent};
use mujina_winutil::process;

use crate::{cdp, marker, registry_keys, shortcuts};

/// The marker file that makes Steam open its UI debugging port, and the port itself.
pub struct UiDebugging {
    pub steam_directory: Option<PathBuf>,
    pub port: u16,
}

impl Check for UiDebugging {
    fn id(&self) -> &'static str {
        "steam ui link"
    }

    fn title(&self) -> Msg {
        Msg::new("Steam's debugging port")
    }

    fn examine(&self) -> Finding {
        let Some(directory) = &self.steam_directory else {
            return self.found(Severity::Warning, "Steam not found");
        };
        if !marker::exists(directory) {
            return self.found(
                Severity::Warning,
                "marker file missing; Mujina creates it the next time it brings up Steam",
            );
        }
        if !registry_keys::client_pid().is_some_and(process::is_running) {
            return self.found(Severity::Ok, "marker file present; Steam is not running");
        }
        match cdp::steam_ui(self.port) {
            Ok(_) => self.found(Severity::Ok, format!("port {} answers", self.port)),
            Err(_) => self.found(
                Severity::Warning,
                format!(
                    "Steam runs but port {} does not answer; it was started before the marker \
                     file existed. Restart Steam once for Mujina to use its UI link",
                    self.port
                ),
            ),
        }
    }
}

/// Which shortcut the device button sends in a game.
pub struct OverlayShortcut {
    pub steam_directory: Option<PathBuf>,
}

impl Check for OverlayShortcut {
    fn id(&self) -> &'static str {
        "steam overlay key"
    }

    fn title(&self) -> Msg {
        Msg::new("Overlay shortcut")
    }

    fn examine(&self) -> Finding {
        let configured = self
            .steam_directory
            .as_deref()
            .zip(registry_keys::active_user())
            .and_then(|(directory, account)| shortcuts::configured_overlay(directory, account));
        match configured {
            Some(chord) => self.found(Severity::Ok, format!("{chord} (from Steam's settings)")),
            None => self.found(
                Severity::Ok,
                format!("{} (Steam's default)", shortcuts::default_overlay()),
            ),
        }
    }
}

/// Whether Windows lets Mujina read the Wi-Fi network's name and signal, which since Windows 11
/// 24H2 count as location data. Only Steam's Wi-Fi fix reads them, so only it asks.
pub struct LocationPermission;

/// The id of [`LocationPermission`]'s findings.
pub const LOCATION_PERMISSION: &str = "location permission";

impl Check for LocationPermission {
    fn id(&self) -> &'static str {
        LOCATION_PERMISSION
    }

    fn title(&self) -> Msg {
        Msg::new("Location permission")
    }

    fn examine(&self) -> Finding {
        let (severity, detail, summary) = match location::consent() {
            LocationConsent::DeniedEverywhere => (
                Severity::Warning,
                "location access is off in Windows; the Wi-Fi icon shows a generic connection",
                Msg::new(
                    "Location is off in Windows. Steam shows a connection without the network's \
                     name.",
                ),
            ),
            LocationConsent::Unpackaged => (
                Severity::Ok,
                "not applicable (unpackaged)",
                Msg::new("Not needed: Mujina runs unpackaged."),
            ),
            LocationConsent::Granted => (
                Severity::Ok,
                "granted",
                Msg::new("Allowed. Steam can show the name of the Wi-Fi network."),
            ),
            LocationConsent::DeniedForApp => (
                Severity::Warning,
                "declined for Mujina; the Wi-Fi icon shows a generic connection \
                 (Settings > Privacy & security > Location)",
                Msg::new(
                    "Declined for Mujina. Steam shows a connection without the network's name.",
                ),
            ),
            LocationConsent::NotAsked => (
                Severity::Ok,
                "not asked yet; Windows asks the first time Xbox mode is entered",
                Msg::new("Not asked yet. Windows asks the first time you enter Xbox mode."),
            ),
        };
        // Windows' own page is where the permission is given, whatever was found.
        self.found(severity, detail)
            .saying(summary)
            .remedied_by(Remedy::LocationSettings)
    }
}

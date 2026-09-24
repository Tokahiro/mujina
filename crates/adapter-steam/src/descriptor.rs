//! Steam's options under `[launcher.steam]` and what it offers. Portable, so tested everywhere.

use mujina_application::Msg;
use mujina_application::launcher::{LauncherCaps, LauncherDescriptor, OptionTable};
use mujina_application::settings::schema::{self, Applies, SettingKind, SettingSpec};

pub struct SteamDescriptor;

pub static DESCRIPTOR: SteamDescriptor = SteamDescriptor;

// Each text is a `Msg`, so that i18n-check holds lang/de.po to it; the setting keeps its English.
const SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: "ui_link",
        kind: SettingKind::Toggle { default: true },
        title: Msg::new("Use Steam's debugging port").english(),
        help: Msg::new(
            "For Home and Library, the “game is starting” screen and the Wi-Fi icon. The port has \
             no password: any program you run can control Steam through it (SECURITY.md).",
        )
        .english(),
        applies: Applies::NextSession,
        requires: None,
        required: false,
    },
    SettingSpec {
        key: "wifi_indicator",
        kind: SettingKind::Toggle { default: true },
        title: Msg::new("Correct the Wi-Fi icon in Steam").english(),
        help: Msg::new(
            "Steam otherwise shows the Wi-Fi as disconnected. Also lets the device button open \
             Steam's menus directly. Needs the debugging port.",
        )
        .english(),
        applies: Applies::NextSession,
        requires: Some("ui_link"),
        required: false,
    },
];

// A real section header: Steam is the default launcher, so its options are set in place.
const TEMPLATE: &str = r#"[launcher.steam]
# Steam Big Picture, kind = "steam" (the default). Its UI link is Steam's debugging port, which
# has no password (see SECURITY.md).
# ui_link = true            # through it: Home and Library, the "game is starting" screen, Wi-Fi
# wifi_indicator = true     # keep Big Picture's Wi-Fi icon correct; needs ui_link
"#;

/// `[launcher.steam]`, with the defaults where it says nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteamOptions {
    /// Steam's debugging port may be used: the marker that opens it is kept in place, and Big
    /// Picture's pages are reached through it. The user's consent, as SECURITY.md explains it.
    pub ui_link: bool,
    pub wifi_indicator: bool,
}

impl SteamOptions {
    pub fn read(options: &OptionTable) -> Self {
        Self {
            ui_link: schema::flag(SETTINGS, options, "ui_link"),
            wifi_indicator: schema::flag(SETTINGS, options, "wifi_indicator"),
        }
    }

    pub fn wifi_fix(self) -> bool {
        self.ui_link && self.wifi_indicator
    }
}

impl LauncherDescriptor for SteamDescriptor {
    fn id(&self) -> &'static str {
        "steam"
    }

    fn name(&self) -> &'static str {
        Msg::new("Steam Big Picture").english()
    }

    fn settings(&self) -> &'static [SettingSpec] {
        SETTINGS
    }

    fn template(&self) -> &'static str {
        TEMPLATE
    }

    /// Games are known through Steam's registry key, and the menus by their shortcuts, whatever
    /// the options; the pages only through the UI link.
    fn capabilities(&self, options: &OptionTable) -> LauncherCaps {
        LauncherCaps {
            navigation: SteamOptions::read(options).ui_link,
            ..LauncherCaps::ALL
        }
    }

    /// A tool that feeds Big Picture's Wi-Fi icon as Mujina does.
    fn conflicting_processes(&self) -> &'static [&'static str] {
        &["steamwififeeder.exe"]
    }

    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        &[("de", include_str!("../lang/de.po"))]
    }
}

#[cfg(test)]
mod tests {
    use mujina_application::settings::SettingValue;

    use super::*;

    #[test]
    fn the_descriptor_keeps_the_rules() {
        mujina_application::testing::conformance(&DESCRIPTOR);
    }

    fn options(pairs: &[(&str, bool)]) -> OptionTable {
        pairs
            .iter()
            .map(|(key, on)| ((*key).to_string(), SettingValue::Bool(*on)))
            .collect()
    }

    #[test]
    fn both_are_on_by_default() {
        let steam = SteamOptions::read(&OptionTable::new());
        assert!(steam.ui_link && steam.wifi_indicator && steam.wifi_fix());
    }

    #[test]
    fn the_wifi_fix_needs_the_ui_link() {
        let steam = SteamOptions::read(&options(&[("ui_link", false)]));
        assert!(!steam.ui_link);
        assert!(steam.wifi_indicator);
        assert!(!steam.wifi_fix());
        let steam = SteamOptions::read(&options(&[("wifi_indicator", false)]));
        assert!(steam.ui_link && !steam.wifi_fix());
    }

    #[test]
    fn without_the_ui_link_there_is_no_navigation_but_the_rest_stays() {
        let caps = DESCRIPTOR.capabilities(&options(&[("ui_link", false)]));
        assert!(!caps.navigation);
        assert!(caps.menu && caps.overlay && caps.game_detection);
        assert_eq!(
            DESCRIPTOR.capabilities(&OptionTable::new()),
            LauncherCaps::ALL
        );
    }
}

//! What a device button means in the current situation.

/// A device button, as its device numbers them. [`decide`] treats every button alike; the id is
/// carried so that per-button bindings can be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ButtonId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundRole {
    /// The launcher's own UI (e.g. Steam Big Picture).
    LauncherUi,
    /// A game the launcher started; or, while a game runs whose window and processes the launcher
    /// cannot find, a window for which [`WindowShape::looks_like_full_screen_game`] holds.
    Game,
    /// Anything else: the desktop, another app, a game the launcher cannot be tied to.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAction {
    /// Leave the button's default behaviour alone.
    Pass,
    /// Swallow the press: the launcher has nothing for it, and the button's own meaning (on a
    /// OneXPlayer, Show Desktop) would throw the user out of the launcher.
    Ignore,
    Menu,
    Overlay,
    /// Bring the launcher to the front, as the home button does.
    Home,
    ReturnToGame,
}

/// What a launcher offers that Mujina can use, as its descriptor reports it for its options.
// Independent abilities of a launcher, not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LauncherCaps {
    /// It tells when a game runs, and which window is the game's.
    pub game_detection: bool,
    /// It has a main menu the button can open, by a shortcut or directly.
    pub menu: bool,
    /// It has an overlay over games the button can open, by a shortcut or directly.
    pub overlay: bool,
    /// It can show the page Windows asks for (home, library).
    pub navigation: bool,
}

impl LauncherCaps {
    /// Everything: what Steam Big Picture offers.
    pub const ALL: Self = Self {
        game_detection: true,
        menu: true,
        overlay: true,
        navigation: true,
    };
}

/// How the window in front is shown: tells a full-screen game from other windows when the
/// launcher cannot tie the window to the game it runs.
// Independent facts about one window, not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowShape {
    /// It has a title bar or a sizing border, as windows of desktop apps have, maximised or not.
    pub framed: bool,
    /// Its rectangle is exactly its monitor's. A maximised window with a frame is larger: its
    /// borders lie off the screen.
    pub fills_monitor: bool,
    /// It is maximised: by the user, or by Xbox mode, which shows apps full screen.
    pub maximized: bool,
    /// The window manager hides it, although it counts as visible.
    pub cloaked: bool,
    /// It belongs to the Windows shell: the desktop, the taskbar, the task switcher.
    pub shell: bool,
    /// It is a packaged app's: the Xbox app, a Store app, Settings (in the frame Windows draws
    /// around such an app, too), Mujina's own windows. A game Steam starts is not.
    pub packaged: bool,
}

impl WindowShape {
    /// Games in full screen, borderless or exclusive, have this shape. A desktop app that goes
    /// full screen itself (a video, F11) is the one case this cannot tell from a game.
    ///
    /// Doubts count against a game: a wrong match sends a shortcut that does nothing visible,
    /// while a miss leads to the launcher, which shows the game. The exception: a process that
    /// cannot be opened counts as not packaged: a game's protected process may refuse to be opened.
    pub const fn looks_like_full_screen_game(self) -> bool {
        !self.framed
            && self.fills_monitor
            && !self.maximized
            && !self.cloaked
            && !self.shell
            && !self.packaged
    }
}

/// The decision table of the "home" style button.
///
/// The foreground wins over a running game: with the launcher UI in front, its menu is what the
/// user is looking at. On the desktop, with another window in front, the button keeps its own
/// meaning, a game running or not: nothing is sent into a window not known to be the game. A
/// missing menu or overlay never falls back to the button's own meaning (a device shortcut such
/// as Show Desktop would leave the launcher).
pub const fn decide(
    foreground: ForegroundRole,
    game_running: bool,
    console_experience: bool,
    caps: LauncherCaps,
) -> ButtonAction {
    match (foreground, game_running, console_experience) {
        (ForegroundRole::LauncherUi, _, _) if caps.menu => ButtonAction::Menu,
        (ForegroundRole::Game, _, _) if caps.overlay => ButtonAction::Overlay,
        (ForegroundRole::LauncherUi, _, _) | (ForegroundRole::Game, _, false) => {
            ButtonAction::Ignore
        }
        (ForegroundRole::Other, true, true) => ButtonAction::ReturnToGame,
        (ForegroundRole::Game | ForegroundRole::Other, _, true) => ButtonAction::Home,
        (ForegroundRole::Other, _, false) => ButtonAction::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_MENU: LauncherCaps = LauncherCaps {
        menu: false,
        ..LauncherCaps::ALL
    };
    const NO_OVERLAY: LauncherCaps = LauncherCaps {
        overlay: false,
        ..LauncherCaps::ALL
    };

    #[test]
    fn decision_table() {
        use ButtonAction::{Home, Menu, Overlay, Pass, ReturnToGame};
        use ForegroundRole::{Game, LauncherUi, Other};
        let all = LauncherCaps::ALL;

        for console in [false, true] {
            for game_running in [false, true] {
                assert_eq!(decide(LauncherUi, game_running, console, all), Menu);
                assert_eq!(decide(Game, game_running, console, all), Overlay);
            }
        }
        assert_eq!(decide(Other, true, true, all), ReturnToGame);
        // On the desktop, outside the game, the button keeps its own meaning.
        assert_eq!(decide(Other, true, false, all), Pass);
        assert_eq!(decide(Other, false, true, all), Home);
        assert_eq!(decide(Other, false, false, all), Pass);
    }

    #[test]
    fn without_a_menu_the_press_is_swallowed_not_passed_on() {
        use ButtonAction::{Home, Ignore, Overlay, Pass, ReturnToGame};
        use ForegroundRole::{Game, LauncherUi, Other};

        for console in [false, true] {
            for game_running in [false, true] {
                assert_eq!(decide(LauncherUi, game_running, console, NO_MENU), Ignore);
                assert_eq!(decide(Game, game_running, console, NO_MENU), Overlay);
            }
        }
        // Away from the launcher's UI, a missing menu changes nothing.
        assert_eq!(decide(Other, true, true, NO_MENU), ReturnToGame);
        assert_eq!(decide(Other, true, false, NO_MENU), Pass);
        assert_eq!(decide(Other, false, true, NO_MENU), Home);
        assert_eq!(decide(Other, false, false, NO_MENU), Pass);
    }

    #[test]
    fn without_an_overlay_the_button_leads_home_in_the_console_experience_only() {
        use ButtonAction::{Home, Ignore, Menu, Pass, ReturnToGame};
        use ForegroundRole::{Game, LauncherUi, Other};

        for game_running in [false, true] {
            assert_eq!(decide(Game, game_running, true, NO_OVERLAY), Home);
            assert_eq!(decide(Game, game_running, false, NO_OVERLAY), Ignore);
            assert_eq!(decide(LauncherUi, game_running, true, NO_OVERLAY), Menu);
        }
        assert_eq!(decide(Other, true, false, NO_OVERLAY), Pass);
        // Back to the game, or home, needs no overlay.
        assert_eq!(decide(Other, true, true, NO_OVERLAY), ReturnToGame);
        assert_eq!(decide(Other, false, true, NO_OVERLAY), Home);
        assert_eq!(decide(Other, false, false, NO_OVERLAY), Pass);
    }

    /// A game drawn borderless over its whole screen.
    const FULL_SCREEN: WindowShape = WindowShape {
        framed: false,
        fills_monitor: true,
        maximized: false,
        cloaked: false,
        shell: false,
        packaged: false,
    };

    #[test]
    fn only_a_frameless_window_filling_its_monitor_looks_like_a_game() {
        assert!(FULL_SCREEN.looks_like_full_screen_game());
        // A browser as Xbox mode shows it: framed and maximised, its borders off the screen.
        let browser = WindowShape {
            framed: true,
            fills_monitor: false,
            maximized: true,
            ..FULL_SCREEN
        };
        assert!(!browser.looks_like_full_screen_game());
        // Each doubt on its own is enough.
        for doubt in [
            WindowShape {
                framed: true,
                ..FULL_SCREEN
            },
            WindowShape {
                fills_monitor: false,
                ..FULL_SCREEN
            },
            WindowShape {
                maximized: true,
                ..FULL_SCREEN
            },
            WindowShape {
                cloaked: true,
                ..FULL_SCREEN
            },
            // The desktop covers the screen without a frame too.
            WindowShape {
                shell: true,
                ..FULL_SCREEN
            },
            WindowShape {
                packaged: true,
                ..FULL_SCREEN
            },
        ] {
            assert!(!doubt.looks_like_full_screen_game(), "{doubt:?}");
        }
        assert!(!WindowShape::default().looks_like_full_screen_game());
    }

    #[test]
    fn a_launcher_with_neither_never_passes_the_button_on_inside_it() {
        let neither = LauncherCaps::default();
        for console in [false, true] {
            for game_running in [false, true] {
                assert_eq!(
                    decide(ForegroundRole::LauncherUi, game_running, console, neither),
                    ButtonAction::Ignore
                );
            }
        }
    }
}

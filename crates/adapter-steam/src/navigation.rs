//! Showing a page of Big Picture through its own router, over the debugging port. Steam's
//! `steam://open/games` and its relatives belong to the desktop client and leave Big Picture
//! where it is.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::cdp::{self, CdpError, Session};
use crate::indicator::UiLink;

/// One of Big Picture's UIs whose side menu the device button opens and closes.
#[derive(Debug, Clone, Copy)]
pub enum MenuHost {
    BigPicture,
    /// The running game's overlay: a separate, hidden copy of Big Picture's UI that Steam draws
    /// over the game while one of its menus is open.
    GameOverlay,
}

/// Opens the host's main menu, or closes any open side menu, as Steam's own button does; `{host}`
/// is an expression for the UI instance. Close only through the menu store: taking the overlay
/// down by hand (its composition state) leaves the desktop in front of the game.
const TOGGLE_TEMPLATE: &str = "(function(){try{\
     var w={host};\
     if(!w)return 'not there';\
     var m=w.MenuStore;\
     if(!m||typeof m.OpenMainMenu!=='function'||typeof m.GetOpenSideMenu!=='function'\
     ||typeof m.CloseSideMenus!=='function')return 'missing';\
     if(m.GetOpenSideMenu()){m.CloseSideMenus();return 'closed';}\
     m.OpenMainMenu();return 'opened';\
     }catch(e){return String(e);}})()";

const BIG_PICTURE: &str = "SteamUIStore.WindowStore.GamepadUIMainWindowInstance";
/// The overlay of the game Steam counts as the main one; the only overlay if there is just one.
const GAME_OVERLAY: &str = "(function(){\
     var l=SteamUIStore.WindowStore.OverlayWindows||[];\
     var a=SteamUIStore.MainRunningApp&&SteamUIStore.MainRunningApp.appid;\
     return l.find(function(i){return a&&i.MainRunningAppID==a;})||(l.length==1?l[0]:null);\
     })()";

impl MenuHost {
    fn name(self) -> &'static str {
        match self {
            Self::BigPicture => "Big Picture's menu",
            Self::GameOverlay => "the in-game overlay",
        }
    }

    fn script(self) -> String {
        let host = match self {
            Self::BigPicture => BIG_PICTURE,
            Self::GameOverlay => GAME_OVERLAY,
        };
        TOGGLE_TEMPLATE.replace("{host}", host)
    }
}

/// Whether a host's menu functions exist, per session of the link. A Steam update may move them;
/// the device button then sends the shortcut until the next session, which every Steam start
/// brings.
#[derive(Debug, Default)]
struct HostHealth {
    /// The session in which the functions turned out to be missing; 0 for none.
    missing_in: AtomicU64,
    /// The session of a `TypeError` that no success has followed yet; 0 for none. One may come
    /// from a UI that is being reloaded, two in a row do not.
    type_error_in: AtomicU64,
}

/// What an answer of [`TOGGLE_TEMPLATE`] means for the device button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The menu opened or closed.
    Done,
    /// It did not, and will not in this session: from now on the shortcut.
    Missing,
    /// It did not, this time.
    Failed,
}

impl HostHealth {
    fn usable(&self, session: u64) -> bool {
        self.missing_in.load(Ordering::Relaxed) != session
    }

    fn record(&self, session: u64, answer: &str) -> Verdict {
        if answer == "opened" || answer == "closed" {
            self.type_error_in.store(0, Ordering::Relaxed);
            return Verdict::Done;
        }
        let missing = answer == "missing"
            || (answer.starts_with("TypeError")
                && self.type_error_in.swap(session, Ordering::Relaxed) == session);
        if missing {
            self.missing_in.store(session, Ordering::Relaxed);
            Verdict::Missing
        } else {
            Verdict::Failed
        }
    }
}

#[derive(Debug, Default)]
struct Health {
    big_picture: HostHealth,
    overlay: HostHealth,
}

impl Health {
    fn of(&self, host: MenuHost) -> &HostHealth {
        match host {
            MenuHost::BigPicture => &self.big_picture,
            MenuHost::GameOverlay => &self.overlay,
        }
    }
}

/// The device button's direct way to Big Picture's menus, over the link the Wi-Fi indicator
/// keeps.
pub struct DirectMenus {
    link: UiLink,
    /// Shared with the presses, which the worker carries out.
    health: Arc<Health>,
}

impl DirectMenus {
    pub fn new(link: UiLink) -> Self {
        Self {
            link,
            health: Arc::default(),
        }
    }

    /// Opens or closes the host's side menu through its menu store, which works wherever the
    /// keyboard focus is: after a programmatic bring-to-front, `Ctrl+1` goes nowhere. Returns at
    /// once whether the press was taken on (not while the link is down or the host's functions
    /// are missing); the link's worker carries it out, as the caller also runs the keyboard hook.
    pub fn toggle(&self, host: MenuHost) -> bool {
        let Some(session) = self.link.session() else {
            return false;
        };
        if !self.health.of(host).usable(session) {
            return false;
        }
        self.link.press(MenuPress {
            host,
            session,
            health: Arc::clone(&self.health),
        })
    }
}

/// A press of the device button that [`DirectMenus::toggle`] took on, for the worker that keeps
/// the link to carry out.
pub struct MenuPress {
    host: MenuHost,
    /// The link's session in which it was taken on.
    session: u64,
    health: Arc<Health>,
}

impl MenuPress {
    pub fn session(&self) -> u64 {
        self.session
    }

    /// Runs the host's script with `evaluate` and logs the outcome. `false` only when the link
    /// failed: the next press then sends the keyboard shortcut.
    pub fn carry_out(self, evaluate: impl FnOnce(&str) -> Result<Value, CdpError>) -> bool {
        let host = self.host;
        let answer = match evaluate(&host.script()) {
            Ok(answer) => answer.as_str().unwrap_or("no answer").to_string(),
            // The link works; the script did not.
            Err(error @ CdpError::Script(_)) => {
                log::warn!("{} not opened: {error}", host.name());
                return true;
            }
            Err(error) => {
                log::warn!(
                    "{} not opened: {error}; the next press sends the keyboard shortcut",
                    host.name()
                );
                return false;
            }
        };
        match self.health.of(host).record(self.session, &answer) {
            Verdict::Done => log::debug!("{} {answer}", host.name()),
            Verdict::Missing => log::warn!(
                "{} cannot be opened directly ({answer}); the device button sends the keyboard \
             shortcut until the link to Steam's UI is re-established",
                host.name()
            ),
            Verdict::Failed => log::warn!("{} not opened: {answer}", host.name()),
        }
        true
    }
}

/// Big Picture's start page.
pub const HOME_ROUTE: &str = "/library/home";
/// Big Picture's route of the user's games.
pub const LIBRARY_ROUTE: &str = "/library";

/// Right after a start the window is there before the router is.
pub const START_PATIENCE: Duration = Duration::from_secs(10);
const PAUSE: Duration = Duration::from_millis(250);

/// The answer of [`expression`] when the router took the route.
const DONE: &str = "ok";

/// Asks Big Picture's router for `route`. Every step is checked, as a Steam update may move
/// these internals: the answer then says what is missing.
fn expression(route: &str) -> String {
    format!(
        "(function(){{try{{\
         var s=window.SteamUIStore&&SteamUIStore.WindowStore;\
         var w=s&&s.GamepadUIMainWindowInstance;\
         if(!w)return 'Big Picture has no main window yet';\
         if(typeof w.Navigate!=='function')return 'Big Picture has no Navigate()';\
         w.Navigate('{route}');return '{DONE}';\
         }}catch(e){{return String(e);}}}})()"
    )
}

/// A new session with Steam's shared script context, where Big Picture's stores live.
pub fn shared_session(port: u16) -> Result<Session, CdpError> {
    let url = cdp::steam_ui(port)?
        .shared_context
        .ok_or_else(|| CdpError::new("Steam's script context is not there"))?;
    Session::connect(port, &url)
}

/// Runs `script` over a session of its own, which is closed again after it.
fn attempt(port: u16, script: &str) -> Result<String, CdpError> {
    let answer = shared_session(port)?.evaluate(script)?;
    Ok(answer.as_str().unwrap_or("no answer").to_string())
}

/// Shows `route` in Big Picture, which is on screen. Fails when Steam's UI debugging is off.
pub fn show(port: u16, route: &str, patience: Duration) -> Result<(), String> {
    let script = expression(route);
    let started = Instant::now();
    loop {
        let problem = match attempt(port, &script) {
            Ok(answer) if answer == DONE => return Ok(()),
            Ok(answer) => answer,
            Err(error) => error.to_string(),
        };
        if started.elapsed() >= patience {
            return Err(problem);
        }
        thread::sleep(PAUSE);
    }
}

/// Big Picture's own "a game is running" screen: a spinner on black.
const RUNNING_ROUTE: &str = "/apprunning";

/// How long after a game's start to step in. Big Picture shows its launch screen (the controller
/// layout) on top of whatever page it is on; this only settles what lies underneath.
const RUNNING_DELAY: Duration = Duration::from_secs(2);

/// Keeps Big Picture on its "a game is running" screen while a game loads, which may take 20 s
/// and more. Left alone, Big Picture sometimes stays on the game's page, where "Play" has turned
/// into "Continue" and the launch looks failed. Fire and forget, on its own thread: the caller
/// is the agent's event loop.
pub fn show_running_game(port: u16) {
    let spawned = thread::Builder::new()
        .name("steam-running-screen".into())
        .spawn(move || {
            thread::sleep(RUNNING_DELAY);
            let script = format!(
                "(function(){{try{{\
                 var w=SteamUIStore.WindowStore.GamepadUIMainWindowInstance;\
                 if(!w)return 'Big Picture is not up';\
                 if(!SteamUIStore.MainRunningApp)return 'no game is running any more';\
                 if(w.m_history.location.pathname!=='{RUNNING_ROUTE}')w.Navigate('{RUNNING_ROUTE}');\
                 return '{DONE}';\
                 }}catch(e){{return String(e);}}}})()"
            );
            match attempt(port, &script) {
                Ok(answer) if answer == DONE => {
                    log::debug!("Big Picture is on its running-game screen");
                }
                Ok(answer) => log::debug!("running-game screen not shown: {answer}"),
                Err(error) => log::debug!("running-game screen not shown: {error}"),
            }
        });
    if let Err(error) = spawned {
        log::debug!("running-game screen not shown: {error}");
    }
}

/// When to look at Big Picture after a game has ended.
const AFTER_GAME_SAMPLES: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// What Big Picture thinks of itself: its page, whether its document has the focus and is
/// visible, and whether its controller navigation is active.
const UI_STATE: &str = "(function(){try{\
     var w=SteamUIStore.WindowStore.GamepadUIMainWindowInstance;\
     if(!w)return 'Big Picture is not up';\
     var d=w.BrowserWindow.document;\
     var nav=w.FocusNavActiveSubscribableValue;\
     return JSON.stringify({page:w.m_history.location.pathname,hasFocus:d.hasFocus(),\
     visibility:d.visibilityState,navActive:nav?nav.Value:null,\
     running:SteamUIStore.MainRunningApp?SteamUIStore.MainRunningApp.appid:null});\
     }catch(e){return String(e);}})()";

/// Diagnostics for "after a game the controller does nothing until the screen is touched": logs,
/// a few times, where Windows sends input and Big Picture's own state. Only with debug logging.
pub fn log_state_after_game(port: u16) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let spawned = thread::Builder::new()
        .name("steam-after-game".into())
        .spawn(move || {
            for pause in AFTER_GAME_SAMPLES {
                thread::sleep(pause);
                let ui = attempt(port, UI_STATE).unwrap_or_else(|error| error.to_string());
                log::debug!(
                    "after the game: in front {}; input {}; Big Picture {ui}",
                    mujina_winutil::window::describe_foreground(),
                    mujina_winutil::window::describe_input_focus(),
                );
            }
        });
    if let Err(error) = spawned {
        log::debug!("no diagnostics after the game: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_expression_names_the_route_and_guards_every_step() {
        let script = expression(LIBRARY_ROUTE);
        assert!(script.contains("w.Navigate('/library')"));
        assert!(script.contains("try{"));
        assert!(script.contains("return 'ok'"));
    }

    #[test]
    fn missing_functions_count_for_their_session_only() {
        let health = HostHealth::default();
        assert_eq!(health.record(1, "opened"), Verdict::Done);
        assert_eq!(health.record(1, "not there"), Verdict::Failed);
        assert!(health.usable(1));

        assert_eq!(health.record(1, "missing"), Verdict::Missing);
        assert!(!health.usable(1));
        // Steam has started again, perhaps updated.
        assert!(health.usable(2));
    }

    #[test]
    fn a_type_error_counts_only_twice_in_a_row() {
        let health = HostHealth::default();
        let error = "TypeError: Cannot read properties of undefined";
        assert_eq!(health.record(1, error), Verdict::Failed);
        assert_eq!(health.record(1, "closed"), Verdict::Done);
        assert_eq!(health.record(1, error), Verdict::Failed);
        assert!(health.usable(1));
        assert_eq!(health.record(1, error), Verdict::Missing);
        assert!(!health.usable(1));

        // One from the last session does not count in the next.
        assert_eq!(health.record(2, error), Verdict::Failed);
        assert!(health.usable(2));
    }

    #[test]
    fn nothing_is_taken_on_without_a_live_link() {
        let menus = DirectMenus::new(UiLink::standing(None));
        assert!(!menus.toggle(MenuHost::BigPicture));
        assert!(!menus.toggle(MenuHost::GameOverlay));

        let menus = DirectMenus::new(UiLink::standing(Some(1)));
        menus.health.of(MenuHost::GameOverlay).record(1, "missing");
        assert!(!menus.toggle(MenuHost::GameOverlay));
    }

    fn press(host: MenuHost, health: &Arc<Health>) -> MenuPress {
        MenuPress {
            host,
            session: 1,
            health: Arc::clone(health),
        }
    }

    #[test]
    fn a_press_says_whether_the_link_held() {
        let health = Arc::default();
        let refused = |_: &str| Err(CdpError::new("debugging port: refused"));
        assert!(!press(MenuHost::BigPicture, &health).carry_out(refused));
        let threw = |_: &str| Err(CdpError::Script("TypeError".to_string()));
        assert!(
            press(MenuHost::BigPicture, &health).carry_out(threw),
            "the link works"
        );

        let mut ran = String::new();
        let opened = press(MenuHost::GameOverlay, &health).carry_out(|script| {
            ran = script.to_string();
            Ok(Value::from("opened"))
        });
        assert!(opened);
        assert!(ran.contains("OverlayWindows"), "the overlay's own script");
        let missing = |_: &str| Ok(Value::from("missing"));
        assert!(press(MenuHost::GameOverlay, &health).carry_out(missing));
        assert!(
            !health.overlay.usable(1),
            "the next press takes the shortcut"
        );
        assert!(health.big_picture.usable(1));
    }
}

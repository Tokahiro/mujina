//! Makes Big Picture's Wi-Fi icon tell the truth (ADR-0006): `assets/hook.js`, run in Steam's
//! shared JavaScript context, patches the missing access point into what the UI receives. A
//! worker keeps one debugging session per Steam run, because a script registered with
//! `Page.addScriptToEvaluateOnNewDocument` lives only as long as its session.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::cdp::{self, CdpError, Session};
use crate::navigation::{self, MenuPress};
use crate::wifi::WifiStatus;

/// Steam's fixed debugging port.
pub const STEAM_DEBUG_PORT: u16 = 8080;

/// A function of the hook's version, which is set here and nowhere else.
const HOOK_FUNCTION: &str = include_str!("../assets/hook.js");
/// This version of the hook; an older one in the same context cannot do without a reload.
const HOOK_VERSION: u32 = 3;
/// Whether waiting will do: Steam's UI is still loading, or it has the network store the hook
/// feeds when the UI subscribed before the hook arrived.
const HOOK_CAN_MANAGE: &str = "!!(document.readyState !== 'complete' || !window.SteamClient || \
     (window.SystemNetworkStore && \
     typeof window.SystemNetworkStore.OnNetworkDevicesChanged === 'function'))";
/// `true` once the hook is in place *and* data reaches the UI through it. Otherwise the error of
/// a hook that has stopped trying, if its last attempt threw one, else `false`.
const READINESS: &str = "!!(window.__steamWifiHooked && window.__steamWifiCb) || \
     (!window.__steamWifiWaiting && window.__steamWifiError) || false";

fn hook_source() -> String {
    format!("({HOOK_FUNCTION})({HOOK_VERSION})")
}

/// Whether this version of the hook is in place.
fn is_hooked() -> String {
    format!("window.__steamWifiHooked === {HOOK_VERSION}")
}

/// One round of attempts; it has to outlast a cold Steam start with an update check. The next
/// sign of life from Steam starts a new round.
const PATIENCE: Duration = Duration::from_secs(120);
/// Probe the port back to back this long into a round: Steam's script context appears then, and
/// every tenth of a second counts. A refused loopback connection is cheap.
const EAGER_PHASE: Duration = Duration::from_secs(30);
const EAGER_PAUSE: Duration = Duration::from_millis(100);
/// After the eager phase, pauses double from one second up to this; every sign of life from
/// Steam cuts the current pause short.
const MAX_PAUSE: Duration = Duration::from_secs(8);
/// How often readiness is checked once a session is open.
const READY_POLL: Duration = Duration::from_millis(250);
/// How long the UI gets to subscribe through a freshly injected hook before it is taken to have
/// subscribed earlier, and is reloaded. Short, because only an early reload is an invisible one.
const RELOAD_DELAY: Duration = Duration::from_millis(1500);
/// For this long after a reload, failing calls are expected and the session is kept.
const RELOAD_SETTLE: Duration = Duration::from_secs(10);
/// Never reload more often than this, whatever else goes wrong.
const RELOAD_INTERVAL: Duration = Duration::from_secs(60);
/// A link that answered this recently is not checked again.
const TRUST_PERIOD: Duration = Duration::from_secs(30);

enum Command {
    Connect,
    Show {
        ssid: String,
        bars: u8,
    },
    /// A press of the device button that was taken on while the link was up.
    Press(MenuPress),
}

/// The worker's front: cheap to clone, so that the WLAN worker can feed it too.
#[derive(Clone)]
pub struct SteamWifiIndicator {
    commands: Sender<Command>,
    link: Arc<AtomicU64>,
}

impl SteamWifiIndicator {
    /// Starts the worker thread.
    pub fn start(port: u16) -> Option<Self> {
        Self::start_with(port, RELOAD_DELAY)
    }

    fn start_with(port: u16, reload_delay: Duration) -> Option<Self> {
        let (commands, receiver) = channel();
        let link = Arc::new(AtomicU64::new(DOWN));
        let worker_link = Arc::clone(&link);
        thread::Builder::new()
            .name("steam-ui".into())
            .spawn(move || {
                Worker {
                    port,
                    reload_delay,
                    session: None,
                    sessions: 0,
                    link: worker_link,
                    shown: None,
                    logged_bars: None,
                    verified: None,
                    unsubscribed_since: None,
                    reloaded: false,
                    last_reload: None,
                    hook_error: None,
                    presses: None,
                }
                .run(&receiver);
            })
            .ok()?;
        Some(Self { commands, link })
    }

    /// The link to Steam's UI that the worker keeps, for the device button's menus.
    pub fn link(&self) -> UiLink {
        UiLink {
            up: Arc::clone(&self.link),
            commands: self.commands.clone(),
        }
    }
}

/// What [`UiLink`] holds while no session is up.
const DOWN: u64 = 0;

/// Whether Steam's UI can be reached, as the worker last found: up once its session is open with
/// the hook ready, down once the session is dropped. The worker looks only on signs of life from
/// Steam, so this may be hours old.
#[derive(Clone)]
pub struct UiLink {
    /// The number of the worker's session while it is up, [`DOWN`] otherwise. A new number means
    /// a new session: Steam, or the link to it, has started afresh.
    up: Arc<AtomicU64>,
    commands: Sender<Command>,
}

impl UiLink {
    pub fn session(&self) -> Option<u64> {
        match self.up.load(Ordering::Relaxed) {
            DOWN => None,
            session => Some(session),
        }
    }

    /// Queues a press for the worker. `false` when there is no worker to take it.
    pub fn press(&self, press: MenuPress) -> bool {
        self.commands.send(Command::Press(press)).is_ok()
    }

    /// A link as `session` says, with no worker behind it.
    #[cfg(test)]
    pub(crate) fn standing(session: Option<u64>) -> Self {
        let (commands, _) = channel();
        Self {
            up: Arc::new(AtomicU64::new(session.unwrap_or(DOWN))),
            commands,
        }
    }
}

impl SteamWifiIndicator {
    /// Steam showed a sign of life: the worker makes sure the link is up. Cheap when it is.
    pub fn connect(&self) {
        let _ = self.commands.send(Command::Connect);
    }

    /// Shows `status` in Big Picture's Wi-Fi icon, now or once the link is up.
    pub fn show(&self, status: &WifiStatus) {
        // While disconnected Steam's own report is correct; there is nothing to patch.
        if let WifiStatus::Connected { ssid, bars } = status {
            let _ = self.commands.send(Command::Show {
                ssid: ssid.clone(),
                bars: bars.get(),
            });
        }
    }
}

/// Where one step of the worker got to.
enum Progress {
    /// The UI has subscribed through the hook.
    Ready,
    Waiting,
    /// The hook failed and stopped trying. The UI's next document tries again, as does the next
    /// session.
    HookFailed,
}

struct Worker {
    port: u16,
    reload_delay: Duration,
    session: Option<Session>,
    /// How many sessions have been opened: the current one's number.
    sessions: u64,
    /// Shared with [`UiLink`].
    link: Arc<AtomicU64>,
    /// What the icon should show; pushed again whenever a new session is established.
    shown: Option<(String, u8)>,
    /// The bars last written to the log at info level.
    logged_bars: Option<u8>,
    /// When the session last proved ready.
    verified: Option<Instant>,
    /// Since when Big Picture has been up without having subscribed through the hook.
    unsubscribed_since: Option<Instant>,
    /// The one reload this session is allowed has been spent.
    reloaded: bool,
    /// When the UI was last reloaded, across sessions: the guard against reloading in a loop.
    last_reload: Option<Instant>,
    /// The hook's failure last written to the log, so that it is written once.
    hook_error: Option<String>,
    /// The session over which the device button's presses go, kept open between them.
    presses: Option<PressSession>,
}

/// The device button's own session: dropping it after a failed press never costs the hook's
/// session, on which the Wi-Fi fix depends.
struct PressSession {
    /// The number of the link's session it was opened in; another means Steam started afresh.
    opened_in: u64,
    session: Session,
}

/// A call in `session` failed although the link was up. It counts as down until the worker has
/// looked again. A newer session stays up: the failure was not about it.
fn lose(link: &AtomicU64, session: u64) {
    let _ = link.compare_exchange(session, DOWN, Ordering::Relaxed, Ordering::Relaxed);
}

impl Worker {
    fn run(mut self, commands: &Receiver<Command>) {
        while let Ok(command) = commands.recv() {
            let mut look = match command {
                Command::Connect => true,
                Command::Show { ssid, bars } => {
                    self.shown = Some((ssid, bars));
                    if self.verified.is_some() {
                        self.push();
                    }
                    false
                }
                // A press that found the link down has it looked at again.
                Command::Press(press) => !self.carry_out(press),
            };
            while look {
                self.ensure_ready(commands);
                look = false;
                // Connections that piled up meanwhile asked for the same thing; statuses and
                // presses are taken in order.
                while let Ok(queued) = commands.try_recv() {
                    match queued {
                        Command::Connect => {}
                        Command::Show { ssid, bars } => {
                            self.shown = Some((ssid, bars));
                            self.push();
                        }
                        Command::Press(press) => look |= !self.carry_out(press),
                    }
                }
            }
        }
    }

    /// Carries out a press over the presses' session, (re)opened as needed. `false` when the link
    /// turned out to be down; then only the presses' session is dropped, not the hook's.
    fn carry_out(&mut self, press: MenuPress) -> bool {
        let link_session = press.session();
        // Opened while an earlier link was up, so with a Steam that may be gone.
        if self
            .presses
            .as_ref()
            .is_some_and(|presses| presses.opened_in != link_session)
        {
            self.presses = None;
        }
        let (port, presses) = (self.port, &mut self.presses);
        let held = press.carry_out(|script| {
            if let Some(mut kept) = presses.take() {
                match kept.session.evaluate(script) {
                    // Steam may have closed it (its web helper restarted, say): retry over a new
                    // one. Not after a timeout: the script may have run, and a second run would
                    // close the menu again.
                    Err(CdpError::Link(error)) => {
                        log::debug!("the session kept for presses is gone ({error}); reopening");
                    }
                    answer => {
                        *presses = Some(kept);
                        return answer;
                    }
                }
            }
            let fresh = PressSession {
                opened_in: link_session,
                session: navigation::shared_session(port)?,
            };
            presses.insert(fresh).session.evaluate(script)
        });
        if !held {
            self.presses = None;
            lose(&self.link, link_session);
        }
        held
    }

    /// Works towards a live session through which the UI has subscribed. Called on every sign of
    /// life from Steam, which is also how a restarted Steam is noticed.
    fn ensure_ready(&mut self, commands: &Receiver<Command>) {
        // A link someone else found down is looked at again, however recently it answered.
        let up = self.link.load(Ordering::Relaxed) != DOWN;
        if self.session.is_some()
            && up
            && self.verified.is_some_and(|at| at.elapsed() < TRUST_PERIOD)
        {
            return;
        }
        let round = Instant::now();
        let give_up = round + PATIENCE;
        let mut pause = Duration::ZERO;
        while Instant::now() < give_up {
            if !self.pause(commands, pause) {
                return;
            }
            match self.advance() {
                Ok(Progress::Ready) => {
                    self.verified = Some(Instant::now());
                    self.link.store(self.sessions, Ordering::Relaxed);
                    self.hook_error = None;
                    self.push();
                    return;
                }
                Ok(Progress::Waiting) => {}
                // Logged where it was found; waiting out the round would change nothing.
                Ok(Progress::HookFailed) => return,
                Err(error) => {
                    // Without a session this is "Steam is not up yet", many times a second.
                    if self.session.is_some() {
                        log::debug!("Wi-Fi indicator: {error}");
                    }
                    // Right after a reload the script context is gone for a moment and calls fail.
                    // Dropping the session then would drop the hook registration with it.
                    let reloading = self
                        .last_reload
                        .is_some_and(|at| at.elapsed() < RELOAD_SETTLE);
                    if !reloading {
                        self.forget_session();
                    }
                }
            }
            pause = if self.session.is_some() {
                READY_POLL
            } else if round.elapsed() < EAGER_PHASE {
                EAGER_PAUSE
            } else {
                (pause * 2).clamp(Duration::from_secs(1), MAX_PAUSE)
            };
        }
        log::warn!(
            "Wi-Fi indicator: no link to Steam's UI; if Steam was started before Mujina created \
             its marker file, restart Steam once"
        );
    }

    fn forget_session(&mut self) {
        self.session = None;
        // Steam's end, most likely, which is the presses' session's end too.
        self.presses = None;
        self.link.store(DOWN, Ordering::Relaxed);
        self.verified = None;
        self.unsubscribed_since = None;
        self.reloaded = false;
    }

    /// Waits between attempts; a connect request ends the pause at once, and presses are carried
    /// out meanwhile. `false` when the agent is shutting down.
    fn pause(&mut self, commands: &Receiver<Command>, pause: Duration) -> bool {
        let deadline = Instant::now() + pause;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match commands.recv_timeout(left) {
                Ok(Command::Connect) | Err(RecvTimeoutError::Timeout) => return true,
                Ok(Command::Show { ssid, bars }) => self.shown = Some((ssid, bars)),
                // A press that finds the link down ends the pause, as a sign of life does.
                Ok(Command::Press(press)) => {
                    if !self.carry_out(press) {
                        return true;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    /// One step towards a UI that has subscribed through the hook.
    fn advance(&mut self) -> Result<Progress, CdpError> {
        if self.session.is_none() {
            let Some(url) = cdp::steam_ui(self.port)?.shared_context else {
                return Ok(Progress::Waiting);
            };
            let mut session = Session::connect(self.port, &url)?;
            // For every future document of this context (a new session always registers) ...
            session.call("Page.enable", &json!({}))?;
            session.call(
                "Page.addScriptToEvaluateOnNewDocument",
                &json!({ "source": hook_source() }),
            )?;
            // ... and for the current one right now. The hook itself refuses to run twice.
            let mut failure = None;
            if session.evaluate(&is_hooked())? != true {
                match session.evaluate(&hook_source()) {
                    Ok(_) => log::info!("Wi-Fi indicator: hook injected"),
                    // The link works, the hook does not. The session stays, and with it the
                    // registration for the UI's next document.
                    Err(CdpError::Script(error)) => failure = Some(error),
                    Err(error) => return Err(error),
                }
            }
            self.sessions += 1;
            self.session = Some(session);
            if let Some(error) = failure {
                self.hook_failed(error);
                return Ok(Progress::HookFailed);
            }
        }
        let Some(session) = &mut self.session else {
            return Ok(Progress::Waiting);
        };

        match session.evaluate(READINESS)? {
            Value::Bool(true) => return Ok(Progress::Ready),
            Value::String(error) => {
                self.hook_failed(error);
                return Ok(Progress::HookFailed);
            }
            _ => {}
        }
        // A UI that subscribed before the hook is fed through its network store by the hook.
        // Reloading is the last resort, for a UI that is up without such a store.
        let since = *self.unsubscribed_since.get_or_insert_with(Instant::now);
        let recently = self
            .last_reload
            .is_some_and(|at| at.elapsed() < RELOAD_INTERVAL);
        if !self.reloaded
            && !recently
            && since.elapsed() >= self.reload_delay
            && session.evaluate(HOOK_CAN_MANAGE)? != true
        {
            log::info!(
                "Wi-Fi indicator: Steam's UI has no network store to feed; reloading it so that \
                 it subscribes through the hook"
            );
            session.call("Page.reload", &json!({}))?;
            self.reloaded = true;
            self.last_reload = Some(Instant::now());
        }
        Ok(Progress::Waiting)
    }

    /// Logs what stopped the hook, once per distinct error: every sign of life from Steam asks
    /// again.
    fn hook_failed(&mut self, error: String) {
        if self.hook_error.as_ref() != Some(&error) {
            log::warn!("Wi-Fi indicator: the hook failed: {error}");
            self.hook_error = Some(error);
        }
    }

    fn push(&mut self) {
        let (Some(session), Some((ssid, bars))) = (&mut self.session, &self.shown) else {
            return;
        };
        let expression = format!(
            "window.__steamWifi={{ssid:{},strength:{bars}}};\
             window.__steamWifiRefire&&window.__steamWifiRefire();'ok'",
            json!(ssid)
        );
        match session.evaluate(&expression) {
            Ok(_) => {
                // The network name is location data, and logs end up in public bug reports:
                // only redacted, and only at debug level.
                log::debug!("Wi-Fi indicator: pushed {}, {bars} bars", redacted(ssid));
                if self.logged_bars != Some(*bars) {
                    log::info!("Wi-Fi indicator: {bars} bars");
                    self.logged_bars = Some(*bars);
                }
            }
            // Not the link's fault: a new session would not do any better.
            Err(error @ CdpError::Script(_)) => log::debug!("Wi-Fi indicator: {error}"),
            Err(error) => {
                log::debug!("Wi-Fi indicator: {error}");
                self.forget_session();
            }
        }
    }
}

/// A network name as the log may show it: the first character and the length, `H…(8)`. Enough
/// to see in a bug report that the network changed, not enough to name it.
fn redacted(ssid: &str) -> String {
    let first = ssid.chars().next().map(String::from).unwrap_or_default();
    format!("{first}\u{2026}({})", ssid.chars().count())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;

    use serde_json::Value;
    use tungstenite::Message;

    use super::*;
    use crate::navigation::{DirectMenus, MenuHost};
    use crate::wifi::SignalBars;

    fn serve_target_list(mut http: TcpStream, port: u16) {
        let mut request = [0u8; 1024];
        let _ = http.read(&mut request).unwrap();
        let body = json!([
            { "title": "Steam Big Picture Mode", "url": "https://steamloopback.host/?useragent=Valve%20Steam%20Gamepad" },
            { "title": "SharedJSContext", "url": "about:blank",
              "webSocketDebuggerUrl": format!("ws://localhost:{port}/devtools/page/ABC") }
        ])
        .to_string();
        write!(
            http,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    }

    /// A stand-in for Steam: serves the target list, and up to `sockets` websockets, numbered
    /// from 1, that answer each expression with `answer` and report each call to `seen` as
    /// `"<socket>: <method> <expression>"`. A socket hangs up, without a closing handshake,
    /// after an expression for which `hangs_up` is true.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "moved into the server thread"
    )]
    fn fake_steam(
        listener: TcpListener,
        answer: fn(&str) -> Value,
        seen: mpsc::Sender<String>,
        sockets: usize,
        hangs_up: fn(usize, &str) -> bool,
    ) {
        let port = listener.local_addr().unwrap().port();
        let mut opened = 0;
        while let Ok((stream, _)) = listener.accept() {
            // The request line tells a target list request from a websocket's handshake;
            // peeking leaves it for whoever answers.
            let mut start = [0u8; 9];
            let _ = stream.peek(&mut start);
            if start.starts_with(b"GET /json") {
                serve_target_list(stream, port);
                continue;
            }
            opened += 1;
            if opened > sockets {
                // Dropped before the handshake.
                continue;
            }
            let seen = seen.clone();
            thread::spawn(move || serve_socket(stream, opened, answer, hangs_up, &seen));
        }
    }

    fn serve_socket(
        stream: TcpStream,
        number: usize,
        answer: fn(&str) -> Value,
        hangs_up: fn(usize, &str) -> bool,
        seen: &mpsc::Sender<String>,
    ) {
        let mut socket = tungstenite::accept(stream).unwrap();
        while let Ok(message) = socket.read() {
            let Ok(text) = message.into_text() else {
                continue;
            };
            let Ok(request) = serde_json::from_str::<Value>(text.as_str()) else {
                continue;
            };
            let method = request["method"].as_str().unwrap_or_default();
            let expression = request["params"]["expression"].as_str().unwrap_or_default();
            let value = answer(expression);
            let reply = json!({ "id": request["id"], "result": { "result": { "value": value } } });
            socket.send(Message::text(reply.to_string())).unwrap();
            let _ = seen.send(format!("{number}: {method} {expression}"));
            if hangs_up(number, expression) {
                return;
            }
        }
    }

    /// For a fake Steam whose sockets stay open.
    fn never(_: usize, _: &str) -> bool {
        false
    }

    fn connected(ssid: &str, quality: u8) -> WifiStatus {
        WifiStatus::Connected {
            ssid: ssid.to_string(),
            bars: SignalBars::from_quality(quality),
        }
    }

    fn next_matching(seen: &mpsc::Receiver<String>, needle: &str) -> String {
        let wait = Duration::from_secs(20);
        loop {
            let line = seen.recv_timeout(wait).unwrap();
            if line.contains(needle) {
                return line;
            }
        }
    }

    #[test]
    fn network_names_are_redacted() {
        assert_eq!(redacted("HomeWLAN"), "H\u{2026}(8)");
        assert_eq!(redacted("caf\u{e9} \"5G\""), "c\u{2026}(9)");
        assert_eq!(redacted("\u{72e2}\u{306e}\u{5de3}"), "\u{72e2}\u{2026}(3)");
        assert_eq!(redacted("x"), "x\u{2026}(1)");
        assert_eq!(redacted(""), "\u{2026}(0)");
    }

    #[test]
    fn pushes_the_status_over_one_session_without_reloading() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_by_steam, seen) = mpsc::channel();
        thread::spawn(move || fake_steam(listener, |_| json!(true), seen_by_steam, 1, never));

        let indicator = SteamWifiIndicator::start(port).unwrap();
        assert_eq!(indicator.link().session(), None);
        indicator.show(&connected("caf\u{e9} \"5G\"", 80));
        indicator.connect();

        let push = next_matching(&seen, "ssid:");
        assert!(push.contains(r#"ssid:"café \"5G\"""#), "{push}");
        assert!(push.contains("strength:4"), "{push}");
        // Up before the first push.
        assert_eq!(indicator.link().session(), Some(1));

        // A second status goes over the same session: the fake accepts only one websocket.
        indicator.show(&connected("other", 30));
        let push = next_matching(&seen, "ssid:");
        assert!(
            push.starts_with("1: ")
                && push.contains(r#"ssid:"other""#)
                && push.contains("strength:2"),
            "{push}"
        );
    }

    #[test]
    fn injects_the_hook_and_reloads_only_after_the_delay() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_by_steam, seen) = mpsc::channel();
        // Never subscribed: Big Picture was there before the hook.
        thread::spawn(move || fake_steam(listener, |_| json!(false), seen_by_steam, 1, never));

        let started = Instant::now();
        let delay = Duration::from_secs(2);
        let indicator = SteamWifiIndicator::start_with(port, delay).unwrap();
        indicator.connect();

        next_matching(&seen, "Page.addScriptToEvaluateOnNewDocument");
        next_matching(&seen, &hook_source());
        next_matching(&seen, "Page.reload");
        assert!(started.elapsed() >= delay, "reloaded too early");
        assert_eq!(indicator.link().session(), None);
    }

    #[test]
    fn a_failed_hook_ends_the_round_until_steam_shows_signs_of_life_again() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_by_steam, seen) = mpsc::channel();
        let answer = |expression: &str| {
            if expression == READINESS {
                json!("TypeError: N.RegisterForDeviceChanges is undefined")
            } else {
                json!(false)
            }
        };
        thread::spawn(move || fake_steam(listener, answer, seen_by_steam, 1, never));

        let indicator = SteamWifiIndicator::start(port).unwrap();
        indicator.connect();
        next_matching(&seen, &hook_source());
        next_matching(&seen, READINESS);
        // No polling: a ready check every READY_POLL would have come by now.
        let quiet = seen.recv_timeout(READY_POLL * 4);
        assert!(quiet.is_err(), "{quiet:?}");
        assert_eq!(indicator.link().session(), None);

        // The next sign of life asks once more, and does not inject into the same document again.
        indicator.connect();
        let line = seen.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(line.ends_with(READINESS), "{line}");
    }

    #[test]
    fn a_lost_link_is_down_only_for_the_session_that_failed() {
        let link = AtomicU64::new(2);
        lose(&link, 1);
        assert_eq!(link.load(Ordering::Relaxed), 2);
        lose(&link, 2);
        assert_eq!(link.load(Ordering::Relaxed), DOWN);
    }

    /// Steam as the tests of presses need it: the menu stores open their menus, and the hook is
    /// ready at once.
    fn menus_answer(expression: &str) -> Value {
        if expression.contains("MenuStore") {
            json!("opened")
        } else {
            json!(true)
        }
    }

    /// A worker whose link is up, over a fake Steam that takes `sockets` websockets.
    fn linked(
        sockets: usize,
        hangs_up: fn(usize, &str) -> bool,
    ) -> (SteamWifiIndicator, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_by_steam, seen) = mpsc::channel();
        thread::spawn(move || fake_steam(listener, menus_answer, seen_by_steam, sockets, hangs_up));
        let indicator = SteamWifiIndicator::start(port).unwrap();
        indicator.show(&connected("home", 80));
        indicator.connect();
        // Pushed only once the link is up.
        next_matching(&seen, "ssid:");
        assert_eq!(indicator.link().session(), Some(1));
        (indicator, seen)
    }

    #[test]
    fn presses_go_one_after_the_other_over_a_session_of_their_own() {
        let (indicator, seen) = linked(2, never);
        let menus = DirectMenus::new(indicator.link());
        assert!(menus.toggle(MenuHost::BigPicture));
        assert!(menus.toggle(MenuHost::GameOverlay));
        assert!(menus.toggle(MenuHost::BigPicture));

        // In the order pressed, and all over the second websocket: the first is the hook's.
        let hosts = [
            "GamepadUIMainWindowInstance",
            "OverlayWindows",
            "GamepadUIMainWindowInstance",
        ];
        for host in hosts {
            let press = next_matching(&seen, "MenuStore");
            assert!(press.starts_with("2: ") && press.contains(host), "{press}");
        }
    }

    #[test]
    fn a_press_whose_kept_session_steam_closed_goes_over_a_new_one() {
        let (indicator, seen) = linked(3, |socket, expression| {
            socket == 2 && expression.contains("MenuStore")
        });
        let menus = DirectMenus::new(indicator.link());
        assert!(menus.toggle(MenuHost::BigPicture));
        let press = next_matching(&seen, "MenuStore");
        assert!(press.starts_with("2: "), "{press}");

        assert!(menus.toggle(MenuHost::GameOverlay));
        let press = next_matching(&seen, "MenuStore");
        assert!(
            press.starts_with("3: ") && press.contains("OverlayWindows"),
            "{press}"
        );
        assert!(menus.toggle(MenuHost::BigPicture));
        let press = next_matching(&seen, "MenuStore");
        assert!(press.starts_with("3: "), "kept in turn: {press}");
        assert_eq!(indicator.link().session(), Some(1), "never down");
    }

    #[test]
    fn a_press_that_fails_costs_the_link_for_a_moment_but_never_the_hook_session() {
        // The press's own session is refused.
        let (indicator, seen) = linked(1, never);
        let menus = DirectMenus::new(indicator.link());
        assert!(menus.toggle(MenuHost::BigPicture));

        // The worker looks at the link again, over the hook's session, which it kept; and the
        // hook is not registered anew.
        let looked = next_matching(&seen, READINESS);
        assert!(looked.starts_with("1: "), "{looked}");
        let pushed = next_matching(&seen, "ssid:");
        assert!(pushed.starts_with("1: "), "{pushed}");
        assert_eq!(
            indicator.link().session(),
            Some(1),
            "up again, the same session"
        );
    }

    #[test]
    fn the_hook_learns_its_version_from_here() {
        // A function expression the version is passed to: no statement of its own, so it can be
        // run in the same context any number of times.
        assert!(HOOK_FUNCTION.trim_end().ends_with("})"));
        assert!(!HOOK_FUNCTION.contains("__steamWifiHooked === 2"));
        assert!(hook_source().ends_with(&format!(")({HOOK_VERSION})")));
        assert_eq!(
            is_hooked(),
            format!("window.__steamWifiHooked === {HOOK_VERSION}")
        );
    }
}

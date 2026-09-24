//! The resident agent: reacts to what happens while the console experience is on.
//!
//! The agent is an event handler. An adapter-side event loop turns kernel events into
//! [`AgentEvent`]s and feeds them in one at a time; everything the agent does in response goes
//! out through ports. It never waits, sleeps or polls, so an idle system means an idle agent.

mod press;

use mujina_domain::button::{self, ButtonAction, ButtonId, ForegroundRole, WindowShape};
use mujina_domain::keys::{HoldTiming, KeyChord};
use mujina_domain::supervision::{self, ExitPolicy, LauncherExit};

pub use self::press::{GameSeen, InFront, Outcome, PressReport, Seen};
use crate::device::{DeviceSelection, Devices, Suppression};
use crate::launcher::{self, LauncherCaps, LauncherDescriptor, LauncherSelection, OptionTable};
use crate::ports::{
    DeviceButtons, Direct, ForegroundProbe, FseState, FullScreenExperience, GameWhereabouts,
    HomeActivator, KeySender, SessionLauncher,
};
use crate::settings::{Settings, SettingsSource};

/// Something happened that the agent may care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    /// The full screen experience was switched on or off.
    FseChanged(FseState),
    /// Another window came to the front. `None` when its process could not be identified.
    ForegroundChanged { process_name: Option<String> },
    /// The launcher's own state changed (game started or ended, launcher restarted, …).
    LauncherStateChanged,
    /// The home role just started the launcher, or switched it to its console UI.
    LauncherStarted,
    /// The session ends: sign-out, shutdown, or the Restart Manager closing the agent.
    SessionEnding,
    /// The launcher process ended.
    LauncherExited(LauncherExit),
    /// This button of the device was pressed.
    ButtonPressed(ButtonId),
    /// The configuration was changed and should be read again.
    SettingsChanged,
}

/// Whether the agent wants to keep running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

/// Everything the agent reaches the outside world through.
#[derive(Clone, Copy)]
pub struct AgentPorts<'a> {
    pub fse: &'a dyn FullScreenExperience,
    pub foreground: &'a dyn ForegroundProbe,
    pub launcher: &'a dyn SessionLauncher,
    /// What the running launcher is: what it offers, and which of its options apply at once.
    pub descriptor: &'a dyn LauncherDescriptor,
    /// The device's buttons as they run; they report presses through the event loop.
    pub buttons: &'a dyn DeviceButtons,
    /// The devices Mujina has, for what the running one's buttons are.
    pub devices: Devices,
    /// Sends the launcher's shortcuts, whichever device the button comes from.
    pub keys: &'a dyn KeySender,
    pub home: &'a dyn HomeActivator,
    /// Read again when the configuration changes.
    pub settings: &'a dyn SettingsSource,
}

/// The part of [`Settings`] the agent acts on, plus how it was started.
#[derive(Debug, Clone, Default)]
pub struct AgentSettings {
    /// Keep running on the desktop (development and troubleshooting).
    pub standalone: bool,
    /// The device whose buttons are mapped; none while the button is switched off.
    pub device: DeviceSelection,
    pub exit_policy: ExitPolicy,
    pub timing: HoldTiming,
    /// Used instead of the launcher's own menu shortcut.
    pub menu: Option<KeyChord>,
    /// Used instead of the launcher's own overlay shortcut.
    pub overlay: Option<KeyChord>,
    /// Tell the launcher when a game starts, so it can keep its "starting" screen up.
    pub game_start_screen: bool,
    /// The launcher's own options, as the configuration names them.
    pub launcher: LauncherSelection,
}

impl AgentSettings {
    pub fn from_settings(settings: &Settings, standalone: bool) -> Self {
        Self {
            standalone,
            device: settings.active_device(),
            exit_policy: settings.exit_policy,
            timing: settings.timing,
            menu: settings.menu,
            overlay: settings.overlay,
            game_start_screen: settings.game_start_screen,
            launcher: settings.launcher.clone(),
        }
    }
}

pub struct AgentService<'a> {
    ports: AgentPorts<'a>,
    settings: AgentSettings,
    /// The launcher's options as the session started with them. Those that wait for the next
    /// session stay so however the configuration changes.
    started: OptionTable,
    /// The launcher's options that apply at once, as it was last given them.
    applied_live: OptionTable,
    /// What the launcher offers with the options it runs with: as started, and again after each
    /// change to those that apply at once.
    caps: LauncherCaps,
    /// The device whose buttons run: as the session started with it, or as the device's adapter
    /// took a change over.
    device: DeviceSelection,
    game_running: bool,
    /// What the last button press came to, until it is taken for the log.
    last_press: Option<PressReport>,
}

impl<'a> AgentService<'a> {
    /// `settings.device` is the device the composition root started the buttons with.
    pub fn new(ports: AgentPorts<'a>, settings: AgentSettings) -> Self {
        let started = settings.launcher.options.clone();
        let applied_live = launcher::live_options(ports.descriptor.settings(), &started);
        let caps = ports.descriptor.capabilities(&started);
        let device = settings.device.clone();
        Self {
            ports,
            settings,
            started,
            applied_live,
            caps,
            device,
            game_running: false,
            last_press: None,
        }
    }

    /// Brings the agent's picture of the world up to date. Call once before the first event.
    ///
    /// What the button means is decided when it is pressed, from what is in front at that
    /// moment. Deciding ahead of time from foreground events proved fragile; behind the Windows
    /// welcome screen, for one, those events never arrive.
    pub fn start(&mut self) {
        self.game_running = self.ports.launcher.game_running();
    }

    pub fn handle(&mut self, event: &AgentEvent) -> Flow {
        let flow = self.react(event);
        // What a launcher does beside the ports (Steam keeps its link to Big Picture up) hears
        // of every event, once the agent has acted on it.
        self.ports.launcher.observe(event);
        flow
    }

    fn react(&mut self, event: &AgentEvent) -> Flow {
        match event {
            AgentEvent::SessionEnding => return Flow::Exit,
            // What is in front is looked at when the button is pressed, and the event loop
            // looks for the launcher's process itself: nothing to do here.
            AgentEvent::FseChanged(FseState::Active)
            | AgentEvent::ForegroundChanged { .. }
            | AgentEvent::LauncherStarted => {}
            AgentEvent::FseChanged(_) => {
                if !self.settings.standalone {
                    return Flow::Exit;
                }
            }
            AgentEvent::LauncherStateChanged => {
                let was_running = self.game_running;
                self.game_running = self.ports.launcher.game_running();
                if self.game_running && !was_running && self.settings.game_start_screen {
                    self.ports.launcher.game_started();
                }
                if was_running && !self.game_running {
                    self.ports.launcher.game_ended();
                }
            }
            AgentEvent::LauncherExited(exit) => self.supervise(*exit),
            AgentEvent::ButtonPressed(button) => self.press_button(*button),
            AgentEvent::SettingsChanged => self.reconfigure(),
        }
        Flow::Continue
    }

    /// Takes over what can change under a running session; what cannot (the launcher, those of
    /// its options that wait for the next session, and a device whose adapter is not the one
    /// running) stays as it was started.
    fn reconfigure(&mut self) {
        let loaded = self.ports.settings.load();
        let settings = AgentSettings::from_settings(&loaded.settings, self.settings.standalone);
        // Compared with what was asked last, so that a device that has to wait is not asked for
        // again on every change; going back to the one running is asked for, and taken.
        if settings.device != self.settings.device
            && self.ports.buttons.reconfigure(&settings.device)
        {
            self.device = settings.device.clone();
        }
        // Another launcher's options mean nothing to the one running. Compared with what it was
        // last given, not with the file before: that may have named another launcher meanwhile.
        let descriptor = self.ports.descriptor;
        if settings.launcher.id == descriptor.id() {
            let specs = descriptor.settings();
            let live = launcher::live_options(specs, &settings.launcher.options);
            if live != self.applied_live {
                self.ports.launcher.reconfigure(&live);
                // What it offers may follow such an option too, and the button goes by that.
                let running = launcher::with_live(specs, &self.started, &live);
                self.caps = descriptor.capabilities(&running);
                self.applied_live = live;
            }
        }
        self.settings = settings;
    }

    fn supervise(&self, exit: LauncherExit) {
        let active = self.ports.fse.state() == FseState::Active;
        if supervision::should_relaunch(self.settings.exit_policy, exit, active) {
            // Nothing to fall back to: the user still has the home button.
            let _ = self.ports.home.activate_home();
        }
    }

    /// What the launcher offers the button: its own menu and overlay, or a shortcut the user
    /// configured for one.
    fn offers(&self) -> LauncherCaps {
        LauncherCaps {
            menu: self.caps.menu || self.settings.menu.is_some(),
            overlay: self.caps.overlay || self.settings.overlay.is_some(),
            ..self.caps
        }
    }

    /// What is in front, and what of the launcher's game can be found. Looks, does not
    /// remember: a button press is rare, and a stale picture of the foreground makes the button
    /// do nothing at all.
    fn look(&mut self) -> Seen {
        let console = self.ports.fse.state() == FseState::Active;
        let in_front = self.ports.foreground.foreground_process();
        self.game_running = self.ports.launcher.game_running();
        let seen = |in_front, game| Seen {
            in_front,
            game,
            shape: None,
            console,
        };
        if in_front
            .as_deref()
            .is_some_and(|name| self.ports.launcher.owns_process(name))
        {
            let game = if self.game_running {
                GameSeen::Running
            } else {
                GameSeen::None
            };
            return seen(InFront::LauncherUi, game);
        }
        if !self.game_running {
            return seen(InFront::Other, GameSeen::None);
        }
        match self.ports.launcher.game_whereabouts() {
            GameWhereabouts::InFront => return seen(InFront::Game, GameSeen::InFront),
            GameWhereabouts::Behind => return seen(InFront::Other, GameSeen::Behind),
            // The launcher knows the game's processes, and the window in front is none of them.
            GameWhereabouts::NoWindow { known: true } => {
                return seen(InFront::Other, GameSeen::NoWindow);
            }
            // Nothing of the game runs any more, whatever the launcher still counts: a launcher
            // may keep counting a game as running while something it started is still open.
            GameWhereabouts::Gone => return seen(InFront::Other, GameSeen::Gone),
            GameWhereabouts::NoWindow { known: false } => {}
        }
        // A game runs, the launcher says, but it cannot tell which processes are the game's and
        // finds no window of it. It may have ended while something it started still counts as
        // it, or it is a game the launcher cannot be tied to (one started through a launcher of
        // its own), which may well be the window in front. Only a window shaped as a game in
        // full screen is taken for it: anything else may be any app, and the overlay's shortcut
        // would do nothing visible there.
        let shape = self.ports.foreground.foreground_shape();
        let in_front = if shape.is_some_and(WindowShape::looks_like_full_screen_game) {
            InFront::TakenForGame
        } else {
            InFront::Other
        };
        Seen {
            shape,
            ..seen(in_front, GameSeen::NotFound)
        }
    }

    /// Whether `button` of the running device is swallowed, and so has to be passed on where
    /// Mujina has nothing for it. One the device only observes did its own thing already.
    fn swallowed(&self, button: ButtonId) -> bool {
        let running = self.device.id.as_deref();
        let spec = running
            .and_then(|id| self.ports.devices.find(id))
            .and_then(|device| device.buttons().into_iter().find(|spec| spec.id == button));
        spec.is_none_or(|spec| spec.suppression == Suppression::Swallowed)
    }

    /// What the last button press came to, once: for the composition root to log.
    pub fn take_press_report(&mut self) -> Option<PressReport> {
        self.last_press.take()
    }

    fn press_button(&mut self, button: ButtonId) {
        // Switched off, or no device: a press that still arrives (from a device that only
        // observes its buttons) means nothing.
        let report = if self.device.is_none() {
            PressReport {
                seen: None,
                outcome: Outcome::Off,
            }
        } else {
            let seen = self.look();
            PressReport {
                seen: Some(seen),
                outcome: self.carry_out(button, &seen),
            }
        };
        self.last_press = Some(report);
    }

    fn carry_out(&self, button: ButtonId, seen: &Seen) -> Outcome {
        let foreground = match seen.in_front {
            InFront::LauncherUi => ForegroundRole::LauncherUi,
            InFront::Game | InFront::TakenForGame => ForegroundRole::Game,
            InFront::Other => ForegroundRole::Other,
        };
        // A game that has ended counts as none, whatever the launcher still says.
        let action = button::decide(foreground, seen.game.running(), seen.console, self.offers());
        match action {
            // The launcher's own way first: its shortcut only works while the right part of it
            // has the keyboard focus, which it may not have after being brought to the front. A
            // menu shortcut the user configured is theirs, though.
            ButtonAction::Menu
                if self.settings.menu.is_none()
                    && self.ports.launcher.open_menu() == Direct::Taken =>
            {
                Outcome::Menu { direct: true }
            }
            ButtonAction::Menu => {
                let chord = self
                    .settings
                    .menu
                    .or_else(|| self.ports.launcher.menu_shortcut());
                self.send(chord, Outcome::Menu { direct: false })
            }
            // The launcher's own way, as for the menu, and without a keystroke into the game;
            // only where the launcher itself knows the game to be the window in front, since
            // that is where it draws its overlay.
            ButtonAction::Overlay
                if seen.in_front == InFront::Game
                    && self.settings.overlay.is_none()
                    && self.ports.launcher.open_overlay() == Direct::Taken =>
            {
                Outcome::Overlay { direct: true }
            }
            // Into the window in front: the game the launcher recognises, or one taken for the
            // game by its shape.
            ButtonAction::Overlay => {
                let chord = self
                    .settings
                    .overlay
                    .or_else(|| self.ports.launcher.overlay_shortcut());
                match chord {
                    // A window only taken for the game has no way into the overlay but the
                    // shortcut. Without one the press would do nothing: the button leads where it
                    // does away from a game instead.
                    None if seen.in_front == InFront::TakenForGame => {
                        if seen.console {
                            self.home()
                        } else {
                            self.pass(button)
                        }
                    }
                    chord => self.send(chord, Outcome::Overlay { direct: false }),
                }
            }
            // Never a shortcut into the window in front, which is not the game: back to the
            // game where its window was found, else to the launcher, which shows the game if
            // there still is one. The home role does the same when the window is gone by then.
            ButtonAction::ReturnToGame
                if seen.game == GameSeen::Behind && self.ports.home.activate_game().is_ok() =>
            {
                Outcome::BackToGame
            }
            ButtonAction::ReturnToGame | ButtonAction::Home => self.home(),
            ButtonAction::Pass => self.pass(button),
            // The launcher has nothing for it, and what the button does by itself would lead
            // away from the launcher.
            ButtonAction::Ignore => Outcome::Swallowed,
        }
    }

    /// Sends `chord` as `sent` says; a launcher that opens a menu only directly has no shortcut
    /// to fall back on.
    fn send(&self, chord: Option<KeyChord>, sent: Outcome) -> Outcome {
        match chord {
            Some(chord) => {
                self.ports.keys.send_chord(chord, self.settings.timing);
                sent
            }
            None => Outcome::NothingToSend,
        }
    }

    fn home(&self) -> Outcome {
        // Nothing to fall back to if this fails; the adapter logs why.
        Outcome::Home {
            failed: self.ports.home.activate_home().is_err(),
        }
    }

    /// Not ours to interpret here: the button does what it always did.
    fn pass(&self, button: ButtonId) -> Outcome {
        if self.swallowed(button) {
            self.ports.buttons.pass_on(button);
            Outcome::PassedOn
        } else {
            Outcome::LeftToDevice
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::OptionTable;
    use crate::ports::LauncherState;
    use crate::ports::absent::Absent;
    use crate::settings::SettingValue;
    use crate::settings::schema::{Applies, SettingKind, SettingSpec};
    use crate::testing::{
        FakeDevice, FakeForeground, FakeFse, FakeHomeActivator, FakeInput, FakeLauncher,
        FakeLauncherDescriptor, FakeSettingsSource,
    };
    use mujina_domain::keys::VirtualKey;

    /// A launcher with everything, one option that applies at once and one that waits.
    static FAKE: FakeLauncherDescriptor = FakeLauncherDescriptor {
        settings: &[
            SettingSpec {
                key: "start_screen",
                kind: SettingKind::Toggle { default: true },
                title: "Start screen",
                help: "",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "link",
                kind: SettingKind::Toggle { default: true },
                title: "Link",
                help: "",
                applies: Applies::NextSession,
                requires: None,
                required: false,
            },
        ],
        template: "# [launcher.fake]\n# start_screen = true\n# link = true\n",
        ..FakeLauncherDescriptor::named("fake", "Fake", LauncherCaps::ALL)
    };

    /// A launcher with neither a menu nor an overlay, which cannot tell games either.
    static BARE: FakeLauncherDescriptor = FakeLauncherDescriptor::named(
        "bare",
        "Bare",
        LauncherCaps {
            game_detection: false,
            menu: false,
            overlay: false,
            navigation: false,
        },
    );

    struct Rig {
        fse: FakeFse,
        foreground: FakeForeground,
        launcher: FakeLauncher,
        descriptor: &'static dyn LauncherDescriptor,
        input: FakeInput,
        home: FakeHomeActivator,
        settings: FakeSettingsSource,
    }

    impl Rig {
        fn new() -> Self {
            Self {
                fse: FakeFse(FseState::Active),
                foreground: FakeForeground::default(),
                launcher: FakeLauncher::installed(LauncherState::UiVisible),
                descriptor: &FAKE,
                input: FakeInput::default(),
                home: FakeHomeActivator::default(),
                settings: FakeSettingsSource::default(),
            }
        }

        /// A launcher as [`BARE`] says: no shortcuts, nothing it opens itself.
        fn bare() -> Self {
            let rig = Self {
                descriptor: &BARE,
                ..Self::new()
            };
            rig.launcher.menu_shortcut.set(None);
            rig.launcher.overlay_shortcut.set(None);
            rig
        }

        fn ports(&self) -> AgentPorts<'_> {
            AgentPorts {
                fse: &self.fse,
                foreground: &self.foreground,
                launcher: &self.launcher,
                descriptor: self.descriptor,
                buttons: &self.input,
                devices: DEVICES,
                keys: &self.input,
                home: &self.home,
                settings: &self.settings,
            }
        }

        fn service(&self, standalone: bool) -> AgentService<'_> {
            let settings = AgentSettings {
                standalone,
                device: device("keys"),
                game_start_screen: true,
                launcher: selection(self.descriptor.id(), &[]),
                ..running()
            };
            AgentService::new(self.ports(), settings)
        }
    }

    fn selection(id: &str, options: &[(&str, bool)]) -> LauncherSelection {
        LauncherSelection {
            id: id.to_string(),
            options: options
                .iter()
                .map(|(key, on)| ((*key).to_string(), SettingValue::Bool(*on)))
                .collect(),
        }
    }

    /// A press of the first button.
    const PRESS: AgentEvent = AgentEvent::ButtonPressed(ButtonId(0));

    /// A device whose button is swallowed and sent on where Mujina has nothing for it, as the
    /// OneXPlayer's chord is.
    static KEYS: FakeDevice = FakeDevice::named("keys", "Keys");
    /// A device whose own software sees its button too, as a vendor HID report is seen.
    static HID: FakeDevice = FakeDevice {
        buttons: &[(1, "armoury", Suppression::Observed)],
        ..FakeDevice::named("hid", "Hid")
    };
    static DEVICES: Devices = Devices {
        all: &[&KEYS, &HID],
        own: &KEYS,
    };

    fn device(id: &str) -> DeviceSelection {
        DeviceSelection {
            id: Some(id.to_string()),
            ..DeviceSelection::none()
        }
    }

    /// The configuration as the tests change it: with the device the rig runs, unless the test
    /// says otherwise.
    fn configured() -> Settings {
        Settings {
            device: device("keys"),
            ..Settings::default()
        }
    }

    /// How the rig's agent is started: with the device of [`configured`].
    fn running() -> AgentSettings {
        AgentSettings::from_settings(&configured(), false)
    }

    fn foreground(name: &str) -> AgentEvent {
        AgentEvent::ForegroundChanged {
            process_name: Some(name.to_string()),
        }
    }

    #[test]
    fn the_launcher_opens_its_menu_itself_where_it_can() {
        let rig = Rig::new();
        rig.launcher.direct_menu.set(true);
        let mut agent = rig.service(false);
        agent.start();
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert!(rig.launcher.calls().contains(&"open_menu"));
        assert!(rig.input.sent().is_empty(), "no shortcut on top");
    }

    #[test]
    fn a_menu_shortcut_of_the_users_own_is_respected() {
        let rig = Rig::new();
        rig.launcher.direct_menu.set(true);
        let own = KeyChord::pair(VirtualKey::LCONTROL, VirtualKey::DIGIT_1);
        let settings = AgentSettings {
            menu: Some(own),
            ..running()
        };
        let mut agent = AgentService::new(rig.ports(), settings);
        agent.start();
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert!(!rig.launcher.calls().contains(&"open_menu"));
        assert_eq!(rig.input.sent(), [own]);
    }

    #[test]
    fn button_opens_the_menu_in_the_launcher_ui() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();

        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [FakeLauncher::MENU]);
    }

    #[test]
    fn button_works_without_ever_having_seen_a_foreground_event() {
        // Booting into the console experience: the launcher comes to the front behind the
        // welcome screen and no foreground event is delivered.
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        agent.handle(&foreground("logonui.exe"));

        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [FakeLauncher::MENU]);
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

    /// A desktop app maximised, as a maximised browser was read on the desktop: framed, its
    /// borders off the screen. That Xbox mode shows desktop apps so too is an assumption (not
    /// verified; see the on-device checklist); it matters only where the launcher does not know
    /// the game's processes.
    const MAXIMISED_APP: WindowShape = WindowShape {
        framed: true,
        fills_monitor: false,
        maximized: true,
        ..FULL_SCREEN
    };

    #[test]
    fn a_game_that_has_ended_leads_to_the_launcher_not_a_keystroke_into_the_app_in_front() {
        // The game has ended, but a browser it opened keeps it running for the launcher, and the
        // browser is in front. The overlay's shortcut used to go into the browser, where it did
        // nothing visible. However the browser is shown: frameless over its screen even.
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_gone.set(true);
        rig.foreground.set(Some("browser.exe"));
        rig.foreground.shape.set(Some(FULL_SCREEN));

        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty(), "no keystroke into the browser");
        assert_eq!(rig.home.activations.get(), 1);
        assert_eq!(rig.home.game_activations.get(), 0);
        assert_eq!(rig.foreground.shape_asked.get(), 0, "no guess needed");
        let report = agent.take_press_report().unwrap();
        assert_eq!(report.seen.unwrap().game, GameSeen::Gone);

        // The ended game does not end the launcher's picture of it: no "game ended" on a press.
        assert!(!rig.launcher.calls().contains(&"game_ended"));
    }

    #[test]
    fn a_game_that_has_ended_leaves_the_button_its_own_meaning_on_the_desktop() {
        let rig = Rig {
            fse: FakeFse(FseState::Inactive),
            ..Rig::new()
        };
        let mut agent = rig.service(true);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_gone.set(true);
        rig.foreground.set(Some("browser.exe"));

        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty());
        assert_eq!(rig.input.passed_on(), [ButtonId(0)]);
        assert_eq!(rig.home.activations.get(), 0);
    }

    #[test]
    fn a_window_of_none_of_the_game_s_known_processes_is_not_taken_for_it() {
        // The game runs without a window yet (or with none to be found), and the launcher knows
        // its processes: whatever is in front is not the game, however it is shown.
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_known.set(true);
        rig.foreground.set(Some("player.exe"));
        rig.foreground.shape.set(Some(FULL_SCREEN));

        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty());
        assert_eq!(rig.home.activations.get(), 1);
        assert_eq!(rig.foreground.shape_asked.get(), 0);
    }

    #[test]
    fn a_game_whose_window_is_not_found_leads_to_the_launcher_unless_it_may_be_in_front() {
        // The launcher cannot tell the game's processes and finds no window of it; a maximised
        // app in front is not the game.
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.foreground.set(Some("browser.exe"));
        rig.foreground.shape.set(Some(MAXIMISED_APP));

        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty(), "no keystroke into the browser");
        assert_eq!(rig.home.activations.get(), 1);
        assert_eq!(rig.home.game_activations.get(), 0);

        // A window whose shape could not be read is no game either.
        rig.foreground.shape.set(None);
        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty());
        assert_eq!(rig.home.activations.get(), 2);
    }

    #[test]
    fn a_window_taken_for_the_game_without_an_overlay_shortcut_leads_to_the_launcher() {
        // A launcher whose overlay opens only its own way, which is not taken for a window that
        // is only taken for the game: nothing would happen.
        let rig = Rig::new();
        rig.launcher.overlay_shortcut.set(None);
        rig.launcher.direct_overlay.set(true);
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.foreground.set(Some("game.exe"));
        rig.foreground.shape.set(Some(FULL_SCREEN));

        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty());
        assert!(!rig.launcher.calls().contains(&"open_overlay"));
        assert_eq!(rig.home.activations.get(), 1);
        assert_eq!(
            agent.take_press_report().unwrap().outcome,
            Outcome::Home { failed: false }
        );
    }

    #[test]
    fn a_full_screen_window_is_taken_for_a_game_the_launcher_cannot_tie_to_itself() {
        // A game started through a launcher of its own (EA, Ubisoft): the launcher runs it, but
        // finds no window of it. A borderless window over the whole screen is taken for it.
        let rig = Rig::new();
        rig.launcher.direct_overlay.set(true);
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.foreground.set(Some("game.exe"));
        rig.foreground.shape.set(Some(FULL_SCREEN));

        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [FakeLauncher::OVERLAY]);
        assert!(
            !rig.launcher.calls().contains(&"open_overlay"),
            "the launcher's own way is for a game it recognises"
        );
        assert_eq!(rig.home.activations.get(), 0);
    }

    #[test]
    fn the_shape_of_the_window_in_front_is_asked_only_while_the_game_cannot_be_found() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.foreground.shape.set(Some(FULL_SCREEN));
        // No game: home, however the window looks.
        rig.foreground.set(Some("player.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.home.activations.get(), 1);
        // A game behind a full-screen window: back to the game, not the overlay over the other.
        rig.launcher.set_game_running(true);
        rig.launcher.game_in_front.set(Some(false));
        agent.handle(&PRESS);
        assert_eq!(rig.home.game_activations.get(), 1);
        // In the launcher's UI, and in a game it recognises.
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        rig.launcher.game_in_front.set(Some(true));
        rig.foreground.set(Some("game.exe"));
        agent.handle(&PRESS);
        assert_eq!(
            rig.input.sent(),
            [FakeLauncher::MENU, FakeLauncher::OVERLAY]
        );
        // Where the launcher knows the game's processes, or knows that none runs.
        rig.foreground.set(Some("player.exe"));
        rig.launcher.game_in_front.set(None);
        rig.launcher.game_known.set(true);
        agent.handle(&PRESS);
        rig.launcher.game_gone.set(true);
        agent.handle(&PRESS);
        assert_eq!(rig.home.activations.get(), 3);
        assert_eq!(rig.foreground.shape_asked.get(), 0);
    }

    #[test]
    fn a_refused_way_back_to_the_game_leads_to_the_launcher() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_in_front.set(Some(false));
        rig.home.refuse_game.set(true);
        rig.foreground.set(Some("xboxpcapp.exe"));

        agent.handle(&PRESS);
        assert_eq!(rig.home.game_activations.get(), 1);
        assert_eq!(rig.home.activations.get(), 1);
        assert!(
            rig.input.sent().is_empty(),
            "no keystroke into the app in front"
        );
    }

    #[test]
    fn every_press_says_what_it_came_to() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        assert_eq!(agent.take_press_report(), None);
        rig.launcher.set_game_running(true);
        rig.foreground.set(Some("browser.exe"));
        rig.foreground.shape.set(Some(MAXIMISED_APP));
        agent.handle(&PRESS);
        let report = agent.take_press_report().unwrap();
        assert_eq!(report.outcome, Outcome::Home { failed: false });
        assert_eq!(
            report.describe("Fake Launcher"),
            "Fake Launcher (in front: another app, framed, not exactly its screen's size, \
             maximised; a game runs but its window cannot be found; Xbox mode)"
        );
        assert_eq!(agent.take_press_report(), None, "once");

        // Other events say nothing.
        agent.handle(&AgentEvent::LauncherStateChanged);
        assert_eq!(agent.take_press_report(), None);

        // Switched off, the press is still reported.
        rig.settings.set(Settings {
            button_remap: false,
            ..configured()
        });
        agent.handle(&AgentEvent::SettingsChanged);
        agent.handle(&PRESS);
        assert_eq!(
            agent.take_press_report(),
            Some(PressReport {
                seen: None,
                outcome: Outcome::Off
            })
        );
    }

    #[test]
    fn button_opens_the_overlay_in_a_game_the_launcher_recognises() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_in_front.set(Some(true));
        rig.foreground.set(Some("game.exe"));

        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [FakeLauncher::OVERLAY]);
        assert_eq!(rig.home.game_activations.get(), 0);
    }

    #[test]
    fn the_launcher_opens_its_overlay_itself_over_a_game_it_recognises() {
        let rig = Rig::new();
        rig.launcher.direct_overlay.set(true);
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_in_front.set(Some(true));
        rig.foreground.set(Some("game.exe"));

        agent.handle(&PRESS);
        assert!(rig.launcher.calls().contains(&"open_overlay"));
        assert!(rig.input.sent().is_empty(), "no keystroke into the game");
    }

    #[test]
    fn on_the_desktop_a_window_that_is_not_the_game_gets_no_overlay_while_a_game_runs() {
        // It used to get the overlay's shortcut, where it did nothing visible; the button keeps
        // its own meaning there now, as without a game. The game itself still gets the overlay.
        let rig = Rig {
            fse: FakeFse(FseState::Inactive),
            ..Rig::new()
        };
        rig.launcher.direct_overlay.set(true);
        let mut agent = rig.service(true);
        agent.start();
        rig.launcher.set_game_running(true);
        for behind in [None, Some(false)] {
            rig.launcher.game_in_front.set(behind);
            rig.foreground.set(Some("unknown.exe"));
            agent.handle(&PRESS);
        }
        assert!(!rig.launcher.calls().contains(&"open_overlay"));
        assert!(rig.input.sent().is_empty());
        assert_eq!(rig.input.passed_on(), [ButtonId(0), ButtonId(0)]);
        assert_eq!(
            rig.home.activations.get() + rig.home.game_activations.get(),
            0
        );

        rig.launcher.game_in_front.set(Some(true));
        rig.foreground.set(Some("game.exe"));
        agent.handle(&PRESS);
        assert!(rig.launcher.calls().contains(&"open_overlay"));
        // A window taken for the game by its shape gets the overlay's shortcut.
        rig.launcher.game_in_front.set(None);
        rig.foreground.shape.set(Some(FULL_SCREEN));
        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [FakeLauncher::OVERLAY]);
    }

    #[test]
    fn button_leads_back_to_the_game_behind_something_else() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_in_front.set(Some(false));
        rig.foreground.set(Some("xboxpcapp.exe"));

        agent.handle(&PRESS);
        assert_eq!(rig.home.game_activations.get(), 1);
        assert!(rig.input.sent().is_empty());
        assert_eq!(rig.home.activations.get(), 0);
    }

    #[test]
    fn button_leads_back_to_the_launcher_from_anywhere_else() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        rig.foreground.set(Some("xboxpcapp.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.home.activations.get(), 1);
        assert!(rig.input.sent().is_empty());
        assert!(rig.input.passed_on().is_empty());
    }

    #[test]
    fn button_keeps_its_own_meaning_on_the_desktop() {
        let rig = Rig {
            fse: FakeFse(FseState::Inactive),
            ..Rig::new()
        };
        let mut agent = rig.service(true);
        agent.start();
        rig.foreground.set(Some("explorer.exe"));
        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty());
        assert_eq!(rig.input.passed_on(), [ButtonId(0)]);
        assert_eq!(rig.home.activations.get(), 0);
    }

    #[test]
    fn the_launcher_is_told_once_when_a_game_starts() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        let told = || {
            rig.launcher
                .calls()
                .iter()
                .filter(|call| **call == "game_started")
                .count()
        };

        agent.handle(&AgentEvent::LauncherStateChanged);
        assert_eq!(told(), 0, "no game yet");
        rig.launcher.set_game_running(true);
        agent.handle(&AgentEvent::LauncherStateChanged);
        agent.handle(&AgentEvent::LauncherStateChanged);
        assert_eq!(told(), 1, "once per start, not per state change");
        rig.launcher.set_game_running(false);
        agent.handle(&AgentEvent::LauncherStateChanged);
        rig.launcher.set_game_running(true);
        agent.handle(&AgentEvent::LauncherStateChanged);
        assert_eq!(told(), 2);
    }

    #[test]
    fn leaves_when_the_session_ends_even_standalone() {
        let rig = Rig::new();
        assert_eq!(
            rig.service(true).handle(&AgentEvent::SessionEnding),
            Flow::Exit
        );
    }

    #[test]
    fn leaves_with_the_full_screen_experience_unless_standalone() {
        let rig = Rig::new();
        assert_eq!(
            rig.service(false)
                .handle(&AgentEvent::FseChanged(FseState::Active)),
            Flow::Continue
        );
        assert_eq!(
            rig.service(false)
                .handle(&AgentEvent::FseChanged(FseState::Inactive)),
            Flow::Exit
        );
        assert_eq!(
            rig.service(true)
                .handle(&AgentEvent::FseChanged(FseState::Inactive)),
            Flow::Continue
        );
    }

    #[test]
    fn the_launcher_hears_of_every_event() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();
        let events = [
            foreground("fakelauncher.exe"),
            AgentEvent::LauncherStarted,
            AgentEvent::SettingsChanged,
            AgentEvent::FseChanged(FseState::Inactive),
        ];
        for event in events.clone() {
            agent.handle(&event);
        }
        rig.launcher.set_game_running(true);
        agent.handle(&AgentEvent::LauncherStateChanged);
        assert_eq!(
            rig.launcher.observed()[..4],
            events,
            "in order, also the one that ends the agent"
        );
        // Passed on as well as acted on.
        assert_eq!(rig.launcher.calls(), ["game_started"]);
        assert_eq!(
            rig.launcher.observed().last(),
            Some(&AgentEvent::LauncherStateChanged)
        );
    }

    #[test]
    fn the_device_runs_as_the_session_started_it() {
        let rig = Rig::new();
        rig.service(false).start();
        assert!(
            rig.input.reconfigured().is_empty(),
            "the composition root started it with the device already"
        );
    }

    #[test]
    fn changed_settings_apply_without_a_restart() {
        let rig = Rig::new();
        rig.launcher.direct_menu.set(true);
        let mut agent = rig.service(false);
        agent.start();
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty(), "the launcher's own menu first");

        let own = KeyChord::pair(VirtualKey::LCONTROL, VirtualKey::DIGIT_1);
        rig.settings.set(Settings {
            menu: Some(own),
            ..configured()
        });
        assert_eq!(agent.handle(&AgentEvent::SettingsChanged), Flow::Continue);
        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [own]);
        assert!(
            rig.input.reconfigured().is_empty(),
            "the unchanged device is left alone"
        );
    }

    #[test]
    fn the_device_follows_the_settings() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();

        rig.settings.set(Settings {
            button_remap: false,
            ..configured()
        });
        agent.handle(&AgentEvent::SettingsChanged);
        assert_eq!(rig.input.reconfigured(), [DeviceSelection::none()]);
        // Switched off, a press that still comes means nothing.
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty());

        let other = DeviceSelection {
            options: OptionTable::from([("mode".to_string(), SettingValue::Bool(true))]),
            ..device("keys")
        };
        rig.settings.set(Settings {
            device: other.clone(),
            ..configured()
        });
        agent.handle(&AgentEvent::SettingsChanged);
        assert_eq!(
            rig.input.reconfigured(),
            [DeviceSelection::none(), other],
            "its options too"
        );
        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [FakeLauncher::MENU]);
    }

    #[test]
    fn a_device_the_running_adapter_cannot_take_over_waits_for_the_next_session() {
        let rig = Rig {
            fse: FakeFse(FseState::Inactive),
            ..Rig::new()
        };
        rig.input.foreign.borrow_mut().push("hid");
        let mut agent = rig.service(true);
        agent.start();

        rig.settings.set(Settings {
            device: device("hid"),
            ..configured()
        });
        agent.handle(&AgentEvent::SettingsChanged);
        agent.handle(&AgentEvent::SettingsChanged);
        assert_eq!(rig.input.reconfigured(), [device("hid")], "asked once");
        // The device that runs is still the one started: its button is passed on.
        rig.foreground.set(Some("explorer.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.input.passed_on(), [ButtonId(0)]);

        // Back to the one running: asked for, and taken.
        rig.settings.set(configured());
        agent.handle(&AgentEvent::SettingsChanged);
        assert_eq!(rig.input.reconfigured(), [device("hid"), device("keys")]);
    }

    #[test]
    fn a_button_the_device_only_observes_is_never_passed_on() {
        // On the desktop Mujina has nothing for the button: a swallowed one is sent on, one the
        // device's own software saw anyway is left at that.
        for (id, button, passed) in [("keys", 0, 1), ("hid", 1, 0)] {
            let rig = Rig {
                fse: FakeFse(FseState::Inactive),
                ..Rig::new()
            };
            let settings = AgentSettings {
                standalone: true,
                device: device(id),
                ..running()
            };
            let mut agent = AgentService::new(rig.ports(), settings);
            agent.start();
            rig.foreground.set(Some("explorer.exe"));
            agent.handle(&AgentEvent::ButtonPressed(ButtonId(button)));
            assert_eq!(rig.input.passed_on().len(), passed, "{id}");
            assert!(rig.input.sent().is_empty());
            assert_eq!(rig.home.activations.get(), 0);
        }
    }

    #[test]
    fn a_reload_keeps_how_the_agent_was_started() {
        let rig = Rig::new();
        let mut agent = rig.service(true);
        agent.start();
        agent.handle(&AgentEvent::SettingsChanged);
        assert_eq!(
            agent.handle(&AgentEvent::FseChanged(FseState::Inactive)),
            Flow::Continue,
            "still standalone"
        );
    }

    #[test]
    fn crashed_launcher_is_brought_back_through_the_home_role() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();

        let clean = LauncherExit {
            exit_code: Some(0),
            uptime_secs: 600,
        };
        agent.handle(&AgentEvent::LauncherExited(clean));
        assert_eq!(
            rig.home.activations.get(),
            0,
            "the user quit the launcher on purpose"
        );

        let crash = LauncherExit {
            exit_code: Some(0xC000_0005),
            uptime_secs: 600,
        };
        agent.handle(&AgentEvent::LauncherExited(crash));
        assert_eq!(rig.home.activations.get(), 1);

        let crash_loop = LauncherExit {
            exit_code: Some(1),
            uptime_secs: 3,
        };
        agent.handle(&AgentEvent::LauncherExited(crash_loop));
        assert_eq!(rig.home.activations.get(), 1, "no relaunch loop");
    }

    #[test]
    fn without_the_input_adapter_the_rest_keeps_working() {
        let rig = Rig::new();
        let ports = AgentPorts {
            buttons: &Absent,
            keys: &Absent,
            ..rig.ports()
        };
        let settings = AgentSettings {
            device: device("keys"),
            ..running()
        };
        let mut agent = AgentService::new(ports, settings);
        agent.start();

        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        // The launcher is asked first; the shortcut after it goes to the stand-in, which drops
        // it, and the agent carries on.
        assert!(rig.launcher.calls().contains(&"open_menu"));

        let crash = LauncherExit {
            exit_code: Some(0xC000_0005),
            uptime_secs: 600,
        };
        agent.handle(&AgentEvent::LauncherExited(crash));
        assert_eq!(rig.home.activations.get(), 1);
    }

    #[test]
    fn a_launcher_without_a_menu_swallows_the_button_in_its_ui() {
        // Passed on, the button would be the device's own chord: Show Desktop on a OneXPlayer.
        let rig = Rig::bare();
        let mut agent = rig.service(false);
        agent.start();
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert!(rig.input.sent().is_empty());
        assert!(rig.input.passed_on().is_empty());
        assert!(!rig.launcher.calls().contains(&"open_menu"));
        assert_eq!(rig.home.activations.get(), 0);
    }

    #[test]
    fn a_menu_shortcut_of_ones_own_gives_any_launcher_a_menu() {
        let rig = Rig::bare();
        let own = KeyChord::pair(VirtualKey::LCONTROL, VirtualKey::DIGIT_1);
        let settings = AgentSettings {
            menu: Some(own),
            ..running()
        };
        let mut agent = AgentService::new(rig.ports(), settings);
        agent.start();
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.input.sent(), [own]);
    }

    #[test]
    fn a_menu_opened_only_directly_sends_nothing_when_it_was_not_taken() {
        let rig = Rig::new();
        rig.launcher.menu_shortcut.set(None);
        let mut agent = rig.service(false);
        agent.start();
        rig.foreground.set(Some("fakelauncher.exe"));
        agent.handle(&PRESS);
        assert!(rig.launcher.calls().contains(&"open_menu"));
        assert!(rig.input.sent().is_empty());
        assert!(rig.input.passed_on().is_empty());
    }

    #[test]
    fn without_an_overlay_the_button_leads_home_from_a_game_in_xbox_mode() {
        static NO_OVERLAY: FakeLauncherDescriptor = FakeLauncherDescriptor::named(
            "no_overlay",
            "No overlay",
            LauncherCaps {
                overlay: false,
                ..LauncherCaps::ALL
            },
        );
        let rig = Rig {
            descriptor: &NO_OVERLAY,
            ..Rig::new()
        };
        rig.launcher.overlay_shortcut.set(None);
        let mut agent = rig.service(false);
        agent.start();
        rig.launcher.set_game_running(true);
        rig.launcher.game_in_front.set(Some(true));
        rig.foreground.set(Some("game.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.home.activations.get(), 1);
        assert!(rig.input.sent().is_empty());
        assert!(!rig.launcher.calls().contains(&"open_overlay"));

        // Behind something else, with no game window to go back to: home as well.
        rig.launcher.game_in_front.set(None);
        rig.foreground.set(Some("xboxpcapp.exe"));
        agent.handle(&PRESS);
        assert_eq!(rig.home.activations.get(), 2);
        assert!(rig.input.sent().is_empty());

        // On the desktop a game in front keeps the button to itself.
        let desktop = Rig {
            fse: FakeFse(FseState::Inactive),
            descriptor: &NO_OVERLAY,
            ..Rig::new()
        };
        let mut agent = desktop.service(true);
        agent.start();
        desktop.launcher.set_game_running(true);
        desktop.launcher.game_in_front.set(Some(true));
        desktop.foreground.set(Some("game.exe"));
        agent.handle(&PRESS);
        assert_eq!(desktop.home.activations.get(), 0);
        assert!(desktop.input.passed_on().is_empty());
    }

    #[test]
    fn options_that_apply_at_once_reach_the_running_launcher() {
        let rig = Rig::new();
        let mut agent = rig.service(false);
        agent.start();

        rig.settings.set(Settings {
            launcher: selection("fake", &[("start_screen", false), ("link", false)]),
            ..configured()
        });
        agent.handle(&AgentEvent::SettingsChanged);
        let live: OptionTable = selection("fake", &[("start_screen", false)]).options;
        assert_eq!(rig.launcher.reconfigured(), std::slice::from_ref(&live));

        // Only what waits for the next session changed: nothing to tell.
        rig.settings.set(Settings {
            launcher: selection("fake", &[("start_screen", false), ("link", true)]),
            ..configured()
        });
        agent.handle(&AgentEvent::SettingsChanged);
        // Another launcher's options are not this one's.
        rig.settings.set(Settings {
            launcher: selection("other", &[("start_screen", true)]),
            ..configured()
        });
        agent.handle(&AgentEvent::SettingsChanged);
        assert_eq!(rig.launcher.reconfigured(), [live]);
    }

    #[test]
    fn a_launcher_given_its_options_again_after_another_was_named() {
        let rig = Rig::new();
        let settings = AgentSettings {
            launcher: selection("fake", &[("start_screen", false)]),
            ..running()
        };
        let mut agent = AgentService::new(rig.ports(), settings);
        agent.start();
        // Another launcher named, the option unset meanwhile, then this one again: the file
        // before says nothing, but the launcher still runs with `false`.
        for launcher in [selection("other", &[]), selection("fake", &[])] {
            rig.settings.set(Settings {
                launcher,
                ..configured()
            });
            agent.handle(&AgentEvent::SettingsChanged);
        }
        assert_eq!(rig.launcher.reconfigured(), [OptionTable::new()]);
    }

    /// A launcher whose menu is an option that applies at once, and needs one that waits.
    struct Switchable;

    const SWITCHABLE: &[SettingSpec] = &[
        SettingSpec {
            key: "menu",
            kind: SettingKind::Toggle { default: false },
            title: "Menu",
            help: "",
            applies: Applies::Live,
            requires: None,
            required: false,
        },
        SettingSpec {
            key: "link",
            kind: SettingKind::Toggle { default: true },
            title: "Link",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: false,
        },
    ];

    impl LauncherDescriptor for Switchable {
        fn id(&self) -> &'static str {
            "switchable"
        }

        fn name(&self) -> &'static str {
            "Switchable"
        }

        fn settings(&self) -> &'static [SettingSpec] {
            SWITCHABLE
        }

        fn template(&self) -> &'static str {
            "# [launcher.switchable]\n# menu = false\n# link = true\n"
        }

        fn capabilities(&self, options: &OptionTable) -> LauncherCaps {
            let on = |key| crate::settings::schema::flag(SWITCHABLE, options, key);
            LauncherCaps {
                menu: on("menu") && on("link"),
                ..LauncherCaps::ALL
            }
        }
    }

    #[test]
    fn what_a_launcher_offers_follows_its_options_that_apply_at_once() {
        static LAUNCHER: Switchable = Switchable;
        crate::testing::conformance(&LAUNCHER);
        let rig = Rig {
            descriptor: &LAUNCHER,
            ..Rig::new()
        };
        let mut agent = rig.service(false);
        agent.start();
        rig.foreground.set(Some("fakelauncher.exe"));
        let press = |agent: &mut AgentService<'_>| {
            let before = rig.input.sent().len();
            agent.handle(&PRESS);
            rig.input.sent().len() > before
        };
        let options = |pairs: &[(&str, bool)]| {
            rig.settings.set(Settings {
                launcher: selection("switchable", pairs),
                ..configured()
            });
        };
        // No menu as started: the press is swallowed.
        assert!(!press(&mut agent));

        options(&[("menu", true)]);
        agent.handle(&AgentEvent::SettingsChanged);
        assert!(press(&mut agent));

        // What waits for the next session stays as the session started.
        options(&[("menu", true), ("link", false)]);
        agent.handle(&AgentEvent::SettingsChanged);
        assert!(press(&mut agent));

        // Unset, it is back to its default.
        options(&[]);
        agent.handle(&AgentEvent::SettingsChanged);
        assert!(!press(&mut agent));
        assert!(rig.input.passed_on().is_empty());
    }
}

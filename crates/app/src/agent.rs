//! Entry point of the agent role: resident while the console experience is on.

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use log::LevelFilter;
use mujina_adapter_config::ConfigFile;
use mujina_adapter_keyboard::KeyboardSender;
use mujina_adapter_kit::plugin::{DeviceParts, DeviceRuntime, SessionParts};
use mujina_adapter_windows::agent_control::AgentInstance;
use mujina_adapter_windows::agent_loop::AgentLoop;
use mujina_adapter_windows::foreground::WindowsForeground;
use mujina_adapter_windows::home_activator::ProtocolHomeActivator;
use mujina_adapter_windows::{launcher_signal, log_file, paths, settings_signal};
use mujina_application::agent::{AgentEvent, AgentPorts, AgentService, AgentSettings, Flow};
use mujina_application::device::DeviceDescriptor;
use mujina_application::launcher::{self, LauncherDescriptor};
use mujina_application::ports::absent::Absent;
use mujina_application::ports::{DeviceButtons, FseState, KeySender};
use mujina_application::settings::{LoadedSettings, Settings, SettingsSource};
use mujina_winutil::com;
use mujina_winutil::cost::{self, ProcessCost};
use mujina_winutil::wait::WaitSource;

use crate::compose::{self, Adapters, Role};
use crate::registry;

/// Windows may start the home app a little before it reports the experience as active.
const FSE_GRACE: Duration = Duration::from_secs(120);

/// The configuration as the agent re-reads it when told it changed: says in the log what it
/// found, follows the log level, and names what waits for the next session.
struct AgentConfig {
    file: ConfigFile,
    started_with: Settings,
    /// The launcher running this session.
    launcher: &'static dyn LauncherDescriptor,
    /// What runs the device this session started with.
    device_runtime: &'static dyn DeviceRuntime,
    debug_marker: bool,
}

impl SettingsSource for AgentConfig {
    fn load(&self) -> LoadedSettings {
        let loaded = self.file.load();
        log::info!("configuration changed; applying it");
        for note in &loaded.notes {
            log::warn!("configuration: {note}");
        }
        log::set_max_level(level(loaded.settings.detailed_log || self.debug_marker));
        let (now, before) = (&loaded.settings.launcher, &self.started_with.launcher);
        if now.id == before.id {
            let waiting = launcher::waiting_for_next_session(
                self.launcher.settings(),
                &before.options,
                &now.options,
            );
            for key in waiting {
                log::info!(
                    "launcher.{}.{key} applies the next time Xbox mode is entered",
                    now.id
                );
            }
        } else {
            log::info!("the launcher changes the next time Xbox mode is entered");
        }
        // Another device of the same runtime (another key-chord device) is taken over at once, and
        // so is none; a device another runtime runs has to wait.
        if registry::device_waits(self.device_runtime, loaded.settings.device.id.as_deref()) {
            log::info!("the device changes the next time Xbox mode is entered");
        }
        loaded
    }
}

fn level(detailed: bool) -> LevelFilter {
    if detailed {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    }
}

/// `standalone`: keep running on the desktop (troubleshooting).
pub fn run(standalone: bool) -> ExitCode {
    let data_dir = paths::data_dir();
    // The `debug` marker file still works; the configuration is the documented way.
    let debug_marker = data_dir.join("debug").exists();
    let _ = log_file::init(&data_dir, "agent", level(debug_marker));

    let Some(_instance) = AgentInstance::claim() else {
        log::debug!("another agent is already running");
        return ExitCode::SUCCESS;
    };
    let started = Instant::now();
    log::info!(
        "started: version {}, standalone {standalone}",
        env!("CARGO_PKG_VERSION")
    );
    // Opened first, so that as little as possible of what the home role signals while the agent
    // gets ready is missed: the home role starts the agent, then the launcher. A signal before
    // this point is lost with its event (`launcher_signal::notify`); the look for the launcher's
    // process before the first wait finds a launcher started meanwhile all the same.
    let mut sources = signals();
    // This thread activates the home role through ShellExecute, which wants COM first. The
    // shell wrapper enters an STA for each call anyway; entered here for the thread's life, COM
    // is not closed, nor what the shell loaded unloaded, after every activation. The thread
    // pumps messages throughout, as an STA thread must.
    let _apartment = com::Apartment::sta().inspect_err(|code| {
        log::warn!("COM could not be initialised (error {code:#x}); activations may fail");
    });

    let mut adapters = Adapters::new(Role::Agent);
    let Some(SessionParts {
        launcher,
        sources: launcher_sources,
    }) = adapters.session.take()
    else {
        // Built for the agent role, so never missing; nothing to supervise without it.
        log::error!("the launcher's part for the agent was not built");
        return ExitCode::FAILURE;
    };
    let settings = &adapters.settings.settings;
    log::set_max_level(level(settings.detailed_log || debug_marker));
    describe(&adapters);

    // The launcher's shortcuts go out this way whatever the device.
    let keys = KeyboardSender::start();
    if keys.is_none() {
        log::error!("the sender thread could not be started; no shortcut can be sent");
    }
    let (buttons, device_sources) = match adapters.device.take() {
        Some(DeviceParts { buttons, sources }) => (Some(buttons), sources),
        None => (None, Vec::new()),
    };
    // Before the launcher's own: of the handles signalled together, the wait reports the first.
    sources.extend(device_sources);
    sources.extend(launcher_sources);
    let config = AgentConfig {
        file: compose::config(),
        started_with: settings.clone(),
        launcher: adapters.plugin.descriptor,
        device_runtime: adapters.device_runtime,
        debug_marker,
    };

    let absent = Absent;
    let home = ProtocolHomeActivator;
    let foreground = WindowsForeground;
    let ports = AgentPorts {
        fse: &adapters.fse,
        foreground: &foreground,
        launcher: launcher.as_ref(),
        descriptor: adapters.plugin.descriptor,
        buttons: buttons
            .as_deref()
            .map_or(&absent as &dyn DeviceButtons, |buttons| buttons),
        devices: registry::devices(),
        keys: keys
            .as_ref()
            .map_or(&absent as &dyn KeySender, |keys| keys as &dyn KeySender),
        home: &home,
        settings: &config,
    };
    let mut service = AgentService::new(ports, AgentSettings::from_settings(settings, standalone));
    service.start();

    let event_loop = AgentLoop {
        fse: &adapters.fse,
        launcher: launcher.as_ref(),
        sources,
        grace: FSE_GRACE,
    };
    let farewell = Rc::new(Farewell {
        started,
        startup: cost::of_this_process(),
        events: Cell::new(0),
        said: Cell::new(false),
    });
    let last_words = Rc::clone(&farewell);
    // What the button led to is named as the user knows it, e.g. "Steam Big Picture".
    let launcher_name = adapters.launcher.display_name();
    event_loop.run(
        &mut |event| {
            // After the closing lines nothing is counted or logged (only the session ending
            // comes then), so they stay the last lines and their count stays true.
            if !farewell.said.get() {
                farewell.events.set(farewell.events.get() + 1);
                log::debug!("{event:?}");
            }
            let flow = service.handle(&event);
            // One line per press, in roles only: it shows that the press arrived, and what it
            // came to and why, without the names of what the user had open. Taken in any case,
            // and after the closing lines not logged, as the events are not.
            if let Some(press) = service.take_press_report()
                && !farewell.said.get()
            {
                log::info!("device button pressed: {}", press.describe(&launcher_name));
            }
            if flow == Flow::Exit && !farewell.said.get() {
                say_why_leaving(&event);
            }
            flow
        },
        // What ends the session (a sign-out, a shutdown, the Restart Manager) is logged just
        // before, by `session_end`, which has the message's flags.
        Box::new(move || last_words.say()),
    );

    farewell.say();
    ExitCode::SUCCESS
}

/// The session's closing lines. Said once: when the event loop ends, or before that from inside
/// `WM_ENDSESSION`, after which Windows may end the process at any moment.
struct Farewell {
    started: Instant,
    startup: Option<ProcessCost>,
    /// Events handled before the closing lines; what comes after them is not counted.
    events: Cell<u64>,
    said: Cell<bool>,
}

impl Farewell {
    fn say(&self) {
        if self.said.replace(true) {
            return;
        }
        // Flushing first looks the log up by name, so these lines land in the current file even
        // if another role rotated it since the agent last looked.
        log::logger().flush();
        let session = self.started.elapsed();
        log::info!("leaving after {} s", session.as_secs());
        log_cost(self.startup, session, self.events.get());
        log::logger().flush();
    }
}

/// Says why the agent leaves after `event`, before the closing lines.
fn say_why_leaving(event: &AgentEvent) {
    match event {
        // Said just before, by `session_end`, which has the message's flags.
        AgentEvent::SessionEnding => {}
        // Not "ended": also when the grace period at start-up passed without Xbox mode on.
        AgentEvent::FseChanged(FseState::Inactive) => log::info!("Xbox mode is off"),
        AgentEvent::FseChanged(FseState::Unavailable) => {
            log::info!("Windows no longer reports Xbox mode");
        }
        other => log::info!("leaving on {other:?}"),
    }
}

/// The named events through which other Mujina processes speak to the agent, as wait sources
/// of its event loop.
fn signals() -> Vec<Box<dyn WaitSource<AgentEvent>>> {
    let mut sources: Vec<Box<dyn WaitSource<AgentEvent>>> = Vec::new();
    match settings_signal::listen() {
        Some(source) => sources.push(Box::new(source)),
        None => {
            log::warn!("configuration changes will only apply the next time Xbox mode is entered");
        }
    }
    match launcher_signal::listen() {
        Some(source) => sources.push(Box::new(source)),
        None => log::warn!(
            "a launcher the home role starts is noticed only once its window comes to the front"
        ),
    }
    sources
}

/// What the agent starts with, for the log.
fn describe(adapters: &Adapters) {
    for note in &adapters.settings.notes {
        log::warn!("configuration: {note}");
    }
    let device = adapters.settings.settings.device.id.as_deref();
    let named = device
        .and_then(|id| registry::devices().find(id))
        .map_or_else(|| "none".to_string(), DeviceDescriptor::name);
    log::info!(
        "device \"{}\" / \"{}\", button profile: {named}, launcher: {}",
        adapters.system.manufacturer,
        adapters.system.product,
        adapters.launcher.display_name()
    );
}

/// States what the session cost, so that "idle means idle" is a number anyone can check on their
/// own device rather than a claim.
fn log_cost(startup: Option<ProcessCost>, session: Duration, events: u64) {
    let (Some(startup), Some(total)) = (startup, cost::of_this_process()) else {
        return;
    };
    let running = total.cpu.saturating_sub(startup.cpu);
    let tenths_of_mb = total.peak_working_set_bytes * 10 / (1024 * 1024);
    let share = if session.is_zero() {
        0.0
    } else {
        running.as_secs_f64() / session.as_secs_f64() * 100.0
    };
    log::info!(
        "cost: {} ms CPU to start, then {} ms over {} s ({share:.3} % of one core) for {events} \
         events; peak memory {}.{} MB",
        startup.cpu.as_millis(),
        running.as_millis(),
        session.as_secs(),
        tenths_of_mb / 10,
        tenths_of_mb % 10,
    );
}

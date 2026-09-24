//! Fills the window from the same sources `mujinactl` uses, and changes settings the same way.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mujina_app::tool::{self, Applied, Diagnosis, Diagnostics};
use mujina_application::Msg;
use mujina_application::device::DeviceSelection;
use mujina_application::doctor::{Finding, Remedy};
use mujina_application::settings::schema;
use mujina_application::settings::{SettingChange, SettingValue, Settings};
use mujina_domain::chord::TriggerChord;
use mujina_winutil::window as winutil_window;
use mujina_winutil::{gamepad, locale, process};
use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::feedback::{self, Refusal};
use crate::form;
use crate::pad::{Action, Navigator};
use crate::status::{InUse, Probe};
use crate::ui::{
    CapturePhase, Change, CheckRow, DeviceInfo, Icons, LogLine, MainWindow, Nav, Notice, RowData,
    Rows, Said, ToastKind,
};
use crate::{capture, log, nav, setup, status, texts};

/// How much of the log the Log page shows.
const LOG_LINES: usize = 400;

/// How often the controller is read while the app is in front.
const PAD_INTERVAL: Duration = Duration::from_millis(33);

pub fn run() -> Result<(), slint::PlatformError> {
    let Some(_instance) = tool::claim_settings_instance() else {
        show_running();
        return Ok(());
    };
    let window = MainWindow::new()?;
    let data_dir = tool::data_dir();
    // The chosen language before anything is worded; Slint wants a window to exist first.
    let stored = tool::configuration().stored("interface.language");
    use_language(&form::as_text(stored));
    window.set_version(env!("CARGO_PKG_VERSION").into());
    window.set_data_directory(shown(&data_dir).into());
    // In Xbox mode there is no desktop to put a window on. `--full-screen` shows that layout on
    // the desktop, to try it out.
    let console =
        tool::in_xbox_mode() || std::env::args().any(|argument| argument == "--full-screen");
    window.set_fullscreen(console);
    window.window().set_fullscreen(console);
    window.global::<Nav>().on_step(|enabled, current, delta| {
        let enabled: Vec<bool> = enabled.iter().collect();
        nav::step(&enabled, current, delta)
    });
    window.global::<Rows>().on_enabled(|sections| {
        ModelRc::from(Rc::new(VecModel::from(nav::enabled(sections.iter()))))
    });
    // Until the first checks are back, the verdict and the home card say they are under way.
    show_home_checking(&window);
    refresh(&window);
    refresh_log(&window, &data_dir);
    connect(&window, &data_dir);
    let _pad = follow_controller(&window);
    use_own_icon(20);
    window.run()
}

/// Swaps Slint's one large window icon for the executable's own small images, once the window
/// exists: Slint makes it when the event loop starts. Tries `attempts` times.
fn use_own_icon(attempts: u8) {
    Timer::single_shot(Duration::from_millis(50), move || {
        if !winutil_window::use_own_icon() && attempts > 1 {
            use_own_icon(attempts - 1);
        }
    });
}

/// Brings the window of the app that is open already to the front; started again, the app
/// must not open a second window next to it.
fn show_running() {
    let own = std::process::id();
    let running = winutil_window::top_level_windows()
        .into_iter()
        .filter(|window| window.visible && window.process_id != own)
        .find(|window| {
            process::image_name(window.process_id)
                .is_some_and(|name| name.eq_ignore_ascii_case("mujina-settings.exe"))
        });
    if let Some(window) = running {
        winutil_window::bring_to_foreground(window.handle);
    }
}

/// Reads the controller while the app is in front and turns it into the keys the window's own
/// navigation understands (arrows, Return, Escape). The timer
/// lives as long as the returned value, which is as long as the window.
fn follow_controller(window: &MainWindow) -> Timer {
    let weak = window.as_weak();
    let mut navigator = Navigator::default();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, PAD_INTERVAL, move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        // Only the window in front takes the controller; a press meant for Steam must not
        // toggle anything here.
        if winutil_window::foreground_process_id() != Some(std::process::id()) {
            navigator.reset();
            return;
        }
        let state = gamepad::first_connected().map(|pad| (pad.buttons, pad.left_x, pad.left_y));
        let actions = navigator.update(state, Instant::now());
        // A capture waits for the device button, and the controller must not wander off
        // meanwhile: B cancels it. Its result closes with A or B.
        let phase = window.get_capture_phase();
        if phase != CapturePhase::None {
            let result = phase != CapturePhase::Waiting;
            let closes = actions.iter().any(|action| {
                matches!(action, Action::Back) || (result && matches!(action, Action::Activate))
            });
            if closes {
                dismiss_capture(&window);
            }
            return;
        }
        for action in actions {
            match action {
                Action::Up => press(&window, Key::UpArrow),
                Action::Down => press(&window, Key::DownArrow),
                Action::Left => press(&window, Key::LeftArrow),
                Action::Right => press(&window, Key::RightArrow),
                Action::Activate => press(&window, Key::Return),
                Action::Back => press(&window, Key::Escape),
                Action::NextPage => {
                    let pages = window.get_page_count();
                    window.invoke_select_page((window.get_page() + 1) % pages);
                }
                Action::PreviousPage => {
                    let pages = window.get_page_count();
                    window.invoke_select_page((window.get_page() + pages - 1) % pages);
                }
            }
        }
    });
    timer
}

/// A key press and release, as if typed.
fn press(window: &MainWindow, key: Key) {
    let text: SharedString = key.into();
    window
        .window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    window
        .window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

/// Every callback of the window.
fn connect(window: &MainWindow, data_dir: &Path) {
    let weak = window.as_weak();
    window.on_refresh(move || {
        if let Some(window) = weak.upgrade() {
            refresh(&window);
        }
    });
    let weak = window.as_weak();
    window.on_check(move || {
        if let Some(window) = weak.upgrade() {
            start_checks(&window);
        }
    });
    let weak = window.as_weak();
    let log_dir = data_dir.to_path_buf();
    window.on_refresh_log(move || {
        if let Some(window) = weak.upgrade() {
            refresh_log(&window, &log_dir);
        }
    });
    let weak = window.as_weak();
    window.on_quit(move || {
        if let Some(window) = weak.upgrade() {
            // The last window hidden ends the event loop, and with it the app.
            let _ = window.hide();
        }
    });
    let folder = data_dir.to_path_buf();
    window.on_open_data_directory(move || {
        // Nothing sensible to do if Explorer does not start.
        let _ = std::process::Command::new("explorer.exe")
            .arg(&folder)
            .spawn();
    });

    let weak = window.as_weak();
    let change = window.global::<Change>();
    change.on_flag(move |key, on| {
        if let Some(window) = weak.upgrade() {
            apply(&window, Ok(vec![form::flag_change(&key, on)]));
        }
    });
    let weak = window.as_weak();
    change.on_number(move |key, value| {
        if let Some(window) = weak.upgrade() {
            let setting = SettingChange::set(&key, SettingValue::Integer(i64::from(value)));
            apply(&window, Ok(vec![setting]));
        }
    });
    let weak = window.as_weak();
    change.on_text(move |key, text| {
        if let Some(window) = weak.upgrade() {
            edit(&window, &key, &text);
        }
    });
    let weak = window.as_weak();
    change.on_choice(move |key, index| {
        if let Some(window) = weak.upgrade() {
            choose(&window, &key, index);
        }
    });
    let weak = window.as_weak();
    window.on_leave_setup(move || {
        if let Some(window) = weak.upgrade()
            && DRAFT.with(Cell::take).is_some()
        {
            // The page showed the launcher being chosen; back to the one in use.
            show_settings(&window);
        }
    });

    let weak = window.as_weak();
    window.on_capture(move || {
        if let Some(window) = weak.upgrade() {
            start_capture(&window);
        }
    });
    let weak = window.as_weak();
    window.on_dismiss_capture(move || {
        if let Some(window) = weak.upgrade() {
            dismiss_capture(&window);
        }
    });

    connect_system(window);
}

/// The System page's callbacks.
fn connect_system(window: &MainWindow) {
    let weak = window.as_weak();
    window.on_register_home(move || {
        if let Some(window) = weak.upgrade() {
            register(&window, true);
        }
    });
    let weak = window.as_weak();
    window.on_unregister_home(move || {
        if let Some(window) = weak.upgrade() {
            register(&window, false);
        }
    });
    let weak = window.as_weak();
    window.on_check_action(move |id| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        // What the check whose row it is offered, as last found.
        let remedy = SYSTEM_FINDINGS.with(|shown| {
            shown
                .borrow()
                .iter()
                .find(|finding| finding.id == id.as_str())
                .and_then(|finding| finding.remedy)
        });
        match remedy {
            // Windows shows its own page; nothing to report if it does not.
            Some(Remedy::LocationSettings) => {
                mujina_winutil::shell::open("ms-settings:privacy-location");
            }
            Some(Remedy::StartAgent) => start_agent(&window),
            None => {}
        }
    });
    let weak = window.as_weak();
    window.on_remove_mujina(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        match tool::start_removal() {
            // Mujina Setup removes this app's package, whose processes the removal would stop:
            // this app ends first. The last window hidden ends the event loop.
            Ok(()) => {
                let _ = window.hide();
            }
            Err(error) => show_toast(
                &window,
                feedback::refused(&Refusal::Reason(format!(
                    "Mujina Setup did not start: {error}"
                ))),
            ),
        }
    });
    let weak = window.as_weak();
    window.on_copy(move |value| {
        if let Some(window) = weak.upgrade() {
            let notice = if mujina_winutil::clipboard::set_text(&value) {
                feedback::notice(ToastKind::Now, Said::Copied)
            } else {
                feedback::notice(ToastKind::Refused, Said::ClipboardBusy)
            };
            show_toast(&window, notice);
        }
    });
}

/// Everything the pages show, read again: the settings at once, the checks on a thread of their
/// own.
fn refresh(window: &MainWindow) {
    show_settings(window);
    start_checks(window);
}

/// What `config.toml` says, as the Setup and Help pages show it. Read after every change, so it
/// stays quick and runs no checks.
fn show_settings(window: &MainWindow) {
    let page = setup::page(&tool::configuration(), DRAFT.with(Cell::get));
    let model = |texts: Vec<String>| {
        let texts: Vec<SharedString> = texts.into_iter().map(SharedString::from).collect();
        ModelRc::from(Rc::new(VecModel::from(texts)))
    };
    window.set_config(page.config);
    window.set_launcher_rows(page.launcher.model());
    window.set_device_rows(page.device.model());
    window.set_launcher_names(model(page.launcher_names));
    window.set_profiles(model(page.profiles));
    window.set_button_in_use(page.button_in_use.into());
}

/// Runs the checks behind the Status and System pages on a thread of their own: they ask Steam's
/// debugging port and list the running programs, which must not hold up the window. Until they
/// are back, the Status page says it is checking. One run at a time, as [`Checks`] says.
fn start_checks(window: &MainWindow) {
    window.set_checking(true);
    let Some(run) = CHECKS.with(|checks| checks.borrow_mut().start()) else {
        return;
    };
    let weak = window.as_weak();
    std::thread::spawn(move || {
        // Built here: the launcher's adapter cannot be handed from one thread to another.
        let mut diagnostics = Diagnostics::new();
        let facts = diagnostics.system(&tool::SYSTEM_CHECKS);
        // The System page need not wait for the doctor. Nothing to show once the window is gone.
        let _ = weak.upgrade_in_event_loop(move |window| found_facts(&window, run, &facts));
        let probe = probe(diagnostics.examine());
        let _ = weak.upgrade_in_event_loop(move |window| checked(&window, run, &probe));
    });
}

/// What the Status page shows, found out: the doctor's findings, and the settings they are for.
fn probe(diagnosis: Diagnosis) -> Probe {
    let now = mujina_winutil::time::local_timestamp();
    Probe {
        findings: diagnosis.findings,
        stored_profile: form::as_text(diagnosis.stored_profile),
        button: in_use(&diagnosis.settings.settings.device),
        launcher: diagnosis.launcher,
        time: now.get(11..16).unwrap_or_default().to_string(),
        settings: diagnosis.settings,
        system: diagnosis.system,
    }
}

/// A run's facts for the System page, shown while that run is the latest. They may also come
/// after its findings; the run's number decides, not the order.
fn found_facts(window: &MainWindow, run: u64, facts: &tool::SystemFacts) {
    if CHECKS.with(|checks| checks.borrow().run) != run {
        return;
    }
    // The package's execution alias starts this app in its package; Steam takes it as a program.
    let packaged = mujina_winutil::package::family_name().is_some();
    let steam_target = match std::env::var("LOCALAPPDATA") {
        Ok(local) if packaged => format!(r"{local}\Microsoft\WindowsApps\mujina-settings.exe"),
        _ => String::new(),
    };
    let mut info = status::system_info(facts, &steam_target);
    // Only Mujina Setup's own copy removes in the right order; without it, Settings → Apps.
    info.can_remove = tool::retained_setup().is_some();
    window.set_system_info(info);
    SYSTEM_FINDINGS.with(|shown| shown.borrow_mut().clone_from(&facts.findings));
    show_system_checks(window);
}

/// The System page's rows of what Windows allows, from the findings last found, in the language
/// set.
fn show_system_checks(window: &MainWindow) {
    let icons = window.global::<Icons>();
    let rows: Vec<RowData> = SYSTEM_FINDINGS.with(|shown| {
        shown
            .borrow()
            .iter()
            .map(|finding| status::system_row(finding, &icons))
            .collect()
    });
    window.set_system_checks(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// A run's findings, which end it: shown while it is the latest run. One more run follows if it
/// was asked for meanwhile.
fn checked(window: &MainWindow, run: u64, probe: &Probe) {
    let (latest, again) = CHECKS.with(|checks| checks.borrow_mut().finish(run));
    if latest {
        show_status(window, probe);
    }
    if again {
        start_checks(window);
    }
}

/// The home card while the home app is being read; the rest of the System page, its button
/// among it, stays as it was.
fn show_home_checking(window: &MainWindow) {
    let mut info = window.get_system_info();
    info.home_checking = true;
    window.set_system_info(info);
}

/// The device in effect as the device tile shows it.
fn in_use(device: &DeviceSelection) -> InUse {
    let devices = tool::devices();
    match device.id.as_deref().and_then(|id| devices.find(id)) {
        None => InUse::Nothing,
        Some(found) if devices.is_own(found) => InUse::Own {
            keys: tool::button_keys(device),
        },
        Some(found) => InUse::Device {
            label: setup::button_label(found),
        },
    }
}

/// The Status page: the doctor's verdict, the device tiles and every finding.
fn show_status(window: &MainWindow, probe: &Probe) {
    let findings = &probe.findings;
    window.set_verdict(status::verdict(findings));
    window.set_checking(false);
    window.set_checked_time(probe.time.as_str().into());

    let settings = &probe.settings.settings;
    let (button, button_source) =
        status::button_tile(settings.button_remap, &probe.button, &probe.stored_profile);
    window.set_device(DeviceInfo {
        handheld: probe.system.product.trim().into(),
        maker: probe.system.manufacturer.trim().into(),
        button: button.into(),
        button_source,
        launcher: probe.launcher.as_str().into(),
        wifi: status::wifi(wifi_fix(settings)),
        folder: shown(&tool::data_dir().join("config.toml")).into(),
    });
    window.set_note(probe.settings.notes.join("\n").into());
    let checks: Vec<CheckRow> = findings
        .iter()
        .map(|finding| {
            let check = status::check(finding);
            CheckRow {
                health: check.health,
                title: check.title.into(),
                detail: check.detail.into(),
            }
        })
        .collect();
    window.set_checks(ModelRc::from(Rc::new(VecModel::from(checks))));
    CHECK_TITLES.with(|titles| {
        *titles.borrow_mut() = findings.iter().map(|finding| finding.title).collect();
    });
}

/// Names the checks on the Status and System pages again, in the language set now; what the
/// doctor found stays as it is, English.
fn rename_checks(window: &MainWindow) {
    let checks = window.get_checks();
    CHECK_TITLES.with(|titles| {
        for (index, title) in titles.borrow().iter().enumerate() {
            if let Some(mut row) = checks.row_data(index) {
                row.title = texts::t(title).into();
                checks.set_row_data(index, row);
            }
        }
    });
    show_system_checks(window);
}

/// Whether the launcher's Wi-Fi fix is in effect: its `wifi_indicator`, with the toggle that one
/// requires; `None` with a launcher that has none.
fn wifi_fix(settings: &Settings) -> Option<bool> {
    let launcher = &settings.launcher;
    let specs = tool::launchers().get(&launcher.id).settings();
    let spec = specs.iter().find(|spec| spec.key == "wifi_indicator")?;
    let on = |key: &str| schema::flag(specs, &launcher.options, key);
    Some(on(spec.key) && spec.requires.is_none_or(on))
}

/// Starts the agent as `mujina.exe` next to this app would be started in Xbox mode.
fn start_agent(window: &MainWindow) {
    let notice = match tool::start_agent() {
        Ok(()) => feedback::notice(ToastKind::Now, Said::AgentStarting),
        Err(error) => feedback::refused(&Refusal::Reason(error.to_string())),
    };
    show_toast(window, notice);
    // The agent needs a moment before it can be seen running.
    let weak = window.as_weak();
    Timer::single_shot(Duration::from_millis(1500), move || {
        if let Some(window) = weak.upgrade() {
            start_checks(&window);
        }
    });
}

/// A path of ours as the pages show it.
fn shown(path: &Path) -> String {
    let local = std::env::var("LOCALAPPDATA").ok();
    status::shown_path(&path.display().to_string(), local.as_deref())
}

/// A choice from a list. A launcher that requires a value not stored yet (the generic one's
/// program) stores nothing yet: the page shows it with its rows, and the switch is stored
/// together with what it requires.
fn choose(window: &MainWindow, key: &str, index: i32) {
    if key == "interface.language" {
        choose_language(window, index);
        return;
    }
    if key == "launcher.kind" {
        let draft = DRAFT.with(Cell::take);
        let config = tool::configuration();
        let shown = setup::shown_launcher(
            config.stored("launcher.kind"),
            &config.loaded.settings.launcher.id,
        );
        let chosen = setup::launcher_at(index);
        // Back to the launcher the page showed before another was being chosen: nothing to
        // store.
        if draft.is_some() && chosen.is_some_and(|chosen| chosen.id() == shown.id()) {
            show_settings(window);
            return;
        }
        if let Some(chosen) = chosen
            && !setup::missing(&config, chosen).is_empty()
        {
            DRAFT.with(|draft| draft.set(Some(chosen.id())));
            show_settings(window);
            return;
        }
    }
    apply(window, Ok(vec![setup::choice_change(key, index)]));
}

/// The app's own language: stored like any setting, and in effect at once.
fn choose_language(window: &MainWindow, index: i32) {
    let changes = [setup::choice_change("interface.language", index)];
    let outcome = tool::change(&changes).map_err(|error| Refusal::Reason(error.to_string()));
    if outcome.is_ok() {
        let code = usize::try_from(index)
            .ok()
            .and_then(|index| form::choices("interface.language").get(index))
            .copied()
            .unwrap_or_default();
        use_language(code);
    }
    // The pages word themselves again; the lists and rows Rust fills are filled again, and the
    // checks named again, in the new language.
    show_settings(window);
    rename_checks(window);
    // Not the agent's concern: in effect now, whether or not an agent runs.
    let _ = report(window, outcome.map(|_| Applied::Now));
}

/// Words the app in the language `interface.language` names, where Mujina has it; otherwise, as
/// for "auto", in the first of the Windows display languages it has. Slint's texts and Rust's
/// follow the same decision, which Mujina Setup makes the same way.
fn use_language(chosen: &str) {
    // The first of the Help page's list is "auto"; the rest are the languages lang/ has.
    let languages = form::choices("interface.language").get(1..);
    let language = locale::language(chosen, languages.unwrap_or_default());
    // Only fails without bundled translations, which the build always has.
    let _ = slint::select_bundled_translation(language);
    texts::set(language);
}

/// A text field's new value. A value the launcher being chosen requires switches to it once it
/// requires no other.
fn edit(window: &MainWindow, key: &str, text: &str) {
    let changes = form::text_changes(key, text, setup::spec(key));
    let Some(draft) = DRAFT.with(Cell::get) else {
        apply(window, changes);
        return;
    };
    let config = tool::configuration();
    let launcher = tool::launchers().get(draft);
    let missing = setup::missing(&config, launcher);
    let section = format!("launcher.{draft}.");
    let required = key
        .strip_prefix(&section)
        .and_then(|leaf| missing.iter().find(|spec| spec.key == leaf));
    let Some(required) = required else {
        apply(window, changes);
        return;
    };
    if text.trim().is_empty() {
        let title = texts::launcher_words(launcher)(required.title);
        show_toast(
            window,
            feedback::refused(&feedback::required(required, &title)),
        );
        return;
    }
    // The last value it requires: stored with the switch, and the page shows it in use.
    let switches = missing.len() == 1;
    let changes = changes.map(|mut changes| {
        if switches {
            let fallback = tool::launchers().fallback.id();
            let id = (draft != fallback).then_some(draft);
            changes.insert(0, form::choice("launcher.kind", id));
            DRAFT.with(|shown| shown.set(None));
        }
        changes
    });
    if report(window, change(window, changes)).is_err() && switches {
        DRAFT.with(|shown| shown.set(Some(draft)));
        show_settings(window);
    }
}

/// Stores the changes like `mujinactl config set` does and says what came of it in a toast.
fn apply(window: &MainWindow, changes: Result<Vec<SettingChange>, Refusal>) {
    // The toast has said what came of it.
    let _ = report(window, change(window, changes));
}

/// Stores the changes and shows the pages what the file says now; no toast. The checks are not
/// run again for it: a switch must answer at once, and the pages with checks run them when
/// entered.
fn change(
    window: &MainWindow,
    changes: Result<Vec<SettingChange>, Refusal>,
) -> Result<Applied, Refusal> {
    let outcome = changes.and_then(|changes| {
        tool::change(&changes).map_err(|error| Refusal::Reason(error.to_string()))
    });
    show_settings(window);
    outcome
}

/// The toast for what came of a change.
fn report(window: &MainWindow, outcome: Result<Applied, Refusal>) -> Result<Applied, Refusal> {
    show_toast(window, feedback::of(&outcome));
    outcome
}

thread_local! {
    /// A launcher chosen on the Setup page but not stored yet, for want of a value it requires:
    /// the page shows it with its rows until that is entered, or the page is left.
    static DRAFT: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// Counts the toasts shown, so a toast's timer only hides its own toast.
    static TOASTS: Cell<u64> = const { Cell::new(0) };
    /// The runs of the checks.
    static CHECKS: RefCell<Checks> = RefCell::new(Checks::default());
    /// The capture under way, and what it came to.
    static CAPTURE: RefCell<Capture> = RefCell::new(Capture::default());
    /// The titles of the checks the Status page lists, in its order: named again when the
    /// language changes.
    static CHECK_TITLES: RefCell<Vec<Msg>> = const { RefCell::new(Vec::new()) };
    /// The findings the System page shows, as last found: named again when the language
    /// changes.
    static SYSTEM_FINDINGS: RefCell<Vec<Finding>> = const { RefCell::new(Vec::new()) };
}

/// The runs of the checks. One at a time: a run can wait on Steam's debugging port for seconds,
/// and pages shown meanwhile need one run after it, not one each.
#[derive(Default)]
struct Checks {
    /// Counts the runs, so a result is shown only from the latest one.
    run: u64,
    /// A run is under way.
    running: bool,
    /// Asked for while a run was under way: one more run follows it.
    again: bool,
}

impl Checks {
    /// A run asked for: its number when it starts now, `None` when it follows the one under way.
    fn start(&mut self) -> Option<u64> {
        if self.running {
            self.again = true;
            return None;
        }
        self.running = true;
        self.run += 1;
        Some(self.run)
    }

    /// Run `run` is back: whether it is still the latest, so its result is shown, and whether
    /// one more run was asked for meanwhile.
    fn finish(&mut self, run: u64) -> (bool, bool) {
        self.running = false;
        (self.run == run, std::mem::take(&mut self.again))
    }

    /// The home app changed. A run under way may have read the old one, so none of its result is
    /// shown.
    fn forget_home(&mut self) {
        self.run += 1;
    }
}

/// Shows a toast and hides it again after its dwell time; a newer one replaces it.
fn show_toast(window: &MainWindow, notice: Notice) {
    let serial = TOASTS.with(|toasts| {
        toasts.set(toasts.get() + 1);
        toasts.get()
    });
    let dwell = feedback::dwell(notice.kind);
    window.set_toast(notice);
    window.set_toast_shown(true);
    let weak = window.as_weak();
    Timer::single_shot(dwell, move || {
        if TOASTS.with(Cell::get) != serial {
            return;
        }
        if let Some(window) = weak.upgrade() {
            window.set_toast_shown(false);
        }
    });
}

#[derive(Default)]
struct Capture {
    /// Set to stop the watcher while it waits.
    cancel: Option<Arc<AtomicBool>>,
    /// Counts the overlay's seconds down for as long as it is held.
    _countdown: Option<Timer>,
    /// The toast for the stored button, shown once the overlay is closed.
    toast: Option<Notice>,
}

/// Watches for the device button on a thread of its own (the hook needs a thread that waits for
/// it), and stores what was seen once it is back.
fn start_capture(window: &MainWindow) {
    let seconds = i32::try_from(tool::CAPTURE_TIME.as_secs()).unwrap_or(10);
    window.set_capture_seconds(seconds);
    window.set_capture_total(seconds);
    window.set_capture_phase(CapturePhase::Waiting);

    let countdown = Timer::default();
    let weak = window.as_weak();
    countdown.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        if let Some(window) = weak.upgrade() {
            window.set_capture_seconds((window.get_capture_seconds() - 1).max(0));
        }
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let watching = Arc::clone(&cancel);
    let weak = window.as_weak();
    std::thread::spawn(move || {
        let result = capture::capture(&watching);
        if watching.load(Ordering::Relaxed) {
            return;
        }
        // Nothing to report to once the window is gone.
        let _ = weak.upgrade_in_event_loop(move |window| captured(&window, result));
    });
    CAPTURE.with(|capture| {
        *capture.borrow_mut() = Capture {
            cancel: Some(cancel),
            _countdown: Some(countdown),
            toast: None,
        };
    });
}

/// The overlay's result: the button stored, nothing seen, or the watcher failing.
fn captured(window: &MainWindow, result: Result<Option<TriggerChord>, String>) {
    // The button did what it always does too: a program with administrator rights (OneXConsole
    // on a OneXPlayer) sends it, and such input cannot be held back.
    bring_back(window);
    let (phase, detail, toast) = match result {
        Ok(Some(button)) => {
            let outcome = change(window, Ok(capture::changes(button)));
            let chord = button.keys.to_string();
            window.set_capture_result(feedback::of(&outcome));
            let phase = if outcome.is_ok() {
                CapturePhase::Captured
            } else {
                CapturePhase::Refused
            };
            let toast = feedback::captured(&chord, &outcome);
            (phase, chord, Some(toast))
        }
        Ok(None) => (CapturePhase::NothingSeen, String::new(), None),
        Err(error) => (CapturePhase::Failed, error, None),
    };
    CAPTURE.with(|capture| {
        *capture.borrow_mut() = Capture {
            toast,
            ..Capture::default()
        };
    });
    window.set_capture_detail(detail.into());
    window.set_capture_phase(phase);
}

/// Cancels a capture that waits, or closes its result and says in a toast what was stored.
fn dismiss_capture(window: &MainWindow) {
    let capture = CAPTURE.with(|capture| std::mem::take(&mut *capture.borrow_mut()));
    if let Some(cancel) = capture.cancel {
        cancel.store(true, Ordering::Relaxed);
    }
    window.set_capture_phase(CapturePhase::None);
    if let Some(notice) = capture.toast {
        show_toast(window, notice);
    }
}

/// Restores and fronts the window after something else (the device button's own "show
/// desktop") pushed it away.
fn bring_back(window: &MainWindow) {
    window.window().set_minimized(false);
    let own = std::process::id();
    let ours = winutil_window::top_level_windows()
        .into_iter()
        .find(|candidate| candidate.visible && candidate.process_id == own);
    if let Some(ours) = ours
        && !winutil_window::bring_to_foreground(ours.handle)
    {
        // The user asked for this a moment ago; see ADR-0001 for the one synthetic key tap.
        winutil_window::claim_foreground(ours.handle);
    }
}

fn register(window: &MainWindow, make_home: bool) {
    let notice = if make_home {
        feedback::registered(tool::make_home_app().map_err(|error| error.to_string()))
    } else {
        feedback::unregistered(tool::give_home_app_back().map_err(|error| error.to_string()))
    };
    // Windows reads the home app when Xbox mode is entered, so a change applies next time.
    let changed = notice.kind == ToastKind::NextTime;
    show_toast(window, notice);
    // The home app changed. Until it is read again the card says it is checking, not the old
    // home app, which can be for seconds while a run already under way waits on Steam.
    if changed {
        CHECKS.with(|checks| checks.borrow_mut().forget_home());
        show_home_checking(window);
    }
    start_checks(window);
}

fn refresh_log(window: &MainWindow, data_dir: &Path) {
    let (lines, error) = match std::fs::read_to_string(data_dir.join("mujina.log")) {
        Ok(text) => (log::newest_first(&text, LOG_LINES), String::new()),
        Err(error) => (Vec::new(), error.to_string()),
    };
    window.set_log_error(error.into());
    let lines: Vec<LogLine> = lines
        .into_iter()
        .map(|line| LogLine {
            time: line.time.into(),
            level: line.level.into(),
            message: line.message.into(),
        })
        .collect();
    window.set_log_lines(ModelRc::from(Rc::new(VecModel::from(lines))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_asked_for_meanwhile_become_one_more_run() {
        let mut checks = Checks::default();
        assert_eq!(checks.start(), Some(1));
        assert_eq!(checks.start(), None);
        assert_eq!(checks.start(), None);
        assert_eq!(checks.finish(1), (true, true));
        assert_eq!(checks.start(), Some(2));
        assert_eq!(checks.finish(2), (true, false));
    }

    #[test]
    fn a_run_under_way_shows_nothing_once_the_home_app_changed() {
        let mut checks = Checks::default();
        assert_eq!(checks.start(), Some(1));
        checks.forget_home();
        assert_eq!(checks.start(), None);
        assert_eq!(checks.finish(1), (false, true));
        assert_eq!(checks.start(), Some(3));
    }
}

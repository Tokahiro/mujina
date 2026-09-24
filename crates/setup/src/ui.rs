//! The Setup window. Checking the device and running the plan each happen on a thread of their
//! own.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use mujina_winutil::{clipboard, locale, shell, window as winutil_window};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, VecModel};

use crate::cli::Mode;
use crate::host_windows::{self, WindowsHost};
use crate::payload;
use crate::plan::{self, Choice, Facts, Package, Step};
use crate::run::{self, ErrorKind, Failure as RunFailure, Journal as _, Probe as _, StepState};

// Generated code follows Slint's conventions, not this workspace's lints.
#[allow(
    unused,
    unsafe_op_in_unsafe_fn,
    clippy::all,
    clippy::pedantic,
    clippy::undocumented_unsafe_blocks
)]
mod slint_ui {
    slint::include_modules!();
}

use slint_ui::{
    Failure, FailureKind, SetupPhase, SetupWindow, Situation, StepKind, StepRow,
    StepState as RowState,
};

/// The languages `lang/` has, English (the texts as written) first.
const LANGUAGES: [&str; 2] = ["en", "de"];

/// What the window knows between its callbacks, on its own thread.
#[derive(Default)]
struct State {
    facts: Option<Facts>,
    /// The failure shown, for Copy details.
    failure: Option<RunFailure>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

pub fn run(mode: Mode, log: Option<PathBuf>) -> ExitCode {
    let Ok(window) = SetupWindow::new() else {
        return ExitCode::FAILURE;
    };
    // No language chosen (Setup has no config.toml): the first Windows display language it has.
    // Slint wants a window to exist first; this fails only without bundled translations.
    let _ = slint::select_bundled_translation(locale::language("", &LANGUAGES));
    let log = host_windows::log_file(log);
    let version = payload::attached().map_or_else(
        |_| env!("CARGO_PKG_VERSION").to_string(),
        |payload| payload.about.version.to_string(),
    );
    window.set_version(version.into());
    window.set_uninstall(mode == Mode::Uninstall);
    window.set_has_log(log.is_some());
    connect(&window, mode, log.as_deref());

    // Closing while a step runs would cut it off: the close button and Alt+F4 do nothing then.
    let weak = window.as_weak();
    window.window().on_close_requested(move || {
        let running = weak
            .upgrade()
            .is_some_and(|window| window.get_phase() == SetupPhase::Running);
        if running {
            CloseRequestResponse::KeepWindowShown
        } else {
            CloseRequestResponse::HideWindow
        }
    });

    prepare(&window, mode, log.as_deref());
    use_own_icon(20);
    match window.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn connect(window: &SetupWindow, mode: Mode, log: Option<&Path>) {
    let weak = window.as_weak();
    window.on_toggle_home_app(move || {
        if let Some(window) = weak.upgrade() {
            window.set_make_home_app(!window.get_make_home_app());
            let facts = STATE.with_borrow(|state| state.facts);
            if let Some(facts) = facts {
                show_plan(&window, &planned(&facts, &window));
            }
        }
    });
    let weak = window.as_weak();
    let path = log.map(Path::to_path_buf);
    window.on_begin(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let steps = match mode {
            Mode::Uninstall => plan::uninstall(),
            Mode::Install => match STATE.with_borrow(|state| state.facts) {
                Some(facts) => planned(&facts, &window),
                None => return,
            },
        };
        start(&window, steps, path.as_deref());
    });
    let weak = window.as_weak();
    let path = log.map(Path::to_path_buf);
    window.on_try_again(move || {
        if let Some(window) = weak.upgrade() {
            STATE.with_borrow_mut(|state| state.failure = None);
            prepare(&window, mode, path.as_deref());
        }
    });
    let path = log.map(Path::to_path_buf);
    window.on_copy_details(move || {
        if let Some(text) = STATE.with_borrow(|state| {
            state
                .failure
                .as_ref()
                .map(|failure| details(failure, path.as_deref()))
        }) {
            // Nothing to report if the clipboard is busy; the log has it all.
            let _ = clipboard::set_text(&text);
        }
    });
    let path = log.map(Path::to_path_buf);
    window.on_open_log(move || {
        if let Some(path) = &path {
            shell::open(&path.to_string_lossy());
        }
    });
    window.on_open_settings(open_settings);
    let weak = window.as_weak();
    window.on_quit(move || {
        if let Some(window) = weak.upgrade() {
            let _ = window.hide();
        }
    });
}

/// The window before a run. Installing first checks the device on a thread of its own: the
/// package query and the certificate store take a moment.
fn prepare(window: &SetupWindow, mode: Mode, log: Option<&Path>) {
    if mode == Mode::Uninstall {
        show_plan(window, &plan::uninstall());
        window.set_phase(SetupPhase::Ready);
        return;
    }
    window.set_phase(SetupPhase::Checking);
    window.set_steps(ModelRc::default());
    let weak = window.as_weak();
    let log = log.map(Path::to_path_buf);
    std::thread::spawn(move || {
        let host = WindowsHost::new(log);
        let facts = host.facts();
        host.note(&format!("facts: {facts:?}"));
        let _ = weak.upgrade_in_event_loop(move |window| found(&window, &facts));
    });
}

fn found(window: &SetupWindow, facts: &Facts) {
    window.set_situation(situation(facts));
    let installed = facts.installed.map(|version| version.to_string());
    window.set_installed_version(installed.unwrap_or_default().into());
    let build = facts.os_build.map(|build| build.to_string());
    window.set_os_build(build.unwrap_or_default().into());
    window.set_prepared(facts.preparation().complete());
    window.set_stops_agent(plan::stops_agent(facts));
    let before = STATE.with_borrow_mut(|state| state.facts.replace(*facts));
    if let Some(on) = starting_switch(before.as_ref(), facts) {
        window.set_make_home_app(on);
    }
    show_plan(window, &planned(facts, window));
    window.set_phase(SetupPhase::Ready);
}

/// The switch's default the first time. `None` on Try again: a failed run may have installed the
/// package, which would turn a first installation's default off.
fn starting_switch(before: Option<&Facts>, facts: &Facts) -> Option<bool> {
    before.is_none().then(|| plan::home_app_default(facts))
}

fn situation(facts: &Facts) -> Situation {
    match (facts.carried, plan::preflight(facts)) {
        (None, _) => Situation::NoPackage,
        (Some(_), Some(plan::Blocker::WindowsTooOld { .. })) => Situation::WindowsTooOld,
        (Some(carried), None) => match Package::over(facts.installed, carried) {
            Package::Install => Situation::Install,
            Package::Update { .. } => Situation::Update,
            Package::Reinstall => Situation::Reinstall,
            Package::Downgrade { .. } => Situation::Downgrade,
        },
    }
}

/// Empty where nothing can be installed. Pressing Replace is the confirmation a downgrade needs.
fn planned(facts: &Facts, window: &SetupWindow) -> Vec<Step> {
    let choice = Choice {
        make_home_app: window.get_make_home_app(),
        replace_newer: true,
    };
    plan::install(facts, choice).unwrap_or_default()
}

fn show_plan(window: &SetupWindow, steps: &[Step]) {
    let rows: Vec<StepRow> = steps
        .iter()
        .map(|step| StepRow {
            kind: kind(*step),
            state: RowState::Todo,
        })
        .collect();
    window.set_steps(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// Preparing names only what is still missing, so that the prompt says what it is for.
fn kind(step: Step) -> StepKind {
    match step {
        Step::Preflight => StepKind::CheckDevice,
        Step::Prepare(done) => match (done.developer_mode, done.certificate) {
            (true, false) => StepKind::TrustCertificate,
            (false, true) => StepKind::TurnOnDeveloperMode,
            _ => StepKind::Prepare,
        },
        Step::Package(Package::Install) => StepKind::InstallPackage,
        Step::Package(Package::Update { .. }) => StepKind::UpdatePackage,
        Step::Package(Package::Reinstall) => StepKind::ReinstallPackage,
        Step::Package(Package::Downgrade { .. }) => StepKind::ReplacePackage,
        Step::MakeHomeApp => StepKind::MakeHomeApp,
        Step::ArrangeCleanup => StepKind::ArrangeCleanup,
        Step::GiveHomeAppBack => StepKind::GiveHomeAppBack,
        Step::RemovePackage => StepKind::RemovePackage,
        Step::ForgetCreatedFiles => StepKind::ForgetCreatedFiles,
        Step::RemoveCleanup => StepKind::RemoveCleanup,
    }
}

/// The administrator prompt is owned by this window, so it comes up in front of it.
fn start(window: &SetupWindow, steps: Vec<Step>, log: Option<&Path>) {
    show_plan(window, &steps);
    window.set_phase(SetupPhase::Running);
    let weak = window.as_weak();
    let log = log.map(Path::to_path_buf);
    std::thread::spawn(move || {
        let owner = winutil_window::main_window_of(&[std::process::id()]);
        let host = WindowsHost::new(log).with_owner(owner);
        let outcome = run::run(&host, &steps, &mut Rows(weak.clone()));
        let _ = weak.upgrade_in_event_loop(move |window| finished(&window, outcome));
    });
}

struct Rows(slint::Weak<SetupWindow>);

impl run::Progress for Rows {
    fn update(&mut self, index: usize, state: StepState) {
        let state = match state {
            StepState::Running => RowState::Running,
            StepState::Done => RowState::Done,
            StepState::Failed => RowState::Failed,
        };
        let _ = self.0.upgrade_in_event_loop(move |window| {
            let steps = window.get_steps();
            if let Some(mut row) = steps.row_data(index) {
                row.state = state;
                steps.set_row_data(index, row);
            }
        });
    }
}

fn finished(window: &SetupWindow, outcome: Result<(), RunFailure>) {
    match outcome {
        Ok(()) => window.set_phase(SetupPhase::Done),
        Err(failure) => {
            window.set_failure(shown_failure(&failure));
            STATE.with_borrow_mut(|state| state.failure = Some(failure));
            window.set_phase(SetupPhase::Failed);
        }
    }
}

fn shown_failure(failure: &RunFailure) -> Failure {
    let error = &failure.error;
    Failure {
        kind: failure_kind(error.kind),
        code: error.code.map(code_text).unwrap_or_default().into(),
        detail: error.detail.as_str().into(),
    }
}

/// An HRESULT in hex, as Microsoft's tables list them; an exit code or a Win32 error, which have
/// no high bit, in decimal.
fn code_text(code: u32) -> String {
    if code & 0x8000_0000 == 0 {
        code.to_string()
    } else {
        format!("{code:#010X}")
    }
}

fn failure_kind(kind: ErrorKind) -> FailureKind {
    match kind {
        ErrorKind::Declined => FailureKind::Declined,
        ErrorKind::ElevationFailed => FailureKind::ElevationFailed,
        ErrorKind::ElevatedArguments => FailureKind::ElevatedArguments,
        ErrorKind::DeveloperMode => FailureKind::DeveloperMode,
        ErrorKind::Certificate => FailureKind::Certificate,
        ErrorKind::Preparing => FailureKind::Preparing,
        ErrorKind::NoPackage => FailureKind::NoPackage,
        ErrorKind::WindowsTooOld => FailureKind::WindowsTooOld,
        ErrorKind::NewerInstalled => FailureKind::NewerInstalled,
        ErrorKind::SameVersionDiffers => FailureKind::SameVersionDiffers,
        ErrorKind::NotTrusted => FailureKind::NotTrusted,
        ErrorKind::SideloadingOff => FailureKind::SideloadingOff,
        ErrorKind::BlockedByPolicy => FailureKind::BlockedByPolicy,
        ErrorKind::InUse => FailureKind::InUse,
        ErrorKind::DiskFull => FailureKind::DiskFull,
        ErrorKind::Conflict => FailureKind::Conflict,
        ErrorKind::Damaged => FailureKind::Damaged,
        ErrorKind::DeploymentFailed => FailureKind::DeploymentFailed,
        ErrorKind::NotInstalled => FailureKind::NotInstalled,
        ErrorKind::HomeApp => FailureKind::HomeApp,
        ErrorKind::SignInCheck => FailureKind::SignInCheck,
        ErrorKind::Files => FailureKind::Files,
        ErrorKind::Other => FailureKind::Other,
    }
}

/// What Copy details puts on the clipboard: English, for a bug report.
fn details(failure: &RunFailure, log: Option<&Path>) -> String {
    let log = log.map_or_else(
        || "none (Setup ran with administrator rights)".into(),
        Path::to_string_lossy,
    );
    let package = payload::attached().map_or_else(
        |missing| missing.to_string(),
        |payload| format!("{} {}", payload.about.family, payload.about.version),
    );
    format!(
        "Mujina Setup {}\r\nPackage: {package}\r\nStep: {:?}\r\nError: {}\r\nLog: {}",
        env!("CARGO_PKG_VERSION"),
        failure.step,
        failure.error,
        log
    )
}

/// Through the shell, by the package's app ID; Setup starts no program itself.
fn open_settings() {
    if let Ok(payload) = payload::attached() {
        shell::open(&format!(
            r"shell:AppsFolder\{}!Settings",
            payload.about.family
        ));
    }
}

/// Swaps Slint's one large window icon for the executable's own small images, once Slint has
/// made the window (when the event loop starts). Tries `attempts` times.
fn use_own_icon(attempts: u8) {
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        if !winutil_window::use_own_icon() && attempts > 1 {
            use_own_icon(attempts - 1);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Version;
    use crate::run::StepError;

    fn facts() -> Facts {
        Facts {
            os_build: Some(26200),
            dev_mode: true,
            cert_trusted: true,
            installed: None,
            carried: Version::parse("0.28.0.0"),
            home_app_is_this: false,
            agent_running: false,
        }
    }

    #[test]
    fn the_window_names_what_installing_is_here() {
        assert_eq!(situation(&facts()), Situation::Install);
        let with = |installed: &str| Facts {
            installed: Version::parse(installed),
            ..facts()
        };
        assert_eq!(situation(&with("0.27.0.0")), Situation::Update);
        assert_eq!(situation(&with("0.28.0.0")), Situation::Reinstall);
        assert_eq!(situation(&with("0.28.0.1")), Situation::Downgrade);
        let old = Facts {
            os_build: Some(22631),
            ..facts()
        };
        assert_eq!(situation(&old), Situation::WindowsTooOld);
        let none = Facts {
            carried: None,
            ..old
        };
        assert_eq!(situation(&none), Situation::NoPackage);
    }

    #[test]
    fn try_again_keeps_the_home_app_switch_as_the_run_found_it() {
        // A first installation: the switch starts on.
        assert_eq!(starting_switch(None, &facts()), Some(true));
        // The package went on, then a later step failed. Found again, Mujina is installed and
        // not the home app, whose default is off: the switch keeps what the user ran with.
        let installed = Facts {
            installed: Version::parse("0.28.0.0"),
            ..facts()
        };
        assert_eq!(starting_switch(None, &installed), Some(false));
        assert_eq!(starting_switch(Some(&facts()), &installed), None);
    }

    #[test]
    fn preparing_names_only_what_is_missing() {
        let prepared = |developer_mode, certificate| {
            kind(Step::Prepare(plan::Preparation {
                developer_mode,
                certificate,
            }))
        };
        assert_eq!(prepared(true, false), StepKind::TrustCertificate);
        assert_eq!(prepared(false, true), StepKind::TurnOnDeveloperMode);
        assert_eq!(prepared(false, false), StepKind::Prepare);
    }

    #[test]
    fn a_failure_keeps_its_code_as_windows_gives_it() {
        let failure = RunFailure {
            index: 1,
            step: Step::Package(Package::Install),
            error: StepError::with_code(ErrorKind::InUse, 0x8007_3D02, "in use"),
        };
        let shown = shown_failure(&failure);
        assert_eq!(shown.kind, FailureKind::InUse);
        assert_eq!(shown.code.as_str(), "0x80073D02");
        assert_eq!(code_text(3), "3");
        let copied = details(&failure, Some(Path::new(r"C:\log\setup.log")));
        assert!(
            copied.contains("Error: InUse (code 0x80073D02): in use"),
            "{copied}"
        );
        assert!(copied.ends_with(r"Log: C:\log\setup.log"), "{copied}");
        // As administrator without --log there is none, and the details say why.
        let copied = details(&failure, None);
        assert!(
            copied.ends_with("Log: none (Setup ran with administrator rights)"),
            "{copied}"
        );
    }
}

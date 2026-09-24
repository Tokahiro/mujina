//! Mujina Setup: installs, updates and removes Mujina with the steps around the package (see
//! ADR-0012 and ADR-0015). `mujina-setup.exe --help` prints [`cli::USAGE`]: the window by
//! default, `--quiet` without it, `--uninstall`, `--about`. `--elevated` is the administrator
//! part, which the installer starts itself and which takes no arguments; `--cleanup` is the check
//! at sign-in that gives the home app back once Mujina has been removed through Settings → Apps.
//!
//! The crate is cut so that what decides is pure and tested on any system: `plan` (which steps),
//! `run` (carrying them out over small traits, with a fake host in its tests), `cli`, `payload`'s
//! reading of what is attached, and `journal`. Windows runs `host_windows` and `ui`.

#![cfg_attr(windows, windows_subsystem = "windows")]

// Pure and tested everywhere; only Windows runs them.
#[cfg_attr(not(windows), allow(dead_code))]
mod cli;
#[cfg_attr(not(windows), allow(dead_code))]
mod journal;
#[cfg_attr(not(windows), allow(dead_code))]
mod payload;
#[cfg_attr(not(windows), allow(dead_code))]
mod plan;
#[cfg_attr(not(windows), allow(dead_code))]
mod run;

#[cfg(windows)]
mod host_windows;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::process::ExitCode;

    use cli::{Exit, Mode, Request};
    use host_windows::WindowsHost;

    // Before anything loads a DLL: none from the folder this program was started from, which is
    // usually Downloads.
    let _ = mujina_winutil::library::search_only_system32();
    let request = match cli::parse(std::env::args_os().skip(1)) {
        Ok(request) => request,
        Err(error) => {
            mujina_winutil::console::attach_parent();
            eprintln!("error: {error}\n\n{}", cli::USAGE);
            return ExitCode::from(Exit::Usage.code());
        }
    };
    match request {
        Request::Elevated => host_windows::elevated(),
        Request::Cleanup { log } => {
            run::sign_in(&WindowsHost::new(host_windows::log_file(log)));
            ExitCode::SUCCESS
        }
        Request::About => about(),
        Request::Help => {
            mujina_winutil::console::attach_parent();
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Request::Window { mode, log } => {
            if relaunched_outside_package(mode, log.as_deref()) {
                return ExitCode::SUCCESS;
            }
            ui::run(mode, log)
        }
        Request::Quiet {
            mode,
            no_home_app,
            log,
        } => {
            mujina_winutil::console::attach_parent();
            let host = WindowsHost::new(host_windows::log_file(log));
            let outcome = match mode {
                Mode::Install => run::install_unattended(&host, no_home_app),
                Mode::Uninstall => run::uninstall_unattended(&host),
            };
            let exit = Exit::of(&outcome);
            if exit != Exit::Done {
                match host.log_path() {
                    Some(log) => eprintln!("{outcome:?}\nThe log: {}", log.display()),
                    None => eprintln!(
                        "{outcome:?}\nNo log: with administrator rights, Mujina Setup writes one \
                         only where --log says"
                    ),
                }
            }
            ExitCode::from(exit.code())
        }
    }
}

/// `--about`: what package this installer carries, or that it carries none.
#[cfg(windows)]
fn about() -> std::process::ExitCode {
    mujina_winutil::console::attach_parent();
    match payload::attached() {
        Ok(payload) => {
            println!("Mujina Setup {}", env!("CARGO_PKG_VERSION"));
            println!("{}", payload.about);
            std::process::ExitCode::SUCCESS
        }
        Err(missing) => {
            println!("Mujina Setup {}: {missing}", env!("CARGO_PKG_VERSION"));
            std::process::ExitCode::from(cli::Exit::Failed.code())
        }
    }
}

/// Set in the environment of Setup started again outside the package, so that it does so once.
#[cfg(windows)]
const RELAUNCHED: &str = "MUJINA_SETUP_OUTSIDE_PACKAGE";

/// Starts this program again outside Mujina's package, if it runs inside it, and says whether it
/// did. Mujina Settings starts Setup to remove Mujina; a process of the package would be stopped
/// by the removal it runs. Settings already asks Windows to start it outside
/// (`spawn_outside_package`); whether that reaches Setup itself or only what Setup starts is
/// not documented, so Setup makes sure.
#[cfg(windows)]
fn relaunched_outside_package(mode: cli::Mode, log: Option<&std::path::Path>) -> bool {
    use mujina_winutil::{package, process};

    if package::family_name().is_none() || std::env::var_os(RELAUNCHED).is_some() {
        return false;
    }
    let Ok(program) = std::env::current_exe() else {
        return false;
    };
    let log = log.map(|path| path.to_string_lossy().into_owned());
    let mut arguments = Vec::new();
    if mode == cli::Mode::Uninstall {
        arguments.push("--uninstall");
    }
    if let Some(log) = &log {
        arguments.extend(["--log", log.as_str()]);
    }
    // SAFETY: nothing else runs yet: this is the start of `main`, before any thread or window.
    unsafe { std::env::set_var(RELAUNCHED, "1") };
    // A log path with a space is refused; then this one goes on inside the package.
    process::spawn_outside_package(&program, &arguments).is_ok()
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("Mujina Setup only runs on Windows.");
    std::process::ExitCode::FAILURE
}

//! The command line, and the exit codes a script can rely on.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::plan::Refusal;
use crate::run::{ErrorKind, Unattended};

/// Starts the administrator part. It takes no other arguments: it acts only on what is attached
/// to its own file, never on anything an unelevated program could have written or named.
pub const ELEVATED: &str = "--elevated";

pub const USAGE: &str = "\
mujina-setup.exe [OPTIONS]

Without options, Mujina Setup opens its window to install or update Mujina.

OPTIONS:
    --quiet               Install or update without a window. Windows still asks once for
                          administrator rights if Developer Mode is off or Mujina's certificate
                          is not trusted yet. On an update the home app stays as it is.
    --no-home-app         With --quiet: do not make Mujina the home app of Xbox mode
    --uninstall           Remove Mujina (the window, or with --quiet none)
    --log <PATH>          Write the log there instead of %LOCALAPPDATA%\\Mujina Setup\\setup.log
                          (run as administrator, Setup writes a log only with --log)
    --about               Print what package this installer carries
    --help                Print this text

EXIT CODES:
    0  done
    1  a step failed (the log says which and why)
    2  wrong options
    3  the administrator prompt was declined
    4  Mujina cannot be installed here (Windows too old)
    5  a newer version of Mujina is installed (--quiet never replaces it)
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Window {
        mode: Mode,
        log: Option<PathBuf>,
    },
    Quiet {
        mode: Mode,
        no_home_app: bool,
        log: Option<PathBuf>,
    },
    /// The administrator part, which the installer starts itself.
    Elevated,
    /// The check at sign-in, for the attached package's family.
    Cleanup {
        log: Option<PathBuf>,
    },
    About,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Install,
    Uninstall,
}

/// Exit codes for scripts; the administrator part ends with an [`ElevatedFailure`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Done = 0,
    Failed = 1,
    Usage = 2,
    Declined = 3,
    Blocked = 4,
    NewerInstalled = 5,
}

impl Exit {
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn of(outcome: &Unattended) -> Self {
        match outcome {
            Unattended::Done => Self::Done,
            Unattended::Refused(Refusal::Blocked(_)) => Self::Blocked,
            Unattended::Refused(Refusal::NewerInstalled { .. }) => Self::NewerInstalled,
            Unattended::Refused(Refusal::NoPackage) => Self::Failed,
            Unattended::Failed(failure) => match failure.error.kind {
                ErrorKind::Declined => Self::Declined,
                ErrorKind::WindowsTooOld => Self::Blocked,
                // A downgrade the facts did not foresee: Windows refused the package itself.
                ErrorKind::NewerInstalled => Self::NewerInstalled,
                _ => Self::Failed,
            },
        }
    }
}

pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Request, String> {
    let arguments: Vec<OsString> = arguments.into_iter().collect();
    // The administrator part is `--elevated` alone, and nothing that merely contains it.
    if arguments.first().is_some_and(|first| first == ELEVATED) {
        return if arguments.len() == 1 {
            Ok(Request::Elevated)
        } else {
            Err("--elevated takes no arguments".to_string())
        };
    }
    parse_options(arguments).map_err(|error| error.to_string())
}

fn parse_options(arguments: Vec<OsString>) -> Result<Request, lexopt::Error> {
    use lexopt::prelude::*;

    let mut parser = lexopt::Parser::from_args(arguments);
    let (mut quiet, mut no_home_app, mut uninstall, mut about, mut cleanup) =
        (false, false, false, false, false);
    let mut log = None;
    while let Some(argument) = parser.next()? {
        match argument {
            Long("quiet") => quiet = true,
            Long("no-home-app") => no_home_app = true,
            Long("uninstall") => uninstall = true,
            Long("about") => about = true,
            Long("cleanup") => cleanup = true,
            Long("log") => log = Some(PathBuf::from(parser.value()?)),
            Short('h') | Long("help") => return Ok(Request::Help),
            Long("elevated") => return Err("--elevated comes alone".into()),
            other => return Err(other.unexpected()),
        }
    }
    let alone = |what: &str| Err(lexopt::Error::from(format!("{what} comes alone")));
    if about {
        return if quiet || no_home_app || uninstall || cleanup || log.is_some() {
            alone("--about")
        } else {
            Ok(Request::About)
        };
    }
    if cleanup {
        return if quiet || no_home_app || uninstall {
            alone("--cleanup")
        } else {
            Ok(Request::Cleanup { log })
        };
    }
    if no_home_app && (!quiet || uninstall) {
        return Err("--no-home-app goes with --quiet, for an installation".into());
    }
    let mode = if uninstall {
        Mode::Uninstall
    } else {
        Mode::Install
    };
    Ok(if quiet {
        Request::Quiet {
            mode,
            no_home_app,
            log,
        }
    } else {
        Request::Window { mode, log }
    })
}

/// The administrator part's exit codes on failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevatedFailure {
    Arguments = 2,
    DeveloperMode = 3,
    /// The certificate could not be added to the trusted people, or the installer carries none.
    Certificate = 4,
}

impl ElevatedFailure {
    pub fn code(self) -> u8 {
        self as u8
    }

    /// `None` for a code the administrator part never ends with on its own, such as a crash's.
    pub fn from_code(code: u32) -> Option<Self> {
        [Self::Arguments, Self::DeveloperMode, Self::Certificate]
            .into_iter()
            .find(|failure| u32::from(failure.code()) == code)
    }

    pub fn kind(self) -> ErrorKind {
        match self {
            Self::Arguments => ErrorKind::ElevatedArguments,
            Self::DeveloperMode => ErrorKind::DeveloperMode,
            Self::Certificate => ErrorKind::Certificate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Blocker, Step};
    use crate::run::{Failure, StepError};

    fn parsed(arguments: &[&str]) -> Result<Request, String> {
        parse(arguments.iter().map(OsString::from))
    }

    fn window(mode: Mode) -> Request {
        Request::Window { mode, log: None }
    }

    #[test]
    fn the_administrator_part_takes_no_arguments() {
        assert_eq!(parsed(&["--elevated"]), Ok(Request::Elevated));
        assert!(parsed(&["--elevated", "evil.cer"]).is_err());
        assert!(parsed(&["--elevated", ""]).is_err());
        assert!(parsed(&["--elevated", "--elevated"]).is_err());
        // Anywhere else it is refused, rather than taken for the window.
        assert!(parsed(&["--uninstall", "--elevated"]).is_err());
        assert!(parsed(&["--elevated=x"]).is_err());
    }

    #[test]
    fn the_check_at_sign_in_takes_no_family() {
        assert_eq!(parsed(&["--cleanup"]), Ok(Request::Cleanup { log: None }));
        // The family is the attached package's: a name after it is refused.
        assert!(parsed(&["--cleanup", "Mujina_k2veznmcx4n98"]).is_err());
        assert!(parsed(&["--cleanup", "--quiet"]).is_err());
    }

    #[test]
    fn the_window_is_the_default_and_unknown_options_are_refused() {
        assert_eq!(parsed(&[]), Ok(window(Mode::Install)));
        assert_eq!(parsed(&["--uninstall"]), Ok(window(Mode::Uninstall)));
        assert!(parsed(&["--something"]).is_err());
        assert!(parsed(&["install"]).is_err());
    }

    #[test]
    fn quiet_runs_install_or_remove_without_a_window() {
        assert_eq!(
            parsed(&["--quiet"]),
            Ok(Request::Quiet {
                mode: Mode::Install,
                no_home_app: false,
                log: None
            })
        );
        assert_eq!(
            parsed(&["--quiet", "--no-home-app", "--log", r"C:\logs\setup.log"]),
            Ok(Request::Quiet {
                mode: Mode::Install,
                no_home_app: true,
                log: Some(PathBuf::from(r"C:\logs\setup.log"))
            })
        );
        assert_eq!(
            parsed(&["--uninstall", "--quiet"]),
            Ok(Request::Quiet {
                mode: Mode::Uninstall,
                no_home_app: false,
                log: None
            })
        );
        assert!(parsed(&["--no-home-app"]).is_err());
        assert!(parsed(&["--uninstall", "--quiet", "--no-home-app"]).is_err());
        assert!(parsed(&["--log"]).is_err());
    }

    #[test]
    fn about_comes_alone() {
        assert_eq!(parsed(&["--about"]), Ok(Request::About));
        assert!(parsed(&["--about", "--quiet"]).is_err());
        assert_eq!(parsed(&["--help"]), Ok(Request::Help));
        assert_eq!(parsed(&["-h"]), Ok(Request::Help));
    }

    #[test]
    fn every_ending_has_its_own_exit_code() {
        let failed = |kind| {
            Unattended::Failed(Failure {
                index: 0,
                step: Step::Preflight,
                error: StepError::new(kind, ""),
            })
        };
        let cases = [
            (Unattended::Done, Exit::Done),
            (failed(ErrorKind::DeploymentFailed), Exit::Failed),
            (failed(ErrorKind::Declined), Exit::Declined),
            (failed(ErrorKind::WindowsTooOld), Exit::Blocked),
            (failed(ErrorKind::NewerInstalled), Exit::NewerInstalled),
            (
                Unattended::Refused(Refusal::Blocked(Blocker::WindowsTooOld { build: 1 })),
                Exit::Blocked,
            ),
            (
                Unattended::Refused(Refusal::NewerInstalled {
                    installed: crate::plan::Version::parse("1.0.0.0").unwrap(),
                }),
                Exit::NewerInstalled,
            ),
            (Unattended::Refused(Refusal::NoPackage), Exit::Failed),
        ];
        for (outcome, exit) in cases {
            assert_eq!(Exit::of(&outcome), exit, "{outcome:?}");
        }
        let codes = [
            Exit::Done,
            Exit::Failed,
            Exit::Usage,
            Exit::Declined,
            Exit::Blocked,
            Exit::NewerInstalled,
        ]
        .map(Exit::code);
        for (index, code) in codes.iter().enumerate() {
            assert!(!codes[..index].contains(code), "{code}");
        }
    }

    #[test]
    fn every_failure_of_the_administrator_part_has_its_own_code() {
        let failures = [
            ElevatedFailure::Arguments,
            ElevatedFailure::DeveloperMode,
            ElevatedFailure::Certificate,
        ];
        for failure in failures {
            // Neither success nor the generic failure code of a crashed or killed process.
            assert!(failure.code() > 1, "{failure:?}");
            assert_eq!(
                ElevatedFailure::from_code(failure.code().into()),
                Some(failure)
            );
        }
        assert_eq!(ElevatedFailure::from_code(0), None);
        assert_eq!(ElevatedFailure::from_code(1), None);
    }
}

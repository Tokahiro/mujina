//! Self-diagnosis in one command, readable on a device without a keyboard or sent to someone.

use std::fmt::Write as _;

use crate::Msg;
use crate::ports::{
    FseState, FullScreenExperience, HomeAppRegistry, HomeLauncher, LauncherState, PackageIdentity,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Warning,
    Problem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable, English and lower case, e.g. `developer mode`; Mujina Settings looks it up by this.
    pub id: &'static str,
    /// Its name in Mujina Settings, e.g. "Developer Mode", translated by its crate's catalog.
    pub title: Msg,
    pub severity: Severity,
    /// What it found, in English, as `mujinactl doctor` and the log say it.
    pub detail: String,
    /// A translated sentence for Mujina Settings' System page; `None` when the detail says it all.
    pub summary: Option<Msg>,
    pub remedy: Option<Remedy>,
}

impl Finding {
    #[must_use]
    pub fn saying(self, summary: Msg) -> Self {
        Self {
            summary: Some(summary),
            ..self
        }
    }

    #[must_use]
    pub fn remedied_by(self, remedy: Remedy) -> Self {
        Self {
            remedy: Some(remedy),
            ..self
        }
    }
}

/// What a page can offer to fix a finding; the check chooses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remedy {
    /// Windows' page of location permissions.
    LocationSettings,
    StartAgent,
}

/// A doctor check from an adapter (OS settings, a launcher's files).
pub trait Check {
    /// Its findings' [`Finding::id`], known before examining.
    fn id(&self) -> &'static str;

    fn title(&self) -> Msg;

    fn examine(&self) -> Finding;

    fn found(&self, severity: Severity, detail: impl Into<String>) -> Finding
    where
        Self: Sized,
    {
        Finding {
            id: self.id(),
            title: self.title(),
            severity,
            detail: detail.into(),
            summary: None,
            remedy: None,
        }
    }
}

pub struct Doctor<'a> {
    pub fse: &'a dyn FullScreenExperience,
    pub identity: &'a dyn PackageIdentity,
    pub registry: &'a dyn HomeAppRegistry,
    pub launcher: &'a dyn HomeLauncher,
    /// Adapters' checks, run after the doctor's own: the operating system's, then the launcher's.
    pub checks: &'a [Box<dyn Check>],
}

/// Id of the doctor's Xbox mode check.
pub const FULL_SCREEN_EXPERIENCE: &str = "full screen experience";

const FSE: (&str, Msg) = (FULL_SCREEN_EXPERIENCE, Msg::new("Xbox mode"));
const PACKAGE: (&str, Msg) = ("package", Msg::new("Mujina's package"));
const HOME_APP: (&str, Msg) = ("home app", Msg::new("Home app"));
const LAUNCHER: (&str, Msg) = ("launcher", Msg::new("Launcher"));

fn finding((id, title): (&'static str, Msg), severity: Severity, detail: String) -> Finding {
    Finding {
        id,
        title,
        severity,
        detail,
        summary: None,
        remedy: None,
    }
}

type Own<'d> = (&'static str, fn(&Doctor<'d>) -> Finding);

impl<'d> Doctor<'d> {
    fn own() -> [Own<'d>; 4] {
        [
            (FSE.0, Self::check_fse),
            (PACKAGE.0, Self::check_package),
            (HOME_APP.0, Self::check_home_app),
            (LAUNCHER.0, Self::check_launcher),
        ]
    }

    pub fn examine(&self) -> Vec<Finding> {
        self.examine_only(|_| true)
    }

    /// Only the checks whose id `wanted` accepts, in [`examine`](Self::examine)'s order.
    pub fn examine_only(&self, wanted: impl Fn(&str) -> bool) -> Vec<Finding> {
        let own = Self::own()
            .into_iter()
            .filter(|(id, _)| wanted(id))
            .map(|(_, check)| check(self));
        let theirs = self
            .checks
            .iter()
            .filter(|check| wanted(check.id()))
            .map(|check| check.examine());
        own.chain(theirs).collect()
    }

    /// As [`examine`](Self::examine), but reuses the `known` findings.
    pub fn examine_knowing(&self, mut known: Vec<Finding>) -> Vec<Finding> {
        let mut take = |id: &str| {
            let at = known.iter().position(|finding| finding.id == id)?;
            Some(known.swap_remove(at))
        };
        let mut found = Vec::new();
        for (id, check) in Self::own() {
            found.push(take(id).unwrap_or_else(|| check(self)));
        }
        for check in self.checks {
            found.push(take(check.id()).unwrap_or_else(|| check.examine()));
        }
        found
    }

    fn check_fse(&self) -> Finding {
        let (severity, detail, summary) = match self.fse.state() {
            FseState::Active => (
                Severity::Ok,
                "active",
                Msg::new("On. You are in Xbox mode now."),
            ),
            FseState::Inactive => (
                Severity::Ok,
                "supported, currently on the desktop",
                Msg::new("Available. You are on the desktop now."),
            ),
            FseState::Unavailable => (
                Severity::Problem,
                "this Windows build has no full screen experience API",
                Msg::new("This Windows has no Xbox mode."),
            ),
        };
        finding(FSE, severity, detail.to_string()).saying(summary)
    }

    fn check_package(&self) -> Finding {
        match self.identity.app_user_model_id() {
            Some(id) => finding(PACKAGE, Severity::Ok, id),
            None => finding(
                PACKAGE,
                Severity::Problem,
                "running unpackaged; Windows only accepts a packaged app as home app".to_string(),
            ),
        }
    }

    fn check_home_app(&self) -> Finding {
        let ours = self.identity.app_user_model_id();
        let (severity, detail) = match self.registry.current() {
            Ok(Some(current)) if Some(&current) == ours.as_ref() => {
                (Severity::Ok, "Mujina is the home app".to_string())
            }
            Ok(Some(other)) => (Severity::Warning, format!("home app is {other}")),
            Ok(None) => (Severity::Warning, "no home app is configured".to_string()),
            Err(error) => (Severity::Problem, error.to_string()),
        };
        finding(HOME_APP, severity, detail)
    }

    fn check_launcher(&self) -> Finding {
        let name = self.launcher.display_name();
        let (severity, detail) = match self.launcher.locate() {
            Ok(install) => {
                let state = match self.launcher.state() {
                    LauncherState::NotRunning => "not running",
                    LauncherState::RunningWithoutUi => "running without console UI",
                    LauncherState::UiVisible => "console UI on screen",
                };
                (
                    Severity::Ok,
                    format!("{name} at {} ({state})", install.executable.display()),
                )
            }
            Err(error) => (Severity::Problem, format!("{name}: {error}")),
        };
        finding(LAUNCHER, severity, detail)
    }
}

/// Plain text, one finding per line.
pub fn render(findings: &[Finding]) -> String {
    let mut text = String::new();
    for finding in findings {
        let mark = match finding.severity {
            Severity::Ok => "ok  ",
            Severity::Warning => "warn",
            Severity::Problem => "FAIL",
        };
        // Writing into a String cannot fail.
        let _ = writeln!(text, "[{mark}] {}: {}", finding.id, finding.detail);
    }
    text
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::testing::{FakeFse, FakeHomeAppRegistry, FakeIdentity, FakeLauncher};

    #[test]
    fn healthy_system_has_no_complaints() {
        let fse = FakeFse(FseState::Active);
        let identity = FakeIdentity::packaged("Mujina_abc!App");
        let registry = FakeHomeAppRegistry::with_current(Some("Mujina_abc!App"));
        let launcher = FakeLauncher::installed(LauncherState::UiVisible);
        let doctor = Doctor {
            fse: &fse,
            identity: &identity,
            registry: &registry,
            launcher: &launcher,
            checks: &[],
        };

        let findings = doctor.examine();
        assert!(
            findings.iter().all(|f| f.severity == Severity::Ok),
            "{findings:?}"
        );
        assert_eq!(render(&findings).lines().count(), 4);
    }

    #[test]
    fn problems_are_named() {
        let fse = FakeFse(FseState::Unavailable);
        let identity = FakeIdentity::unpackaged();
        let registry = FakeHomeAppRegistry::with_current(Some("Other!App"));
        let launcher = FakeLauncher::missing();
        let doctor = Doctor {
            fse: &fse,
            identity: &identity,
            registry: &registry,
            launcher: &launcher,
            checks: &[],
        };

        let severities: Vec<_> = doctor.examine().iter().map(|f| f.severity).collect();
        assert_eq!(
            severities,
            [
                Severity::Problem,
                Severity::Problem,
                Severity::Warning,
                Severity::Problem
            ]
        );
    }

    struct Quiet;

    impl Check for Quiet {
        fn id(&self) -> &'static str {
            "quiet"
        }

        fn title(&self) -> Msg {
            Msg::new("Quiet")
        }

        fn examine(&self) -> Finding {
            self.found(Severity::Ok, "calm")
        }
    }

    #[test]
    fn a_page_asks_for_the_checks_it_shows_by_their_ids() {
        let fse = FakeFse(FseState::Active);
        let identity = FakeIdentity::unpackaged();
        let registry = FakeHomeAppRegistry::default();
        let launcher = FakeLauncher::missing();
        let checks: [Box<dyn Check>; 1] = [Box::new(Quiet)];
        let doctor = Doctor {
            fse: &fse,
            identity: &identity,
            registry: &registry,
            launcher: &launcher,
            checks: &checks,
        };
        let found = doctor.examine_only(|id| id == "quiet" || id == "full screen experience");
        let ids: Vec<&str> = found.iter().map(|finding| finding.id).collect();
        assert_eq!(ids, ["full screen experience", "quiet"]);
        assert_eq!(found[0].title, Msg::new("Xbox mode"));
        assert_eq!(
            found[1],
            Finding {
                id: "quiet",
                title: Msg::new("Quiet"),
                severity: Severity::Ok,
                detail: "calm".into(),
                summary: None,
                remedy: None,
            }
        );
        assert_eq!(render(&found[1..]), "[ok  ] quiet: calm\n");
    }

    struct Counted {
        id: &'static str,
        looked: Rc<Cell<u32>>,
    }

    impl Check for Counted {
        fn id(&self) -> &'static str {
            self.id
        }

        fn title(&self) -> Msg {
            Msg::new("Counted")
        }

        fn examine(&self) -> Finding {
            self.looked.set(self.looked.get() + 1);
            self.found(Severity::Ok, format!("look {}", self.looked.get()))
        }
    }

    #[test]
    fn what_a_page_found_a_moment_ago_is_not_looked_at_again() {
        let fse = FakeFse(FseState::Active);
        let identity = FakeIdentity::unpackaged();
        let registry = FakeHomeAppRegistry::default();
        let launcher = FakeLauncher::missing();
        let (first, second) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let checks: [Box<dyn Check>; 2] = [
            Box::new(Counted {
                id: "first",
                looked: Rc::clone(&first),
            }),
            Box::new(Counted {
                id: "second",
                looked: Rc::clone(&second),
            }),
        ];
        let doctor = Doctor {
            fse: &fse,
            identity: &identity,
            registry: &registry,
            launcher: &launcher,
            checks: &checks,
        };
        let known = doctor.examine_only(|id| id == "second" || id == "launcher");
        let all = doctor.examine_knowing(known.clone());
        let ids: Vec<&str> = all.iter().map(|finding| finding.id).collect();
        assert_eq!(
            ids,
            [
                "full screen experience",
                "package",
                "home app",
                "launcher",
                "first",
                "second"
            ]
        );
        assert_eq!((first.get(), second.get()), (1, 1));
        assert_eq!((&all[3], &all[5]), (&known[0], &known[1]));
    }

    #[test]
    fn a_finding_can_say_it_in_a_sentence_while_the_report_keeps_its_detail() {
        let identity = FakeIdentity::unpackaged();
        let registry = FakeHomeAppRegistry::default();
        let launcher = FakeLauncher::missing();
        let xbox_mode = |state| {
            let fse = FakeFse(state);
            let doctor = Doctor {
                fse: &fse,
                identity: &identity,
                registry: &registry,
                launcher: &launcher,
                checks: &[],
            };
            doctor
                .examine_only(|id| id == FULL_SCREEN_EXPERIENCE)
                .remove(0)
        };
        let desktop = xbox_mode(FseState::Inactive);
        assert_eq!(
            desktop.summary,
            Some(Msg::new("Available. You are on the desktop now."))
        );
        assert_eq!(
            render(&[desktop]),
            "[ok  ] full screen experience: supported, currently on the desktop\n"
        );
        let missing = xbox_mode(FseState::Unavailable);
        assert_eq!(
            (missing.severity, missing.summary),
            (
                Severity::Problem,
                Some(Msg::new("This Windows has no Xbox mode."))
            )
        );
        let quiet = Quiet
            .found(Severity::Warning, "loud")
            .saying(Msg::new("Loud."))
            .remedied_by(Remedy::StartAgent);
        assert_eq!(
            (quiet.summary, quiet.remedy),
            (Some(Msg::new("Loud.")), Some(Remedy::StartAgent))
        );
    }
}

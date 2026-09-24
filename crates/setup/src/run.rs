//! Carries out a plan over traits the host implements; the host only does what it is told.

use std::fmt;

use crate::plan::{self, Choice, Facts, Package, Refusal, Step};

/// The window words `kind` in the user's language; `code` and `detail` stay raw, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepError {
    pub kind: ErrorKind,
    /// An HRESULT, a Win32 error or an exit code, where there is one.
    pub code: Option<u32>,
    pub detail: String,
}

impl StepError {
    pub fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            code: None,
            detail: detail.into(),
        }
    }

    pub fn with_code(kind: ErrorKind, code: u32, detail: impl Into<String>) -> Self {
        Self {
            kind,
            code: Some(code),
            detail: detail.into(),
        }
    }

    /// A failed `Add-AppxPackage`: its kind from the HRESULT in what PowerShell said.
    pub fn deployment(output: &str) -> Self {
        match hresult_in(output) {
            Some(code) => Self::with_code(deployment_kind(code), code, output.trim()),
            None => Self::new(ErrorKind::DeploymentFailed, output.trim()),
        }
    }
}

/// `kind (code 0x…): detail`, as the log and Copy details have it.
impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.kind)?;
        if let Some(code) = self.code {
            write!(f, " (code {code:#010X})")?;
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

/// What went wrong, as far as the user can act on it; each has its sentence in `ui/setup.slint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The administrator prompt was declined: `ShellExecuteExW` reported `ERROR_CANCELLED`.
    Declined,
    /// Windows did not start the administrator part (a policy, say); `code` is the Win32 error.
    ElevationFailed,
    ElevatedArguments,
    DeveloperMode,
    Certificate,
    /// The administrator part ended with a code of none of the above (a crash, say).
    Preparing,
    NoPackage,
    WindowsTooOld,
    /// 0x80073D06, ERROR_INSTALL_PACKAGE_DOWNGRADE.
    NewerInstalled,
    /// 0x80073CFB, ERROR_PACKAGE_ALREADY_EXISTS: the same version, but not the same file.
    SameVersionDiffers,
    /// 0x800B0100, 0x800B0109, 0x800B010A: no signature, or its certificate is not trusted.
    NotTrusted,
    /// 0x80073CFF, ERROR_INSTALL_POLICY_FAILURE: sideloading is off (Developer Mode).
    SideloadingOff,
    /// 0x80073D01, ERROR_DEPLOYMENT_BLOCKED_BY_POLICY.
    BlockedByPolicy,
    /// 0x80073D02, ERROR_PACKAGES_IN_USE.
    InUse,
    /// 0x80073CF4, ERROR_INSTALL_OUT_OF_DISK_SPACE.
    DiskFull,
    /// 0x80073CF3, ERROR_INSTALL_RESOLVE_DEPENDENCY_FAILED: a conflict with an installed package.
    Conflict,
    /// 0x80073CF0, 0x80073CF2, 0x8007000B, 0x80080204…: the package cannot be opened or read.
    Damaged,
    /// Any other deployment error; `code` says which.
    DeploymentFailed,
    /// This Mujina (this package family) is not installed.
    NotInstalled,
    HomeApp,
    SignInCheck,
    /// A file Mujina placed in another program's folder could not be deleted.
    Files,
    Other,
}

/// The kind of a deployment error, by the HRESULTs of Microsoft's troubleshooting table
/// (learn.microsoft.com/windows/win32/appxpkg/troubleshooting).
pub fn deployment_kind(hresult: u32) -> ErrorKind {
    match hresult {
        0x8007_3D06 => ErrorKind::NewerInstalled,
        0x8007_3CFB => ErrorKind::SameVersionDiffers,
        0x800B_0100 | 0x800B_0109 | 0x800B_010A => ErrorKind::NotTrusted,
        0x8007_3CFF => ErrorKind::SideloadingOff,
        0x8007_3D01 => ErrorKind::BlockedByPolicy,
        0x8007_3D02 => ErrorKind::InUse,
        0x8007_3CF4 => ErrorKind::DiskFull,
        0x8007_3CF3 => ErrorKind::Conflict,
        0x8007_3CF0 | 0x8007_3CF2 | 0x8007_000B | 0x8008_0200..=0x8008_0216 => ErrorKind::Damaged,
        _ => ErrorKind::DeploymentFailed,
    }
}

/// The first HRESULT in `text` (`0x` and eight hex digits, a failure: high bit set), as
/// PowerShell's deployment errors name it: "Deployment failed with HRESULT: 0x80073CFB, …".
pub fn hresult_in(text: &str) -> Option<u32> {
    let bytes = text.as_bytes();
    (0..bytes.len()).find_map(|start| {
        let digits = text.get(start..)?.strip_prefix("0x")?.get(..8)?;
        let end = start + 2 + 8;
        // Exactly eight digits: not the start of a longer number.
        if bytes.get(end).is_some_and(u8::is_ascii_hexdigit) {
            return None;
        }
        u32::from_str_radix(digits, 16)
            .ok()
            .filter(|code| code & 0x8000_0000 != 0)
    })
}

/// Reads the device without changing anything.
pub trait Probe {
    fn facts(&self) -> Facts;
}

pub trait Trust {
    /// Developer Mode on and the certificate trusted, behind one prompt.
    fn prepare(&self) -> Result<(), StepError>;
}

pub trait Packages {
    fn add(&self, how: Package) -> Result<(), StepError>;
    /// Fails with [`ErrorKind::NotInstalled`] without this Mujina.
    fn remove(&self) -> Result<(), StepError>;
    fn installed(&self) -> bool;
}

pub trait HomeApp {
    fn make(&self) -> Result<(), StepError>;
    /// Gives the setting back if this Mujina has it; if another app has it, touches nothing.
    fn give_back(&self) -> Result<(), StepError>;
}

/// The check at sign-in, its copy of Setup, and its count of failed attempts.
pub trait SignInCheck {
    /// Also starts the count of failed attempts again.
    fn arrange_check(&self) -> Result<(), StepError>;
    /// Also removes the copy and the count of failed attempts.
    fn remove_check(&self) -> Result<(), StepError>;
    fn attempts(&self) -> u32;
    fn record_attempts(&self, attempts: u32) -> Result<(), StepError>;
}

/// The files Mujina made outside its own folders, as recorded when it made them.
pub trait CreatedFiles {
    /// Deletes every recorded file, then the record, which stays while a file cannot be deleted.
    fn forget(&self) -> Result<(), StepError>;
    fn remain(&self) -> bool;
}

pub trait Journal {
    fn note(&self, line: &str);
}

pub trait Host: Probe + Trust + Packages + HomeApp + SignInCheck + CreatedFiles + Journal {}

impl<T: Probe + Trust + Packages + HomeApp + SignInCheck + CreatedFiles + Journal> Host for T {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepState {
    Running,
    Done,
    Failed,
}

pub trait Progress {
    fn update(&mut self, index: usize, state: StepState);
}

impl Progress for () {
    fn update(&mut self, _: usize, _: StepState) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub index: usize,
    pub step: Step,
    pub error: StepError,
}

pub fn run(host: &impl Host, steps: &[Step], progress: &mut impl Progress) -> Result<(), Failure> {
    for (index, &step) in steps.iter().enumerate() {
        progress.update(index, StepState::Running);
        host.note(&format!("{step:?}: started"));
        match execute(host, step) {
            Ok(()) => {
                host.note(&format!("{step:?}: done"));
                progress.update(index, StepState::Done);
            }
            Err(error) => {
                host.note(&format!("{step:?}: failed: {error}"));
                progress.update(index, StepState::Failed);
                return Err(Failure { index, step, error });
            }
        }
    }
    Ok(())
}

fn execute(host: &impl Host, step: Step) -> Result<(), StepError> {
    match step {
        Step::Preflight => match plan::preflight(&host.facts()) {
            None => Ok(()),
            Some(plan::Blocker::WindowsTooOld { build }) => Err(StepError::new(
                ErrorKind::WindowsTooOld,
                format!(
                    "Windows build {build}; Mujina needs {}",
                    plan::REQUIRED_BUILD
                ),
            )),
        },
        Step::Prepare(_) => host.prepare(),
        Step::Package(how) => host.add(how),
        Step::ArrangeCleanup => {
            // The check is Mujina's only way back from a removal through Settings → Apps: only
            // for a package that is really there.
            if !host.installed() {
                return Err(StepError::new(
                    ErrorKind::NotInstalled,
                    "the installed package was not found",
                ));
            }
            host.arrange_check()
        }
        Step::MakeHomeApp => host.make(),
        Step::GiveHomeAppBack => host.give_back(),
        Step::RemovePackage => host.remove(),
        Step::ForgetCreatedFiles => host.forget(),
        Step::RemoveCleanup => {
            if host.remain() {
                host.note("files Mujina made are still listed: the check at sign-in stays");
                return Ok(());
            }
            host.remove_check()
        }
    }
}

/// How often the check at sign-in tries to give the home app back before it removes itself
/// anyway: a failure that keeps repeating will not go away.
pub const SIGN_IN_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedIn {
    Installed,
    /// All cleaned up, or the check stays only for files that could not be deleted yet.
    Done,
    /// Giving the home app back failed this many times; the check tries again next sign-in.
    WillRetry(u32),
    /// Giving the home app back failed [`SIGN_IN_ATTEMPTS`] times; the check removed itself.
    GaveUp,
}

/// The check at sign-in (`--cleanup`). Once Mujina is removed, it gives the home app back and
/// deletes Mujina's files; it removes itself only once the home app is back, or it gave up.
pub fn sign_in(host: &impl Host) -> SignedIn {
    if host.installed() {
        return SignedIn::Installed;
    }
    host.note("sign-in: Mujina is not installed any more");
    let home = host.give_back();
    if let Err(error) = &home {
        host.note(&format!(
            "sign-in: giving the home app back failed: {error}"
        ));
    }
    if let Err(error) = host.forget() {
        host.note(&format!("sign-in: removing Mujina's files failed: {error}"));
    }
    let give_up = home.is_err();
    if give_up {
        let attempts = host.attempts().saturating_add(1);
        if attempts < SIGN_IN_ATTEMPTS {
            if let Err(error) = host.record_attempts(attempts) {
                host.note(&format!("sign-in: counting the attempt failed: {error}"));
            }
            host.note(&format!(
                "sign-in: the check stays and tries again (attempt {attempts} of \
                 {SIGN_IN_ATTEMPTS})"
            ));
            return SignedIn::WillRetry(attempts);
        }
        host.note(&format!(
            "sign-in: the home app could not be given back {SIGN_IN_ATTEMPTS} times; the check \
             removes itself"
        ));
    }
    if host.remain() {
        host.note("sign-in: files Mujina made are still listed: the check stays");
    } else if let Err(error) = host.remove_check() {
        host.note(&format!("sign-in: removing the check failed: {error}"));
    }
    if give_up {
        SignedIn::GaveUp
    } else {
        SignedIn::Done
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unattended {
    Done,
    Refused(Refusal),
    Failed(Failure),
}

/// `--quiet`: installs as the window would with its default home app switch. A newer installed
/// version is never replaced: nobody could confirm it.
pub fn install_unattended(host: &impl Host, no_home_app: bool) -> Unattended {
    let facts = host.facts();
    host.note(&format!("facts: {facts:?}"));
    let choice = Choice {
        make_home_app: !no_home_app && plan::home_app_default(&facts),
        replace_newer: false,
    };
    match plan::install(&facts, choice) {
        Err(refusal) => {
            host.note(&format!("refused: {refusal:?}"));
            Unattended::Refused(refusal)
        }
        Ok(steps) => finish(run(host, &steps, &mut ())),
    }
}

pub fn uninstall_unattended(host: &impl Host) -> Unattended {
    finish(run(host, &plan::uninstall(), &mut ()))
}

fn finish(result: Result<(), Failure>) -> Unattended {
    match result {
        Ok(()) => Unattended::Done,
        Err(failure) => Unattended::Failed(failure),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::plan::Version;

    pub struct FakeHost {
        pub facts: Cell<Facts>,
        pub installed: Cell<bool>,
        /// The first call whose name is listed fails, once per listing.
        pub failing: RefCell<Vec<&'static str>>,
        pub calls: RefCell<Vec<String>>,
        pub log: RefCell<Vec<String>>,
        pub attempts: Cell<u32>,
        pub files_remain: Cell<bool>,
    }

    impl FakeHost {
        pub fn new(facts: Facts) -> Self {
            Self {
                installed: Cell::new(facts.installed.is_some()),
                facts: Cell::new(facts),
                failing: RefCell::default(),
                calls: RefCell::default(),
                log: RefCell::default(),
                attempts: Cell::new(0),
                files_remain: Cell::new(false),
            }
        }

        pub fn fail(&self, call: &'static str) {
            self.failing.borrow_mut().push(call);
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }

        fn call(&self, name: &str, kind: ErrorKind) -> Result<(), StepError> {
            self.calls.borrow_mut().push(name.to_string());
            let mut failing = self.failing.borrow_mut();
            match failing.iter().position(|failing| *failing == name) {
                Some(at) => {
                    failing.remove(at);
                    Err(StepError::new(kind, format!("{name} failed")))
                }
                None => Ok(()),
            }
        }
    }

    impl Probe for FakeHost {
        fn facts(&self) -> Facts {
            self.facts.get()
        }
    }

    impl Trust for FakeHost {
        fn prepare(&self) -> Result<(), StepError> {
            self.call("prepare", ErrorKind::Declined)
        }
    }

    impl Packages for FakeHost {
        fn add(&self, how: Package) -> Result<(), StepError> {
            let name = match how {
                Package::Install => "add",
                Package::Update { .. } => "update",
                Package::Reinstall => "reinstall",
                Package::Downgrade { .. } => "downgrade",
            };
            self.call(name, ErrorKind::DeploymentFailed)?;
            self.installed.set(true);
            Ok(())
        }

        fn remove(&self) -> Result<(), StepError> {
            if !self.installed.get() {
                self.calls.borrow_mut().push("remove".to_string());
                return Err(StepError::new(ErrorKind::NotInstalled, "not installed"));
            }
            self.call("remove", ErrorKind::DeploymentFailed)?;
            self.installed.set(false);
            Ok(())
        }

        fn installed(&self) -> bool {
            self.installed.get()
        }
    }

    impl HomeApp for FakeHost {
        fn make(&self) -> Result<(), StepError> {
            self.call("make home app", ErrorKind::HomeApp)
        }

        fn give_back(&self) -> Result<(), StepError> {
            self.call("give home app back", ErrorKind::HomeApp)
        }
    }

    impl SignInCheck for FakeHost {
        fn arrange_check(&self) -> Result<(), StepError> {
            self.call("arrange check", ErrorKind::SignInCheck)?;
            self.attempts.set(0);
            Ok(())
        }

        fn remove_check(&self) -> Result<(), StepError> {
            self.call("remove check", ErrorKind::SignInCheck)?;
            self.attempts.set(0);
            Ok(())
        }

        fn attempts(&self) -> u32 {
            self.attempts.get()
        }

        fn record_attempts(&self, attempts: u32) -> Result<(), StepError> {
            self.attempts.set(attempts);
            Ok(())
        }
    }

    impl CreatedFiles for FakeHost {
        fn forget(&self) -> Result<(), StepError> {
            self.call("forget files", ErrorKind::Files)
        }

        fn remain(&self) -> bool {
            self.files_remain.get()
        }
    }

    impl Journal for FakeHost {
        fn note(&self, line: &str) {
            self.log.borrow_mut().push(line.to_string());
        }
    }

    pub fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    pub fn installed_before() -> Facts {
        Facts {
            os_build: Some(26200),
            dev_mode: true,
            cert_trusted: true,
            installed: Some(version("0.27.0.0")),
            carried: Some(version("0.28.0.0")),
            home_app_is_this: true,
            agent_running: false,
        }
    }

    pub fn untouched() -> Facts {
        Facts {
            dev_mode: false,
            cert_trusted: false,
            installed: None,
            home_app_is_this: false,
            ..installed_before()
        }
    }

    #[derive(Default)]
    pub struct Recorded(pub Vec<(usize, StepState)>);

    impl Progress for Recorded {
        fn update(&mut self, index: usize, state: StepState) {
            self.0.push((index, state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::plan::Choice;

    const HOME: Choice = Choice {
        make_home_app: true,
        replace_newer: false,
    };

    fn installing(host: &FakeHost, choice: Choice) -> Result<(), Failure> {
        let steps = plan::install(&host.facts(), choice).unwrap();
        run(host, &steps, &mut ())
    }

    #[test]
    fn a_first_installation_runs_every_step_in_order() {
        let host = FakeHost::new(untouched());
        let mut progress = Recorded::default();
        let steps = plan::install(&host.facts(), HOME).unwrap();
        run(&host, &steps, &mut progress).unwrap();
        assert_eq!(
            host.calls(),
            ["prepare", "add", "arrange check", "make home app"]
        );
        // Running, then Done, for every step.
        assert_eq!(progress.0.len(), steps.len() * 2);
        assert_eq!(progress.0.last(), Some(&(steps.len() - 1, StepState::Done)));
    }

    #[test]
    fn a_declined_prompt_stops_before_anything_changes() {
        let host = FakeHost::new(untouched());
        host.fail("prepare");
        let failure = installing(&host, HOME).unwrap_err();
        assert_eq!(failure.step, Step::Prepare(plan::Preparation::default()));
        assert_eq!(failure.error.kind, ErrorKind::Declined);
        assert_eq!(host.calls(), ["prepare"]);
        assert!(!host.installed());
        assert!(host.log.borrow().iter().any(|line| line.contains("failed")));
    }

    #[test]
    fn mujina_becomes_the_home_app_only_with_the_safety_net_in_place() {
        let host = FakeHost::new(untouched());
        host.fail("arrange check");
        let failure = installing(&host, HOME).unwrap_err();
        assert_eq!(failure.step, Step::ArrangeCleanup);
        assert!(!host.calls().contains(&"make home app".to_string()));
    }

    #[test]
    fn a_failed_package_step_arranges_nothing() {
        let host = FakeHost::new(untouched());
        host.fail("add");
        installing(&host, HOME).unwrap_err();
        assert_eq!(host.calls(), ["prepare", "add"]);
    }

    #[test]
    fn versions_choose_how_the_package_goes_on() {
        let cases = [
            (None, "add"),
            (Some("0.27.0.0"), "update"),
            (Some("0.28.0.0"), "reinstall"),
        ];
        for (installed, call) in cases {
            let host = FakeHost::new(Facts {
                installed: installed.map(version),
                ..installed_before()
            });
            installing(&host, HOME).unwrap();
            assert_eq!(host.calls().first().map(String::as_str), Some(call));
        }
        let host = FakeHost::new(Facts {
            installed: Some(version("0.29.0.0")),
            ..installed_before()
        });
        let confirmed = Choice {
            replace_newer: true,
            ..HOME
        };
        installing(&host, confirmed).unwrap();
        assert_eq!(host.calls().first().map(String::as_str), Some("downgrade"));
    }

    #[test]
    fn the_pre_flight_is_checked_again_before_anything_changes() {
        let host = FakeHost::new(untouched());
        let steps = plan::install(&host.facts(), HOME).unwrap();
        // Between the window showing the plan and the button: a device that turns out too old.
        host.facts.set(Facts {
            os_build: Some(22631),
            ..untouched()
        });
        let failure = run(&host, &steps, &mut ()).unwrap_err();
        assert_eq!(failure.step, Step::Preflight);
        assert_eq!(failure.error.kind, ErrorKind::WindowsTooOld);
        assert!(host.calls().is_empty());
    }

    #[test]
    fn removal_gives_the_home_app_back_first_and_the_check_goes_last() {
        let host = FakeHost::new(installed_before());
        run(&host, &plan::uninstall(), &mut ()).unwrap();
        assert_eq!(
            host.calls(),
            [
                "give home app back",
                "remove",
                "forget files",
                "remove check"
            ]
        );
    }

    #[test]
    fn removal_keeps_the_check_while_files_remain() {
        let host = FakeHost::new(installed_before());
        host.files_remain.set(true);
        run(&host, &plan::uninstall(), &mut ()).unwrap();
        assert!(!host.calls().contains(&"remove check".to_string()));
    }

    #[test]
    fn removing_another_mujina_is_refused_before_anything_of_its_own_goes() {
        let host = FakeHost::new(untouched());
        let failure = run(&host, &plan::uninstall(), &mut ()).unwrap_err();
        assert_eq!(failure.step, Step::RemovePackage);
        assert_eq!(failure.error.kind, ErrorKind::NotInstalled);
        assert_eq!(host.calls(), ["give home app back", "remove"]);
    }

    #[test]
    fn the_check_at_sign_in_does_nothing_while_mujina_is_installed() {
        let host = FakeHost::new(installed_before());
        assert_eq!(sign_in(&host), SignedIn::Installed);
        assert!(host.calls().is_empty());
        assert!(host.log.borrow().is_empty());
    }

    #[test]
    fn the_check_at_sign_in_goes_once_the_home_app_is_back() {
        let host = FakeHost::new(untouched());
        assert_eq!(sign_in(&host), SignedIn::Done);
        assert_eq!(
            host.calls(),
            ["give home app back", "forget files", "remove check"]
        );
    }

    #[test]
    fn the_check_at_sign_in_stays_while_the_home_app_cannot_be_given_back() {
        let host = FakeHost::new(untouched());
        for attempt in 1..SIGN_IN_ATTEMPTS {
            host.fail("give home app back");
            assert_eq!(sign_in(&host), SignedIn::WillRetry(attempt));
            assert!(!host.calls().contains(&"remove check".to_string()));
        }
        assert!(
            host.log
                .borrow()
                .iter()
                .any(|line| line.contains("giving the home app back failed"))
        );
        assert_eq!(sign_in(&host), SignedIn::Done);
        assert_eq!(host.attempts.get(), 0);
    }

    #[test]
    fn installing_again_gives_the_check_all_its_attempts() {
        let host = FakeHost::new(untouched());
        for _ in 0..2 {
            host.fail("give home app back");
            sign_in(&host);
        }
        assert_eq!(host.attempts.get(), 2);
        // Installed again before the check gave up, then removed the same way again.
        installing(&host, HOME).unwrap();
        host.installed.set(false);
        for attempt in 1..SIGN_IN_ATTEMPTS {
            host.fail("give home app back");
            assert_eq!(sign_in(&host), SignedIn::WillRetry(attempt));
        }
    }

    #[test]
    fn the_check_at_sign_in_gives_up_after_its_attempts() {
        let host = FakeHost::new(untouched());
        for _ in 1..SIGN_IN_ATTEMPTS {
            host.fail("give home app back");
            sign_in(&host);
        }
        host.fail("give home app back");
        assert_eq!(sign_in(&host), SignedIn::GaveUp);
        assert_eq!(
            host.calls().last().map(String::as_str),
            Some("remove check")
        );
        assert!(
            host.log
                .borrow()
                .iter()
                .any(|line| line.contains("removes itself"))
        );
    }

    #[test]
    fn unattended_installs_keep_the_home_app_as_it_is_and_never_downgrade() {
        let host = FakeHost::new(Facts {
            home_app_is_this: false,
            ..installed_before()
        });
        assert_eq!(install_unattended(&host, false), Unattended::Done);
        assert!(!host.calls().contains(&"make home app".to_string()));
        let host = FakeHost::new(untouched());
        assert_eq!(install_unattended(&host, false), Unattended::Done);
        assert!(host.calls().contains(&"make home app".to_string()));
        let host = FakeHost::new(untouched());
        assert_eq!(install_unattended(&host, true), Unattended::Done);
        assert!(!host.calls().contains(&"make home app".to_string()));
        let host = FakeHost::new(Facts {
            installed: Some(version("0.29.0.0")),
            ..installed_before()
        });
        assert_eq!(
            install_unattended(&host, false),
            Unattended::Refused(Refusal::NewerInstalled {
                installed: version("0.29.0.0")
            })
        );
        assert!(host.calls().is_empty());
    }

    #[test]
    fn hresults_are_found_in_what_powershell_says() {
        let said = "Add-AppxPackage : Deployment failed with HRESULT: 0x80073D06, The package \
                    could not be installed because a higher version of this package is already \
                    installed.";
        assert_eq!(hresult_in(said), Some(0x8007_3D06));
        let error = StepError::deployment(said);
        assert_eq!(error.kind, ErrorKind::NewerInstalled);
        assert_eq!(error.code, Some(0x8007_3D06));
        // Lower case, and a success code before it, which is no failure.
        assert_eq!(
            hresult_in("x 0x00000001 then 0x800b0109 (untrusted)"),
            Some(0x800B_0109)
        );
        // Not eight digits, or none at all.
        assert_eq!(hresult_in("0x800B01090 and 0x8007"), None);
        assert_eq!(hresult_in("Access is denied."), None);
        assert_eq!(
            StepError::deployment("Access is denied.").kind,
            ErrorKind::DeploymentFailed
        );
    }

    #[test]
    fn known_deployment_errors_have_kinds_of_their_own() {
        assert_eq!(deployment_kind(0x8007_3CFB), ErrorKind::SameVersionDiffers);
        assert_eq!(deployment_kind(0x800B_010A), ErrorKind::NotTrusted);
        assert_eq!(deployment_kind(0x8007_3CFF), ErrorKind::SideloadingOff);
        assert_eq!(deployment_kind(0x8007_3D02), ErrorKind::InUse);
        assert_eq!(deployment_kind(0x8008_0204), ErrorKind::Damaged);
        assert_eq!(deployment_kind(0x8007_0005), ErrorKind::DeploymentFailed);
    }

    #[test]
    fn an_error_says_its_kind_code_and_detail() {
        let error = StepError::with_code(ErrorKind::InUse, 0x8007_3D02, "in use");
        assert_eq!(error.to_string(), "InUse (code 0x80073D02): in use");
        assert_eq!(
            StepError::new(ErrorKind::Declined, "").to_string(),
            "Declined"
        );
    }
}

//! Which steps an installation or removal takes, and in which order, from what Setup found out
//! about the device. Pure, so that every order and every branch is a test, not a comment; it
//! builds and is tested on any system.
//!
//! The order follows one rule: a safety net comes before the change it guards. The check at
//! sign-in, which gives the home app setting back should Mujina be removed through Settings →
//! Apps, is arranged before Mujina becomes the home app; the home app is given back before the
//! package goes.

use std::fmt;

/// The Windows build Mujina's package requires (`MinVersion` 10.0.26100.0 in
/// `packaging/AppxManifest.xml.in`): Windows 11 24H2.
pub const REQUIRED_BUILD: u32 = 26100;

/// A package version, as Windows orders them: four numbers, compared from the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub build: u16,
    pub revision: u16,
}

impl Version {
    /// `major.minor.build.revision`, as the manifest writes it.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.').map(|part| part.parse::<u16>().ok());
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            build: parts.next()??,
            revision: parts.next()??,
        };
        parts.next().is_none().then_some(version)
    }

    /// The version in a package full name, `Name_1.2.3.4_x64__publisherid`: the second of its
    /// fields. A package name holds no underscore, so the fields are unambiguous.
    pub fn from_full_name(full_name: &str) -> Option<Self> {
        Self::parse(full_name.split('_').nth(1)?)
    }
}

/// `0.27.0` for a release (revision 0), `0.27.0.158` for a CI build.
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)?;
        if self.revision != 0 {
            write!(f, ".{}", self.revision)?;
        }
        Ok(())
    }
}

/// What Setup found out about the device, before it changes anything. Read-only to gather.
// Independent yes-or-no facts, each read on its own; no two of them make a state together.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// Windows' build number; `None` if it could not be read, which blocks nothing.
    pub os_build: Option<u32>,
    pub dev_mode: bool,
    /// Exactly the certificate this installer carries is trusted, not merely one with the same
    /// name, such as the one before a renewal.
    pub cert_trusted: bool,
    /// The version of this Mujina (this package family) installed for the user, if any.
    pub installed: Option<Version>,
    /// The version of the package this installer carries; `None` for a build that carries none.
    pub carried: Option<Version>,
    /// Mujina, exactly this family, is the home app of Xbox mode now.
    pub home_app_is_this: bool,
    /// Mujina's background agent runs in this session (it does in Xbox mode).
    pub agent_running: bool,
}

impl Facts {
    /// What an earlier installation already did to this machine.
    pub fn preparation(&self) -> Preparation {
        Preparation {
            developer_mode: self.dev_mode,
            certificate: self.cert_trusted,
        }
    }
}

/// What the user chose in the window, or on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub make_home_app: bool,
    /// Replacing a newer installed version with the one carried was confirmed.
    pub replace_newer: bool,
}

/// What an earlier installation already did to this machine. Two facts, so that the prompt says
/// what it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Preparation {
    pub developer_mode: bool,
    pub certificate: bool,
}

impl Preparation {
    pub fn complete(self) -> bool {
        self.developer_mode && self.certificate
    }
}

/// How the carried package goes onto the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Package {
    /// Not installed yet.
    Install,
    /// An older version is installed.
    Update { from: Version },
    /// The same version is installed: Windows installs it again only if it is exactly the same
    /// file (Microsoft's troubleshooting table, 0x80073CFB).
    Reinstall,
    /// A newer version is installed, and replacing it was confirmed: `Add-AppxPackage
    /// -ForceUpdateFromAnyVersion`, which "force[s] a specific version of a package to be staged
    /// or registered, regardless of whether a higher version is already staged or registered".
    Downgrade { from: Version },
}

impl Package {
    /// What installing `carried` over `installed` is.
    pub fn over(installed: Option<Version>, carried: Version) -> Self {
        match installed {
            None => Self::Install,
            Some(from) if from < carried => Self::Update { from },
            Some(from) if from == carried => Self::Reinstall,
            Some(from) => Self::Downgrade { from },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The device can run Mujina (see [`preflight`]): checked again right before anything
    /// changes, since the window may have been open for a while.
    Preflight,
    /// Developer Mode on and the certificate trusted, whichever of the two is missing: once per
    /// machine, with one prompt. Carries what an earlier installation already did.
    Prepare(Preparation),
    Package(Package),
    /// A check at sign-in that gives the home app setting back once Mujina has been removed
    /// through Settings → Apps, where Windows runs nothing of Mujina's (ADR-0012). It does
    /// nothing while the package is installed, so arranging it before the home app is safe.
    ArrangeCleanup,
    MakeHomeApp,
    GiveHomeAppBack,
    RemovePackage,
    /// The files Mujina made outside its own folders (a launcher's marker file, say), as
    /// recorded when it made them; then the record.
    ForgetCreatedFiles,
    RemoveCleanup,
}

/// Why nothing may be installed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    /// Windows is older than [`REQUIRED_BUILD`]: Windows would refuse the package, after the
    /// administrator prompt and the changes to the machine.
    WindowsTooOld { build: u32 },
}

/// Why [`install`] gives no steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// This build of Setup carries no package (a CI check, a local build without one).
    NoPackage,
    Blocked(Blocker),
    /// A newer version is installed and replacing it was not confirmed.
    NewerInstalled {
        installed: Version,
    },
}

/// The read-only pre-flight: only a Windows build known to be too old blocks. Anything else
/// (Xbox mode not found, another home app) cannot be told for sure before installing, so it
/// blocks nothing.
pub fn preflight(facts: &Facts) -> Option<Blocker> {
    facts
        .os_build
        .filter(|&build| build < REQUIRED_BUILD)
        .map(|build| Blocker::WindowsTooOld { build })
}

/// Whether the home app switch starts on. On a first installation it does. On an update it says
/// what is the case now, so that an update never takes the home app from an app the user chose
/// after installing Mujina.
pub fn home_app_default(facts: &Facts) -> bool {
    facts.installed.is_none() || facts.home_app_is_this
}

/// Whether installing stops Mujina's running agent: `Add-AppxPackage -ForceApplicationShutdown`
/// ends the package's processes. It starts again the next time Xbox mode starts Mujina.
pub fn stops_agent(facts: &Facts) -> bool {
    facts.installed.is_some() && facts.agent_running
}

pub fn install(facts: &Facts, choice: Choice) -> Result<Vec<Step>, Refusal> {
    let carried = facts.carried.ok_or(Refusal::NoPackage)?;
    if let Some(blocker) = preflight(facts) {
        return Err(Refusal::Blocked(blocker));
    }
    let package = Package::over(facts.installed, carried);
    if let Package::Downgrade { from } = package
        && !choice.replace_newer
    {
        return Err(Refusal::NewerInstalled { installed: from });
    }
    let mut steps = vec![Step::Preflight];
    let prepared = facts.preparation();
    if !prepared.complete() {
        steps.push(Step::Prepare(prepared));
    }
    steps.push(Step::Package(package));
    steps.push(Step::ArrangeCleanup);
    if choice.make_home_app {
        steps.push(Step::MakeHomeApp);
    }
    Ok(steps)
}

/// Developer Mode and the certificate stay: removing them needs administrator rights again, and
/// Developer Mode may serve other apps.
pub fn uninstall() -> Vec<Step> {
    vec![
        // Before the package goes: on some Windows builds Task View breaks when the selected
        // home app is removed while still selected.
        Step::GiveHomeAppBack,
        Step::RemovePackage,
        // After the package, which can then make none of them again; before the check at
        // sign-in goes, so that it tries again if a file cannot be deleted now.
        Step::ForgetCreatedFiles,
        Step::RemoveCleanup,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    /// A first installation on a device never prepared, Windows new enough.
    fn fresh() -> Facts {
        Facts {
            os_build: Some(26200),
            dev_mode: false,
            cert_trusted: false,
            installed: None,
            carried: Some(version("0.28.0.0")),
            home_app_is_this: false,
            agent_running: false,
        }
    }

    fn prepared(installed: &str) -> Facts {
        Facts {
            dev_mode: true,
            cert_trusted: true,
            installed: Some(version(installed)),
            ..fresh()
        }
    }

    const HOME: Choice = Choice {
        make_home_app: true,
        replace_newer: false,
    };
    const NO_HOME: Choice = Choice {
        make_home_app: false,
        replace_newer: false,
    };

    fn position(steps: &[Step], wanted: Step) -> usize {
        steps.iter().position(|step| *step == wanted).unwrap()
    }

    #[test]
    fn versions_compare_from_the_left_and_show_their_revision_only_when_set() {
        assert!(version("0.27.0.158") < version("0.28.0.0"));
        assert!(version("0.27.1.0") > version("0.27.0.999"));
        assert!(version("1.0.0.0") > version("0.99.99.99"));
        assert_eq!(version("0.27.0.0").to_string(), "0.27.0");
        assert_eq!(version("0.27.0.158").to_string(), "0.27.0.158");
        for bad in ["", "0.27.0", "0.27.0.0.1", "0.27.x.0", "0.27.0.70000"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_full_name_carries_its_version() {
        assert_eq!(
            Version::from_full_name("Mujina_0.27.0.0_x64__k2veznmcx4n98"),
            Some(version("0.27.0.0"))
        );
        assert_eq!(Version::from_full_name("Mujina"), None);
        assert_eq!(Version::from_full_name("Mujina_x64__k2veznmcx4n98"), None);
    }

    #[test]
    fn a_first_installation_prepares_the_machine_once_and_nets_before_the_home_app() {
        let facts = fresh();
        let steps = install(&facts, HOME).unwrap();
        assert_eq!(
            steps,
            [
                Step::Preflight,
                Step::Prepare(Preparation::default()),
                Step::Package(Package::Install),
                Step::ArrangeCleanup,
                Step::MakeHomeApp,
            ]
        );
    }

    #[test]
    fn the_safety_net_comes_before_the_home_app_in_every_branch() {
        for facts in [fresh(), prepared("0.27.0.0"), prepared("0.28.0.0")] {
            let steps = install(&facts, HOME).unwrap();
            assert!(
                position(&steps, Step::ArrangeCleanup) < position(&steps, Step::MakeHomeApp),
                "{steps:?}"
            );
            assert_eq!(steps.last(), Some(&Step::MakeHomeApp), "{steps:?}");
            assert_eq!(steps.first(), Some(&Step::Preflight), "{steps:?}");
        }
    }

    #[test]
    fn an_update_needs_no_prompt_and_the_home_app_is_optional() {
        let from = version("0.27.0.0");
        let steps = install(&prepared("0.27.0.0"), NO_HOME).unwrap();
        assert_eq!(
            steps,
            [
                Step::Preflight,
                Step::Package(Package::Update { from }),
                Step::ArrangeCleanup,
            ]
        );
    }

    #[test]
    fn the_same_version_is_installed_again() {
        let steps = install(&prepared("0.28.0.0"), NO_HOME).unwrap();
        assert!(
            steps.contains(&Step::Package(Package::Reinstall)),
            "{steps:?}"
        );
    }

    #[test]
    fn a_newer_installed_version_is_replaced_only_when_confirmed() {
        let facts = prepared("0.29.0.0");
        assert_eq!(
            install(&facts, HOME),
            Err(Refusal::NewerInstalled {
                installed: version("0.29.0.0")
            })
        );
        let confirmed = Choice {
            replace_newer: true,
            ..HOME
        };
        let steps = install(&facts, confirmed).unwrap();
        assert!(
            steps.contains(&Step::Package(Package::Downgrade {
                from: version("0.29.0.0")
            })),
            "{steps:?}"
        );
    }

    #[test]
    fn a_ci_build_of_the_same_release_is_newer_than_it() {
        assert_eq!(
            Package::over(Some(version("0.28.0.158")), version("0.28.0.0")),
            Package::Downgrade {
                from: version("0.28.0.158")
            }
        );
    }

    #[test]
    fn a_renewed_certificate_is_asked_for_alone() {
        let facts = Facts {
            cert_trusted: false,
            ..prepared("0.27.0.0")
        };
        let steps = install(&facts, NO_HOME).unwrap();
        let prepare = Step::Prepare(Preparation {
            developer_mode: true,
            certificate: false,
        });
        assert_eq!(steps.get(1), Some(&prepare), "{steps:?}");
    }

    #[test]
    fn only_a_windows_build_known_to_be_too_old_blocks() {
        let old = Facts {
            os_build: Some(22631),
            ..fresh()
        };
        let blocked = Blocker::WindowsTooOld { build: 22631 };
        assert_eq!(preflight(&old), Some(blocked));
        assert_eq!(install(&old, HOME), Err(Refusal::Blocked(blocked)));
        // Before anything else, the downgrade question too.
        let old_and_newer = Facts {
            os_build: Some(22631),
            ..prepared("0.29.0.0")
        };
        assert_eq!(
            install(&old_and_newer, HOME),
            Err(Refusal::Blocked(blocked))
        );
        for build in [None, Some(REQUIRED_BUILD), Some(26200)] {
            let facts = Facts {
                os_build: build,
                ..fresh()
            };
            assert_eq!(preflight(&facts), None, "{build:?}");
        }
    }

    #[test]
    fn a_build_without_a_package_installs_nothing() {
        let facts = Facts {
            carried: None,
            ..fresh()
        };
        assert_eq!(install(&facts, HOME), Err(Refusal::NoPackage));
    }

    #[test]
    fn an_update_keeps_the_home_app_as_the_user_left_it() {
        assert!(home_app_default(&fresh()));
        let mut update = prepared("0.27.0.0");
        assert!(!home_app_default(&update));
        update.home_app_is_this = true;
        assert!(home_app_default(&update));
    }

    #[test]
    fn only_an_update_stops_a_running_agent() {
        let mut facts = prepared("0.27.0.0");
        assert!(!stops_agent(&facts));
        facts.agent_running = true;
        assert!(stops_agent(&facts));
        facts.installed = None;
        assert!(!stops_agent(&facts));
    }

    #[test]
    fn the_home_app_is_given_back_before_the_package_goes() {
        let steps = uninstall();
        assert!(
            position(&steps, Step::GiveHomeAppBack) < position(&steps, Step::RemovePackage),
            "{steps:?}"
        );
        assert!(!steps.iter().any(|step| matches!(step, Step::Prepare(_))));
    }

    #[test]
    fn created_files_go_after_the_package_and_before_the_check_at_sign_in() {
        let steps = uninstall();
        let forget = position(&steps, Step::ForgetCreatedFiles);
        assert!(position(&steps, Step::RemovePackage) < forget, "{steps:?}");
        assert!(forget < position(&steps, Step::RemoveCleanup), "{steps:?}");
    }
}

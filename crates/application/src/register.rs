//! Making Mujina the console home app, and handing the setting back cleanly.
//!
//! Handing it back matters: on some Windows builds Task View stops working when the configured
//! home app is uninstalled while still selected. `mujinactl`, Mujina Settings and Mujina Setup
//! (its removal, and its check at sign-in once the package is gone) all hand it back by the one
//! rule here, [`home_app_repair`].

use crate::ports::{HomeAppRegistry, PackageIdentity, PortError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    Registered,
    AlreadyRegistered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnregisterOutcome {
    /// The previous home app was put back.
    Restored(String),
    /// There was no previous home app; the setting was removed.
    Cleared,
    /// Some other app is the home app; nothing was touched.
    NotRegistered,
}

/// What to do with the home app setting when this Mujina stops being the home app, from what the
/// setting and its backup say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeAppRepair {
    /// This Mujina is the home app: put the previous one back.
    Restore(String),
    /// This Mujina is the home app and there was none before it (or the backup names a Mujina,
    /// this one or another, whose package may have gone since): remove the setting.
    Clear,
    /// Another app is the home app, another Mujina among them: leave it alone.
    Leave,
}

/// The rule for giving the home app back. `ours` is this Mujina's app ID, `<family>!App`; only
/// exactly that counts as Mujina, so a contributor's build installed beside the release (another
/// family) is another app.
pub fn home_app_repair(current: Option<&str>, backup: Option<&str>, ours: &str) -> HomeAppRepair {
    match current {
        Some(current) if same_app(current, ours) => match backup {
            Some(previous) if !previous.is_empty() && !same_product(previous, ours) => {
                HomeAppRepair::Restore(previous.to_string())
            }
            _ => HomeAppRepair::Clear,
        },
        _ => HomeAppRepair::Leave,
    }
}

/// Whether two app IDs (`<package family>!<application id>`) name the same app. Windows compares
/// package family names without regard to case; the application id is compared as written.
pub fn same_app(one: &str, other: &str) -> bool {
    match (one.split_once('!'), other.split_once('!')) {
        (Some((family, app)), Some((other_family, other_app))) => {
            family.eq_ignore_ascii_case(other_family) && app == other_app
        }
        _ => false,
    }
}

/// Whether two app IDs name the same app of the same product, whoever signed it: the package
/// name (the family up to its last `_`, where the publisher's id begins) and the application id.
/// A release and a contributor's build of Mujina are two packages of one product.
pub fn same_product(one: &str, other: &str) -> bool {
    fn parts(id: &str) -> Option<(&str, &str)> {
        let (family, app) = id.split_once('!')?;
        let (name, _publisher) = family.rsplit_once('_')?;
        Some((name, app))
    }
    match (parts(one), parts(other)) {
        (Some((name, app)), Some((other_name, other_app))) => {
            name.eq_ignore_ascii_case(other_name) && app == other_app
        }
        _ => false,
    }
}

/// Why the home app setting was not changed. The message names the port's error too, so that
/// error is not also given as the source: whoever prints the chain would print it twice.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    #[error("this process has no package identity; install the MSIX package first")]
    NotPackaged,
    #[error("home app setting: {0}")]
    Registry(PortError),
}

// Written out: `#[from]` would make the port's error the source as well.
impl From<PortError> for RegisterError {
    fn from(error: PortError) -> Self {
        Self::Registry(error)
    }
}

pub struct HomeAppRegistration<'a> {
    identity: &'a dyn PackageIdentity,
    registry: &'a dyn HomeAppRegistry,
}

impl<'a> HomeAppRegistration<'a> {
    pub fn new(identity: &'a dyn PackageIdentity, registry: &'a dyn HomeAppRegistry) -> Self {
        Self { identity, registry }
    }

    pub fn register(&self) -> Result<RegisterOutcome, RegisterError> {
        let ours = self
            .identity
            .app_user_model_id()
            .ok_or(RegisterError::NotPackaged)?;
        match self.registry.current()? {
            Some(current) if same_app(&current, &ours) => {
                return Ok(RegisterOutcome::AlreadyRegistered);
            }
            // Another Mujina, say a release beside a contributor's build: the app before it stays
            // the one to go back to. Every Mujina shares one backup, and a Mujina in it would be
            // put back after its package may have gone, which breaks Task View.
            Some(other) if same_product(&other, &ours) => {}
            Some(other) => self.registry.set_backup(Some(&other))?,
            // Exactly what was there before: a backup left from an earlier time would otherwise
            // come back in place of no home app at all.
            None => self.registry.set_backup(None)?,
        }
        self.registry.set(&ours)?;
        Ok(RegisterOutcome::Registered)
    }

    /// Gives the setting back to the app before Mujina, if Mujina is the home app, and forgets
    /// the backup. Otherwise it touches nothing, the backup least of all: every Mujina package
    /// shares it, so while another app is the home app it may be another Mujina's, which that
    /// one needs when it goes. `mujinactl unregister`, Mujina Settings, Setup's removal and its
    /// check at sign-in all come here.
    pub fn unregister(&self) -> Result<UnregisterOutcome, RegisterError> {
        let ours = self
            .identity
            .app_user_model_id()
            .ok_or(RegisterError::NotPackaged)?;
        let current = self.registry.current()?;
        let backup = self.registry.backup()?;
        let outcome = match home_app_repair(current.as_deref(), backup.as_deref(), &ours) {
            HomeAppRepair::Restore(previous) => {
                self.registry.set(&previous)?;
                UnregisterOutcome::Restored(previous)
            }
            HomeAppRepair::Clear => {
                self.registry.clear()?;
                UnregisterOutcome::Cleared
            }
            HomeAppRepair::Leave => return Ok(UnregisterOutcome::NotRegistered),
        };
        if backup.is_some() {
            self.registry.set_backup(None)?;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeHomeAppRegistry, FakeIdentity};

    const OURS: &str = "Mujina_abc!App";
    const XBOX: &str = "Microsoft.GamingApp_8wekyb3d8bbwe!Microsoft.Xbox.App";

    #[test]
    fn register_backs_up_the_previous_home_app() {
        let identity = FakeIdentity::packaged(OURS);
        let registry = FakeHomeAppRegistry::with_current(Some(XBOX));
        let registration = HomeAppRegistration::new(&identity, &registry);

        assert_eq!(registration.register(), Ok(RegisterOutcome::Registered));
        assert_eq!(registry.current().unwrap().as_deref(), Some(OURS));
        assert_eq!(registry.backup().unwrap().as_deref(), Some(XBOX));

        // A second run must not overwrite the backup with ourselves.
        assert_eq!(
            registration.register(),
            Ok(RegisterOutcome::AlreadyRegistered)
        );
        assert_eq!(registry.backup().unwrap().as_deref(), Some(XBOX));
    }

    #[test]
    fn unregister_restores_the_backup() {
        let identity = FakeIdentity::packaged(OURS);
        let registry = FakeHomeAppRegistry::with_current(Some(XBOX));
        let registration = HomeAppRegistration::new(&identity, &registry);
        registration.register().unwrap();

        assert_eq!(
            registration.unregister(),
            Ok(UnregisterOutcome::Restored(XBOX.to_string()))
        );
        assert_eq!(registry.current().unwrap().as_deref(), Some(XBOX));
        assert_eq!(registry.backup().unwrap(), None);
    }

    #[test]
    fn unregister_without_backup_clears_the_setting() {
        let identity = FakeIdentity::packaged(OURS);
        let registry = FakeHomeAppRegistry::with_current(None);
        let registration = HomeAppRegistration::new(&identity, &registry);
        registration.register().unwrap();

        assert_eq!(registration.unregister(), Ok(UnregisterOutcome::Cleared));
        assert_eq!(registry.current().unwrap(), None);
    }

    #[test]
    fn unregister_leaves_a_foreign_home_app_alone() {
        let identity = FakeIdentity::packaged(OURS);
        let registry = FakeHomeAppRegistry::with_current(Some(XBOX));
        let registration = HomeAppRegistration::new(&identity, &registry);

        assert_eq!(
            registration.unregister(),
            Ok(UnregisterOutcome::NotRegistered)
        );
        assert_eq!(registry.current().unwrap().as_deref(), Some(XBOX));
    }

    #[test]
    fn the_previous_home_app_comes_back() {
        assert_eq!(
            home_app_repair(Some(OURS), Some(XBOX), OURS),
            HomeAppRepair::Restore(XBOX.to_string())
        );
        assert_eq!(
            home_app_repair(Some(OURS), None, OURS),
            HomeAppRepair::Clear
        );
        assert_eq!(
            home_app_repair(Some(OURS), Some(""), OURS),
            HomeAppRepair::Clear
        );
        // Package family names compare without regard to case.
        assert_eq!(
            home_app_repair(Some("MUJINA_ABC!App"), None, OURS),
            HomeAppRepair::Clear
        );
        // A backup that names this Mujina itself is no app to go back to.
        assert_eq!(
            home_app_repair(Some(OURS), Some("mujina_abc!App"), OURS),
            HomeAppRepair::Clear
        );
    }

    #[test]
    fn another_home_app_is_left_alone() {
        assert_eq!(
            home_app_repair(Some(XBOX), Some(XBOX), OURS),
            HomeAppRepair::Leave
        );
        assert_eq!(
            home_app_repair(None, Some(XBOX), OURS),
            HomeAppRepair::Leave
        );
        // Another app of the same package is not the home app entry.
        assert_eq!(
            home_app_repair(Some("Mujina_abc!Settings"), None, OURS),
            HomeAppRepair::Leave
        );
        // Not by prefix: a family that merely starts the same is another package.
        assert_eq!(
            home_app_repair(Some("Mujina_abcd!App"), None, OURS),
            HomeAppRepair::Leave
        );
    }

    /// A contributor's build: the same package name, another publisher, so another family.
    const DEV: &str = "Mujina_zexajcypj3ggw!App";

    #[test]
    fn another_mujina_is_another_app() {
        assert_eq!(home_app_repair(Some(DEV), None, OURS), HomeAppRepair::Leave);
        // Nor is it an app to go back to: its package may have gone since.
        assert_eq!(
            home_app_repair(Some(OURS), Some(DEV), OURS),
            HomeAppRepair::Clear
        );
    }

    #[test]
    fn one_product_whoever_signed_it() {
        assert!(same_product(OURS, DEV));
        assert!(same_product("MUJINA_abc!App", DEV));
        // Another app of the package, or another package that only starts the same, is not.
        assert!(!same_product(OURS, "Mujina_abc!Settings"));
        assert!(!same_product(OURS, "MujinaDev_zexajcypj3ggw!App"));
        assert!(!same_product(OURS, XBOX));
        assert!(!same_product("no-family!App", "no-family!App"));
    }

    /// The release, then a contributor's build over it; both registrations, then `remove`.
    fn release_then_dev() -> (FakeHomeAppRegistry, FakeIdentity, FakeIdentity) {
        let registry = FakeHomeAppRegistry::with_current(Some(XBOX));
        let release = FakeIdentity::packaged(OURS);
        let dev = FakeIdentity::packaged(DEV);
        HomeAppRegistration::new(&release, &registry)
            .register()
            .unwrap();
        HomeAppRegistration::new(&dev, &registry)
            .register()
            .unwrap();
        // The build took over from the release, and the app before both stays the backup.
        assert_eq!(registry.current().unwrap().as_deref(), Some(DEV));
        assert_eq!(registry.backup().unwrap().as_deref(), Some(XBOX));
        (registry, release, dev)
    }

    #[test]
    fn removing_the_release_then_the_build_gives_the_xbox_app_back() {
        let (registry, release, dev) = release_then_dev();
        assert_eq!(
            HomeAppRegistration::new(&release, &registry).unregister(),
            Ok(UnregisterOutcome::NotRegistered)
        );
        assert_eq!(
            HomeAppRegistration::new(&dev, &registry).unregister(),
            Ok(UnregisterOutcome::Restored(XBOX.to_string()))
        );
        assert_eq!(registry.current().unwrap().as_deref(), Some(XBOX));
    }

    #[test]
    fn removing_the_build_first_gives_the_xbox_app_back_too() {
        // Not the release, although it is still installed: one backup is shared, and that is
        // its price. Never a package that may be gone.
        let (registry, release, dev) = release_then_dev();
        assert_eq!(
            HomeAppRegistration::new(&dev, &registry).unregister(),
            Ok(UnregisterOutcome::Restored(XBOX.to_string()))
        );
        assert_eq!(
            HomeAppRegistration::new(&release, &registry).unregister(),
            Ok(UnregisterOutcome::NotRegistered)
        );
        assert_eq!(registry.current().unwrap().as_deref(), Some(XBOX));
    }

    #[test]
    fn a_mujina_left_in_the_backup_is_not_put_back() {
        // Written by an earlier Mujina Setup, which recorded the Mujina it took over from.
        let registry = FakeHomeAppRegistry::with_current(Some(DEV));
        registry.set_backup(Some(OURS)).unwrap();
        let dev = FakeIdentity::packaged(DEV);
        assert_eq!(
            HomeAppRegistration::new(&dev, &registry).unregister(),
            Ok(UnregisterOutcome::Cleared)
        );
        assert_eq!(registry.current().unwrap(), None);
    }

    #[test]
    fn registering_again_in_other_case_changes_nothing() {
        let identity = FakeIdentity::packaged(OURS);
        let registry = FakeHomeAppRegistry::with_current(Some("MUJINA_ABC!App"));
        let registration = HomeAppRegistration::new(&identity, &registry);
        assert_eq!(
            registration.register(),
            Ok(RegisterOutcome::AlreadyRegistered)
        );
        assert_eq!(registry.backup().unwrap(), None);
    }

    #[test]
    fn unregister_keeps_the_backup_while_another_app_is_home() {
        let identity = FakeIdentity::packaged(OURS);
        let registry = FakeHomeAppRegistry::with_current(Some(XBOX));
        registry.set_backup(Some(XBOX)).unwrap();
        let registration = HomeAppRegistration::new(&identity, &registry);
        assert_eq!(
            registration.unregister(),
            Ok(UnregisterOutcome::NotRegistered)
        );
        assert_eq!(registry.backup().unwrap().as_deref(), Some(XBOX));
    }

    #[test]
    fn register_records_exactly_what_was_there_before() {
        let identity = FakeIdentity::packaged(OURS);
        // A backup from an earlier time, and since then no home app at all.
        let registry = FakeHomeAppRegistry::with_current(None);
        registry.set_backup(Some(XBOX)).unwrap();
        let registration = HomeAppRegistration::new(&identity, &registry);
        assert_eq!(registration.register(), Ok(RegisterOutcome::Registered));
        assert_eq!(registry.backup().unwrap(), None);
        // So giving it back leaves no home app, as before.
        assert_eq!(registration.unregister(), Ok(UnregisterOutcome::Cleared));
        assert_eq!(registry.current().unwrap(), None);
    }

    #[test]
    fn removing_one_mujina_keeps_the_backup_of_another() {
        // A contributor's build (another family) is the home app, and its backup is the Xbox
        // app. The release, which is not the home app, goes.
        let dev = DEV;
        let registry = FakeHomeAppRegistry::with_current(Some(XBOX));
        let dev_identity = FakeIdentity::packaged(dev);
        HomeAppRegistration::new(&dev_identity, &registry)
            .register()
            .unwrap();
        let release_identity = FakeIdentity::packaged(OURS);
        assert_eq!(
            HomeAppRegistration::new(&release_identity, &registry).unregister(),
            Ok(UnregisterOutcome::NotRegistered)
        );
        assert_eq!(registry.current().unwrap().as_deref(), Some(dev));
        assert_eq!(registry.backup().unwrap().as_deref(), Some(XBOX));
        // When the build goes too, the Xbox app comes back.
        assert_eq!(
            HomeAppRegistration::new(&dev_identity, &registry).unregister(),
            Ok(UnregisterOutcome::Restored(XBOX.to_string()))
        );
        assert_eq!(registry.current().unwrap().as_deref(), Some(XBOX));
        assert_eq!(registry.backup().unwrap(), None);
    }

    #[test]
    fn unpackaged_process_cannot_register() {
        let identity = FakeIdentity::unpackaged();
        let registry = FakeHomeAppRegistry::with_current(None);
        let registration = HomeAppRegistration::new(&identity, &registry);
        assert_eq!(registration.register(), Err(RegisterError::NotPackaged));
    }

    #[test]
    fn a_registry_error_is_named_once() {
        let error = RegisterError::from(PortError::Failed("access denied".to_string()));
        assert_eq!(
            error,
            RegisterError::Registry(PortError::Failed("access denied".to_string()))
        );
        assert_eq!(error.to_string(), "home app setting: access denied");
        // In the message, so not the source as well: a printed chain would say it twice.
        assert!(std::error::Error::source(&error).is_none());
    }
}

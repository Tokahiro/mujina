//! Making Mujina the console home app and handing it back by one rule, [`home_app_repair`]: on
//! some Windows builds Task View breaks when the selected home app is uninstalled.

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

/// What to do with the home app setting when this Mujina stops being the home app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeAppRepair {
    /// This Mujina is the home app: put the previous one back.
    Restore(String),
    /// This Mujina is the home app; the backup is empty or a Mujina whose package may be gone.
    Clear,
    /// Another app is the home app, another Mujina too.
    Leave,
}

/// `ours` is this Mujina's app ID (`<family>!App`); a contributor's build has another family.
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

/// App IDs are `<package family>!<application id>`; Windows ignores case in the family only.
pub fn same_app(one: &str, other: &str) -> bool {
    match (one.split_once('!'), other.split_once('!')) {
        (Some((family, app)), Some((other_family, other_app))) => {
            family.eq_ignore_ascii_case(other_family) && app == other_app
        }
        _ => false,
    }
}

/// Same app whoever signed it: ignores the publisher id after the family's last `_`.
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

/// The port's error is in the message, not the source, so a printed chain does not repeat it.
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
            // Another Mujina: keep the shared backup. A Mujina in it could be put back after its
            // package is gone, which breaks Task View.
            Some(other) if same_product(&other, &ours) => {}
            Some(other) => self.registry.set_backup(Some(&other))?,
            // Record no home app too, or a stale backup would come back in its place.
            None => self.registry.set_backup(None)?,
        }
        self.registry.set(&ours)?;
        Ok(RegisterOutcome::Registered)
    }

    /// Gives the setting back if this Mujina is home; else touches nothing: the backup is shared.
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
        assert_eq!(
            home_app_repair(Some("MUJINA_ABC!App"), None, OURS),
            HomeAppRepair::Clear
        );
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
        assert_eq!(
            home_app_repair(Some("Mujina_abc!Settings"), None, OURS),
            HomeAppRepair::Leave
        );
        assert_eq!(
            home_app_repair(Some("Mujina_abcd!App"), None, OURS),
            HomeAppRepair::Leave
        );
    }

    /// A contributor's build: same package name, another publisher.
    const DEV: &str = "Mujina_zexajcypj3ggw!App";

    #[test]
    fn another_mujina_is_another_app() {
        assert_eq!(home_app_repair(Some(DEV), None, OURS), HomeAppRepair::Leave);
        assert_eq!(
            home_app_repair(Some(OURS), Some(DEV), OURS),
            HomeAppRepair::Clear
        );
    }

    #[test]
    fn one_product_whoever_signed_it() {
        assert!(same_product(OURS, DEV));
        assert!(same_product("MUJINA_abc!App", DEV));
        assert!(!same_product(OURS, "Mujina_abc!Settings"));
        assert!(!same_product(OURS, "MujinaDev_zexajcypj3ggw!App"));
        assert!(!same_product(OURS, XBOX));
        assert!(!same_product("no-family!App", "no-family!App"));
    }

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
        // Not the release: the shared backup never names a package that may be gone.
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
        // An older Mujina Setup recorded the Mujina it took over from.
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
        let registry = FakeHomeAppRegistry::with_current(None);
        registry.set_backup(Some(XBOX)).unwrap();
        let registration = HomeAppRegistration::new(&identity, &registry);
        assert_eq!(registration.register(), Ok(RegisterOutcome::Registered));
        assert_eq!(registry.backup().unwrap(), None);
        assert_eq!(registration.unregister(), Ok(UnregisterOutcome::Cleared));
        assert_eq!(registry.current().unwrap(), None);
    }

    #[test]
    fn removing_one_mujina_keeps_the_backup_of_another() {
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
        assert!(std::error::Error::source(&error).is_none());
    }
}

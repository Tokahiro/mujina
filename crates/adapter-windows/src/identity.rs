//! Package identity: of the running process, or of a package known by its family.

use mujina_application::ports::PackageIdentity;
use mujina_winutil::package;

/// Id of the `<Application>` element in `packaging/AppxManifest.xml.in`.
const APPLICATION_ID: &str = "App";

#[derive(Debug, Default)]
pub struct WindowsPackageIdentity;

impl PackageIdentity for WindowsPackageIdentity {
    fn app_user_model_id(&self) -> Option<String> {
        package::family_name().map(|family| app_user_model_id(&family))
    }
}

/// The identity of the Mujina package of `family`, for Mujina Setup, which runs outside the
/// package, also before it is installed and after it is removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyIdentity {
    family: String,
}

impl FamilyIdentity {
    pub fn new(family: &str) -> Self {
        Self {
            family: family.to_string(),
        }
    }
}

impl PackageIdentity for FamilyIdentity {
    fn app_user_model_id(&self) -> Option<String> {
        Some(app_user_model_id(&self.family))
    }
}

pub fn app_user_model_id(family: &str) -> String {
    format!("{family}!{APPLICATION_ID}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_family_names_its_home_app_entry() {
        assert_eq!(
            FamilyIdentity::new("Mujina_k2veznmcx4n98").app_user_model_id(),
            Some("Mujina_k2veznmcx4n98!App".to_string())
        );
    }
}

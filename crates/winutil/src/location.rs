//! Whether this process may know where the device is, as Windows' privacy settings say. Since
//! Windows 11 24H2 that includes the name and signal strength of the Wi-Fi network: the WLAN
//! service asks for the location permission before it tells them.

use crate::package;
use crate::registry::{Hive, read_string};

/// What Windows' privacy settings say about the location permission of this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationConsent {
    Granted,
    /// Windows asks the first time it is needed.
    NotAsked,
    /// Declined for this app.
    DeniedForApp,
    /// Location access is off for every app.
    DeniedEverywhere,
    /// Not a packaged app: nothing to grant.
    Unpackaged,
}

/// Reads the consent store, as Windows' privacy settings keep it. Quick: a few registry values.
pub fn consent() -> LocationConsent {
    const STORE: &str =
        r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location";
    const MACHINE: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location";
    let value = |hive, key: &str| read_string(hive, key, "Value").ok().flatten();

    let denied_globally = [
        value(Hive::LocalMachine, MACHINE),
        value(Hive::CurrentUser, STORE),
    ]
    .iter()
    .any(|value| value.as_deref() == Some("Deny"));
    if denied_globally {
        return LocationConsent::DeniedEverywhere;
    }
    let Some(family) = package::family_name() else {
        return LocationConsent::Unpackaged;
    };
    match value(Hive::CurrentUser, &format!(r"{STORE}\{family}")).as_deref() {
        Some("Allow") => LocationConsent::Granted,
        Some("Deny") => LocationConsent::DeniedForApp,
        _ => LocationConsent::NotAsked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_runner_is_unpackaged_unless_location_is_off_everywhere() {
        assert!(matches!(
            consent(),
            LocationConsent::Unpackaged | LocationConsent::DeniedEverywhere
        ));
    }
}

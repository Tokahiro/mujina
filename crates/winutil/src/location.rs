//! Whether Windows' privacy settings let this process know the device's location. Since Windows
//! 11 24H2 the WLAN service needs this permission to report the Wi-Fi name and signal strength.

use crate::package;
use crate::registry::{Hive, read_string};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationConsent {
    Granted,
    /// Windows asks the first time it is needed.
    NotAsked,
    DeniedForApp,
    DeniedEverywhere,
    /// Not a packaged app: nothing to grant.
    Unpackaged,
}

/// Quick: reads a few registry values of Windows' consent store.
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

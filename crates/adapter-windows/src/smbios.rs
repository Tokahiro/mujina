//! Which machine this is, for choosing its device. The package's identity is `identity.rs`.

use mujina_application::device::SystemIdentity;
use mujina_winutil::registry::{Hive, read_string};

/// SMBIOS strings as Windows mirrors them into the registry; empty where it has none.
pub fn identity() -> SystemIdentity {
    const KEY: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
    let value = |name| {
        read_string(Hive::LocalMachine, KEY, name)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    SystemIdentity {
        manufacturer: value("SystemManufacturer"),
        product: value("SystemProductName"),
    }
}

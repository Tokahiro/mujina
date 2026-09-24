//! Devices as plug-ins (ADR-0013). Descriptors are trait objects so that a profile file read at
//! start-up can be one too.

use std::collections::BTreeMap;

pub use mujina_domain::button::ButtonId;

use crate::launcher::OptionTable;
use crate::settings::schema::SettingSpec;

/// What the firmware says the machine is (SMBIOS system manufacturer and product name).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SystemIdentity {
    pub manufacturer: String,
    pub product: String,
}

/// Whether a button still reaches the rest of the system when Mujina maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suppression {
    /// Mujina takes it first and replays it where it has no use for it (a key chord).
    Swallowed,
    /// Mujina only watches it; the device's own software reacts too (no program can hold back a
    /// vendor HID report without a driver). The device's `doctor` checks should name that program.
    Observed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonSpec {
    /// What the device reports it as; unique within the device.
    pub id: ButtonId,
    /// A name for configuration and logs: lower case, digits and `_`, e.g. `desktop`.
    pub key: String,
    /// What it is called on the device, short enough for a tile: "Desktop button".
    pub label: String,
    pub suppression: Suppression,
}

/// What Mujina knows about a device before running it. Free of side effects.
pub trait DeviceDescriptor: Send + Sync {
    /// `[device.<id>]`: lower case, digits, `_` and `-`. Never changes: users' files name it.
    fn id(&self) -> &str;

    fn name(&self) -> String;

    /// `profile = "auto"` takes the first device that matches.
    fn matches(&self, identity: &SystemIdentity) -> bool;

    /// Its extra buttons, at least one.
    fn buttons(&self) -> Vec<ButtonSpec>;

    /// The keys of `[device.<id>]`.
    fn settings(&self) -> &[SettingSpec];

    /// Rules across options; a note means no button is mapped. No side effects.
    fn validate(&self, _options: &OptionTable, _notes: &mut Vec<String>) {}

    /// As a launcher's [`catalogs`](crate::launcher::LauncherDescriptor::catalogs), plus its
    /// buttons' labels.
    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

#[derive(Clone, Copy)]
pub struct Devices {
    pub all: &'static [&'static dyn DeviceDescriptor],
    /// The user's own button (`[device.button]`); among [`all`](Self::all), but no profile.
    pub own: &'static dyn DeviceDescriptor,
}

impl Devices {
    /// Includes [`own`](Self::own).
    pub fn find(&self, id: &str) -> Option<&'static dyn DeviceDescriptor> {
        self.all
            .iter()
            .copied()
            .find(|descriptor| descriptor.id() == id)
    }

    pub fn is_own(&self, descriptor: &dyn DeviceDescriptor) -> bool {
        descriptor.id() == self.own.id()
    }

    /// What `[device] profile` can name, in order: all but [`own`](Self::own).
    pub fn profiles(&self) -> impl Iterator<Item = &'static dyn DeviceDescriptor> + '_ {
        self.all
            .iter()
            .copied()
            .filter(|descriptor| !self.is_own(*descriptor))
    }
}

/// The device whose buttons Mujina maps, with its options, as the configuration chose it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceSelection {
    /// `None` for no button at all.
    pub id: Option<String>,
    /// Its options: `[device.<id>]`, or `[device.button]` for a button of one's own.
    pub options: OptionTable,
}

impl DeviceSelection {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_none(&self) -> bool {
        self.id.is_none()
    }
}

/// What `[device] profile` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChoice {
    /// The first device that says it is this machine; the default.
    Auto,
    /// No button.
    None,
    Named(String),
}

impl DeviceChoice {
    /// `profile` as the configuration spells it; unset is `auto`.
    pub fn from_profile(profile: Option<&str>) -> Self {
        match profile {
            None | Some("auto") => Self::Auto,
            Some("none") => Self::None,
            Some(id) => Self::Named(id.to_string()),
        }
    }
}

/// The user's own button when `own` (`[device.button]`) is usable, else what `choice` asks for,
/// with its options from `sections` (`[device.<id>]`). What cannot be used becomes a note.
pub fn select(
    devices: &Devices,
    identity: &SystemIdentity,
    choice: &DeviceChoice,
    own: &OptionTable,
    sections: &BTreeMap<String, OptionTable>,
    notes: &mut Vec<String>,
) -> DeviceSelection {
    if !own.is_empty() {
        let mut problems = Vec::new();
        devices.own.validate(own, &mut problems);
        if problems.is_empty() {
            return DeviceSelection {
                id: Some(devices.own.id().to_string()),
                options: own.clone(),
            };
        }
        notes.extend(
            problems
                .into_iter()
                .map(|problem| format!("[device.button] ignored: {problem}")),
        );
    }
    let chosen = match choice {
        DeviceChoice::None => return DeviceSelection::none(),
        DeviceChoice::Named(id) => devices.profiles().find(|device| device.id() == id),
        DeviceChoice::Auto => devices.profiles().find(|device| device.matches(identity)),
    };
    let Some(device) = chosen else {
        notes.push(match choice {
            DeviceChoice::Named(id) => format!("device profile \"{id}\" does not exist"),
            _ => format!(
                "no device profile for \"{}\" / \"{}\"; the device button is not mapped",
                identity.manufacturer, identity.product
            ),
        });
        return DeviceSelection::none();
    };
    let options = sections.get(device.id()).cloned().unwrap_or_default();
    let mut problems = Vec::new();
    device.validate(&options, &mut problems);
    if !problems.is_empty() {
        notes.extend(problems.into_iter().map(|problem| {
            format!(
                "device {}: {problem}; the device button is not mapped",
                device.id()
            )
        }));
        return DeviceSelection::none();
    }
    DeviceSelection {
        id: Some(device.id().to_string()),
        options,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingValue;
    use crate::testing::FakeDevice;

    static ONEXPLAYER: FakeDevice = FakeDevice {
        manufacturer: "ONE-NETBOOK",
        ..FakeDevice::named("onexplayer", "OneXPlayer")
    };
    static ALLY: FakeDevice = FakeDevice {
        manufacturer: "ASUSTeK COMPUTER INC.",
        needs: Some("mode"),
        ..FakeDevice::named("ally", "ROG Ally")
    };
    /// Stands in for the keyboard crate's button of one's own.
    static OWN: FakeDevice = FakeDevice {
        needs_both: Some(("modifier", "key")),
        ..FakeDevice::named("custom", "Your own button")
    };
    static DEVICES: Devices = Devices {
        all: &[&ONEXPLAYER, &ALLY, &OWN],
        own: &OWN,
    };

    fn machine(manufacturer: &str) -> SystemIdentity {
        SystemIdentity {
            manufacturer: manufacturer.to_string(),
            product: "whatever".to_string(),
        }
    }

    fn table(pairs: &[(&str, &str)]) -> OptionTable {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), SettingValue::Text((*value).to_string())))
            .collect()
    }

    fn chosen(
        manufacturer: &str,
        choice: &DeviceChoice,
        own: &OptionTable,
        sections: &BTreeMap<String, OptionTable>,
    ) -> (Option<String>, Vec<String>) {
        let mut notes = Vec::new();
        let selection = select(
            &DEVICES,
            &machine(manufacturer),
            choice,
            own,
            sections,
            &mut notes,
        );
        (selection.id, notes)
    }

    #[test]
    fn auto_takes_the_device_that_says_it_is_this_machine() {
        let none = BTreeMap::new();
        let (device, notes) = chosen("ONE-NETBOOK", &DeviceChoice::Auto, &table(&[]), &none);
        assert_eq!(device.as_deref(), Some("onexplayer"));
        assert!(notes.is_empty(), "{notes:?}");

        let (device, notes) = chosen("Contoso", &DeviceChoice::Auto, &table(&[]), &none);
        assert_eq!(device, None);
        assert_eq!(
            notes,
            ["no device profile for \"Contoso\" / \"whatever\"; the device button is not mapped"]
        );
    }

    #[test]
    fn none_maps_nothing_and_an_id_names_a_device_whatever_the_machine() {
        let none = BTreeMap::new();
        let (device, notes) = chosen("ONE-NETBOOK", &DeviceChoice::None, &table(&[]), &none);
        assert_eq!((device, notes.len()), (None, 0));

        let named = DeviceChoice::from_profile(Some("onexplayer"));
        let (device, notes) = chosen("Contoso", &named, &table(&[]), &none);
        assert_eq!((device.as_deref(), notes.len()), (Some("onexplayer"), 0));

        let unknown = DeviceChoice::from_profile(Some("steam-deck"));
        let (device, notes) = chosen("ONE-NETBOOK", &unknown, &table(&[]), &none);
        assert_eq!(device, None);
        assert_eq!(notes, ["device profile \"steam-deck\" does not exist"]);
    }

    #[test]
    fn a_button_of_ones_own_wins_over_any_profile() {
        let none = BTreeMap::new();
        let own = table(&[("modifier", "LCTRL"), ("key", "F24")]);
        for choice in [
            DeviceChoice::Auto,
            DeviceChoice::None,
            DeviceChoice::Named("onexplayer".into()),
        ] {
            let (device, notes) = chosen("ONE-NETBOOK", &choice, &own, &none);
            assert_eq!(device.as_deref(), Some("custom"), "{choice:?}");
            assert!(notes.is_empty(), "{notes:?}");
        }
    }

    #[test]
    fn a_button_of_ones_own_that_cannot_be_used_leaves_the_profile_in_charge() {
        let none = BTreeMap::new();
        let half = table(&[("modifier", "LCTRL")]);
        let (device, notes) = chosen("ONE-NETBOOK", &DeviceChoice::Auto, &half, &none);
        assert_eq!(device.as_deref(), Some("onexplayer"));
        assert_eq!(
            notes,
            ["[device.button] ignored: it needs both modifier and key"]
        );
    }

    #[test]
    fn the_button_of_ones_own_is_no_profile() {
        let none = BTreeMap::new();
        let named = DeviceChoice::from_profile(Some("custom"));
        let (device, notes) = chosen("ONE-NETBOOK", &named, &table(&[]), &none);
        assert_eq!(device, None);
        assert_eq!(notes, ["device profile \"custom\" does not exist"]);
        assert!(DEVICES.find("custom").is_some());
        assert_eq!(DEVICES.profiles().count(), 2);
    }

    #[test]
    fn a_device_gets_its_own_section_and_its_rules_are_kept() {
        let ally = DeviceChoice::from_profile(Some("ally"));
        let mut sections = BTreeMap::new();
        let (device, notes) = chosen("ASUSTeK COMPUTER INC.", &ally, &table(&[]), &sections);
        assert_eq!(device, None);
        assert_eq!(
            notes,
            ["device ally: needs mode; the device button is not mapped"]
        );

        sections.insert("ally".to_string(), table(&[("mode", "quiet")]));
        let mut notes = Vec::new();
        let selection = select(
            &DEVICES,
            &machine("ASUSTeK COMPUTER INC."),
            &DeviceChoice::Auto,
            &table(&[]),
            &sections,
            &mut notes,
        );
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(selection.id.as_deref(), Some("ally"));
        assert_eq!(selection.options, table(&[("mode", "quiet")]));
    }

    #[test]
    fn the_profile_is_read_as_the_configuration_spells_it() {
        assert_eq!(DeviceChoice::from_profile(None), DeviceChoice::Auto);
        assert_eq!(DeviceChoice::from_profile(Some("auto")), DeviceChoice::Auto);
        assert_eq!(DeviceChoice::from_profile(Some("none")), DeviceChoice::None);
        assert_eq!(
            DeviceChoice::from_profile(Some("x")),
            DeviceChoice::Named("x".into())
        );
    }
}

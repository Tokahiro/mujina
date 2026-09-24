//! Devices as plug-ins: what the configuration, the tools and the agent know about a handheld
//! and its extra buttons without naming it.
//!
//! A device describes itself with a [`DeviceDescriptor`]: its id, which machines it is, its
//! buttons and its options under `[device.<id>]`. Descriptors are trait objects, not a table of
//! functions, because most devices are data: a profile file read at start-up is a descriptor as
//! much as a device crate is. The composition root lists them (ADR-0013); nothing in this ring or
//! in the configuration adapter changes for a new one. What a device does at run time is behind
//! the port [`DeviceButtons`](crate::ports::DeviceButtons).

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
    /// Mujina takes it before anything else sees it, and sends it on only where it has nothing
    /// to do with it (a key chord).
    Swallowed,
    /// Mujina only watches it: the device's own software reacts to the same press whatever
    /// Mujina does (a vendor HID report, which no program can hold back without a driver).
    /// Mujina can add to what the button does, never replace it; the device's `doctor` checks
    /// should say which program also reacts.
    Observed,
}

/// One extra button of a device.
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

/// What Mujina knows about a device before running it: enough for the configuration to choose
/// it and read its options, and for the tools and the agent to name its buttons. Free of side
/// effects, so it is built and tested everywhere.
pub trait DeviceDescriptor: Send + Sync {
    /// What `[device] profile = "..."` names, and `[device.<id>]` holds the options of: lower
    /// case, digits, `_` and `-`. Users have it in their files, so it never changes.
    fn id(&self) -> &str;

    /// What Mujina Settings lists it as, e.g. "OneXPlayer (show desktop button)".
    fn name(&self) -> String;

    /// Whether this is the machine `identity` describes; `profile = "auto"` takes the first
    /// device that says so.
    fn matches(&self, identity: &SystemIdentity) -> bool;

    /// Its extra buttons, at least one.
    fn buttons(&self) -> Vec<ButtonSpec>;

    /// Its options, the keys of `[device.<id>]`; most devices have none. Nothing else is read
    /// there: an unknown key, or a value of the wrong kind, is skipped with a note of its own.
    fn settings(&self) -> &[SettingSpec];

    /// Rules across options that [`settings`](Self::settings) cannot say. A note means the
    /// device cannot be used with these options, and no button is mapped. Only looks.
    fn validate(&self, _options: &OptionTable, _notes: &mut Vec<String>) {}

    /// The translations of its texts (its name, its buttons' labels, and the title and help of
    /// each setting), by the language they are in, as a launcher's
    /// [`catalogs`](crate::launcher::LauncherDescriptor::catalogs). A profile read from a file
    /// has none: its words stay as written.
    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// The devices compiled in, as the composition root lists them.
#[derive(Clone, Copy)]
pub struct Devices {
    pub all: &'static [&'static dyn DeviceDescriptor],
    /// A button of the user's own, whose options are `[device.button]`: in use whenever that
    /// section names one, whatever `[device] profile` says. It is among [`all`](Self::all), but
    /// neither `auto` nor a profile's id chooses it.
    pub own: &'static dyn DeviceDescriptor,
}

impl Devices {
    /// The device `id` names, the one of one's own included.
    pub fn find(&self, id: &str) -> Option<&'static dyn DeviceDescriptor> {
        self.all
            .iter()
            .copied()
            .find(|descriptor| descriptor.id() == id)
    }

    /// Whether `descriptor` is the button of one's own.
    pub fn is_own(&self, descriptor: &dyn DeviceDescriptor) -> bool {
        descriptor.id() == self.own.id()
    }

    /// The devices `[device] profile` can name, in order: all but the button of one's own.
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
    /// The descriptor's id; `None` for no button at all.
    pub id: Option<String>,
    /// Its options: `[device.<id>]`, or `[device.button]` for a button of one's own.
    pub options: OptionTable,
}

impl DeviceSelection {
    /// No button: nothing is caught.
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
    /// The device of this id.
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

/// The device the configuration comes to on the machine `identity` describes: a button of one's
/// own when `own`, `[device.button]` as read, names one that can be used; else what `choice`
/// asks for. `sections` holds `[device.<id>]` of each device that has options. Whatever cannot
/// be used is a note, and then no button is mapped.
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
    /// Stands in for the keyboard crate's button of one's own: both keys, or neither.
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

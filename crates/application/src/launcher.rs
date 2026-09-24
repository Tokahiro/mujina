//! Launchers as plug-ins: descriptors the composition root lists (ADR-0013).

use std::collections::BTreeMap;

use crate::settings::SettingValue;
use crate::settings::schema::{Applies, SettingSpec};

// Defined in the domain, where the button is decided.
pub use mujina_domain::button::LauncherCaps;

/// A section's options by key, only those set (defaults: [`SettingSpec::value_in`]). Never nested.
pub type OptionTable = BTreeMap<String, SettingValue>;

/// The launcher the configuration names, with its options as read and checked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LauncherSelection {
    /// Empty where nothing was read: the default launcher then applies.
    pub id: String,
    pub options: OptionTable,
}

/// What Mujina knows about a launcher before running it. Portable and free of side effects.
pub trait LauncherDescriptor: Sync {
    /// `[launcher.<id>]`: lower case, digits and `_`. Never changes: users' files name it.
    fn id(&self) -> &'static str;

    fn name(&self) -> &'static str;

    /// The keys of `[launcher.<id>]`.
    fn settings(&self) -> &'static [SettingSpec];

    /// Its part of the configuration template: `[launcher.<id>]` and every option of
    /// [`settings`](Self::settings), commented out, each with a word on what it does.
    fn template(&self) -> &'static str;

    /// What it offers with `options`; asked again when live options change ([`with_live`]).
    fn capabilities(&self, options: &OptionTable) -> LauncherCaps;

    /// Rules across options; a note makes the default launcher apply. No side effects.
    fn validate(&self, _options: &OptionTable, _notes: &mut Vec<String>) {}

    /// Programs that would fight with Mujina (lower-case file names); `doctor` warns about them.
    fn conflicting_processes(&self) -> &'static [&'static str] {
        &[]
    }

    /// Translations of its name, settings texts and checks' titles, as (`"de"`, `include_str!`d
    /// gettext `.po` file) pairs.
    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

#[derive(Clone, Copy)]
pub struct Launchers {
    pub all: &'static [&'static dyn LauncherDescriptor],
    /// Used where the configuration names no launcher, or one that cannot be used.
    pub fallback: &'static dyn LauncherDescriptor,
}

impl Launchers {
    pub fn find(&self, id: &str) -> Option<&'static dyn LauncherDescriptor> {
        self.all
            .iter()
            .copied()
            .find(|descriptor| descriptor.id() == id)
    }

    pub fn get(&self, id: &str) -> &'static dyn LauncherDescriptor {
        self.find(id).unwrap_or(self.fallback)
    }
}

/// The part of `options` that applies at once ([`Applies::Live`]).
pub fn live_options(specs: &[SettingSpec], options: &OptionTable) -> OptionTable {
    options
        .iter()
        .filter(|(key, _)| {
            specs
                .iter()
                .any(|spec| spec.key == key.as_str() && spec.applies == Applies::Live)
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Live options as `live` has them (one missing: back to its default), the rest as `started`.
pub fn with_live(specs: &[SettingSpec], started: &OptionTable, live: &OptionTable) -> OptionTable {
    let applies_at_once = |key: &str| {
        specs
            .iter()
            .any(|spec| spec.key == key && spec.applies == Applies::Live)
    };
    let mut running = started.clone();
    running.retain(|key, _| !applies_at_once(key));
    running.extend(live_options(specs, live));
    running
}

pub fn waiting_for_next_session(
    specs: &[SettingSpec],
    before: &OptionTable,
    after: &OptionTable,
) -> Vec<&'static str> {
    specs
        .iter()
        .filter(|spec| spec.applies == Applies::NextSession)
        .filter(|spec| spec.value_in(before) != spec.value_in(after))
        .map(|spec| spec.key)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::schema::SettingKind;
    use crate::testing::FakeLauncherDescriptor;

    static FIRST: FakeLauncherDescriptor =
        FakeLauncherDescriptor::named("first", "First", LauncherCaps::ALL);
    static SECOND: FakeLauncherDescriptor =
        FakeLauncherDescriptor::named("second", "Second", LauncherCaps::ALL);
    static LAUNCHERS: Launchers = Launchers {
        all: &[&FIRST, &SECOND],
        fallback: &FIRST,
    };

    const SPECS: &[SettingSpec] = &[
        SettingSpec {
            key: "now",
            kind: SettingKind::Toggle { default: false },
            title: "",
            help: "",
            applies: Applies::Live,
            requires: None,
            required: false,
        },
        SettingSpec {
            key: "later",
            kind: SettingKind::Toggle { default: true },
            title: "",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: false,
        },
    ];

    fn options(pairs: &[(&str, bool)]) -> OptionTable {
        pairs
            .iter()
            .map(|(key, on)| ((*key).to_string(), SettingValue::Bool(*on)))
            .collect()
    }

    #[test]
    fn a_launcher_is_found_by_its_id_or_else_the_fallback_applies() {
        assert_eq!(
            LAUNCHERS.find("second").map(LauncherDescriptor::id),
            Some("second")
        );
        assert!(LAUNCHERS.find("third").is_none());
        assert_eq!(LAUNCHERS.get("third").id(), "first");
        assert_eq!(LAUNCHERS.get("").id(), "first");
    }

    #[test]
    fn only_live_options_reach_a_running_launcher() {
        let all = options(&[("now", true), ("later", false), ("unknown", true)]);
        assert_eq!(live_options(SPECS, &all), options(&[("now", true)]));
    }

    #[test]
    fn a_running_launcher_takes_the_live_options_and_keeps_the_rest_as_started() {
        let started = options(&[("now", true), ("later", false)]);
        assert_eq!(
            with_live(SPECS, &started, &options(&[("later", true)])),
            options(&[("later", false)])
        );
        assert_eq!(
            with_live(SPECS, &OptionTable::new(), &options(&[("now", false)])),
            options(&[("now", false)])
        );
    }

    #[test]
    fn a_change_that_waits_is_named_and_a_default_written_out_is_none() {
        let before = OptionTable::new();
        assert_eq!(
            waiting_for_next_session(SPECS, &before, &options(&[("later", false)])),
            ["later"]
        );
        assert!(waiting_for_next_session(SPECS, &before, &options(&[("later", true)])).is_empty());
        assert!(waiting_for_next_session(SPECS, &before, &options(&[("now", true)])).is_empty());
    }
}

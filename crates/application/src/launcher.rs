//! Launchers as plug-ins: what the configuration, the tools and the use cases know about a
//! launcher without naming it.
//!
//! A launcher crate describes itself with a [`LauncherDescriptor`]: its id, its options under
//! `[launcher.<id>]` and what it offers. The composition root lists the launchers compiled in
//! (ADR-0013); nothing in this ring or in the configuration adapter changes for a new one. What
//! a launcher does is behind the ports [`HomeLauncher`](crate::ports::HomeLauncher) and
//! [`SessionLauncher`](crate::ports::SessionLauncher).

use std::collections::BTreeMap;

use crate::settings::SettingValue;
use crate::settings::schema::{Applies, SettingSpec};

// Defined in the domain, where the button is decided; a launcher says it through its descriptor.
pub use mujina_domain::button::LauncherCaps;

/// A launcher's options, `[launcher.<id>]` as the configuration holds them, by key. Only what
/// is set; a setting's default applies to the rest ([`SettingSpec::value_in`]). Flat: a value
/// is never a table.
pub type OptionTable = BTreeMap<String, SettingValue>;

/// The launcher the configuration names, with its options as read and checked.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LauncherSelection {
    /// The descriptor's id. Empty where nothing was read: the default launcher then applies.
    pub id: String,
    pub options: OptionTable,
}

/// What Mujina knows about a launcher before running it: enough for the configuration to read
/// and check its options, for the tools to show them, and for the button to know what it may
/// ask for. Portable and free of side effects, so it is built and tested everywhere.
pub trait LauncherDescriptor: Sync {
    /// What `kind = "..."` names and `[launcher.<id>]` holds the options of: lower case,
    /// digits and `_`. Users have it in their files, so it never changes.
    fn id(&self) -> &'static str;

    /// What Mujina Settings lists it as, e.g. "Steam Big Picture".
    fn name(&self) -> &'static str;

    /// Its options, the keys of `[launcher.<id>]`. Nothing else is read there: an unknown key,
    /// or a value of the wrong kind, is skipped with a note of its own.
    fn settings(&self) -> &'static [SettingSpec];

    /// Its part of the configuration template: `[launcher.<id>]` and every option, commented
    /// out, each with a word on what it does. A test holds it to [`settings`](Self::settings).
    fn template(&self) -> &'static str;

    /// What it offers with `options`: decides what the device button asks of it, and which of
    /// Mujina Settings' rows apply. A running agent asks again whenever options that apply at
    /// once change, with those as changed and the rest as its session started
    /// ([`with_live`]).
    fn capabilities(&self, options: &OptionTable) -> LauncherCaps;

    /// Rules across options that [`settings`](Self::settings) cannot say, such as a default
    /// taken from another option. A note means the options cannot be used as they are, and the
    /// default launcher is used instead. Called whenever the configuration is read or a change
    /// checked, so it only looks.
    fn validate(&self, _options: &OptionTable, _notes: &mut Vec<String>) {}

    /// Programs that do this launcher's job the way Mujina does and would fight with it (file
    /// names, lower case); `doctor` warns while one runs.
    fn conflicting_processes(&self) -> &'static [&'static str] {
        &[]
    }

    /// The translations of its texts (its name, and the title and help of each setting), by the
    /// language they are in: (`"de"`, a gettext `.po` file, `include_str!`d). Mujina Settings
    /// shows them in its language; without one, English. Its checks' titles may be in them too.
    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// The launchers compiled in, as the composition root lists them.
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

    /// The launcher `id` names, or the fallback.
    pub fn get(&self, id: &str) -> &'static dyn LauncherDescriptor {
        self.find(id).unwrap_or(self.fallback)
    }
}

/// The part of `options` a running launcher takes over: the settings that apply at once.
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

/// What a running launcher's options come to after a change: those that apply at once as
/// `live` has them (one missing there is back to its default), the rest as `started`.
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

/// The settings of `specs` that changed from `before` to `after` and wait for the next session.
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
        // A live option unset is back to its default; one that waits stays as started.
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
        // The default, now written down: nothing changes.
        assert!(waiting_for_next_session(SPECS, &before, &options(&[("later", true)])).is_empty());
        // Live changes do not wait.
        assert!(waiting_for_next_session(SPECS, &before, &options(&[("now", true)])).is_empty());
    }
}

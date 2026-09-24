//! Settings as data, so the configuration reader and the tools need not know them. Launchers and
//! devices describe their own ([`LauncherDescriptor::settings`], [`DeviceDescriptor::settings`]).

use crate::device::DeviceDescriptor;
use crate::launcher::{LauncherDescriptor, OptionTable};
use crate::settings::SettingValue;

/// One setting, by its key within its section: `ui_link` in `[launcher.steam]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    /// Lower case, digits and `_`, e.g. `wifi_indicator`.
    pub key: &'static str,
    pub kind: SettingKind,
    /// A short English name and its translation key. Write it as `Msg::new("…").english()` so
    /// `cargo xtask i18n-check` finds it. Core settings stay English: Mujina Settings words them.
    pub title: &'static str,
    /// A sentence or two shown under the title; English, like the title.
    pub help: &'static str,
    pub applies: Applies,
    /// A toggle of the same section without which this setting does nothing.
    pub requires: Option<&'static str>,
    /// The section cannot be used without a value here. Only for kinds without a default.
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    Toggle {
        default: bool,
    },
    Choice {
        values: &'static [&'static str],
        default: &'static str,
    },
    /// Nothing applies while it is not set.
    Text {
        format: TextFormat,
    },
    /// Edited as one line with [`TextFormat::ArgumentList`]'s rules, e.g. a program's arguments.
    TextList,
    Number {
        min: i64,
        max: i64,
        default: i64,
    },
}

/// What a text holds, for the field that edits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFormat {
    Plain,
    /// A file, such as a launcher's program.
    Path,
    /// Arguments as they are typed on a command line: spaces separate, quotes group.
    ArgumentList,
    /// A key combination, such as `LCTRL+1`.
    Chord,
}

/// When a change reaches a running agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    /// At once: the agent re-reads the configuration when told it changed (ADR-0010).
    Live,
    /// The next time Xbox mode is entered: what the launcher was started with, or what the home
    /// role and the agent must agree on for a whole session.
    NextSession,
}

impl SettingKind {
    /// What a note asks for when a value of another kind was given.
    pub fn expected(&self) -> String {
        match self {
            SettingKind::Toggle { .. } => "true or false".to_string(),
            SettingKind::Choice { values, .. } => {
                let quoted: Vec<String> =
                    values.iter().map(|value| format!("\"{value}\"")).collect();
                format!("one of {}", quoted.join(", "))
            }
            SettingKind::Text { .. } => "text in quotes".to_string(),
            SettingKind::TextList => "a list of texts in quotes".to_string(),
            SettingKind::Number { min, max, .. } => format!("a whole number from {min} to {max}"),
        }
    }

    pub fn admits(&self, value: &SettingValue) -> bool {
        match (self, value) {
            (SettingKind::Toggle { .. }, SettingValue::Bool(_))
            | (SettingKind::Text { .. }, SettingValue::Text(_))
            | (SettingKind::TextList, SettingValue::TextList(_)) => true,
            (SettingKind::Choice { values, .. }, SettingValue::Text(text)) => {
                values.contains(&text.as_str())
            }
            (SettingKind::Number { min, max, .. }, SettingValue::Integer(number)) => {
                (min..=max).contains(&number)
            }
            _ => false,
        }
    }

    pub fn default_value(&self) -> Option<SettingValue> {
        match self {
            SettingKind::Toggle { default } => Some(SettingValue::Bool(*default)),
            SettingKind::Choice { default, .. } => Some(SettingValue::Text((*default).to_string())),
            SettingKind::Number { default, .. } => Some(SettingValue::Integer(*default)),
            SettingKind::Text { .. } | SettingKind::TextList => None,
        }
    }
}

impl SettingSpec {
    /// What applies: the value in `options`, or else the default.
    pub fn value_in(&self, options: &OptionTable) -> Option<SettingValue> {
        options
            .get(self.key)
            .cloned()
            .or_else(|| self.kind.default_value())
    }
}

/// Whether toggle `key` is on in `options` or by default; `false` for a key that is no toggle.
pub fn flag(specs: &[SettingSpec], options: &OptionTable, key: &str) -> bool {
    let spec = specs.iter().find(|spec| spec.key == key);
    matches!(
        spec.and_then(|spec| spec.value_in(options)),
        Some(SettingValue::Bool(true))
    )
}

#[derive(Debug, Clone, Copy)]
pub struct SettingSection {
    /// Dotted, e.g. `device.button`.
    pub name: &'static str,
    pub settings: &'static [SettingSpec],
}

/// Mujina's own settings; `[launcher.<id>]` and `[device.<id>]` come from the descriptors.
pub static CORE: &[SettingSection] = &[
    SettingSection {
        name: "features",
        settings: &[
            SettingSpec {
                key: "button_remap",
                kind: SettingKind::Toggle { default: true },
                title: "Use the device button",
                help: "Opens the launcher's menu, or the overlay while a game runs.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "game_start_screen",
                kind: SettingKind::Toggle { default: true },
                title: "Keep the starting screen while a game loads",
                help: "The launcher stays on its \"game is starting\" screen until the game \
                       appears. Only for a launcher that notices games.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "launch_screen",
                kind: SettingKind::Toggle { default: false },
                title: "Black screen while the launcher starts",
                help: "Instead of Xbox mode's own backdrop.",
                applies: Applies::NextSession,
                requires: None,
                required: false,
            },
        ],
    },
    SettingSection {
        name: "launcher",
        settings: &[
            // Not a choice of fixed values: the launchers are those compiled in.
            SettingSpec {
                key: "kind",
                kind: SettingKind::Text {
                    format: TextFormat::Plain,
                },
                title: "Your launcher",
                help: "The id of a launcher Mujina has, such as \"steam\" (the default) or \
                       \"generic\".",
                applies: Applies::NextSession,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "on_exit",
                kind: SettingKind::Choice {
                    values: &["relaunch_on_crash", "relaunch_always", "nothing"],
                    default: "relaunch_on_crash",
                },
                title: "When the launcher closes",
                help: "Whether Mujina brings the launcher back after a crash, always, or never.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "menu",
                kind: SettingKind::Text {
                    format: TextFormat::Chord,
                },
                title: "Menu shortcut",
                help: "Sent instead of the launcher's own.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "overlay",
                kind: SettingKind::Text {
                    format: TextFormat::Chord,
                },
                title: "Overlay shortcut",
                help: "Sent inside games instead of the launcher's own.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
        ],
    },
    SettingSection {
        name: "device",
        settings: &[SettingSpec {
            key: "profile",
            kind: SettingKind::Text {
                format: TextFormat::Plain,
            },
            title: "Which button your handheld has",
            help: "\"auto\" (the default), \"none\", or the id of a device Mujina has.",
            applies: Applies::Live,
            requires: None,
            required: false,
        }],
    },
    SettingSection {
        name: "device.button",
        settings: &[
            SettingSpec {
                key: "modifier",
                kind: SettingKind::Text {
                    format: TextFormat::Plain,
                },
                title: "Button modifier",
                help: "The modifier key of a button of your own, e.g. LWIN. Wins over the \
                       profile.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "key",
                kind: SettingKind::Text {
                    format: TextFormat::Plain,
                },
                title: "Button key",
                help: "The key of a button of your own, e.g. D.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "injected_only",
                kind: SettingKind::Toggle { default: true },
                title: "Only when sent by a program",
                help: "The button counts only when a program sends its keys, as the device's \
                       own software does, not when they are typed.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
        ],
    },
    SettingSection {
        name: "timing",
        settings: &[
            SettingSpec {
                key: "modifier_gap_ms",
                kind: SettingKind::Number {
                    min: 0,
                    max: 65_535,
                    default: 20,
                },
                title: "Pause between keys",
                help: "Between the keys of a shortcut Mujina sends, in milliseconds.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "key_hold_ms",
                kind: SettingKind::Number {
                    min: 0,
                    max: 65_535,
                    default: 50,
                },
                title: "Key held for",
                help: "How long each key of a shortcut is held, in milliseconds.",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
        ],
    },
    SettingSection {
        name: "logging",
        settings: &[SettingSpec {
            key: "level",
            kind: SettingKind::Choice {
                values: &["info", "debug"],
                default: "info",
            },
            title: "Detailed log",
            help: "\"debug\" also logs every event the agent sees, for bug reports.",
            applies: Applies::Live,
            requires: None,
            required: false,
        }],
    },
    SettingSection {
        name: "interface",
        settings: &[SettingSpec {
            key: "language",
            kind: SettingKind::Choice {
                values: &["auto", "en", "de"],
                default: "en",
            },
            title: "Language",
            help: "Of Mujina Settings: \"auto\" follows Windows.",
            applies: Applies::Live,
            requires: None,
            required: false,
        }],
    },
];

/// The setting dotted `key` names, among the core settings, `launchers` and `devices`.
pub fn find(
    key: &str,
    launchers: &[&dyn LauncherDescriptor],
    devices: &[&'static dyn DeviceDescriptor],
) -> Option<&'static SettingSpec> {
    let (section, leaf) = key.rsplit_once('.')?;
    let settings = if let Some(core) = CORE.iter().find(|core| core.name == section) {
        core.settings
    } else if let Some(id) = section.strip_prefix("launcher.") {
        launchers
            .iter()
            .find(|launcher| launcher.id() == id)?
            .settings()
    } else {
        let id = section.strip_prefix("device.")?;
        devices.iter().find(|device| device.id() == id)?.settings()
    };
    settings.iter().find(|spec| spec.key == leaf)
}

/// Whether an agent running the launcher `running` (an id) takes a change of dotted `key` over at
/// once. Unknown keys and other launchers' options wait for the next session.
pub fn takes_effect_live(
    key: &str,
    launchers: &[&dyn LauncherDescriptor],
    devices: &[&'static dyn DeviceDescriptor],
    running: &str,
) -> bool {
    let of_another = key
        .rsplit_once('.')
        .and_then(|(section, _)| section.strip_prefix("launcher."))
        .is_some_and(|id| id != running);
    !of_another && find(key, launchers, devices).is_some_and(|spec| spec.applies == Applies::Live)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::LauncherCaps;
    use crate::settings::Settings;
    use crate::testing::{FakeDevice, FakeLauncherDescriptor};

    static WITH_A_MODE: FakeDevice = FakeDevice {
        settings: &[
            SettingSpec {
                key: "mode",
                kind: SettingKind::Toggle { default: false },
                title: "Mode",
                help: "",
                applies: Applies::Live,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "link",
                kind: SettingKind::Toggle { default: true },
                title: "Link",
                help: "",
                applies: Applies::NextSession,
                requires: None,
                required: false,
            },
        ],
        ..FakeDevice::named("ally", "ROG Ally")
    };

    static WITH_A_LINK: FakeLauncherDescriptor = FakeLauncherDescriptor {
        settings: &[
            SettingSpec {
                key: "link",
                kind: SettingKind::Toggle { default: true },
                title: "Link",
                help: "",
                applies: Applies::NextSession,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "start_screen",
                kind: SettingKind::Toggle { default: false },
                title: "Start screen",
                help: "",
                applies: Applies::Live,
                requires: Some("link"),
                required: false,
            },
        ],
        template: "# [launcher.fake]\n# link = true\n# start_screen = false\n",
        ..FakeLauncherDescriptor::named("fake", "Fake", LauncherCaps::ALL)
    };

    #[test]
    fn only_what_a_session_depends_on_waits_for_the_next_one() {
        static SECOND: FakeLauncherDescriptor = FakeLauncherDescriptor {
            template: "# [launcher.second]\n# link = true\n# start_screen = false\n",
            ..FakeLauncherDescriptor {
                id: "second",
                ..WITH_A_LINK
            }
        };
        let launchers: [&dyn LauncherDescriptor; 2] = [&WITH_A_LINK, &SECOND];
        let takes_effect_live = |key| takes_effect_live(key, &launchers, &[&WITH_A_MODE], "fake");
        for live in [
            "features.button_remap",
            "features.game_start_screen",
            "launcher.menu",
            "launcher.on_exit",
            "device.profile",
            "device.button.key",
            "timing.key_hold_ms",
            "logging.level",
            "launcher.fake.start_screen",
            "device.ally.mode",
        ] {
            assert!(takes_effect_live(live), "{live}");
        }
        for later in [
            "launcher.kind",
            "features.launch_screen",
            "launcher.fake.link",
            "features.surprise",
            "launcher.fake.surprise",
            "launcher.other.link",
            // Another launcher's.
            "launcher.second.start_screen",
            "features",
            "device.ally.link",
            "device.ally.surprise",
            "device.other.mode",
        ] {
            assert!(!takes_effect_live(later), "{later}");
        }
    }

    #[test]
    fn the_core_defaults_are_those_of_the_settings() {
        let defaults = Settings::default();
        let value = |key: &str| find(key, &[], &[]).and_then(|spec| spec.kind.default_value());
        assert_eq!(
            value("features.button_remap"),
            Some(SettingValue::Bool(defaults.button_remap))
        );
        assert_eq!(
            value("features.game_start_screen"),
            Some(SettingValue::Bool(defaults.game_start_screen))
        );
        assert_eq!(
            value("features.launch_screen"),
            Some(SettingValue::Bool(defaults.launch_screen))
        );
        assert_eq!(
            value("timing.modifier_gap_ms"),
            Some(SettingValue::Integer(
                defaults.timing.modifier_gap_ms.into()
            ))
        );
        assert_eq!(
            value("timing.key_hold_ms"),
            Some(SettingValue::Integer(defaults.timing.key_hold_ms.into()))
        );
        assert_eq!(
            value("logging.level"),
            Some(SettingValue::Text("info".into()))
        );
    }

    #[test]
    fn each_core_section_is_sound() {
        for section in CORE {
            crate::testing::check_settings(section.name, section.settings);
        }
    }

    #[test]
    fn kinds_take_their_own_values_only() {
        let toggle = SettingKind::Toggle { default: false };
        assert!(toggle.admits(&SettingValue::Bool(true)));
        assert!(!toggle.admits(&SettingValue::Text("true".into())));
        let choice = SettingKind::Choice {
            values: &["a", "b"],
            default: "a",
        };
        assert!(choice.admits(&SettingValue::Text("b".into())));
        assert!(!choice.admits(&SettingValue::Text("c".into())));
        assert_eq!(choice.expected(), "one of \"a\", \"b\"");
        let number = SettingKind::Number {
            min: 10,
            max: 20,
            default: 15,
        };
        assert!(number.admits(&SettingValue::Integer(20)));
        assert!(!number.admits(&SettingValue::Integer(21)));
        assert_eq!(number.expected(), "a whole number from 10 to 20");
        assert!(SettingKind::TextList.admits(&SettingValue::TextList(Vec::new())));
        assert!(!SettingKind::TextList.admits(&SettingValue::Text(String::new())));
    }

    #[test]
    fn a_toggle_is_what_is_set_or_else_its_default() {
        let specs = WITH_A_LINK.settings;
        let mut options = OptionTable::new();
        assert!(flag(specs, &options, "link"));
        assert!(!flag(specs, &options, "start_screen"));
        assert!(!flag(specs, &options, "surprise"));
        options.insert("link".into(), SettingValue::Bool(false));
        assert!(!flag(specs, &options, "link"));
    }
}

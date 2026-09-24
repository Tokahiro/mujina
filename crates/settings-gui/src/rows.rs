//! Setup rows from a launcher's or device's `SettingSpec`s, so a new one needs no change here.

use mujina_application::settings::SettingValue;
use mujina_application::settings::schema::{self, Applies, SettingKind, SettingSpec, TextFormat};
use mujina_application::{Msg, launcher::OptionTable};
use slint::{ModelRc, SharedString, VecModel};

use crate::form;
use crate::ui::{PluginRows, RowData, RowKind};

const PATH_EXAMPLE: &str = "C:\\…\\Launcher.exe";

// The words Mujina Settings adds to a launcher's or a device's own.
pub const REQUIRED: Msg = Msg::new("Required.");
pub const APPLIES_NOW: Msg = Msg::new("Applies at once in Xbox mode.");
pub const APPLIES_NEXT_TIME: Msg = Msg::new("Takes effect the next time you enter Xbox mode.");
pub const ENTER_TO_SWITCH: Msg = Msg::new("Enter it to switch to this launcher.");
pub const NOT_SET: Msg = Msg::new("Not set");
/// An empty list of arguments.
pub const NOTHING: Msg = Msg::new("Nothing");

/// A launcher's switches join What Mujina fixes; a device's rows stay with its button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Launcher,
    Device,
}

#[derive(Debug, Default)]
pub struct Rows {
    pub main: Vec<RowData>,
    pub features: Vec<RowData>,
    pub advanced: Vec<RowData>,
}

impl Rows {
    pub fn model(self) -> PluginRows {
        let model = |rows: Vec<RowData>| ModelRc::from(std::rc::Rc::new(VecModel::from(rows)));
        PluginRows {
            main: model(self.main),
            features: model(self.features),
            advanced: model(self.advanced),
        }
    }
}

/// The rows of `specs` for `[section]`. `draft`: the launcher is chosen but not stored yet.
pub fn build(
    owner: Owner,
    section: &str,
    specs: &[SettingSpec],
    options: &OptionTable,
    draft: bool,
    words: &dyn Fn(&str) -> String,
) -> Rows {
    let mut rows = Rows::default();
    for spec in specs {
        let mut row = row(spec, options, words);
        row.key = format!("{section}.{}", spec.key).into();
        row.disabled = spec
            .requires
            .is_some_and(|toggle| !schema::flag(specs, options, toggle));
        let help = words(spec.help);
        let feature = owner == Owner::Launcher && matches!(spec.kind, SettingKind::Toggle { .. });
        let detail = !spec.required
            && matches!(
                spec.kind,
                SettingKind::Number { .. }
                    | SettingKind::Text {
                        format: TextFormat::Plain | TextFormat::Chord
                    }
            );
        row.description = if spec.required && draft && spec.value_in(options).is_none() {
            words(ENTER_TO_SWITCH.english())
        } else if spec.required {
            joined(&help, &words(REQUIRED.english()))
        } else if feature {
            let applies = match spec.applies {
                Applies::Live => APPLIES_NOW,
                Applies::NextSession => APPLIES_NEXT_TIME,
            };
            joined(&help, &words(applies.english()))
        } else {
            help
        }
        .into();
        if feature {
            rows.features.push(row);
        } else if detail {
            row.small = true;
            rows.advanced.push(row);
        } else {
            rows.main.push(row);
        }
    }
    rows
}

fn row(spec: &SettingSpec, options: &OptionTable, words: &dyn Fn(&str) -> String) -> RowData {
    let value = spec.value_in(options);
    let title = words(spec.title).into();
    match spec.kind {
        SettingKind::Toggle { .. } => RowData {
            kind: RowKind::Toggle,
            title,
            on: value == Some(SettingValue::Bool(true)),
            ..RowData::default()
        },
        SettingKind::Choice { values, .. } => {
            let shown: Vec<SharedString> = values.iter().map(|value| words(value).into()).collect();
            let chosen = form::as_text(value);
            RowData {
                kind: RowKind::Choice,
                title,
                options: ModelRc::from(std::rc::Rc::new(VecModel::from(shown))),
                selected: form::index_of(values, &chosen),
                ..RowData::default()
            }
        }
        SettingKind::Text { format } => RowData {
            kind: RowKind::Text,
            title,
            value: form::as_text(value).into(),
            placeholder: match format {
                TextFormat::Path => PATH_EXAMPLE.to_string(),
                TextFormat::ArgumentList => words(NOTHING.english()),
                TextFormat::Plain | TextFormat::Chord => words(NOT_SET.english()),
            }
            .into(),
            ..RowData::default()
        },
        SettingKind::TextList => RowData {
            kind: RowKind::Text,
            title,
            value: form::as_text(value).into(),
            placeholder: words(NOTHING.english()).into(),
            ..RowData::default()
        },
        SettingKind::Number { min, max, default } => {
            let number = match value {
                Some(SettingValue::Integer(number)) => number,
                _ => default,
            };
            let int = |number: i64| {
                i32::try_from(number).unwrap_or(if number < 0 { i32::MIN } else { i32::MAX })
            };
            RowData {
                kind: RowKind::Number,
                title,
                number: int(number),
                minimum: int(min),
                maximum: int(max),
                step: 1,
                ..RowData::default()
            }
        }
    }
}

fn joined(first: &str, second: &str) -> String {
    if first.is_empty() {
        second.to_string()
    } else {
        format!("{first} {second}")
    }
}

#[cfg(test)]
mod tests {
    use mujina_application::launcher::{LauncherCaps, LauncherDescriptor};
    use mujina_application::settings::SettingChange;
    use mujina_application::testing::{FakeDevice, FakeLauncherDescriptor, conformance};
    use mujina_i18n::Localizer;
    use slint::Model;

    use super::*;
    use crate::texts;

    /// A launcher Mujina Settings has never heard of, with its own German.
    static PLAYNITE: FakeLauncherDescriptor = FakeLauncherDescriptor {
        settings: &[
            SettingSpec {
                key: "fullscreen",
                kind: SettingKind::Toggle { default: true },
                title: "Start Playnite in full screen",
                help: "Its full-screen mode instead of its desktop one.",
                applies: Applies::NextSession,
                requires: None,
                required: false,
            },
            SettingSpec {
                key: "path",
                kind: SettingKind::Text {
                    format: TextFormat::Path,
                },
                title: "Playnite's program",
                help: "Playnite.FullscreenApp.exe, where Playnite is installed.",
                applies: Applies::NextSession,
                requires: None,
                required: true,
            },
        ],
        template: "# [launcher.playnite]\n# fullscreen = true\n# path = ''\n",
        catalogs: &[(
            "de",
            "msgid \"Playnite\"\nmsgstr \"Playnite\"\n\n\
             msgid \"Start Playnite in full screen\"\nmsgstr \"Playnite im Vollbild starten\"\n\n\
             msgid \"Its full-screen mode instead of its desktop one.\"\n\
             msgstr \"Sein Vollbildmodus statt seines Desktopmodus.\"\n\n\
             msgid \"Playnite's program\"\nmsgstr \"Playnites Programm\"\n\n\
             msgid \"Playnite.FullscreenApp.exe, where Playnite is installed.\"\n\
             msgstr \"Playnite.FullscreenApp.exe, dort, wo Playnite installiert ist.\"\n",
        )],
        ..FakeLauncherDescriptor::named(
            "playnite",
            "Playnite",
            LauncherCaps {
                game_detection: true,
                menu: true,
                overlay: false,
                navigation: false,
            },
        )
    };

    /// Like `texts::launcher_words`: `extra` catalogs first, then this app's, in `language`.
    fn words(language: &str, extra: &[(&str, &str)]) -> impl Fn(&str) -> String {
        let mut localizer = Localizer::new();
        for (catalog_language, po) in extra.iter().chain(&texts::CATALOGS) {
            localizer.add(catalog_language, po).unwrap();
        }
        localizer.set(language);
        move |english| localizer.text(english).to_string()
    }

    fn rows_of(rows: &[RowData]) -> Vec<(String, RowKind, String, String)> {
        rows.iter()
            .map(|row| {
                (
                    row.key.to_string(),
                    row.kind,
                    row.title.to_string(),
                    row.description.to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn a_new_launcher_gets_its_rows_in_its_own_words_without_a_change_here() {
        conformance(&PLAYNITE);
        let german = words("de", PLAYNITE.catalogs());
        let rows = build(
            Owner::Launcher,
            "launcher.playnite",
            PLAYNITE.settings(),
            &OptionTable::new(),
            false,
            &german,
        );
        assert_eq!(
            rows_of(&rows.features),
            [(
                "launcher.playnite.fullscreen".to_string(),
                RowKind::Toggle,
                "Playnite im Vollbild starten".to_string(),
                "Sein Vollbildmodus statt seines Desktopmodus. Wird beim nächsten Wechsel in den \
                 Xbox-Modus wirksam."
                    .to_string(),
            )]
        );
        assert!(rows.features[0].on, "on by default");
        assert_eq!(
            rows_of(&rows.main),
            [(
                "launcher.playnite.path".to_string(),
                RowKind::Text,
                "Playnites Programm".to_string(),
                "Playnite.FullscreenApp.exe, dort, wo Playnite installiert ist. Erforderlich."
                    .to_string(),
            )]
        );
        assert_eq!(rows.main[0].placeholder, PATH_EXAMPLE);
        assert!(rows.advanced.is_empty());

        // Chosen but not stored yet.
        let draft = build(
            Owner::Launcher,
            "launcher.playnite",
            PLAYNITE.settings(),
            &OptionTable::new(),
            true,
            &german,
        );
        assert_eq!(
            draft.main[0].description,
            "Gib es an, um zu diesem Launcher zu wechseln."
        );

        let key = "launcher.playnite.path";
        let spec = schema::find(key, &[&PLAYNITE], &[]);
        assert_eq!(
            form::text_changes(key, r" C:\Playnite\Playnite.FullscreenApp.exe ", spec).unwrap(),
            [SettingChange::set(
                key,
                SettingValue::Text(r"C:\Playnite\Playnite.FullscreenApp.exe".into())
            )]
        );
    }

    #[test]
    fn steams_rows_read_as_before_and_the_wifi_fix_needs_the_ui_link() {
        let steam = mujina_app::tool::launchers().get("steam");
        let options = OptionTable::from([("ui_link".to_string(), SettingValue::Bool(false))]);
        let english = words("en", steam.catalogs());
        let rows = build(
            Owner::Launcher,
            "launcher.steam",
            steam.settings(),
            &options,
            false,
            &english,
        );
        assert!(rows.main.is_empty() && rows.advanced.is_empty());
        assert_eq!(
            rows_of(&rows.features),
            [
                (
                    "launcher.steam.ui_link".to_string(),
                    RowKind::Toggle,
                    "Use Steam's debugging port".to_string(),
                    "For Home and Library, the “game is starting” screen and the Wi-Fi icon. The \
                     port has no password: any program you run can control Steam through it \
                     (SECURITY.md). Takes effect the next time you enter Xbox mode."
                        .to_string(),
                ),
                (
                    "launcher.steam.wifi_indicator".to_string(),
                    RowKind::Toggle,
                    "Correct the Wi-Fi icon in Steam".to_string(),
                    "Steam otherwise shows the Wi-Fi as disconnected. Also lets the device button \
                     open Steam's menus directly. Needs the debugging port. Takes effect the \
                     next time you enter Xbox mode."
                        .to_string(),
                ),
            ]
        );
        assert!(!rows.features[0].on);
        assert!(rows.features[1].disabled, "greyed out without the UI link");
        let german = words("de", steam.catalogs());
        let rows = build(
            Owner::Launcher,
            "launcher.steam",
            steam.settings(),
            &OptionTable::new(),
            false,
            &german,
        );
        assert_eq!(rows.features[1].title, "WLAN-Symbol in Steam korrigieren");
        assert!(!rows.features[1].disabled);
    }

    #[test]
    fn the_generic_launchers_details_go_under_advanced() {
        let generic = mujina_app::tool::launchers().get("generic");
        let english = words("en", generic.catalogs());
        let options = OptionTable::from([(
            "arguments".to_string(),
            SettingValue::TextList(vec!["--config".into(), "My Games".into()]),
        )]);
        let rows = build(
            Owner::Launcher,
            "launcher.generic",
            generic.settings(),
            &options,
            false,
            &english,
        );
        let keys = |rows: &[RowData]| -> Vec<String> {
            rows.iter().map(|row| row.key.to_string()).collect()
        };
        assert_eq!(
            keys(&rows.main),
            ["launcher.generic.executable", "launcher.generic.arguments"]
        );
        assert_eq!(
            rows.main[0].description,
            "The launcher Mujina starts. Required."
        );
        assert_eq!(rows.main[1].value, r#"--config "My Games""#);
        assert_eq!(rows.main[1].placeholder, "Nothing");
        assert!(rows.features.is_empty());
        assert_eq!(
            keys(&rows.advanced),
            ["launcher.generic.window_class", "launcher.generic.process"]
        );
        assert!(rows.advanced.iter().all(|row| row.small));
    }

    #[test]
    fn a_devices_options_stay_with_its_button_and_its_details_go_under_advanced() {
        static PAD: FakeDevice = FakeDevice {
            settings: &[
                SettingSpec {
                    key: "mode",
                    kind: SettingKind::Choice {
                        values: &["quiet", "loud"],
                        default: "quiet",
                    },
                    title: "Mode",
                    help: "",
                    applies: Applies::Live,
                    requires: None,
                    required: false,
                },
                SettingSpec {
                    key: "delay",
                    kind: SettingKind::Number {
                        min: 0,
                        max: 900,
                        default: 30,
                    },
                    title: "Delay",
                    help: "",
                    applies: Applies::Live,
                    requires: None,
                    required: false,
                },
            ],
            ..FakeDevice::named("pad", "Pad")
        };
        let options = OptionTable::from([("mode".to_string(), SettingValue::Text("loud".into()))]);
        let english = words("en", &[]);
        let rows = build(
            Owner::Device,
            "device.pad",
            PAD.settings,
            &options,
            false,
            &english,
        );
        assert!(rows.features.is_empty());
        assert_eq!(rows.main[0].kind, RowKind::Choice);
        assert_eq!(rows.main[0].selected, 1);
        let shown: Vec<String> = rows.main[0].options.iter().map(Into::into).collect();
        assert_eq!(shown, ["quiet", "loud"]);
        let delay = &rows.advanced[0];
        assert_eq!(
            (delay.kind, delay.number, delay.minimum, delay.maximum),
            (RowKind::Number, 30, 0, 900)
        );
    }
}

//! The generic launcher as the configuration and the tools see it: `[launcher.generic]`, and
//! how its options become a [`GenericLauncherConfig`].

use std::path::PathBuf;

use mujina_application::Msg;
use mujina_application::launcher::{LauncherCaps, LauncherDescriptor, OptionTable};
use mujina_application::settings::SettingValue;
use mujina_application::settings::schema::{Applies, SettingKind, SettingSpec, TextFormat};

pub struct GenericDescriptor;

pub static DESCRIPTOR: GenericDescriptor = GenericDescriptor;

// Each text is a `Msg`, so that i18n-check holds lang/de.po to it; the setting keeps its English.
const SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: "executable",
        kind: SettingKind::Text {
            format: TextFormat::Path,
        },
        title: Msg::new("Program").english(),
        help: Msg::new("The launcher Mujina starts.").english(),
        applies: Applies::NextSession,
        requires: None,
        required: true,
    },
    SettingSpec {
        key: "arguments",
        kind: SettingKind::TextList,
        title: Msg::new("Start it with").english(),
        help: Msg::new(
            "Added to its command line as you would type it there, with quotes around an argument \
             that contains spaces. Optional.",
        )
        .english(),
        applies: Applies::NextSession,
        requires: None,
        required: false,
    },
    SettingSpec {
        key: "window_class",
        kind: SettingKind::Text {
            format: TextFormat::Plain,
        },
        title: Msg::new("Window class").english(),
        help: Msg::new("Helps bring another launcher to the front.").english(),
        applies: Applies::NextSession,
        requires: None,
        required: false,
    },
    SettingSpec {
        key: "process",
        kind: SettingKind::Text {
            format: TextFormat::Plain,
        },
        title: Msg::new("Process name").english(),
        help: Msg::new("Only if it differs from the program's file name.").english(),
        applies: Applies::NextSession,
        requires: None,
        required: false,
    },
];

const TEMPLATE: &str = r#"# A launcher Mujina does not know, with kind = "generic" above:
# [launcher.generic]
# executable = 'C:\Path\To\Frontend.exe'
# arguments = ["--fullscreen"]
# window_class = "..."            # class of its full-screen window, if known (helps focusing)
# process = "frontend.exe"        # default: the executable's file name
"#;

/// The generic launcher's settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GenericLauncherConfig {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    /// Class name of the launcher's full-screen window. Without it a running process counts as
    /// "UI visible", and its main window is what gets focused.
    pub window_class: Option<String>,
    /// File name of the launcher's process, e.g. `playnite.fullscreenapp.exe`.
    pub process_name: String,
}

impl GenericLauncherConfig {
    /// `[launcher.generic]` as the launcher uses it, or what makes it unusable.
    pub fn from_options(options: &OptionTable) -> Result<Self, String> {
        let executable = text(options, "executable")
            .filter(|executable| !executable.is_empty())
            .ok_or_else(|| "launcher.generic.executable is not set".to_string())?;
        // Not `Path::file_name`: this crate also builds where `\` is no separator.
        let file_name = executable.rsplit(['\\', '/']).next().unwrap_or_default();
        let process_name = text(options, "process").unwrap_or(file_name).to_string();
        if process_name.is_empty() {
            return Err("launcher.generic.executable has no file name".to_string());
        }
        let arguments = match options.get("arguments") {
            Some(SettingValue::TextList(arguments)) => arguments.clone(),
            _ => Vec::new(),
        };
        Ok(Self {
            executable: PathBuf::from(executable),
            arguments,
            window_class: text(options, "window_class")
                .filter(|class| !class.is_empty())
                .map(str::to_string),
            process_name,
        })
    }

    /// The program's file name, as the pages and the log call the launcher.
    pub fn program_name(&self) -> String {
        let shown = self.executable.display().to_string();
        match shown.rsplit(['\\', '/']).next() {
            Some(name) if !name.is_empty() => name.to_string(),
            _ if !shown.is_empty() => shown,
            _ => "generic launcher".to_string(),
        }
    }
}

fn text<'a>(options: &'a OptionTable, key: &str) -> Option<&'a str> {
    match options.get(key) {
        Some(SettingValue::Text(text)) => Some(text),
        _ => None,
    }
}

impl LauncherDescriptor for GenericDescriptor {
    fn id(&self) -> &'static str {
        "generic"
    }

    fn name(&self) -> &'static str {
        Msg::new("Another launcher").english()
    }

    fn settings(&self) -> &'static [SettingSpec] {
        SETTINGS
    }

    fn template(&self) -> &'static str {
        TEMPLATE
    }

    /// Only a menu (`ESC`); games, an overlay and pages it cannot know.
    fn capabilities(&self, _options: &OptionTable) -> LauncherCaps {
        LauncherCaps {
            game_detection: false,
            menu: true,
            overlay: false,
            navigation: false,
        }
    }

    /// The process name defaults to the program's file name, so a program without one is
    /// refused. A missing program is left to the configuration, which requires the setting.
    fn validate(&self, options: &OptionTable, notes: &mut Vec<String>) {
        if !options.contains_key("executable") {
            return;
        }
        if let Err(note) = GenericLauncherConfig::from_options(options) {
            notes.push(note);
        }
    }

    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        &[("de", include_str!("../lang/de.po"))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(pairs: &[(&str, SettingValue)]) -> OptionTable {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    fn text(value: &str) -> SettingValue {
        SettingValue::Text(value.to_string())
    }

    #[test]
    fn the_descriptor_keeps_the_rules() {
        mujina_application::testing::conformance(&DESCRIPTOR);
    }

    #[test]
    fn the_configuration_carries_what_the_launcher_needs() {
        let config = GenericLauncherConfig::from_options(&options(&[
            ("executable", text(r"C:\Frontend\frontend.exe")),
            (
                "arguments",
                SettingValue::TextList(vec!["--fullscreen".to_string()]),
            ),
            ("window_class", text("FrontendWindow")),
        ]))
        .unwrap();
        assert_eq!(
            config.executable,
            PathBuf::from(r"C:\Frontend\frontend.exe")
        );
        assert_eq!(config.arguments, ["--fullscreen"]);
        assert_eq!(config.window_class.as_deref(), Some("FrontendWindow"));
        assert_eq!(config.process_name, "frontend.exe");
        assert_eq!(config.program_name(), "frontend.exe");
    }

    #[test]
    fn a_named_process_wins_over_the_executable_file_name() {
        let config = GenericLauncherConfig::from_options(&options(&[
            ("executable", text(r"C:\Frontend\start.exe")),
            ("process", text("Frontend.Main.exe")),
            ("window_class", text("")),
        ]))
        .unwrap();
        assert_eq!(config.process_name, "Frontend.Main.exe");
        assert_eq!(config.window_class, None);
        assert!(config.arguments.is_empty());
    }

    #[test]
    fn a_program_without_a_file_name_is_refused() {
        let mut notes = Vec::new();
        DESCRIPTOR.validate(
            &options(&[("executable", text(r"C:\Frontend\"))]),
            &mut notes,
        );
        assert_eq!(notes, ["launcher.generic.executable has no file name"]);

        // Missing altogether, it is the configuration's to say.
        let mut notes = Vec::new();
        DESCRIPTOR.validate(&options(&[("process", text("x.exe"))]), &mut notes);
        assert!(notes.is_empty(), "{notes:?}");
        assert!(GenericLauncherConfig::from_options(&OptionTable::new()).is_err());
    }

    #[test]
    fn it_has_a_menu_and_nothing_else() {
        let caps = DESCRIPTOR.capabilities(&OptionTable::new());
        assert!(caps.menu);
        assert!(!caps.overlay && !caps.game_detection && !caps.navigation);
    }
}

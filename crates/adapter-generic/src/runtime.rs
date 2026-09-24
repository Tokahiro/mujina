//! The generic launcher's Windows side.

use mujina_adapter_kit::launcher;
use mujina_adapter_kit::plugin::{LauncherRuntime, SessionParts};
use mujina_application::launcher::OptionTable;
use mujina_application::ports::{
    HomeLauncher, LauncherInstall, LauncherState, PortError, PortResult, SessionLauncher,
};
use mujina_domain::keys::{KeyChord, VirtualKey};
use mujina_winutil::window::{WindowHandle, WindowRule};
use mujina_winutil::{process, window};

use crate::descriptor::GenericLauncherConfig;

pub struct GenericRuntime;

pub static RUNTIME: GenericRuntime = GenericRuntime;

impl GenericRuntime {
    /// Unusable options, which the configuration reader should never let through, give a
    /// launcher that finds nothing and says so, rather than Mujina not starting.
    fn launcher(options: &OptionTable) -> GenericLauncher {
        GenericLauncher::new(GenericLauncherConfig::from_options(options).unwrap_or_default())
    }
}

impl LauncherRuntime for GenericRuntime {
    fn home(&self, options: &OptionTable) -> Box<dyn HomeLauncher> {
        Box::new(Self::launcher(options))
    }

    fn session(&self, options: &OptionTable) -> SessionParts {
        SessionParts {
            launcher: Box::new(Self::launcher(options)),
            // It signals nothing; the agent sees only its process come and go.
            sources: Vec::new(),
        }
    }
}

pub struct GenericLauncher {
    config: GenericLauncherConfig,
}

impl GenericLauncher {
    pub fn new(config: GenericLauncherConfig) -> Self {
        Self { config }
    }

    /// Only with a configured window class; without one no window can be told from another.
    fn window_rule(&self) -> Option<WindowRule> {
        Some(WindowRule {
            class_name: self.config.window_class.clone()?,
            process_name: self.config.process_name.clone(),
            require_app_window: false,
            forbid_resizable: false,
        })
    }

    fn find_window(&self) -> Option<WindowHandle> {
        if let Some(rule) = self.window_rule() {
            window::find_top_level(&rule)
        } else {
            let pid = process::pid_of_image(&self.config.process_name)?;
            window::main_window_of(&[pid])
        }
    }
}

/// `ESC`, which most full-screen frontends treat as "menu / back". `[launcher] menu` in
/// `config.toml` overrides it.
fn escape_chord() -> KeyChord {
    KeyChord::from_keys(&[VirtualKey::ESCAPE])
        .unwrap_or(KeyChord::pair(VirtualKey::ESCAPE, VirtualKey::ESCAPE))
}

impl HomeLauncher for GenericLauncher {
    fn display_name(&self) -> String {
        self.config.program_name()
    }

    fn locate(&self) -> PortResult<LauncherInstall> {
        launcher::install_from_executable(self.config.executable.clone())
    }

    fn state(&self) -> LauncherState {
        if process::pid_of_image(&self.config.process_name).is_none() {
            LauncherState::NotRunning
        } else if self.window_rule().is_none() || self.find_window().is_some() {
            // Without a window class a running process is the best "the UI is up" there is.
            LauncherState::UiVisible
        } else {
            LauncherState::RunningWithoutUi
        }
    }

    fn start_ui(&self, install: &LauncherInstall) -> PortResult<()> {
        launcher::launch(install, &self.config.arguments)
    }

    /// Frontends bring their existing instance forward when started again; there is no generic
    /// "switch to full screen" beyond that.
    fn switch_to_ui(&self, install: &LauncherInstall) -> PortResult<()> {
        launcher::launch(install, &self.config.arguments)
    }

    fn focus_ui(&self) -> PortResult<()> {
        let handle = self
            .find_window()
            .ok_or_else(|| PortError::NotFound("the launcher's window".into()))?;
        launcher::focus_ui(handle, "the launcher")
    }
}

/// A generic launcher cannot tell whether a game runs; the device button then opens its menu
/// in the launcher and leads home everywhere else.
impl SessionLauncher for GenericLauncher {
    fn owns_process(&self, process_name: &str) -> bool {
        self.config.process_name.eq_ignore_ascii_case(process_name)
    }

    fn process_id(&self) -> Option<u32> {
        process::pid_of_image(&self.config.process_name)
    }

    /// No overlay: without game detection the button never asks for one.
    fn menu_shortcut(&self) -> Option<KeyChord> {
        Some(escape_chord())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(window_class: Option<&str>) -> GenericLauncherConfig {
        GenericLauncherConfig {
            executable: std::path::PathBuf::from(r"C:\Frontend\frontend.exe"),
            arguments: vec!["--fullscreen".to_string()],
            window_class: window_class.map(str::to_string),
            process_name: "frontend.exe".to_string(),
        }
    }

    #[test]
    fn the_window_rule_needs_a_window_class() {
        assert_eq!(GenericLauncher::new(config(None)).window_rule(), None);
        let rule = GenericLauncher::new(config(Some("FrontendWindow")))
            .window_rule()
            .unwrap();
        assert_eq!(rule.class_name, "FrontendWindow");
        assert_eq!(rule.process_name, "frontend.exe");
        assert!(!rule.require_app_window);
        assert!(!rule.forbid_resizable);
    }

    #[test]
    fn the_process_is_recognised_without_case() {
        let launcher = GenericLauncher::new(config(None));
        assert!(launcher.owns_process("Frontend.exe"));
        assert!(!launcher.owns_process("game.exe"));
    }

    #[test]
    fn a_missing_executable_is_reported() {
        let error = GenericLauncher::new(config(None)).locate().unwrap_err();
        assert!(matches!(error, PortError::NotFound(_)));
    }

    #[test]
    fn it_is_called_by_its_program() {
        assert_eq!(
            GenericLauncher::new(config(None)).display_name(),
            "frontend.exe"
        );
    }

    #[test]
    fn the_menu_is_escape_and_there_is_no_overlay() {
        let launcher = GenericLauncher::new(config(None));
        assert_eq!(
            launcher.menu_shortcut().map(|menu| menu.keys().to_vec()),
            Some(vec![VirtualKey::ESCAPE])
        );
        assert_eq!(launcher.overlay_shortcut(), None);
    }

    #[test]
    fn unusable_options_give_a_launcher_that_finds_nothing() {
        let launcher = GenericRuntime::launcher(&OptionTable::new());
        assert!(launcher.locate().is_err());
        assert!(!launcher.owns_process("steam.exe"));
    }
}

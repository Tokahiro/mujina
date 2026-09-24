//! What `mujinactl doctor` looks at on the Windows side.

use mujina_application::Msg;
use mujina_application::doctor::{Check, Finding, Remedy, Severity};
use mujina_application::ports::FseState;
use mujina_winutil::process;
use mujina_winutil::registry::{Hive, read_u32};

use crate::agent_control;

/// Windows honours the home-app capability of a sideloaded package only in Developer Mode.
pub fn developer_mode() -> bool {
    const KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";
    matches!(
        read_u32(Hive::LocalMachine, KEY, "AllowDevelopmentWithoutDevLicense"),
        Ok(Some(1))
    )
}

/// The id of the check of Developer Mode.
pub const DEVELOPER_MODE: &str = "developer mode";
/// The id of the check of the background agent.
pub const AGENT: &str = "agent";

struct DeveloperMode;

impl Check for DeveloperMode {
    fn id(&self) -> &'static str {
        DEVELOPER_MODE
    }

    fn title(&self) -> Msg {
        Msg::new("Developer Mode")
    }

    fn examine(&self) -> Finding {
        if developer_mode() {
            self.found(Severity::Ok, "on").saying(Msg::new(
                "On. Windows accepts Mujina as home app only with it.",
            ))
        } else {
            self.found(
                Severity::Problem,
                "off; Windows will not accept Mujina as home app (Settings > System > Advanced)",
            )
            .saying(Msg::new(
                "Off. Windows does not accept Mujina as home app without it (Settings › System › \
                 Advanced).",
            ))
        }
    }
}

/// The agent runs in Xbox mode only, so not running is a problem only there.
struct Agent {
    xbox_mode: FseState,
}

impl Check for Agent {
    fn id(&self) -> &'static str {
        AGENT
    }

    fn title(&self) -> Msg {
        Msg::new("Background agent")
    }

    fn examine(&self) -> Finding {
        if agent_control::agent_is_running() {
            self.found(Severity::Ok, "running").saying(Msg::new(
                "Running. It watches for the device button and for games starting.",
            ))
        } else if self.xbox_mode == FseState::Active {
            self.found(
                Severity::Problem,
                "not running; the device button does nothing until it runs",
            )
            .saying(Msg::new(
                "Not running. The device button and the Wi-Fi icon do nothing until it runs.",
            ))
            .remedied_by(Remedy::StartAgent)
        } else {
            self.found(
                Severity::Ok,
                "not running (it starts with Xbox mode and leaves with it)",
            )
            .saying(Msg::new(
                "Not running. It starts with Xbox mode and ends with it.",
            ))
        }
    }
}

/// Other tools that map the same button or feed the same icon would fight with Mujina.
struct ConflictingTools {
    /// Those that only fight over the launcher in use, as its descriptor names them.
    launcher: &'static [&'static str],
}

impl Check for ConflictingTools {
    fn id(&self) -> &'static str {
        "conflicting tools"
    }

    fn title(&self) -> Msg {
        Msg::new("Conflicting tools")
    }

    fn examine(&self) -> Finding {
        // Home-app competitors conflict with any launcher. `steambackmenu.exe` stays here until it
        // is known whether it takes the device button with any launcher.
        const KNOWN: [&str; 3] = ["steambackmenu.exe", "anyfse.exe", "omniconsole.exe"];
        let running = process::running_image_names();
        let found: Vec<&str> = KNOWN
            .into_iter()
            .chain(self.launcher.iter().copied())
            .filter(|name| running.iter().any(|image| image == name))
            .collect();
        if found.is_empty() {
            self.found(Severity::Ok, "none running")
        } else {
            self.found(
                Severity::Warning,
                format!(
                    "{} running; it does the same job as Mujina",
                    found.join(", ")
                ),
            )
        }
    }
}

/// All Windows-side checks. `conflicting`: the launcher descriptor's rival programs.
/// `xbox_mode`: whether Xbox mode is on, the only time the agent has to run.
pub fn all(conflicting: &'static [&'static str], xbox_mode: FseState) -> Vec<Box<dyn Check>> {
    vec![
        Box::new(DeveloperMode),
        Box::new(Agent { xbox_mode }),
        Box::new(ConflictingTools {
            launcher: conflicting,
        }),
    ]
}

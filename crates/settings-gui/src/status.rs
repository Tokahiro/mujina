//! Data for the Status and System pages. What the doctor found stays English on the Status page,
//! as in `mujinactl doctor`.

use mujina_app::tool::SystemFacts;
use mujina_application::device::SystemIdentity;
use mujina_application::doctor::{Finding, Remedy, Severity};
use mujina_application::settings::LoadedSettings;
use slint::Image;

use crate::texts;
use crate::ui::{
    ButtonSource, Health, HomeApp, Icons, RowData, RowKind, SystemInfo, Verdict, WifiFix,
};

/// What the Status page shows, gathered on a worker thread and shown on the window's.
pub struct Probe {
    pub findings: Vec<Finding>,
    pub settings: LoadedSettings,
    pub system: SystemIdentity,
    /// `device.profile` as `config.toml` says it; "" where it relies on the default.
    pub stored_profile: String,
    pub button: InUse,
    pub launcher: String,
    /// When the doctor was done, as "HH:MM" in local time.
    pub time: String,
}

/// The row's key is the finding's id, so its button can be told apart.
pub fn system_row(finding: &Finding, icons: &Icons<'_>) -> RowData {
    let check = check(finding);
    let (label, icon) = match finding.remedy {
        Some(Remedy::LocationSettings) => (texts::t(&texts::WINDOWS_SETTINGS), icons.get_open()),
        Some(Remedy::StartAgent) => (texts::t(&texts::START_IT), icons.get_play()),
        None => (String::new(), Image::default()),
    };
    RowData {
        kind: if label.is_empty() {
            RowKind::Info
        } else {
            RowKind::Button
        },
        key: finding.id.into(),
        small: true,
        // A way out to Windows' own page, not something Mujina does.
        plain: finding.remedy == Some(Remedy::LocationSettings),
        has_status: true,
        health: check.health,
        title: check.title.into(),
        description: said(finding).into(),
        label: label.into(),
        icon,
        ..RowData::default()
    }
}

/// The finding's translated summary, or else its English detail.
pub fn said(finding: &Finding) -> String {
    finding
        .summary
        .as_ref()
        .map_or_else(|| finding.detail.clone(), texts::t)
}

/// `steam_target`: the execution alias Steam starts, "" when Mujina runs unpackaged.
pub fn system_info(facts: &SystemFacts, steam_target: &str) -> SystemInfo {
    let registered = facts.ours.is_some() && facts.ours == facts.home_app;
    let home = match &facts.home_app {
        _ if registered => HomeApp::Mujina,
        Some(_) => HomeApp::Other,
        None => HomeApp::Windows,
    };
    SystemInfo {
        known: true,
        home,
        home_checking: false,
        other_home: facts.home_app.as_deref().unwrap_or_default().into(),
        launcher: facts.launcher.as_str().into(),
        steam_target: steam_target.into(),
        // Not a doctor fact; the caller sets it.
        can_remove: false,
    }
}

pub fn verdict(findings: &[Finding]) -> Verdict {
    let count = |severity| {
        findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    };
    let int = |count: usize| i32::try_from(count).unwrap_or(i32::MAX);
    Verdict {
        known: true,
        problems: int(count(Severity::Problem)),
        warnings: int(count(Severity::Warning)),
        total: int(findings.len()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub health: Health,
    pub title: String,
    pub detail: String,
}

pub fn health(severity: Severity) -> Health {
    match severity {
        Severity::Ok => Health::Ok,
        Severity::Warning => Health::Warning,
        Severity::Problem => Health::Problem,
    }
}

pub fn check(finding: &Finding) -> Check {
    Check {
        health: health(finding.severity),
        title: texts::t(&finding.title),
        detail: finding.detail.clone(),
    }
}

/// The device button in effect, as the device tile shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InUse {
    Nothing,
    Own { keys: String },
    Device { label: String },
}

/// The device button tile: the button ("" without one) and where it comes from. `stored` is
/// `device.profile` as in the file, "" for the default.
pub fn button_tile(remap: bool, in_use: &InUse, stored: &str) -> (String, ButtonSource) {
    if !remap {
        return (String::new(), ButtonSource::Off);
    }
    match in_use {
        InUse::Own { keys } => (keys.clone(), ButtonSource::Own),
        InUse::Device { label } => {
            let automatic = stored.is_empty() || stored == "auto";
            let source = if automatic {
                ButtonSource::Automatic
            } else {
                ButtonSource::Chosen
            };
            (label.clone(), source)
        }
        InUse::Nothing if stored == "none" => (String::new(), ButtonSource::SetToNone),
        InUse::Nothing => (String::new(), ButtonSource::NoProfile),
    }
}

pub fn wifi(on: Option<bool>) -> WifiFix {
    match on {
        Some(true) => WifiFix::On,
        Some(false) => WifiFix::Off,
        None => WifiFix::NotUsed,
    }
}

/// `path` with a leading local app data folder shown as `%LOCALAPPDATA%`: shorter, and keeps the
/// user name out of screenshots.
pub fn shown_path(path: &str, local_app_data: Option<&str>) -> String {
    let rest = local_app_data
        .filter(|base| !base.is_empty())
        .and_then(|base| {
            let head = path.get(..base.len())?;
            if head.eq_ignore_ascii_case(base) {
                path.get(base.len()..)
            } else {
                None
            }
        });
    match rest {
        Some(rest) => format!("%LOCALAPPDATA%{rest}"),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use mujina_application::Msg;

    use super::*;

    #[test]
    fn paths_are_shown_under_local_app_data() {
        let base = Some(r"C:\Users\Someone\AppData\Local");
        assert_eq!(
            shown_path(
                r"C:\Users\Someone\AppData\Local\Packages\M\LocalState",
                base
            ),
            r"%LOCALAPPDATA%\Packages\M\LocalState"
        );
        assert_eq!(shown_path(r"D:\Mujina", base), r"D:\Mujina");
        assert_eq!(shown_path(r"D:\Mujina", None), r"D:\Mujina");
    }

    #[test]
    fn a_finding_is_named_by_its_check_and_keeps_its_words() {
        let finding = Finding {
            id: "conflicting tools",
            title: Msg::new("Conflicting tools"),
            severity: Severity::Warning,
            detail: "anyfse.exe running; it does the same job as Mujina".into(),
            summary: None,
            remedy: None,
        };
        assert_eq!(
            check(&finding),
            Check {
                health: Health::Warning,
                title: "Conflicting tools".into(),
                detail: "anyfse.exe running; it does the same job as Mujina".into(),
            }
        );
        texts::set("de");
        let german = check(&finding);
        assert_eq!(
            (german.title.as_str(), german.detail.as_str()),
            (
                "Konkurrierende Tools",
                "anyfse.exe running; it does the same job as Mujina"
            )
        );
        texts::set("en");
    }

    #[test]
    fn the_button_tile_says_where_the_button_comes_from() {
        let own = InUse::Own {
            keys: "LWIN+D".into(),
        };
        assert_eq!(
            button_tile(true, &own, ""),
            ("LWIN+D".into(), ButtonSource::Own)
        );
        let device = InUse::Device {
            label: "Desktop button".into(),
        };
        assert_eq!(
            button_tile(true, &device, ""),
            ("Desktop button".into(), ButtonSource::Automatic)
        );
        assert_eq!(
            button_tile(true, &device, "onexplayer").1,
            ButtonSource::Chosen
        );
        assert_eq!(
            button_tile(false, &device, ""),
            (String::new(), ButtonSource::Off)
        );
        assert_eq!(
            button_tile(true, &InUse::Nothing, "none").1,
            ButtonSource::SetToNone
        );
        assert_eq!(
            button_tile(true, &InUse::Nothing, "").1,
            ButtonSource::NoProfile
        );
    }

    #[test]
    fn the_verdict_counts_what_needs_attention() {
        let finding = |severity| Finding {
            id: "agent",
            title: Msg::new("Background agent"),
            severity,
            detail: String::new(),
            summary: None,
            remedy: None,
        };
        let findings = [
            finding(Severity::Problem),
            finding(Severity::Warning),
            finding(Severity::Problem),
            finding(Severity::Ok),
        ];
        assert_eq!(
            verdict(&findings),
            Verdict {
                known: true,
                problems: 2,
                warnings: 1,
                total: 4,
            }
        );
    }

    fn facts() -> SystemFacts {
        SystemFacts {
            launcher: "Steam Big Picture".into(),
            ours: Some("Mujina_x!App".into()),
            home_app: None,
            findings: Vec::new(),
        }
    }

    #[test]
    fn the_system_page_knows_whose_home_app_runs() {
        let home = |facts: &SystemFacts| system_info(facts, "").home;
        assert_eq!(home(&facts()), HomeApp::Windows);
        let mut other = facts();
        other.home_app = Some("Xbox!App".into());
        assert_eq!(
            (home(&other), system_info(&other, "").other_home.as_str()),
            (HomeApp::Other, "Xbox!App")
        );
        let mut ours = facts();
        ours.home_app = ours.ours.clone();
        assert_eq!(home(&ours), HomeApp::Mujina);
        // Unpackaged, Mujina has no app ID.
        let mut unpackaged = facts();
        unpackaged.ours = None;
        assert_eq!(home(&unpackaged), HomeApp::Windows);
    }

    #[test]
    fn the_system_page_says_what_was_found_in_the_window_s_language() {
        let finding = Finding {
            id: "developer mode",
            title: Msg::new("Developer Mode"),
            severity: Severity::Problem,
            detail: "off; Windows will not accept Mujina as home app (Settings > System > \
                     Advanced)"
                .into(),
            summary: Some(Msg::new(
                "Off. Windows does not accept Mujina as home app without it (Settings › System \
                 › Advanced).",
            )),
            remedy: None,
        };
        texts::set("de");
        assert_eq!(
            said(&finding),
            "Aus. Ohne ihn nimmt Windows Mujina nicht als Home-App an (Einstellungen › System › \
             Erweitert)."
        );
        assert_eq!(check(&finding).detail, finding.detail);
        let plain = Finding {
            summary: None,
            ..finding.clone()
        };
        assert_eq!(said(&plain), finding.detail);
        texts::set("en");
        assert_eq!(
            said(&finding),
            "Off. Windows does not accept Mujina as home app without it (Settings › System › \
             Advanced)."
        );
    }
}

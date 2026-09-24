//! Which toast a change ends in: applied now, applied the next time Xbox mode is entered, or
//! refused. The texts are in `Words` (controls.slint).

use std::time::Duration;

use mujina_app::tool::Applied;
use mujina_application::register::{RegisterOutcome, UnregisterOutcome};
use mujina_application::settings::schema::{SettingKind, SettingSpec, TextFormat};
use slint::SharedString;

use crate::ui::{Notice, Said, ToastKind};

/// Why a change was not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// What went wrong, in English: the checks `mujinactl config set` runs, or Windows.
    Reason(String),
    /// Not a key combination Mujina can send: the text as typed.
    NotACombination(String),
    /// Another launcher chosen, without its program.
    ProgramRequired,
    /// Another launcher chosen without another value it requires: its title, translated.
    ValueRequired(String),
}

/// The refusal for a required `spec` left empty: a path is the launcher's program, anything else
/// is named by its `title`.
pub fn required(spec: &SettingSpec, title: &str) -> Refusal {
    match spec.kind {
        SettingKind::Text {
            format: TextFormat::Path,
        } => Refusal::ProgramRequired,
        _ => Refusal::ValueRequired(title.to_string()),
    }
}

/// How long a toast stays: a refusal has more to read.
pub fn dwell(kind: ToastKind) -> Duration {
    Duration::from_secs(if kind == ToastKind::Refused { 7 } else { 4 })
}

pub fn notice(kind: ToastKind, said: Said) -> Notice {
    detailed(kind, said, "")
}

fn detailed(kind: ToastKind, said: Said, detail: &str) -> Notice {
    Notice {
        kind,
        said,
        detail: SharedString::from(detail),
    }
}

/// The toast for a stored change. A live setting still waits for Xbox mode when no agent runs
/// to take it over.
pub fn of(outcome: &Result<Applied, Refusal>) -> Notice {
    match outcome {
        Ok(Applied::Now) => notice(ToastKind::Now, Said::Applied),
        Ok(Applied::NextSession) => notice(ToastKind::NextTime, Said::AppliesNextTime),
        Err(refusal) => refused(refusal),
    }
}

pub fn refused(refusal: &Refusal) -> Notice {
    match refusal {
        Refusal::Reason(reason) => detailed(ToastKind::Refused, Said::Refused, reason),
        Refusal::NotACombination(text) => detailed(ToastKind::Refused, Said::NotACombination, text),
        Refusal::ProgramRequired => notice(ToastKind::Refused, Said::ProgramRequired),
        Refusal::ValueRequired(title) => detailed(ToastKind::Refused, Said::ValueRequired, title),
    }
}

/// The toast once the capture overlay closes.
pub fn captured(chord: &str, outcome: &Result<Applied, Refusal>) -> Notice {
    match outcome {
        Ok(Applied::Now) => detailed(ToastKind::Now, Said::CapturedApplied, chord),
        Ok(Applied::NextSession) => {
            detailed(ToastKind::NextTime, Said::CapturedAppliesNextTime, chord)
        }
        Err(refusal) => refused(refusal),
    }
}

/// Making Mujina the home app. Windows reads the home app when Xbox mode is entered.
pub fn registered(outcome: Result<RegisterOutcome, String>) -> Notice {
    match outcome {
        Ok(RegisterOutcome::Registered) => notice(ToastKind::NextTime, Said::NowHome),
        Ok(RegisterOutcome::AlreadyRegistered) => notice(ToastKind::Now, Said::AlreadyHome),
        Err(reason) => refused(&Refusal::Reason(reason)),
    }
}

pub fn unregistered(outcome: Result<UnregisterOutcome, String>) -> Notice {
    match outcome {
        Ok(UnregisterOutcome::Restored(previous)) => {
            detailed(ToastKind::NextTime, Said::HomeRestored, &previous)
        }
        Ok(UnregisterOutcome::Cleared) => notice(ToastKind::NextTime, Said::HomeCleared),
        Ok(UnregisterOutcome::NotRegistered) => notice(ToastKind::Now, Said::HomeNotOurs),
        Err(reason) => refused(&Refusal::Reason(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_has_its_kind() {
        assert_eq!(of(&Ok(Applied::Now)).kind, ToastKind::Now);
        assert_eq!(of(&Ok(Applied::NextSession)).kind, ToastKind::NextTime);
        let reason = "LCTRL+ is not a complete key combination";
        let notice = of(&Err(Refusal::Reason(reason.into())));
        assert_eq!(notice.kind, ToastKind::Refused);
        assert_eq!(notice.said, Said::Refused);
        assert_eq!(notice.detail, reason);
        assert_eq!(dwell(ToastKind::Refused).as_secs(), 7);
        assert_eq!(dwell(ToastKind::NextTime).as_secs(), 4);
    }

    #[test]
    fn a_required_value_left_empty_is_named() {
        use mujina_application::settings::schema::Applies;
        let spec = |kind| SettingSpec {
            key: "value",
            kind,
            title: "Profile",
            help: "",
            applies: Applies::NextSession,
            requires: None,
            required: true,
        };
        let path = spec(SettingKind::Text {
            format: TextFormat::Path,
        });
        assert_eq!(required(&path, "Programm"), Refusal::ProgramRequired);
        let plain = spec(SettingKind::Text {
            format: TextFormat::Plain,
        });
        let refusal = required(&plain, "Profil");
        assert_eq!(refusal, Refusal::ValueRequired("Profil".into()));
        let notice = refused(&refusal);
        assert_eq!(
            (notice.kind, notice.said, notice.detail.as_str()),
            (ToastKind::Refused, Said::ValueRequired, "Profil")
        );
    }

    #[test]
    fn a_capture_names_the_combination_only_once_stored() {
        let stored = captured("LWIN+D", &Ok(Applied::NextSession));
        assert_eq!(
            (stored.kind, stored.said, stored.detail.as_str()),
            (ToastKind::NextTime, Said::CapturedAppliesNextTime, "LWIN+D")
        );
        let refused = captured("LWIN+D", &Err(Refusal::NotACombination("LWIN+".into())));
        assert_eq!(
            (refused.kind, refused.said, refused.detail.as_str()),
            (ToastKind::Refused, Said::NotACombination, "LWIN+")
        );
    }

    #[test]
    fn the_home_app_applies_when_xbox_mode_is_entered() {
        assert_eq!(
            registered(Ok(RegisterOutcome::Registered)).kind,
            ToastKind::NextTime
        );
        assert_eq!(
            registered(Ok(RegisterOutcome::AlreadyRegistered)).kind,
            ToastKind::Now
        );
        let restored = unregistered(Ok(UnregisterOutcome::Restored("Xbox!App".into())));
        assert_eq!(
            (restored.kind, restored.said, restored.detail.as_str()),
            (ToastKind::NextTime, Said::HomeRestored, "Xbox!App")
        );
        assert_eq!(
            unregistered(Ok(UnregisterOutcome::NotRegistered)).kind,
            ToastKind::Now
        );
        assert_eq!(
            unregistered(Err("access denied".into())).said,
            Said::Refused
        );
    }
}

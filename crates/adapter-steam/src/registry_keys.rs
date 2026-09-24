//! Steam's per-user registry state (`HKCU\Software\Valve\Steam`).

use std::path::PathBuf;

use mujina_winutil::registry::{self, Hive};

pub const STEAM: &str = r"Software\Valve\Steam";
const ACTIVE_PROCESS: &str = r"Software\Valve\Steam\ActiveProcess";

/// Full path of `steam.exe`. Steam stores it with forward slashes.
pub fn executable() -> Option<PathBuf> {
    let raw = registry::read_string(Hive::CurrentUser, STEAM, "SteamExe")
        .ok()
        .flatten()?;
    (!raw.is_empty()).then(|| PathBuf::from(raw.replace('/', "\\")))
}

pub fn client_pid() -> Option<u32> {
    registry::read_u32(Hive::CurrentUser, ACTIVE_PROCESS, "pid")
        .ok()
        .flatten()
        .filter(|&pid| pid != 0)
}

/// Account id of the logged-in Steam user; `None` while nobody is logged in.
pub fn active_user() -> Option<u32> {
    registry::read_u32(Hive::CurrentUser, ACTIVE_PROCESS, "ActiveUser")
        .ok()
        .flatten()
        .filter(|&id| id != 0)
}

pub fn running_app_id() -> Option<u32> {
    registry::read_u32(Hive::CurrentUser, STEAM, "RunningAppID")
        .ok()
        .flatten()
        .filter(|&id| id != 0)
}

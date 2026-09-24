//! The marker file that makes Steam open its embedded browser's debugging port.
//!
//! Steam checks for `.cef-enable-remote-debugging` in its install directory at start-up. The
//! network indicator fix talks to Steam's UI through that port, so the marker has to exist
//! before Steam is launched, by Mujina or by anything else. It is made sure of on every home
//! activation; a Steam that was already running without it has to be restarted once.

use std::fs::OpenOptions;
use std::io;
use std::path::{self, Path};

use mujina_winutil::registry::{self, Hive};

const MARKER: &str = ".cef-enable-remote-debugging";

/// Where Mujina lists the files it created outside its own folders: one `REG_SZ` value per file,
/// named by a stable id, holding the absolute path. Setup reads the same key and deletes what is
/// listed when Mujina is uninstalled, without having to know anything about Steam.
const CREATED_KEY: &str = r"Software\Mujina\Created";
/// The marker's entry in that list.
const CREATED_ID: &str = "steam-ui-marker";

/// Creates the marker if it is missing. Returns whether it was created just now.
pub fn ensure(steam_directory: &Path) -> io::Result<bool> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(steam_directory.join(MARKER))
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
    }
}

/// Lists the marker [`ensure`] has just created, so that uninstalling Mujina takes it away again.
/// Only for a marker Mujina created: one that was there before stays the user's.
pub fn record(steam_directory: &Path) -> Result<(), String> {
    let marker = listed_path(steam_directory)?;
    registry::write_string(Hive::CurrentUser, CREATED_KEY, CREATED_ID, &marker)
        .map_err(|error| error.to_string())
}

/// The marker's path as it is listed: absolute and with Windows separators, because whoever
/// deletes it later runs somewhere else.
fn listed_path(steam_directory: &Path) -> Result<String, String> {
    let marker = path::absolute(steam_directory.join(MARKER))
        .map_err(|error| format!("{}: {error}", steam_directory.display()))?;
    marker
        .into_os_string()
        .into_string()
        .map_err(|marker| format!("{} is not valid Unicode", marker.display()))
}

/// Whether the marker is in place.
pub fn exists(steam_directory: &Path) -> bool {
    steam_directory.join(MARKER).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once() {
        let dir = std::env::temp_dir().join(format!("mujina-marker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(ensure(&dir).unwrap());
        assert!(!ensure(&dir).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_listed_path_is_absolute() {
        assert_eq!(
            listed_path(Path::new("C:/Games/Steam")).unwrap(),
            r"C:\Games\Steam\.cef-enable-remote-debugging"
        );
        let relative = listed_path(Path::new("Steam")).unwrap();
        assert!(Path::new(&relative).is_absolute(), "{relative}");
        assert!(
            relative.ends_with(r"\Steam\.cef-enable-remote-debugging"),
            "{relative}"
        );
    }
}

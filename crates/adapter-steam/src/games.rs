//! Telling the running game's processes from everything else on screen. An empty result from
//! the process tree means "cannot tell", never "there is no game".

use std::path::{Path, PathBuf};

use mujina_application::ports::GameWhereabouts;
use mujina_winutil::window::WindowHandle;
use mujina_winutil::{process, window};

use crate::{library, registry_keys};

/// Steam's own processes, which are descendants of the client as well.
const STEAM_PROCESSES: [&str; 6] = [
    "steam.exe",
    "steamwebhelper.exe",
    "steamservice.exe",
    "steamerrorreporter.exe",
    "steamerrorreporter64.exe",
    "gameoverlayui.exe",
];

fn is_steam_process(name: &str) -> bool {
    STEAM_PROCESSES
        .iter()
        .any(|own| own.eq_ignore_ascii_case(name))
}

/// The running game's processes, and how they were found.
#[derive(Debug, PartialEq, Eq)]
enum Found {
    /// The processes whose program lies in the game's folder: all of the game's.
    InFolder(Vec<u32>),
    /// The descendants of the Steam client that are not Steam's own: the game's folder is not
    /// known, or nothing runs from it (a game that runs from elsewhere).
    InTree(Vec<u32>),
    /// The game's folder is known, and nothing runs from it or below Steam: the game has ended,
    /// although Steam still counts it as running.
    Gone,
}

impl Found {
    fn processes(&self) -> &[u32] {
        match self {
            Self::InFolder(ids) | Self::InTree(ids) => ids,
            Self::Gone => &[],
        }
    }
}

/// `installed` is `None` where the game's folder is not known.
fn found(installed: Option<Vec<u32>>, tree: Vec<u32>) -> Found {
    match installed {
        Some(installed) if !installed.is_empty() => Found::InFolder(installed),
        Some(_) if tree.is_empty() => Found::Gone,
        _ => Found::InTree(tree),
    }
}

/// Any program in Steam's folder `steam` outside `steamapps` counts: Steam starts more than
/// [`STEAM_PROCESSES`], e.g. the shader cache's `fossilize-replay64.exe`.
fn is_steams_own(image: &str, steam: Option<&Path>) -> bool {
    let name = image.rsplit(['\\', '/']).next().unwrap_or(image);
    is_steam_process(name)
        || steam.is_some_and(|steam| {
            library::lies_in(image, steam) && !library::lies_in(image, &steam.join("steamapps"))
        })
}

fn tree_processes() -> Vec<u32> {
    let Some(client) = registry_keys::client_pid() else {
        return Vec::new();
    };
    let steam = registry_keys::executable().and_then(|exe| exe.parent().map(Path::to_path_buf));
    process::descendants(client)
        .into_iter()
        .filter(|&pid| {
            process::image_path(pid).is_some_and(|image| !is_steams_own(&image, steam.as_deref()))
        })
        .collect()
}

fn game_folder() -> Option<PathBuf> {
    let app = registry_keys::running_app_id()?;
    let steam = registry_keys::executable()?;
    library::game_folder(steam.parent()?, app)
}

/// Reads Steam's library files and every running process's path: for a button press, not a loop.
fn find() -> Found {
    let installed =
        game_folder().map(|folder| process::running_from(|image| library::lies_in(image, &folder)));
    found(installed, tree_processes())
}

pub fn in_front() -> bool {
    window::foreground_process_id().is_some_and(|pid| find().processes().contains(&pid))
}

pub fn window() -> Option<WindowHandle> {
    window::main_window_of(find().processes())
}

/// One look for all the agent needs to know on a button press.
pub fn whereabouts() -> GameWhereabouts {
    let found = find();
    if matches!(found, Found::Gone) {
        return GameWhereabouts::Gone;
    }
    let processes = found.processes();
    if window::foreground_process_id().is_some_and(|pid| processes.contains(&pid)) {
        GameWhereabouts::InFront
    } else if window::main_window_of(processes).is_some() {
        GameWhereabouts::Behind
    } else {
        GameWhereabouts::NoWindow {
            known: matches!(found, Found::InFolder(_)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_itself_is_not_a_game() {
        assert!(is_steam_process("steamwebhelper.exe"));
        assert!(is_steam_process("GameOverlayUI.exe"));
        assert!(!is_steam_process("Dawnwalker.exe"));
    }

    #[test]
    fn every_program_in_steam_s_folder_but_its_games_is_steam_s_own() {
        let steam = Path::new(r"C:\Program Files (x86)\Steam");
        for own in [
            r"C:\Program Files (x86)\Steam\gameoverlayui64.exe",
            r"C:\Program Files (x86)\Steam\streaming_client.exe",
            r"C:\Program Files (x86)\Steam\bin\fossilize-replay64.exe",
            r"C:\Program Files (x86)\Steam\bin\steam_monitor.exe",
            r"c:\program files (x86)\steam\bin\cef\cef.win64\steamwebhelper.exe",
        ] {
            assert!(is_steams_own(own, Some(steam)), "{own}");
        }
        let game = r"C:\Program Files (x86)\Steam\steamapps\common\Game\game.exe";
        assert!(!is_steams_own(game, Some(steam)));
        assert!(!is_steams_own(
            r"D:\SteamLibrary\steamapps\common\Game\game.exe",
            Some(steam)
        ));
        assert!(!is_steams_own(r"C:\Emulators\emulator.exe", Some(steam)));
        assert!(is_steams_own(r"X:\Steam\steamwebhelper.exe", None));
        assert!(!is_steams_own(r"X:\Steam\fossilize-replay64.exe", None));
    }

    #[test]
    fn the_game_s_folder_decides_where_it_is_known() {
        // 9: a browser the game started.
        assert_eq!(found(Some(vec![7]), vec![7, 9]), Found::InFolder(vec![7]));
        // Started through a launcher that has exited.
        assert_eq!(found(Some(vec![7]), Vec::new()), Found::InFolder(vec![7]));
        assert_eq!(found(Some(Vec::new()), vec![9]), Found::InTree(vec![9]));
        assert_eq!(found(Some(Vec::new()), Vec::new()), Found::Gone);
        assert_eq!(found(None, vec![9]), Found::InTree(vec![9]));
        assert_eq!(found(None, Vec::new()), Found::InTree(Vec::new()));
    }
}

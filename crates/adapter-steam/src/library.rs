//! Where Steam installed a game, from `libraryfolders.vdf` and the game's manifest: Valve's
//! KeyValues text files, no public API.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Text(String),
    Open,
}

/// No token for a closing brace: it only ends a block, which the pairs do not need.
fn tokens(text: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => found.push(Token::Open),
            '"' => {
                let mut value = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => value.push('\n'),
                            Some('t') => value.push('\t'),
                            Some(escaped) => value.push(escaped),
                            None => {}
                        },
                        _ => value.push(c),
                    }
                }
                found.push(Token::Text(value));
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    found
}

/// The values of `key` at any depth, compared without case. A key followed by a block has none.
fn values(text: &str, key: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut tokens = tokens(text).into_iter();
    while let Some(token) = tokens.next() {
        if let Token::Text(name) = token
            && let Some(Token::Text(value)) = tokens.next()
            && name.eq_ignore_ascii_case(key)
        {
            found.push(value);
        }
    }
    found
}

pub fn library_paths(vdf: &str) -> Vec<PathBuf> {
    values(vdf, "path")
        .into_iter()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// The game's folder in `steamapps\common`; `None` unless it is a plain folder name.
pub fn install_dir(manifest: &str) -> Option<String> {
    values(manifest, "installdir")
        .into_iter()
        .next()
        .filter(|name| !name.is_empty() && name != ".." && !name.contains(['\\', '/']))
}

/// `<library>\steamapps\common\<installdir>`; `None` where no library has a manifest of the
/// game (a shortcut to a program Steam did not install, say).
pub fn game_folder(steam_directory: &Path, app_id: u32) -> Option<PathBuf> {
    // Steam keeps the list in `steamapps`, and a copy in `config`.
    let listed = ["steamapps", "config"]
        .iter()
        .find_map(|folder| {
            fs::read_to_string(steam_directory.join(folder).join("libraryfolders.vdf")).ok()
        })
        .map(|vdf| library_paths(&vdf))
        .unwrap_or_default();
    std::iter::once(steam_directory.to_path_buf())
        .chain(listed)
        .find_map(|library| {
            let steamapps = library.join("steamapps");
            let manifest =
                fs::read_to_string(steamapps.join(format!("appmanifest_{app_id}.acf"))).ok()?;
            Some(steamapps.join("common").join(install_dir(&manifest)?))
        })
}

/// Whether the program at `image` lies in `folder` or below it, compared as Windows compares
/// paths: without case, `/` as `\`. `common\Game` does not hold `common\Game 2\game.exe`.
pub fn lies_in(image: &str, folder: &Path) -> bool {
    let plain = |path: &str| path.replace('/', "\\").to_lowercase();
    let mut folder = plain(&folder.to_string_lossy());
    while folder.ends_with('\\') {
        folder.pop();
    }
    if folder.is_empty() {
        return false;
    }
    folder.push('\\');
    plain(image).starts_with(&folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARIES: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"3751260"		"58386703714"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"label"		"path"
		"contentid"		"1096238179812005339"
	}
}
"#;

    const MANIFEST: &str = r#""AppState"
{
	"appid"		"3751260"
	"LauncherPath"		"C:\\Program Files (x86)\\Steam\\steam.exe"
	"name"		"The Blood of Dawnwalker"
	"installdir"		"The Blood of Dawnwalker"
	"InstalledDepots"
	{
		"3751261"
		{
			"manifest"		"123"
		}
	}
}
"#;

    #[test]
    fn the_libraries_are_read_with_their_backslashes() {
        assert_eq!(
            library_paths(LIBRARIES),
            [
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary")
            ],
            "a label named path is a value, not a key"
        );
        assert!(library_paths("").is_empty());
        assert!(library_paths("\"libraryfolders\" {").is_empty());
    }

    #[test]
    fn the_manifest_names_the_game_s_folder() {
        assert_eq!(
            install_dir(MANIFEST).as_deref(),
            Some("The Blood of Dawnwalker")
        );
        assert_eq!(install_dir("\"AppState\" { \"appid\" \"1\" }"), None);
        for odd in ["", "..", r"..\\Windows", "a/b"] {
            let manifest = format!("\"AppState\" {{ \"installdir\" \"{odd}\" }}");
            assert_eq!(install_dir(&manifest), None, "{odd}");
        }
        let commented = "// \"installdir\" \"no\"\n\"AppState\" { \"installdir\" \"yes\" }";
        assert_eq!(install_dir(commented).as_deref(), Some("yes"));
    }

    #[test]
    fn a_program_lies_in_its_folder_whatever_the_case_and_slashes() {
        let folder =
            Path::new(r"C:\Program Files (x86)\Steam\steamapps\common\The Blood of Dawnwalker");
        let game = r"C:\Program Files (x86)\Steam\steamapps\common\The Blood of Dawnwalker\Dawnwalker\Binaries\Win64\Dawnwalker.exe";
        assert!(lies_in(game, folder));
        assert!(lies_in(&game.to_uppercase(), folder));
        assert!(lies_in(&game.replace('\\', "/"), folder));
        assert!(lies_in(
            game,
            Path::new(r"c:\program files (x86)\steam\steamapps\common\the blood of dawnwalker\")
        ));
        assert!(!lies_in(
            r"C:\Program Files (x86)\Steam\steamapps\common\The Blood of Dawnwalker 2\game.exe",
            folder
        ));
        assert!(!lies_in(r"C:\Program Files\Zen Browser\zen.exe", folder));
        assert!(!lies_in(game, Path::new("")));
    }

    #[test]
    fn a_game_is_found_in_any_library_with_its_manifest() {
        let root = std::env::temp_dir().join(format!(
            "mujina-steam-library-{}-{}",
            std::process::id(),
            "a_game_is_found_in_any_library_with_its_manifest"
        ));
        let steam = root.join("Steam");
        let second = root.join("SteamLibrary");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::create_dir_all(second.join("steamapps")).unwrap();
        let escaped = |path: &Path| path.to_string_lossy().replace('\\', "\\\\");
        fs::write(
            steam.join("steamapps").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
                escaped(&steam),
                escaped(&second)
            ),
        )
        .unwrap();
        fs::write(
            second.join("steamapps").join("appmanifest_3751260.acf"),
            MANIFEST,
        )
        .unwrap();

        let found = game_folder(&steam, 3_751_260);
        let missing = game_folder(&steam, 1);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(
            found,
            Some(
                second
                    .join("steamapps")
                    .join("common")
                    .join("The Blood of Dawnwalker")
            )
        );
        assert_eq!(missing, None);
    }
}

//! Between the pages and `config.toml`: choice indices to values, text fields to changes.

use mujina_application::settings::schema::{self, SettingKind, SettingSpec, TextFormat};
use mujina_application::settings::{SettingChange, SettingValue};
use mujina_domain::keys::{KeyChord, VirtualKey};

use crate::feedback::Refusal;

/// The first two entries of the device profile list; built-in profiles follow.
pub const PROFILE_CHOICES: [&str; 2] = ["auto", "none"];
/// The values of the core choice `key` in list order; the first is the default. For
/// `interface.language`: "auto", then the languages `lang/` has, English first.
pub fn choices(key: &str) -> &'static [&'static str] {
    match schema::find(key, &[], &[]).map(|spec| spec.kind) {
        Some(SettingKind::Choice { values, .. }) => values,
        _ => &[],
    }
}

/// The position of `value` in `choices`, or 0 (the default) for anything else.
pub fn index_of(choices: &[&str], value: &str) -> i32 {
    choices
        .iter()
        .position(|choice| *choice == value)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or(0)
}

/// `None`, the default entry, unsets `key`.
pub fn choice(key: &str, value: Option<&str>) -> SettingChange {
    match value {
        Some(value) => SettingChange::set(key, SettingValue::Text(value.to_string())),
        None => SettingChange::unset(key),
    }
}

/// The changes for a text field: a key combination is checked, a list split like a command line,
/// anything else stored as typed. An empty field restores the default.
pub fn text_changes(
    key: &str,
    text: &str,
    spec: Option<&SettingSpec>,
) -> Result<Vec<SettingChange>, Refusal> {
    let text = text.trim();
    if key == "device.button" {
        return button_changes(text);
    }
    if text.is_empty() {
        return Ok(vec![SettingChange::unset(key)]);
    }
    let value = match spec.map(|spec| spec.kind) {
        // Checked here too, so a refusal can say what is wrong in the user's language.
        Some(SettingKind::Text {
            format: TextFormat::Chord,
        }) => {
            let chord =
                KeyChord::parse(text).map_err(|_| Refusal::NotACombination(text.to_string()))?;
            SettingValue::Text(chord.to_string())
        }
        Some(SettingKind::TextList) => SettingValue::TextList(split_arguments(text)),
        _ => SettingValue::Text(text.to_string()),
    };
    Ok(vec![SettingChange::set(key, value)])
}

/// `LWIN+D` becomes `[device.button]` with both keys, set together; empty removes both.
fn button_changes(text: &str) -> Result<Vec<SettingChange>, Refusal> {
    const MODIFIER: &str = "device.button.modifier";
    const KEY: &str = "device.button.key";
    if text.is_empty() {
        return Ok(vec![
            SettingChange::unset(MODIFIER),
            SettingChange::unset(KEY),
        ]);
    }
    let named = text.split_once('+').and_then(|(modifier, key)| {
        Some((
            VirtualKey::from_name(modifier)?,
            VirtualKey::from_name(key)?,
        ))
    });
    match named {
        Some((modifier, key)) => Ok(vec![
            SettingChange::set(MODIFIER, SettingValue::Text(modifier.to_string())),
            SettingChange::set(KEY, SettingValue::Text(key.to_string())),
        ]),
        None => Err(Refusal::NotACombination(text.to_string())),
    }
}

pub fn button_text(modifier: &str, key: &str) -> String {
    if modifier.is_empty() || key.is_empty() {
        String::new()
    } else {
        format!("{modifier}+{key}")
    }
}

/// The detailed log is a level in the file, not a flag.
pub fn flag_change(key: &str, on: bool) -> SettingChange {
    match key {
        "logging.debug" if on => {
            SettingChange::set("logging.level", SettingValue::Text("debug".into()))
        }
        "logging.debug" => SettingChange::unset("logging.level"),
        _ => SettingChange::set(key, SettingValue::Bool(on)),
    }
}

pub fn as_text(value: Option<SettingValue>) -> String {
    match value {
        Some(SettingValue::Text(text)) => text,
        Some(SettingValue::TextList(items)) => join_arguments(&items),
        Some(SettingValue::Integer(number)) => number.to_string(),
        Some(SettingValue::Bool(flag)) => flag.to_string(),
        None => String::new(),
    }
}

/// Splits a launcher's arguments by the Microsoft C runtime's rules, which Rust programs follow
/// too. Arguments only: the program name's rules do not apply.
/// `CommandLineToArgvW` differs only on `""` inside quotes, which [`join_arguments`] never writes.
pub fn split_arguments(text: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    // `""` is an empty argument, so being in one differs from having text in it.
    let mut in_argument = false;
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' if !in_quotes => {
                if in_argument {
                    arguments.push(std::mem::take(&mut current));
                    in_argument = false;
                }
            }
            '\\' => {
                let mut backslashes = 1;
                while chars.next_if_eq(&'\\').is_some() {
                    backslashes += 1;
                }
                if chars.peek() == Some(&'"') {
                    // Halved in front of a quote; an odd one left over makes the quote literal.
                    current.extend(std::iter::repeat_n('\\', backslashes / 2));
                    if backslashes % 2 == 1 {
                        chars.next();
                        current.push('"');
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', backslashes));
                }
                in_argument = true;
            }
            // Inside quotes, two quotes stand for one.
            '"' if in_quotes => {
                if chars.next_if_eq(&'"').is_some() {
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            }
            '"' => {
                in_quotes = true;
                in_argument = true;
            }
            other => {
                current.push(other);
                in_argument = true;
            }
        }
    }
    if in_argument {
        arguments.push(current);
    }
    arguments
}

/// Joins arguments into one line that [`split_arguments`] reads back unchanged.
pub fn join_arguments(arguments: &[String]) -> String {
    let mut line = String::new();
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            line.push(' ');
        }
        let quoted = argument.is_empty() || argument.contains([' ', '\t']);
        if quoted {
            line.push('"');
        }
        let mut backslashes = 0;
        for c in argument.chars() {
            if c == '\\' {
                backslashes += 1;
            } else {
                if c == '"' {
                    line.extend(std::iter::repeat_n('\\', backslashes + 1));
                }
                backslashes = 0;
            }
            line.push(c);
        }
        if quoted {
            // Backslashes at the end would escape the closing quote.
            line.extend(std::iter::repeat_n('\\', backslashes));
            line.push('"');
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `super::text_changes` with the spec looked up as the page does.
    fn text_changes(key: &str, text: &str) -> Result<Vec<SettingChange>, Refusal> {
        let launchers = mujina_app::tool::launchers();
        let spec = schema::find(key, launchers.all, mujina_app::tool::devices().all);
        super::text_changes(key, text, spec)
    }

    #[test]
    fn choices_map_to_values_and_unknown_ones_to_the_default() {
        let on_exit = choices("launcher.on_exit");
        assert_eq!(on_exit, ["relaunch_on_crash", "relaunch_always", "nothing"]);
        assert_eq!(choices("interface.language"), ["auto", "en", "de"]);
        assert_eq!(index_of(on_exit, "nothing"), 2);
        assert_eq!(index_of(on_exit, ""), 0);
        assert_eq!(index_of(on_exit, "explode"), 0);
        assert_eq!(
            choice("launcher.kind", None),
            SettingChange::unset("launcher.kind")
        );
    }

    #[test]
    fn a_button_is_modifier_and_key_set_together() {
        let changes = text_changes("device.button", "lwin + d").unwrap();
        assert_eq!(
            changes,
            [
                SettingChange::set("device.button.modifier", SettingValue::Text("LWIN".into())),
                SettingChange::set("device.button.key", SettingValue::Text("D".into())),
            ]
        );
        assert_eq!(text_changes("device.button", "").unwrap().len(), 2);
        assert!(text_changes("device.button", "LWIN").is_err());
        assert!(text_changes("device.button", "+D").is_err());
        assert_eq!(button_text("LWIN", "D"), "LWIN+D");
        assert_eq!(button_text("", "D"), "");
    }

    #[test]
    fn shortcuts_are_checked_and_stored_as_mujina_writes_them() {
        assert_eq!(
            text_changes("launcher.menu", "lctrl+1").unwrap(),
            [SettingChange::set(
                "launcher.menu",
                SettingValue::Text("LCTRL+1".into())
            )]
        );
        assert!(text_changes("launcher.menu", "LCTRL+").is_err());
        assert!(text_changes("launcher.overlay", "LSHIFT+NOPE").is_err());
        assert_eq!(
            text_changes("launcher.menu", "").unwrap(),
            [SettingChange::unset("launcher.menu")]
        );
    }

    #[test]
    fn text_fields_set_or_restore_the_default() {
        assert_eq!(
            text_changes("launcher.menu", "  ").unwrap(),
            [SettingChange::unset("launcher.menu")]
        );
        assert_eq!(
            text_changes("launcher.generic.arguments", "--a  -b").unwrap(),
            [SettingChange::set(
                "launcher.generic.arguments",
                SettingValue::TextList(vec!["--a".into(), "-b".into()])
            )]
        );
        assert_eq!(
            as_text(Some(SettingValue::TextList(vec![
                "--a".into(),
                "-b".into()
            ]))),
            "--a -b"
        );
    }

    fn owned(arguments: &[&str]) -> Vec<String> {
        arguments.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn arguments_split_like_a_windows_command_line() {
        // The examples of Microsoft's "Parsing C command-line arguments".
        let examples: [(&str, &[&str]); 6] = [
            (r#""a b c" d e"#, &["a b c", "d", "e"]),
            (r#""ab\"c" "\\" d"#, &[r#"ab"c"#, r"\", "d"]),
            (r#"a\\\b d"e f"g h"#, &[r"a\\\b", "de fg", "h"]),
            (r#"a\\\"b c d"#, &[r#"a\"b"#, "c", "d"]),
            (r#"a\\\\"b c" d e"#, &[r"a\\b c", "d", "e"]),
            (r#"a"b"" c d"#, &[r#"ab" c d"#]),
        ];
        for (line, arguments) in examples {
            assert_eq!(split_arguments(line), owned(arguments), "{line}");
        }
        assert_eq!(
            split_arguments(r#"--config "C:\Users\me\My Games\frontend.json""#),
            owned(&["--config", r"C:\Users\me\My Games\frontend.json"])
        );
        assert_eq!(split_arguments(" \t--a\t -b "), owned(&["--a", "-b"]));
        assert_eq!(split_arguments(r#""" x """#), owned(&["", "x", ""]));
        assert_eq!(split_arguments(r#""open"#), owned(&["open"]));
        assert!(split_arguments(" \t ").is_empty());
    }

    #[test]
    fn arguments_are_shown_so_they_read_back_the_same() {
        assert_eq!(
            join_arguments(&owned(&["--config", r"C:\Users\me\My Games\frontend.json"])),
            r#"--config "C:\Users\me\My Games\frontend.json""#
        );
        assert_eq!(
            join_arguments(&owned(&[r"C:\My Games\"])),
            r#""C:\My Games\\""#
        );
        assert_eq!(join_arguments(&owned(&[r"C:\Games\"])), r"C:\Games\");
        assert_eq!(join_arguments(&owned(&[r#"say "hi""#])), r#""say \"hi\"""#);
        assert_eq!(join_arguments(&owned(&["", "-b"])), r#""" -b"#);
        let lists: [&[&str]; 8] = [
            &["--config", r"C:\Users\me\My Games\frontend.json"],
            &[r"C:\My Games\", "next"],
            &[r#"a"b"#, r#"a\"b"#, r#"a\\"b"#],
            &["", "", "x"],
            &["tab\there", r"\\server\share\"],
            &[r#"""#, r"\", r#"\""#],
            &["ü ß", "日本"],
            &[],
        ];
        for list in lists {
            let list = owned(list);
            assert_eq!(split_arguments(&join_arguments(&list)), list, "{list:?}");
        }
        for line in [r#"a"b"" c d"#, r#"a\\\\"b c" d e"#, r#""ab\"c" "\\" d"#] {
            let arguments = split_arguments(line);
            assert_eq!(split_arguments(&join_arguments(&arguments)), arguments);
        }
    }

    #[test]
    fn a_stored_argument_list_is_one_line() {
        let stored = SettingValue::TextList(owned(&["--profile", "My Profile"]));
        assert_eq!(as_text(Some(stored)), r#"--profile "My Profile""#);
        assert_eq!(
            text_changes("launcher.generic.arguments", r#"--profile "My Profile""#).unwrap(),
            [SettingChange::set(
                "launcher.generic.arguments",
                SettingValue::TextList(owned(&["--profile", "My Profile"]))
            )]
        );
    }

    #[test]
    fn the_detailed_log_is_a_level() {
        assert_eq!(
            flag_change("logging.debug", true),
            SettingChange::set("logging.level", SettingValue::Text("debug".into()))
        );
        assert_eq!(
            flag_change("logging.debug", false),
            SettingChange::unset("logging.level")
        );
        assert_eq!(
            flag_change("features.launch_screen", true),
            SettingChange::set("features.launch_screen", SettingValue::Bool(true))
        );
    }
}

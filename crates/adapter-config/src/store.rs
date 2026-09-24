//! Writing `config.toml`: edits keep the user's comments and layout, and what the reader would
//! ignore is refused. One writer at a time; the file is replaced whole.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use mujina_application::device::{Devices, SystemIdentity};
use mujina_application::launcher::Launchers;
use mujina_application::ports::{PortError, PortResult};
use mujina_application::settings::{SettingChange, SettingValue, SettingsStore};
use toml_edit::{Array, DocumentMut, Item, Table, Value};

use crate::{ConfigFile, file, template};

/// Mujina Settings stores on its window's thread, which a stuck writer must not hold up for good.
const LOCK_WAIT: Duration = Duration::from_secs(2);

impl SettingsStore for ConfigFile {
    fn apply(&self, changes: &[SettingChange]) -> PortResult<()> {
        // Held from read to rename, so neither Mujina Settings nor mujinactl loses a change.
        let _lock = lock(&self.path, LOCK_WAIT)?;
        let before = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => template(&self.launchers),
            Err(error) => {
                return Err(PortError::Failed(format!(
                    "{}: {error}",
                    self.path.display()
                )));
            }
        };
        let after = edit(&before, changes).map_err(PortError::Failed)?;
        check(&before, &after, changes, self).map_err(PortError::Failed)?;
        write(&self.path, &after)
    }

    fn stored(&self, key: &str) -> Option<SettingValue> {
        self.snapshot().stored(key)
    }
}

/// `config.toml` as stored at one moment, read and parsed once however many keys are asked.
#[derive(Debug)]
pub struct StoredSnapshot {
    /// `None` without a file or with one that is no TOML: every key then has its default.
    document: Option<DocumentMut>,
}

impl StoredSnapshot {
    pub(crate) fn read(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parsed(&text),
            Err(_) => Self { document: None },
        }
    }

    fn parsed(text: &str) -> Self {
        Self {
            document: text.parse().ok(),
        }
    }

    /// What the file said for `key` itself; `None` where it relies on the default. The answer
    /// [`SettingsStore::stored`] gives, as of the moment this was read.
    pub fn stored(&self, key: &str) -> Option<SettingValue> {
        stored_in(self.document.as_ref()?, key)
    }
}

fn stored_in(document: &DocumentMut, key: &str) -> Option<SettingValue> {
    let (tables, leaf) = split(key).ok()?;
    from_value(find_table(document, &tables)?.get(leaf)?.as_value()?)
}

/// How `mujinactl config set` reads a value: TOML where it is one (`true`, `50`, `"text"`,
/// `["a", "b"]`), plain text otherwise (`LCTRL+1`, `steam`, a path).
pub fn parse_value(text: &str) -> Result<SettingValue, String> {
    let Ok(value) = text.trim().parse::<Value>() else {
        return Ok(SettingValue::Text(text.to_string()));
    };
    from_value(&value).ok_or_else(|| match &value {
        Value::Array(_) => "a list may only hold text".to_string(),
        _ => format!("\"{text}\" is not a kind of value Mujina's settings use"),
    })
}

fn from_value(value: &Value) -> Option<SettingValue> {
    match value {
        Value::Boolean(flag) => Some(SettingValue::Bool(*flag.value())),
        Value::Integer(number) => Some(SettingValue::Integer(*number.value())),
        Value::String(string) => Some(SettingValue::Text(string.value().clone())),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .map(SettingValue::TextList),
        _ => None,
    }
}

fn edit(text: &str, changes: &[SettingChange]) -> Result<String, String> {
    let mut text = text.to_string();
    for change in changes {
        let (tables, leaf) = split(&change.key)?;
        text = match &change.value {
            Some(value) => set_in(&text, &tables, leaf, to_value(value))?,
            None => unset_in(&text, &tables, leaf)?,
        };
    }
    button_rest(text, changes)
}

/// Unsetting the last of `[device.button]`'s keys unsets `injected_only` too: alone it describes
/// no button, and the reader would note it.
fn button_rest(text: String, changes: &[SettingChange]) -> Result<String, String> {
    const BUTTON: [&str; 2] = ["device", "button"];
    let unsets_a_key = changes.iter().any(|change| {
        change.value.is_none()
            && matches!(
                change.key.as_str(),
                "device.button.modifier" | "device.button.key"
            )
    });
    // Set together with unsetting the keys, it is refused as ignored instead.
    let sets_it = changes
        .iter()
        .any(|change| change.value.is_some() && change.key == "device.button.injected_only");
    if !unsets_a_key || sets_it {
        return Ok(text);
    }
    let document = parse(&text)?;
    if present(&document, &BUTTON, "modifier") || present(&document, &BUTTON, "key") {
        return Ok(text);
    }
    unset_in(&text, &BUTTON, "injected_only")
}

fn set_in(text: &str, tables: &[&str], leaf: &str, value: Value) -> Result<String, String> {
    let mut text = text.to_string();
    let mut document = parse(&text)?;
    if let Some(error) = not_a_section(&document, tables) {
        return Err(error);
    }
    if !present(&document, tables, leaf) {
        if let Some(uncommented) = uncomment(&text, tables, leaf, value.to_string().trim())
            .filter(|candidate| candidate.parse::<DocumentMut>().is_ok())
        {
            text = uncommented;
        } else if find_table(&document, tables).is_none() {
            // New sections go to the end, after the template's explanations.
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("\n[");
            text.push_str(&tables.join("."));
            text.push_str("]\n");
        }
        document = parse(&text)?;
    }
    let table = find_table_mut(&mut document, tables)
        .ok_or_else(|| format!("[{}] is not a section", tables.join(".")))?;
    match table.get_mut(leaf) {
        // In place: the key keeps the comment lines stored in front of it.
        Some(item) => {
            let mut value = value;
            if let Some(old) = item.as_value() {
                *value.decor_mut() = old.decor().clone();
            }
            *item = Item::Value(value);
        }
        None => {
            table.insert(leaf, Item::Value(value));
        }
    }
    Ok(document.to_string())
}

/// Why a key under `tables` cannot be set when the file holds one of those tables as something
/// else, such as `features = 5`; only a hand edit can fix that.
fn not_a_section(document: &DocumentMut, tables: &[&str]) -> Option<String> {
    let mut table = document.as_table();
    for (depth, name) in tables.iter().enumerate() {
        let item = table.get(name)?;
        if let Some(inner) = item.as_table() {
            table = inner;
            continue;
        }
        let path = tables[..=depth].join(".");
        let written = match item {
            Item::Value(value) => {
                let mut value = value.clone();
                value.decor_mut().clear();
                format!("{name} = {value}")
            }
            _ => format!("[[{path}]]"),
        };
        return Some(if written.contains('\n') {
            format!("[{path}] is no section in config.toml; fix it by hand first")
        } else {
            format!("[{path}] is `{written}` in config.toml; fix it by hand first")
        });
    }
    None
}

/// Comments the entry out rather than deleting it, which would take the comment lines in front of
/// it along; a commented line can be set again later.
fn unset_in(text: &str, tables: &[&str], leaf: &str) -> Result<String, String> {
    let document = parse(text)?;
    if !present(&document, tables, leaf) {
        return Ok(text.to_string());
    }
    let commented = recomment(text, tables, leaf).filter(|candidate| {
        candidate
            .parse::<DocumentMut>()
            .is_ok_and(|result| !present(&result, tables, leaf))
    });
    if let Some(commented) = commented {
        return Ok(commented);
    }
    // A value over several lines, written by hand: remove it the plain way.
    let mut document = document;
    if let Some(table) = find_table_mut(&mut document, tables) {
        table.remove(leaf);
    }
    Ok(document.to_string())
}

/// Refuses a result the configuration reader would reject or remark on.
fn check(
    before: &str,
    after: &str,
    changes: &[SettingChange],
    config: &ConfigFile,
) -> Result<(), String> {
    let (system, launchers, devices) = (&config.system, &config.launchers, &config.devices);
    let parsed = file::parse(after, launchers, devices)
        .map_err(|error| format!("not saved: {}", error.message()))?;
    // A value set where the reader skips it changes nothing, so it is refused even if the file
    // already held one there.
    let still_ignored: Vec<String> = parsed
        .skipped
        .iter()
        .filter(|skipped| {
            changes
                .iter()
                .any(|change| change.value.is_some() && skipped.covers(&change.key))
        })
        .map(|skipped| skipped.note.clone())
        .collect();
    let mut notes = Vec::new();
    parsed.resolve(system, devices, launchers, &mut notes);
    let known = notes_of(before, system, launchers, devices);
    let refused: Vec<String> = notes
        .into_iter()
        .filter(|note| still_ignored.contains(note) || !known.contains(note))
        .collect();
    if refused.is_empty() {
        Ok(())
    } else {
        Err(format!("not saved: {}", refused.join("; ")))
    }
}

fn notes_of(
    text: &str,
    system: &SystemIdentity,
    launchers: &Launchers,
    devices: &Devices,
) -> Vec<String> {
    let mut notes = Vec::new();
    if let Ok(parsed) = file::parse(text, launchers, devices) {
        parsed.resolve(system, devices, launchers, &mut notes);
    }
    notes
}

/// Beside the file, not on it: every write replaces the file.
fn lock_path(path: &Path) -> PathBuf {
    path.with_extension("toml.lock")
}

/// With write access: std leaves open whether a handle without it can be locked.
pub(crate) fn lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(path))
}

/// Takes the lock on `path`, waiting up to `wait` while another writer holds it.
fn lock(path: &Path, wait: Duration) -> PortResult<File> {
    let failed =
        |error: io::Error| PortError::Failed(format!("{}: {error}", lock_path(path).display()));
    let file = lock_file(path).map_err(failed)?;
    let deadline = Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::Error(error)) => return Err(failed(error)),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(PortError::Failed(
                    "config.toml is busy (another Mujina Settings or mujinactl is writing it); \
                     try again"
                        .to_string(),
                ));
            }
        }
    }
}

/// Replaces the file in one step, so a reader never sees half of it. The caller holds the lock.
fn write(path: &Path, text: &str) -> PortResult<()> {
    // With the process id, a temporary name no other writer uses at the same time.
    static WRITES: AtomicU32 = AtomicU32::new(0);
    remove_stale(path);
    let temporary = path.with_extension(format!(
        "toml.{}-{}.new",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let written = write_through(&temporary, text).and_then(|()| std::fs::rename(&temporary, path));
    if written.is_err() {
        // Should this fail too, the first error is the one to report.
        let _ = std::fs::remove_file(&temporary);
    }
    written.map_err(|error| PortError::Failed(format!("{}: {error}", path.display())))
}

/// Removes temporary files of writes that never reached the rename (a power cut). With the lock
/// held no write is under way, so every one found is left over. Best effort.
fn remove_stale(path: &Path) {
    let name = path.file_name().and_then(|file| file.to_str());
    let (Some(folder), Some(name)) = (path.parent(), name) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let stale = entry.file_name().to_str().is_some_and(|other| {
            other
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix('.'))
                .and_then(|rest| rest.strip_suffix(".new"))
                .is_some_and(|middle| !middle.is_empty())
                // The fixed temporary name older releases used.
                || other.strip_prefix(name) == Some(".new")
        });
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Waits until `text` is on the disk, so a power cut after the rename cannot leave an empty
/// configuration. The file is closed on return, before it is renamed.
fn write_through(path: &Path, text: &str) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// `features.launch_screen` -> (`["features"]`, `launch_screen`).
fn split(key: &str) -> Result<(Vec<&str>, &str), String> {
    let wrong = || format!("\"{key}\" is not a setting; settings look like section.name");
    let parts: Vec<&str> = key.split('.').collect();
    let plain = |part: &&str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    if parts.len() < 2 || !parts.iter().all(plain) {
        return Err(wrong());
    }
    let (leaf, tables) = parts.split_last().ok_or_else(wrong)?;
    Ok((tables.to_vec(), leaf))
}

fn parse(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>()
        .map_err(|error| format!("config.toml is not valid, fix it by hand first: {error}"))
}

fn to_value(value: &SettingValue) -> Value {
    match value {
        SettingValue::Bool(flag) => Value::from(*flag),
        SettingValue::Integer(number) => Value::from(*number),
        SettingValue::Text(text) => Value::from(text.as_str()),
        SettingValue::TextList(items) => {
            Value::Array(items.iter().map(String::as_str).collect::<Array>())
        }
    }
}

fn present(document: &DocumentMut, tables: &[&str], leaf: &str) -> bool {
    find_table(document, tables).is_some_and(|table| table.contains_key(leaf))
}

fn find_table<'a>(document: &'a DocumentMut, tables: &[&str]) -> Option<&'a Table> {
    let mut table = document.as_table();
    for name in tables {
        table = table.get(name)?.as_table()?;
    }
    Some(table)
}

fn find_table_mut<'a>(document: &'a mut DocumentMut, tables: &[&str]) -> Option<&'a mut Table> {
    let mut table = document.as_table_mut();
    for name in tables {
        table = table.get_mut(name)?.as_table_mut()?;
    }
    Some(table)
}

/// Turns the template's `# key = default  # explanation` line in the right section into
/// `key = value  # explanation`, so the file keeps reading like the template.
fn uncomment(text: &str, tables: &[&str], leaf: &str, value: &str) -> Option<String> {
    replace_line(text, tables, &|line| {
        let after = line
            .strip_prefix('#')
            .map(str::trim_start)
            .and_then(|rest| assignment(rest, leaf))?;
        let mut replacement = format!("{leaf} = {value}");
        if let Some(at) = after.find(" #") {
            replacement.push_str("  ");
            replacement.push_str(after[at..].trim_start());
        }
        Some(replacement)
    })
}

/// The reverse of [`uncomment`]: `key = value  # explanation` becomes a comment again.
fn recomment(text: &str, tables: &[&str], leaf: &str) -> Option<String> {
    replace_line(text, tables, &|line| {
        assignment(line, leaf)?;
        Some(format!("# {line}"))
    })
}

/// What follows `leaf =` when `line` assigns `leaf`.
fn assignment<'a>(line: &'a str, leaf: &str) -> Option<&'a str> {
    line.strip_prefix(leaf)
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix('='))
}

/// Replaces the first line in section `tables` that `replace` answers; it sees the line trimmed.
/// A commented-out header, as the template's other launchers have, ends the section above it.
fn replace_line(
    text: &str,
    tables: &[&str],
    replace: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let header = tables.join(".");
    let mut section = String::new();
    let mut done = false;
    let mut result = String::with_capacity(text.len() + 16);
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let commented = trimmed
            .strip_prefix('#')
            .map(str::trim_start)
            .and_then(|rest| rest.strip_prefix('['))
            .and_then(|rest| rest.strip_suffix(']'));
        if let Some(name) = trimmed
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim().to_string();
        } else if let Some(name) = commented {
            // Never the name of a section: `#` is no part of one.
            section = format!("#{}", name.trim());
        } else if !done
            && section == header
            && let Some(replacement) = replace(trimmed)
        {
            let indentation = &line[..line.len() - line.trim_start().len()];
            let ending = &line[line.trim_end().len()..];
            result.push_str(indentation);
            result.push_str(&replacement);
            result.push_str(ending);
            done = true;
            continue;
        }
        result.push_str(line);
    }
    done.then_some(result)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use std::sync::LazyLock;

    use super::*;
    use crate::fakes::{DEVICES, LAUNCHERS};
    use mujina_application::launcher::OptionTable;
    use mujina_application::settings::{Settings, SettingsSource};

    static TEMPLATE: LazyLock<String> = LazyLock::new(|| template(&LAUNCHERS));

    fn onexplayer() -> SystemIdentity {
        SystemIdentity {
            manufacturer: "ONE-NETBOOK".to_string(),
            product: "ONEXPLAYER 3".to_string(),
        }
    }

    fn temp_config(name: &str) -> (ConfigFile, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("mujina-store-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        (
            ConfigFile::for_system(&dir, onexplayer(), LAUNCHERS, DEVICES),
            dir,
        )
    }

    fn line_of<'a>(text: &'a str, start: &str) -> Option<&'a str> {
        text.lines().find(|line| line.starts_with(start))
    }

    fn settings_of(text: &str) -> Settings {
        file::parse(text, &LAUNCHERS, &DEVICES).unwrap().resolve(
            &onexplayer(),
            &DEVICES,
            &LAUNCHERS,
            &mut Vec::new(),
        )
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "config.toml" && name != "config.toml.lock")
            .collect()
    }

    #[test]
    fn a_template_line_is_uncommented_and_keeps_its_explanation() {
        let text = edit(
            &TEMPLATE,
            &[SettingChange::set(
                "features.launch_screen",
                SettingValue::Bool(true),
            )],
        )
        .unwrap();
        let line = line_of(&text, "launch_screen").unwrap();
        assert!(line.starts_with("launch_screen = true"), "{line}");
        assert!(line.contains("# black screen"), "{line}");
        assert!(!text.contains("# launch_screen"), "{text}");
        assert!(text.contains("# button_remap = true"));
        assert_eq!(text.lines().count(), TEMPLATE.lines().count());
    }

    #[test]
    fn setting_again_replaces_the_value_and_keeps_the_comment() {
        let once = edit(
            &TEMPLATE,
            &[SettingChange::set(
                "timing.key_hold_ms",
                SettingValue::Integer(80),
            )],
        )
        .unwrap();
        let twice = edit(
            &once,
            &[SettingChange::set(
                "timing.key_hold_ms",
                SettingValue::Integer(90),
            )],
        )
        .unwrap();
        assert_eq!(line_of(&twice, "key_hold_ms"), Some("key_hold_ms = 90"));
        assert_eq!(twice.matches("key_hold_ms").count(), 1, "{twice}");

        let commented = "[launcher]\nmenu = \"F1\"   # mine\n";
        let changed = edit(
            commented,
            &[SettingChange::set(
                "launcher.menu",
                SettingValue::Text("F2".to_string()),
            )],
        )
        .unwrap();
        assert_eq!(changed, "[launcher]\nmenu = \"F2\"   # mine\n");
    }

    #[test]
    fn unset_removes_the_entry() {
        let set = edit(
            &TEMPLATE,
            &[SettingChange::set(
                "features.game_start_screen",
                SettingValue::Bool(false),
            )],
        )
        .unwrap();
        let unset = edit(&set, &[SettingChange::unset("features.game_start_screen")]).unwrap();
        assert!(line_of(&unset, "game_start_screen").is_none(), "{unset}");
        let line = line_of(&unset, "# game_start_screen").unwrap();
        assert!(line.contains("# keep the launcher on its"), "{line}");
        assert!(unset.contains("# button_remap = true"), "{unset}");
        assert_eq!(unset.lines().count(), TEMPLATE.lines().count());
        assert!(settings_of(&unset).game_start_screen);

        let again = edit(
            &unset,
            &[SettingChange::set(
                "features.game_start_screen",
                SettingValue::Bool(false),
            )],
        )
        .unwrap();
        assert_eq!(again.matches("game_start_screen").count(), 1, "{again}");

        // Unsetting what is not there is no error.
        edit(&TEMPLATE, &[SettingChange::unset("device.button.key")]).unwrap();
    }

    #[test]
    fn a_new_section_goes_to_the_end() {
        let text = edit(
            &TEMPLATE,
            &[
                SettingChange::set("device.button.modifier", SettingValue::Text("LCTRL".into())),
                SettingChange::set("device.button.key", SettingValue::Text("F24".into())),
            ],
        )
        .unwrap();
        let header = text.rfind("\n[device.button]").unwrap();
        assert!(header > text.find("[logging]").unwrap(), "{text}");
        let device = settings_of(&text).device;
        assert_eq!(device.id.as_deref(), Some("custom"));
        assert_eq!(
            device.options.get("key"),
            Some(&SettingValue::Text("F24".into()))
        );
    }

    #[test]
    fn a_section_written_as_a_value_is_named() {
        let set = |text: &str, key: &str| {
            edit(text, &[SettingChange::set(key, SettingValue::Bool(true))]).unwrap_err()
        };
        assert_eq!(
            set("features = 5  # mine\n", "features.launch_screen"),
            "[features] is `features = 5` in config.toml; fix it by hand first"
        );
        assert_eq!(
            set("[launcher]\ngeneric = \"x\"\n", "launcher.generic.process"),
            "[launcher.generic] is `generic = \"x\"` in config.toml; fix it by hand first"
        );
        assert_eq!(
            set("[[device]]\nprofile = \"none\"\n", "device.button.key"),
            "[device] is `[[device]]` in config.toml; fix it by hand first"
        );
        assert_eq!(
            set("features = [\n  1,\n]\n", "features.launch_screen"),
            "[features] is no section in config.toml; fix it by hand first"
        );
    }

    #[test]
    fn keys_that_are_no_setting_are_refused() {
        assert!(edit(&TEMPLATE, &[SettingChange::unset("features")]).is_err());
        assert!(edit(&TEMPLATE, &[SettingChange::unset("Features.X")]).is_err());
        assert!(edit(&TEMPLATE, &[SettingChange::unset("a..b")]).is_err());
    }

    #[test]
    fn what_the_reader_would_reject_or_ignore_is_not_saved() {
        let (config, dir) = temp_config("refuse");
        config.ensure_template();
        let before = std::fs::read_to_string(dir.join("config.toml")).unwrap();

        for change in [
            SettingChange::set("features.surprise", SettingValue::Bool(true)),
            SettingChange::set("features.launch_screen", SettingValue::Text("yes".into())),
            SettingChange::set("launcher.menu", SettingValue::Text("LCTRL+NOPE".into())),
            SettingChange::set("launcher.kind", SettingValue::Text("generic".into())),
        ] {
            let error = config.apply(std::slice::from_ref(&change)).unwrap_err();
            assert!(
                error.to_string().contains("not saved"),
                "{change:?}: {error}"
            );
        }
        let after = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert_eq!(before, after);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn changes_are_applied_together() {
        let (config, dir) = temp_config("together");
        config
            .apply(&[
                SettingChange::set("launcher.kind", SettingValue::Text("generic".into())),
                SettingChange::set(
                    "launcher.generic.executable",
                    SettingValue::Text(r"C:\Frontend\frontend.exe".into()),
                ),
                SettingChange::set(
                    "launcher.generic.arguments",
                    SettingValue::TextList(vec!["--fullscreen".into()]),
                ),
            ])
            .unwrap();
        let loaded = config.load();
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(loaded.settings.launcher.id, "generic");
        assert_eq!(
            loaded.settings.launcher.options.get("arguments"),
            Some(&SettingValue::TextList(vec!["--fullscreen".into()]))
        );
        // A missing file started from the template, comments included.
        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(text.starts_with("# Mujina configuration"), "{text}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_default_launchers_options_are_set_where_the_template_has_them() {
        let text = edit(
            &TEMPLATE,
            &[SettingChange::set(
                "launcher.steam.ui_link",
                SettingValue::Bool(false),
            )],
        )
        .unwrap();
        let line = line_of(&text, "ui_link").unwrap();
        assert!(line.starts_with("ui_link = false"), "{line}");
        assert!(line.contains("# the debugging port"), "{line}");
        assert_eq!(text.lines().count(), TEMPLATE.lines().count());
        assert_eq!(
            settings_of(&text).launcher.options,
            OptionTable::from([("ui_link".to_string(), SettingValue::Bool(false))])
        );
    }

    #[test]
    fn a_commented_section_is_added_not_uncommented_into_the_one_above() {
        // `# [launcher.generic]` follows `[launcher.steam]`; its lines are not Steam's.
        let text = edit(
            &TEMPLATE,
            &[SettingChange::set(
                "launcher.generic.process",
                SettingValue::Text("x.exe".into()),
            )],
        )
        .unwrap();
        assert!(text.contains("# process = \"frontend.exe\""), "{text}");
        let header = text.rfind("\n[launcher.generic]").unwrap();
        assert!(header > text.find("[interface]").unwrap(), "{text}");
        assert_eq!(
            StoredSnapshot::parsed(&text).stored("launcher.generic.process"),
            Some(SettingValue::Text("x.exe".into()))
        );
    }

    #[test]
    fn a_launchers_options_are_checked_like_the_rest() {
        let (config, dir) = temp_config("launcher");
        config.ensure_template();
        for change in [
            SettingChange::set("launcher.steam.ui_link", SettingValue::Text("no".into())),
            SettingChange::set("launcher.steam.surprise", SettingValue::Bool(true)),
            SettingChange::set("launcher.playnite.path", SettingValue::Text("x".into())),
            SettingChange::set("launcher.kind", SettingValue::Text("playnite".into())),
        ] {
            let error = config.apply(std::slice::from_ref(&change)).unwrap_err();
            assert!(
                error.to_string().contains("not saved"),
                "{change:?}: {error}"
            );
        }
        config
            .apply(&[SettingChange::set(
                "launcher.steam.wifi_indicator",
                SettingValue::Bool(false),
            )])
            .unwrap();
        assert_eq!(
            config.stored("launcher.steam.wifi_indicator"),
            Some(SettingValue::Bool(false))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn another_launcher_can_be_left_for_steam_and_back() {
        let (config, dir) = temp_config("switch");
        let generic =
            "[launcher]\nkind = \"generic\"\n[launcher.generic]\nexecutable = 'C:\\F\\f.exe'\n";
        let to = |id: &str| SettingChange::set("launcher.kind", SettingValue::Text(id.into()));
        // Unset (as Mujina Settings does) and set (as `mujinactl config set` does); the check
        // (ADR-0009) refuses neither.
        for back in [SettingChange::unset("launcher.kind"), to("steam")] {
            std::fs::write(config.path(), generic).unwrap();
            config.apply(std::slice::from_ref(&back)).unwrap();
            let loaded = config.load();
            assert_eq!(loaded.settings.launcher.id, "steam", "{back:?}");
            assert!(loaded.notes.is_empty(), "{back:?}: {:?}", loaded.notes);
            config.apply(&[to("generic")]).unwrap();
            assert_eq!(config.load().settings.launcher.id, "generic");
            config.apply(&[to("steam")]).unwrap();
            assert_eq!(config.load().settings.launcher.id, "steam");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stored_values_are_read_back_and_defaults_are_absent() {
        let text = edit(
            &TEMPLATE,
            &[
                SettingChange::set("timing.key_hold_ms", SettingValue::Integer(70)),
                SettingChange::set("launcher.menu", SettingValue::Text("F1".into())),
            ],
        )
        .unwrap();
        let stored = StoredSnapshot::parsed(&text);
        assert_eq!(
            stored.stored("timing.key_hold_ms"),
            Some(SettingValue::Integer(70))
        );
        assert_eq!(
            stored.stored("launcher.menu"),
            Some(SettingValue::Text("F1".into()))
        );
        assert_eq!(stored.stored("features.button_remap"), None);
        assert_eq!(stored.stored("device.button.key"), None);
        assert_eq!(
            StoredSnapshot::parsed("not toml [").stored("timing.key_hold_ms"),
            None
        );
    }

    #[test]
    fn a_snapshot_answers_as_stored_does_with_one_read() {
        let (config, dir) = temp_config("snapshot");
        assert_eq!(config.snapshot().stored("timing.key_hold_ms"), None);
        config
            .apply(&[
                SettingChange::set("timing.key_hold_ms", SettingValue::Integer(70)),
                SettingChange::set("launcher.menu", SettingValue::Text("F1".into())),
                SettingChange::set("features.launch_screen", SettingValue::Bool(true)),
                SettingChange::set(
                    "launcher.generic.arguments",
                    SettingValue::TextList(vec!["--a".into(), "b c".into()]),
                ),
            ])
            .unwrap();
        let snapshot = config.snapshot();
        for key in [
            "timing.key_hold_ms",
            "launcher.menu",
            "features.launch_screen",
            "launcher.generic.arguments",
            "features.button_remap",
            "device.button.key",
            "features",
        ] {
            assert_eq!(snapshot.stored(key), config.stored(key), "{key}");
        }
        assert_eq!(
            snapshot.stored("launcher.generic.arguments"),
            Some(SettingValue::TextList(vec!["--a".into(), "b c".into()]))
        );

        config
            .apply(&[SettingChange::set(
                "timing.key_hold_ms",
                SettingValue::Integer(80),
            )])
            .unwrap();
        assert_eq!(
            snapshot.stored("timing.key_hold_ms"),
            Some(SettingValue::Integer(70))
        );
        assert_eq!(
            config.snapshot().stored("timing.key_hold_ms"),
            Some(SettingValue::Integer(80))
        );

        std::fs::write(config.path(), "not toml [").unwrap();
        assert_eq!(config.snapshot().stored("timing.key_hold_ms"), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn command_line_values_are_read_as_toml_where_they_are_toml() {
        assert_eq!(parse_value("true"), Ok(SettingValue::Bool(true)));
        assert_eq!(parse_value("50"), Ok(SettingValue::Integer(50)));
        assert_eq!(
            parse_value("LCTRL+1"),
            Ok(SettingValue::Text("LCTRL+1".into()))
        );
        assert_eq!(parse_value("steam"), Ok(SettingValue::Text("steam".into())));
        assert_eq!(
            parse_value(r"C:\Games\frontend.exe"),
            Ok(SettingValue::Text(r"C:\Games\frontend.exe".into()))
        );
        assert_eq!(parse_value("\"42\""), Ok(SettingValue::Text("42".into())));
        assert_eq!(
            parse_value(r#"["--a", "b"]"#),
            Ok(SettingValue::TextList(vec!["--a".into(), "b".into()]))
        );
        assert!(parse_value("1.5").is_err());
        assert!(parse_value("[1, 2]").is_err());
    }

    #[test]
    fn a_skipped_key_blocks_no_other_change_and_is_not_set_again() {
        let (config, dir) = temp_config("skipped");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[featurs]\nlaunch_screen = true\n[features]\nsurprise = true\n",
        )
        .unwrap();

        config
            .apply(&[SettingChange::set(
                "launcher.on_exit",
                SettingValue::Text("nothing".into()),
            )])
            .unwrap();
        for change in [
            SettingChange::set("features.surprise", SettingValue::Bool(false)),
            // The same value again: still nothing it would change.
            SettingChange::set("features.surprise", SettingValue::Bool(true)),
            SettingChange::set("featurs.launch_screen", SettingValue::Bool(false)),
        ] {
            let error = config.apply(std::slice::from_ref(&change)).unwrap_err();
            assert!(
                error.to_string().contains("not saved"),
                "{change:?}: {error}"
            );
        }
        config
            .apply(&[SettingChange::unset("features.surprise")])
            .unwrap();
        let loaded = config.load();
        assert_eq!(
            loaded.notes,
            ["[featurs] ignored: unknown section"],
            "{}",
            std::fs::read_to_string(&path).unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn injected_only_is_not_stored_alone_and_goes_with_the_keys() {
        let (config, dir) = temp_config("injected");
        config.ensure_template();
        let alone = SettingChange::set("device.button.injected_only", SettingValue::Bool(false));
        let error = config.apply(std::slice::from_ref(&alone)).unwrap_err();
        assert!(error.to_string().contains("not saved"), "{error}");

        // As Mujina Settings stores a captured button, and clears it again.
        config
            .apply(&[
                SettingChange::set("device.button.modifier", SettingValue::Text("LCTRL".into())),
                SettingChange::set("device.button.key", SettingValue::Text("F24".into())),
                alone.clone(),
            ])
            .unwrap();
        assert_eq!(
            config.load().settings.device.options.get("injected_only"),
            Some(&SettingValue::Bool(false))
        );
        config
            .apply(&[
                SettingChange::unset("device.button.modifier"),
                SettingChange::unset("device.button.key"),
            ])
            .unwrap();
        let loaded = config.load();
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(config.stored("device.button.injected_only"), None);
        assert_eq!(loaded.settings.device.id.as_deref(), Some("onexplayer"));

        std::fs::write(config.path(), "[device.button]\ninjected_only = false\n").unwrap();
        config
            .apply(&[SettingChange::set(
                "timing.key_hold_ms",
                SettingValue::Integer(70),
            )])
            .unwrap();
        let error = config.apply(&[alone]).unwrap_err();
        assert!(error.to_string().contains("not saved"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_write_leaves_nothing_but_the_file_and_its_lock() {
        let (config, dir) = temp_config("clean");
        config
            .apply(&[SettingChange::set(
                "timing.key_hold_ms",
                SettingValue::Integer(70),
            )])
            .unwrap();
        assert_eq!(config.load().settings.timing.key_hold_ms, 70);
        assert_eq!(leftovers(&dir), Vec::<String>::new());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_rename_leaves_no_temporary_file() {
        let (_, dir) = temp_config("rename");
        // A folder in the file's place cannot be replaced by a file, on any system.
        let path = dir.join("config.toml");
        std::fs::create_dir_all(path.join("inside")).unwrap();
        assert!(write(&path, "[timing]\n").is_err());
        assert_eq!(leftovers(&dir), Vec::<String>::new());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_write_removes_what_an_interrupted_one_left() {
        let (config, dir) = temp_config("stale");
        for name in [
            "config.toml.4242-0.new",
            "config.toml.4242-7.new",
            "config.toml.new",
            "notes.new",
        ] {
            std::fs::write(dir.join(name), "[timing]\n").unwrap();
        }
        config
            .apply(&[SettingChange::set(
                "timing.key_hold_ms",
                SettingValue::Integer(70),
            )])
            .unwrap();
        assert_eq!(leftovers(&dir), ["notes.new"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_change_gives_up_when_another_takes_too_long() {
        let (config, dir) = temp_config("busy");
        let held = lock_file(config.path()).unwrap();
        held.lock().unwrap();
        let started = Instant::now();
        let error = lock(config.path(), Duration::from_millis(200)).unwrap_err();
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert!(error.to_string().contains("busy"), "{error}");
        drop(held);
        assert!(lock(config.path(), Duration::ZERO).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_change_waits_while_another_is_stored() {
        let (config, dir) = temp_config("lock");
        let held = lock_file(config.path()).unwrap();
        held.lock().unwrap();

        // While the lock is held, the template is left to the writer holding it.
        config.ensure_template();
        assert!(!config.path().exists());

        let (sender, receiver) = mpsc::channel();
        let folder = dir.clone();
        let writer = std::thread::spawn(move || {
            let config = ConfigFile::for_system(&folder, onexplayer(), LAUNCHERS, DEVICES);
            let result = config.apply(&[SettingChange::set(
                "launcher.menu",
                SettingValue::Text("F1".into()),
            )]);
            sender
                .send(result.map_err(|error| error.to_string()))
                .unwrap();
        });
        assert!(
            receiver.recv_timeout(Duration::from_millis(300)).is_err(),
            "stored without the lock"
        );
        drop(held);
        let stored = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        writer.join().unwrap();
        assert_eq!(stored, Ok(()));
        assert_eq!(
            config.stored("launcher.menu"),
            Some(SettingValue::Text("F1".into()))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

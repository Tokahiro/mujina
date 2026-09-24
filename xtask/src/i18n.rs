//! `cargo xtask i18n-check`: each `.po` under a crate's `lang/` must translate every text the
//! crate shows: `@tr("…")` in its `.slint` files and the shared `ui/`, `Msg::new("…")` in Rust.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{TaskResult, workspace};

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    context: Option<String>,
    /// The English, gettext's msgid.
    id: String,
    /// The English plural, gettext's msgid_plural.
    plural: Option<String>,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.id)?;
        if let Some(plural) = &self.plural {
            write!(f, " | {plural:?}")?;
        }
        match &self.context {
            Some(context) => write!(f, " (in {context})"),
            None => write!(f, " (from Rust)"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    key: Key,
    /// `msgstr`, or `msgstr[0]`, `msgstr[1]`, … for a text with a count.
    translations: Vec<String>,
    fuzzy: bool,
}

impl Entry {
    /// Whether Slint and the Localizer use it: every form translated, not marked fuzzy.
    fn translated(&self) -> bool {
        let forms = if self.key.plural.is_some() { 2 } else { 1 };
        !self.fuzzy
            && self.translations.len() >= forms
            && self.translations.iter().all(|text| !text.is_empty())
    }
}

pub fn check() -> TaskResult {
    let root = workspace::root();
    let extractor = extractor(&root)?;
    let shared = slint_files(&root.join("ui"))?;
    let mut failures = Vec::new();
    let mut unused = Vec::new();
    let mut checked = Vec::new();
    for dir in crate_dirs(&root.join("crates"))? {
        let name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut texts = BTreeSet::new();
        let own = slint_files(&dir)?;
        if !own.is_empty() {
            let files: Vec<PathBuf> = own.into_iter().chain(shared.iter().cloned()).collect();
            let pot = root
                .join("target")
                .join("i18n-check")
                .join(format!("{name}.pot"));
            for entry in extract(&extractor, &files, &pot)? {
                texts.insert(entry.key);
            }
        }
        for id in rust_texts(&dir.join("src"), &root, &mut failures)? {
            texts.insert(Key {
                id,
                ..Key::default()
            });
        }
        let catalogs = catalogs(&dir)?;
        if texts.is_empty() && catalogs.is_empty() {
            continue;
        }
        if catalogs.is_empty() {
            failures.push(format!(
                "crates/{name} has texts to translate but no catalog under lang/"
            ));
            continue;
        }
        for (path, entries) in &catalogs {
            let shown = shown(path, &root);
            let (missing, extra) = compare(&texts, entries);
            failures.extend(
                missing
                    .into_iter()
                    .map(|problem| format!("{shown}: {problem}")),
            );
            unused.extend(extra.into_iter().map(|key| format!("{shown}: {key}")));
        }
        checked.push(format!(
            "{name} ({} texts, {} {})",
            texts.len(),
            catalogs.len(),
            if catalogs.len() == 1 {
                "language"
            } else {
                "languages"
            }
        ));
    }
    // Harmless, but a text nobody shows any more is a translation nobody keeps up.
    for entry in &unused {
        println!("unused: {entry}");
    }
    if failures.is_empty() {
        println!("translations ok: {}", checked.join(", "));
        Ok(())
    } else {
        Err(failures.join("\n       "))
    }
}

/// slint-tr-extractor of Cargo.lock's Slint version, so it reads `.slint` as the compiler does.
/// Installed under `MUJINA_TOOLS` (CI caches it there) or `target/tools`, never the user's cargo.
fn extractor(root: &Path) -> Result<PathBuf, String> {
    let lock = read(&root.join("Cargo.lock"))?;
    let version = locked_version(&lock, "slint").ok_or("Cargo.lock has no slint")?;
    let tools = std::env::var_os("MUJINA_TOOLS")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| root.join("target").join("tools"), PathBuf::from)
        .join(format!("slint-tr-extractor-{version}"));
    let program = tools.join("bin").join(format!(
        "slint-tr-extractor{}",
        std::env::consts::EXE_SUFFIX
    ));
    if program.is_file() {
        return Ok(program);
    }
    println!(
        "installing slint-tr-extractor {version} into {}",
        tools.display()
    );
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args(["install", "--locked", "--quiet", "--root"])
        .arg(&tools)
        .args(["--version", &version, "slint-tr-extractor"])
        .status()
        .map_err(|error| format!("cargo install slint-tr-extractor: {error}"))?;
    if !status.success() {
        return Err(format!("cargo install slint-tr-extractor {version} failed"));
    }
    Ok(program)
}

fn locked_version(lock: &str, package: &str) -> Option<String> {
    let name = format!("name = \"{package}\"");
    let mut lines = lock.lines();
    lines.find(|line| line.trim() == name)?;
    lines
        .next()?
        .trim()
        .strip_prefix("version = \"")?
        .strip_suffix('"')
        .map(str::to_string)
}

fn extract(extractor: &Path, files: &[PathBuf], pot: &Path) -> Result<Vec<Entry>, String> {
    if let Some(dir) = pot.parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    // Without the old file, which it would otherwise add to.
    let _ = std::fs::remove_file(pot);
    let output = Command::new(extractor)
        .arg("-o")
        .arg(pot)
        .args(files)
        .output()
        .map_err(|error| format!("{}: {error}", extractor.display()))?;
    if !output.status.success() {
        return Err(format!(
            "slint-tr-extractor: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse(&read(pot)?).map_err(|error| format!("{}: {error}", pot.display()))
}

/// Every `Msg::new("…")` under `src`; one without a string literal goes to `failures`.
fn rust_texts(src: &Path, root: &Path, failures: &mut Vec<String>) -> Result<Vec<String>, String> {
    let mut texts = Vec::new();
    for file in files(src, "rs")? {
        match msgs(&read(&file)?) {
            Ok(found) => texts.extend(found),
            Err(problem) => failures.push(format!("{}: {problem}", shown(&file, root))),
        }
    }
    Ok(texts)
}

/// The texts of the `Msg::new("…")` in `source`, without comments and inline test modules. Other
/// `#[cfg(test)]` items are read: an extra text costs a translation, a missed one goes unchecked.
fn msgs(source: &str) -> Result<Vec<String>, String> {
    let lines: Vec<&str> = source.lines().collect();
    // Comments and test modules stay as empty lines, so the line numbers are the file's.
    let mut code = String::with_capacity(source.len());
    let mut module_end: Option<String> = None;
    for (index, line) in lines.iter().enumerate() {
        let kept = if let Some(end) = &module_end {
            if line.trim_end() == end {
                module_end = None;
            }
            ""
        } else if line.trim() == "#[cfg(test)]" {
            module_end = test_module_end(lines.get(index + 1..).unwrap_or_default());
            ""
        } else if line.trim_start().starts_with("//") {
            ""
        } else {
            line
        };
        code.push_str(kept);
        code.push('\n');
    }
    let mut texts = Vec::new();
    let mut rest = code.as_str();
    while let Some(at) = rest.find("Msg::new(") {
        let after = rest[at + "Msg::new(".len()..].trim_start();
        let (text, tail) = rust_string(after).ok_or_else(|| {
            let line = code[..code.len() - rest.len() + at].matches('\n').count() + 1;
            format!("line {line}: Msg::new takes a string literal, so that i18n-check sees it")
        })?;
        texts.push(text);
        rest = tail;
    }
    Ok(texts)
}

/// The closing line of the inline module after a `#[cfg(test)]`; `None` for any other item.
/// Relies on rustfmt, which CI enforces, closing a block at the indentation that opened it.
fn test_module_end(after: &[&str]) -> Option<String> {
    let item = after.iter().map(|line| line.trim_end()).find(|line| {
        let code = line.trim_start();
        !code.is_empty() && !code.starts_with("#[") && !code.starts_with("//")
    })?;
    let code = item.trim_start();
    let indent = &item[..item.len() - code.len()];
    let unqualified = ["pub(crate) ", "pub(super) ", "pub "]
        .iter()
        .find_map(|visibility| code.strip_prefix(visibility))
        .unwrap_or(code);
    (unqualified.starts_with("mod ") && code.ends_with('{')).then(|| format!("{indent}}}"))
}

/// The Rust string literal at the start of `code`, unescaped, and what follows it.
fn rust_string(code: &str) -> Option<(String, &str)> {
    let mut chars = code.strip_prefix('"')?.char_indices();
    let mut text = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((text, &code[index + 2..])),
            '\\' => match chars.next()?.1 {
                'n' => text.push('\n'),
                't' => text.push('\t'),
                'r' => text.push('\r'),
                '0' => text.push('\0'),
                'u' => {
                    let hex: String = chars
                        .by_ref()
                        .map(|(_, c)| c)
                        .skip_while(|c| *c == '{')
                        .take_while(|c| *c != '}')
                        .collect();
                    text.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                // A line continued: the line break and the indentation after it are left out.
                '\n' => {
                    while chars.clone().next().is_some_and(|(_, c)| c.is_whitespace()) {
                        chars.next();
                    }
                }
                other => text.push(other),
            },
            other => text.push(other),
        }
    }
    None
}

/// Each `.po` under `dir/lang`, in Slint's `<language>/LC_MESSAGES/` or as `<language>.po`.
fn catalogs(dir: &Path) -> Result<Vec<(PathBuf, Vec<Entry>)>, String> {
    let lang = dir.join("lang");
    if !lang.is_dir() {
        return Ok(Vec::new());
    }
    let mut catalogs = Vec::new();
    for file in files(&lang, "po")? {
        let entries =
            parse(&read(&file)?).map_err(|error| format!("{}: {error}", file.display()))?;
        catalogs.push((file, entries));
    }
    Ok(catalogs)
}

/// What `entries`, one catalog, lacks of `texts`, and which of its entries no text uses.
fn compare(texts: &BTreeSet<Key>, entries: &[Entry]) -> (Vec<String>, Vec<Key>) {
    let mut missing = Vec::new();
    for text in texts {
        let found = entries
            .iter()
            .find(|entry| entry.key.context == text.context && entry.key.id == text.id);
        match found {
            None => missing.push(format!("no translation for {text}")),
            Some(entry) if entry.key.plural.is_some() != text.plural.is_some() => {
                let has = if entry.key.plural.is_some() {
                    "with"
                } else {
                    "without"
                };
                missing.push(format!("{text} is listed {has} a plural"));
            }
            // Slint looks an entry up by its msgid and context alone, so a plural worded
            // differently would keep the old wording's translation.
            Some(entry) if entry.key.plural != text.plural => {
                let plural = entry.key.plural.as_deref().unwrap_or_default();
                missing.push(format!("{text} has the plural {plural:?} in the catalog"));
            }
            Some(entry) if entry.fuzzy => missing.push(format!("{text} is marked fuzzy")),
            Some(entry) if !entry.translated() => {
                missing.push(format!("{text} is not translated"));
            }
            Some(entry) => {
                if let Some((translation, got, want)) = lost_placeholder(entry) {
                    missing.push(format!(
                        "{text} is translated as {translation:?}, with the placeholders {got:?} \
                         for {want:?}"
                    ));
                }
            }
        }
    }
    let unused = entries
        .iter()
        .filter(|entry| {
            !texts
                .iter()
                .any(|text| text.context == entry.key.context && text.id == entry.key.id)
        })
        .map(|entry| entry.key.clone())
        .collect();
    (missing, unused)
}

/// For scaffold's tests: i18n-check's findings for `po` against the `Msg::new` texts of `sources`.
#[cfg(test)]
pub(crate) fn rust_catalog_problems(sources: &[String], po: &str) -> Result<Vec<String>, String> {
    let mut texts = BTreeSet::new();
    for source in sources {
        for id in msgs(source)? {
            texts.insert(Key {
                id,
                ..Key::default()
            });
        }
    }
    let (missing, unused) = compare(&texts, &parse(po)?);
    Ok(missing
        .into_iter()
        .chain(unused.iter().map(|key| format!("unused: {key}")))
        .collect())
}

/// The first translation whose placeholders differ from its English, with both lists.
/// `msgstr[0]`, exactly one in German, is held to the msgid; other forms to the plural.
fn lost_placeholder(entry: &Entry) -> Option<(&str, Vec<&str>, Vec<&str>)> {
    entry
        .translations
        .iter()
        .enumerate()
        .find_map(|(form, translation)| {
            let english = match &entry.key.plural {
                Some(plural) if form > 0 => plural,
                _ => &entry.key.id,
            };
            let (got, want) = (placeholders(translation), placeholders(english));
            (got != want).then_some((translation.as_str(), got, want))
        })
}

/// The placeholders of `text`, `{}`, `{n}` or `{0}`, sorted: a translation may order them
/// differently, but keeps each. `{{` is a brace, not a placeholder.
fn placeholders(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('{') {
        let after = &rest[at + 1..];
        if let Some(tail) = after.strip_prefix('{') {
            rest = tail;
            continue;
        }
        let Some(end) = after.find('}') else {
            break;
        };
        found.push(&rest[at..=at + 1 + end]);
        rest = &after[end + 1..];
    }
    found.sort_unstable();
    found
}

/// A catalog's entries as the Slint build reads them (rspolib), without the obsolete `#~` ones.
fn parse(text: &str) -> Result<Vec<Entry>, String> {
    // rspolib refuses most broken catalogs but panics on some, such as a msgstr without a msgid;
    // the Slint build stops there as well, so either way the catalog is unusable.
    let file = std::panic::catch_unwind(|| rspolib::pofile(text))
        .map_err(|_| "a catalog rspolib cannot read".to_string())?
        .map_err(|error| error.to_string())?;
    let entries = file
        .entries
        .into_iter()
        .filter(|entry| !entry.obsolete)
        .map(|entry| Entry {
            fuzzy: entry.fuzzy(),
            translations: if entry.msgid_plural.is_some() {
                entry.msgstr_plural
            } else {
                entry.msgstr.into_iter().collect()
            },
            key: Key {
                context: entry.msgctxt,
                id: entry.msgid,
                plural: entry.msgid_plural,
            },
        })
        .collect();
    Ok(entries)
}

fn crate_dirs(crates: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(crates)
        .map_err(|error| format!("{}: {error}", crates.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    Ok(dirs)
}

fn slint_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    files(dir, "slint")
}

/// The files under `dir` with `extension`, sorted; none if `dir` does not exist.
fn files(dir: &Path, extension: &str) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    if !dir.is_dir() {
        return Ok(found);
    }
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|found| found == extension) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// `path` relative to the workspace root, with forward slashes, as the messages show it.
fn shown(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(context: Option<&str>, id: &str, plural: Option<&str>) -> Key {
        Key {
            context: context.map(str::to_string),
            id: id.to_string(),
            plural: plural.map(str::to_string),
        }
    }

    const CATALOG: &str = r#"# German texts.
msgid ""
msgstr ""
"Language: de\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgctxt "StatusPage"
msgid "Check again"
msgstr "Erneut prüfen"

#, fuzzy
msgctxt "StatusPage"
msgid "{n} check passed"
msgid_plural "{n} checks passed"
msgstr[0] "{n} Prüfung bestanden"
msgstr[1] "{n} Prüfungen bestanden"
msgid "Another launcher"
msgstr ""
"Ein anderer "
"Launcher"

msgid "Not \"done\""
msgstr ""

#~ msgid "Gone"
#~ msgstr "Weg"
"#;

    #[test]
    fn a_catalog_reads_as_its_entries() {
        let entries = parse(CATALOG).unwrap();
        assert_eq!(
            entries,
            [
                Entry {
                    key: key(Some("StatusPage"), "Check again", None),
                    translations: vec!["Erneut prüfen".into()],
                    fuzzy: false,
                },
                Entry {
                    key: key(
                        Some("StatusPage"),
                        "{n} check passed",
                        Some("{n} checks passed")
                    ),
                    translations: vec![
                        "{n} Prüfung bestanden".into(),
                        "{n} Prüfungen bestanden".into()
                    ],
                    fuzzy: true,
                },
                Entry {
                    key: key(None, "Another launcher", None),
                    translations: vec!["Ein anderer Launcher".into()],
                    fuzzy: false,
                },
                Entry {
                    key: key(None, "Not \"done\"", None),
                    translations: vec![String::new()],
                    fuzzy: false,
                },
            ]
        );
        assert!(parse("msgid \"open").is_err());
        assert!(parse("msgstr[1] \"one form too many\"").is_err());
    }

    #[test]
    fn every_text_needs_its_translation() {
        let entries = parse(CATALOG).unwrap();
        let texts: BTreeSet<Key> = [
            key(Some("StatusPage"), "Check again", None),
            key(
                Some("StatusPage"),
                "{n} check passed",
                Some("{n} checks passed"),
            ),
            key(None, "Another launcher", None),
            key(None, "Not \"done\"", None),
            key(Some("HelpPage"), "Check again", None),
            key(None, "Check again", Some("Check again {n} times")),
        ]
        .into();
        let (missing, unused) = compare(&texts, &entries);
        assert_eq!(
            missing,
            [
                "no translation for \"Check again\" | \"Check again {n} times\" (from Rust)",
                "\"Not \\\"done\\\"\" (from Rust) is not translated",
                "no translation for \"Check again\" (in HelpPage)",
                "\"{n} check passed\" | \"{n} checks passed\" (in StatusPage) is marked fuzzy",
            ]
        );
        assert_eq!(unused, Vec::<Key>::new());

        let counted: BTreeSet<Key> = [key(Some("StatusPage"), "Check again", Some("{n}"))].into();
        let (missing, unused) = compare(&counted, &entries);
        assert_eq!(
            missing,
            ["\"Check again\" | \"{n}\" (in StatusPage) is listed without a plural"]
        );
        assert_eq!(unused.len(), 3);

        let reworded = parse(
            r#"
msgctxt "StatusPage"
msgid "{n} warning"
msgid_plural "{n} warnings"
msgstr[0] "{n} Warnung"
msgstr[1] "{n} Warnungen"

msgctxt "CaptureOverlay"
msgid "Captured {}"
msgstr "Erfasst"

msgctxt "StatusPage"
msgid "One thing needs fixing."
msgid_plural "{n} things need fixing."
msgstr[0] "Eine Sache muss behoben werden."
msgstr[1] "Einiges muss behoben werden."

msgid "Automatic ({})"
msgstr "Automatisch ({}) {{so}}"
"#,
        )
        .unwrap();
        let texts: BTreeSet<Key> = [
            key(
                Some("StatusPage"),
                "{n} warning",
                Some("{n} warnings to see"),
            ),
            key(Some("CaptureOverlay"), "Captured {}", None),
            key(
                Some("StatusPage"),
                "One thing needs fixing.",
                Some("{n} things need fixing."),
            ),
            key(None, "Automatic ({})", None),
        ]
        .into();
        let (missing, _) = compare(&texts, &reworded);
        assert_eq!(
            missing,
            [
                "\"Captured {}\" (in CaptureOverlay) is translated as \"Erfasst\", with the \
                 placeholders [] for [\"{}\"]",
                "\"One thing needs fixing.\" | \"{n} things need fixing.\" (in StatusPage) is \
                 translated as \"Einiges muss behoben werden.\", with the placeholders [] for \
                 [\"{n}\"]",
                "\"{n} warning\" | \"{n} warnings to see\" (in StatusPage) has the plural \
                 \"{n} warnings\" in the catalog",
            ]
        );
    }

    #[test]
    fn placeholders_are_what_a_text_fills_in() {
        assert_eq!(
            placeholders("{1} of {0}, {n} and {} {{not}}"),
            ["{0}", "{1}", "{n}", "{}"]
        );
        assert_eq!(placeholders("{ open"), Vec::<&str>::new());
    }

    #[test]
    fn rust_texts_are_the_literals_of_msg_new() {
        let source = r#"
use mujina_i18n::Msg;
// Msg::new("in a comment")
const AUTOMATIC: Msg = Msg::new("Automatic");
const WITH: Msg = Msg::new( "Automatic ({})" );
const LONG: Msg = Msg::new("Not \"quite\" \
                            done\u{2026}");
#[cfg(test)]
mod tests {
    const TEST: Msg = Msg::new("only in a test");
}
"#;
        assert_eq!(
            msgs(source).unwrap(),
            ["Automatic", "Automatic ({})", "Not \"quite\" done…"]
        );
        let computed = "const NAME: Msg = Msg::new(name);";
        assert!(msgs(computed).unwrap_err().contains("line 1"));

        // Only inline test modules are left out, not `mod fakes;` or other `#[cfg(test)]` items.
        let mixed = r#"
#[cfg(test)]
mod fakes;
const TITLE: Msg = Msg::new("Wi-Fi icon fix");
#[cfg(test)]
use std::cell::Cell;
const HELP: Msg = Msg::new("Help");
impl Tile {
    #[cfg(test)]
    pub(crate) fn standing() -> Self {
        Self
    }
    const NAME: Msg = Msg::new("Name");
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod helpers {
    fn nested() {
        let _ = Msg::new("only in a test");
    }
}
const LAST: Msg = Msg::new("After a test module");
"#;
        assert_eq!(
            msgs(mixed).unwrap(),
            ["Wi-Fi icon fix", "Help", "Name", "After a test module"]
        );
    }

    #[test]
    fn the_extractor_matches_the_slint_in_cargo_lock() {
        let lock = "[[package]]\nname = \"slint\"\nversion = \"1.18.1\"\n\n[[package]]\nname = \
                    \"slint-build\"\nversion = \"1.18.1\"\n";
        assert_eq!(locked_version(lock, "slint").as_deref(), Some("1.18.1"));
        assert_eq!(locked_version(lock, "serde"), None);
    }
}

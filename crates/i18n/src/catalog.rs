//! A gettext `.po` catalog's texts without a context (msgctxt): Slint's texts carry one.

use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    texts: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub problem: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.problem)
    }
}

impl std::error::Error for ParseError {}

#[derive(Default)]
struct Entry {
    context: Option<String>,
    id: String,
    plural: Option<String>,
    translations: Vec<String>,
    fuzzy: bool,
}

/// Which string a line that starts with a quote continues.
#[derive(Clone, Copy)]
enum Field {
    Context,
    Id,
    Plural,
    Translation(usize),
}

impl Catalog {
    /// Reads `po`; obsolete entries (`#~`) are skipped like any other comment.
    pub fn parse(po: &str) -> Result<Self, ParseError> {
        let mut catalog = Self::default();
        let mut entry = Entry::default();
        let mut field = None;
        let mut fuzzy = false;
        // The entry has its translation, so anything but another translation starts the next.
        let mut complete = false;
        for (index, line) in po.lines().enumerate() {
            let failed = |problem| ParseError {
                line: index + 1,
                problem,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let continues = line.starts_with('"');
            if complete && !continues && !line.starts_with("msgstr") {
                catalog.keep(std::mem::take(&mut entry));
                complete = false;
                field = None;
            }
            if let Some(flags) = line.strip_prefix("#,") {
                fuzzy |= flags.split(',').any(|flag| flag.trim() == "fuzzy");
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            if continues {
                let text = unquote(line).ok_or_else(|| failed("a string that does not end"))?;
                let target = match field.ok_or_else(|| failed("a string outside an entry"))? {
                    Field::Context => entry.context.get_or_insert_with(String::new),
                    Field::Id => &mut entry.id,
                    Field::Plural => entry.plural.get_or_insert_with(String::new),
                    Field::Translation(at) => entry
                        .translations
                        .get_mut(at)
                        .ok_or_else(|| failed("a translation out of order"))?,
                };
                target.push_str(&text);
                continue;
            }
            let (keyword, quoted) = line
                .split_once(char::is_whitespace)
                .ok_or_else(|| failed("a keyword without a string"))?;
            let text =
                unquote(quoted.trim()).ok_or_else(|| failed("a string that does not end"))?;
            if !complete {
                entry.fuzzy |= std::mem::take(&mut fuzzy);
            }
            field = Some(match keyword {
                "msgctxt" => {
                    entry.context = Some(text);
                    Field::Context
                }
                "msgid" => {
                    entry.id = text;
                    Field::Id
                }
                "msgid_plural" => {
                    entry.plural = Some(text);
                    Field::Plural
                }
                "msgstr" => {
                    entry.translations = vec![text];
                    complete = true;
                    Field::Translation(0)
                }
                other => {
                    let at = other
                        .strip_prefix("msgstr[")
                        .and_then(|at| at.strip_suffix(']'))
                        .and_then(|at| at.parse::<usize>().ok())
                        .ok_or_else(|| failed("an unknown keyword"))?;
                    if at != entry.translations.len() {
                        return Err(failed("a translation out of order"));
                    }
                    entry.translations.push(text);
                    complete = true;
                    Field::Translation(at)
                }
            });
        }
        catalog.keep(entry);
        Ok(catalog)
    }

    /// `(english, translation)` pairs.
    pub fn texts(&self) -> impl Iterator<Item = (&str, &str)> {
        self.texts
            .iter()
            .map(|(english, translation)| (english.as_str(), translation.as_str()))
    }

    /// Keeps `entry` if Rust uses it. The header is the entry with the empty msgid.
    fn keep(&mut self, entry: Entry) {
        let translation = match entry.translations.as_slice() {
            [translation] if !translation.is_empty() => translation,
            _ => return,
        };
        if entry.context.is_none() && entry.plural.is_none() && !entry.id.is_empty() && !entry.fuzzy
        {
            self.texts.insert(entry.id, translation.clone());
        }
    }
}

fn unquote(quoted: &str) -> Option<String> {
    let inner = quoted.strip_prefix('"')?.strip_suffix('"')?;
    let mut text = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next()? {
            'n' => text.push('\n'),
            't' => text.push('\t'),
            'r' => text.push('\r'),
            other => text.push(other),
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(po: &str) -> Vec<(String, String)> {
        let mut texts: Vec<(String, String)> = Catalog::parse(po)
            .unwrap()
            .texts()
            .map(|(english, translation)| (english.to_string(), translation.to_string()))
            .collect();
        texts.sort();
        texts
    }

    fn pair(english: &str, translation: &str) -> (String, String) {
        (english.to_string(), translation.to_string())
    }

    #[test]
    fn rust_gets_the_texts_without_a_context() {
        let po = r#"# German texts.
msgid ""
msgstr ""
"Language: de\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

msgctxt "StatusPage"
msgid "Unknown"
msgstr "Unbekannt"

msgctxt "StatusPage"
msgid "{n} problem"
msgid_plural "{n} problems"
msgstr[0] "{n} Problem"
msgstr[1] "{n} Probleme"
msgid "Another launcher"
msgstr ""
"Ein anderer "
"Launcher"

msgid "Not \"done\"\n"
msgstr "Nicht „fertig“\n"
"#;
        assert_eq!(
            texts(po),
            [
                pair("Another launcher", "Ein anderer Launcher"),
                pair("Not \"done\"\n", "Nicht „fertig“\n"),
            ]
        );
    }

    #[test]
    fn untranslated_guessed_and_obsolete_texts_stay_english() {
        let po = r#"
msgid "Empty"
msgstr ""

#, fuzzy
msgid "Guessed"
msgstr "Geraten"

#~ msgid "Gone"
#~ msgstr "Weg"

msgid "Kept"
msgstr "Behalten"
"#;
        assert_eq!(texts(po), [pair("Kept", "Behalten")]);
    }

    #[test]
    fn a_broken_catalog_says_where() {
        assert_eq!(
            Catalog::parse("msgid \"Open\"\nmsgstr \"Offen").unwrap_err(),
            ParseError {
                line: 2,
                problem: "a string that does not end"
            }
        );
        assert_eq!(
            Catalog::parse("msgid \"A\"\nmsgstr[1] \"B\"")
                .unwrap_err()
                .line,
            2
        );
        assert!(Catalog::parse("\"stray\"").is_err());
        assert!(Catalog::parse("msgfoo \"x\"").is_err());
    }
}

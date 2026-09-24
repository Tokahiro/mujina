//! Texts that Rust puts into a window, in the user's language. Slint translates its `@tr` texts
//! itself; a text Rust has only at run time goes through a [`Localizer`], which reads the same
//! gettext `.po` catalogs. A [`Msg`]'s English wording is its key (msgid) in every catalog.

mod catalog;

use std::collections::HashMap;

pub use catalog::{Catalog, ParseError};

/// A text as written, in English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Msg(&'static str);

impl Msg {
    /// Takes the English as a string literal, which is how i18n-check finds it.
    pub const fn new(english: &'static str) -> Self {
        Self(english)
    }

    pub const fn english(self) -> &'static str {
        self.0
    }
}

/// The texts of one language at a time; a text no catalog of it has stays English.
#[derive(Debug, Default)]
pub struct Localizer {
    catalogs: Vec<(String, Catalog)>,
    /// The set language's translations; the first catalog added that has a text wins.
    current: HashMap<String, String>,
}

impl Localizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `po`, a catalog of `language` (its folder name under `lang/`); a language may have
    /// several. Takes effect with the next [`set`](Self::set).
    pub fn add(&mut self, language: &str, po: &str) -> Result<(), ParseError> {
        self.catalogs
            .push((language.to_string(), Catalog::parse(po)?));
        Ok(())
    }

    /// Switches to `language`; "en", or a language without a catalog, shows English.
    pub fn set(&mut self, language: &str) {
        let mut current = HashMap::new();
        let catalogs = self
            .catalogs
            .iter()
            .filter(|(of, _)| of.eq_ignore_ascii_case(language));
        for (_, catalog) in catalogs {
            for (english, translation) in catalog.texts() {
                current
                    .entry(english.to_string())
                    .or_insert_with(|| translation.to_string());
            }
        }
        self.current = current;
    }

    /// `msg` in the language set.
    pub fn t(&self, msg: &Msg) -> &str {
        self.text(msg.english())
    }

    /// For a text that comes as data. i18n-check cannot see it here, so the crate it comes from
    /// must also write it as a `Msg`.
    pub fn text<'a>(&'a self, english: &'a str) -> &'a str {
        self.current.get(english).map_or(english, String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DE: &str = r#"
msgid ""
msgstr ""
"Language: de\n"

msgctxt "SetupPage"
msgid "None"
msgstr "Nichts"

msgid "None"
msgstr "Keine"

msgid "Automatic ({})"
msgstr "Automatisch ({})"
"#;

    const NONE: Msg = Msg::new("None");
    const AUTOMATIC: Msg = Msg::new("Automatic ({})");
    const UNKNOWN: Msg = Msg::new("Not in any catalog");

    #[test]
    fn a_text_is_english_until_a_language_with_it_is_set() {
        let mut localizer = Localizer::new();
        localizer.add("de", DE).unwrap();
        assert_eq!(localizer.t(&NONE), "None");
        localizer.set("de");
        // The entry without a context, not Slint's.
        assert_eq!(localizer.t(&NONE), "Keine");
        assert_eq!(localizer.t(&AUTOMATIC), "Automatisch ({})");
        assert_eq!(localizer.t(&UNKNOWN), "Not in any catalog");
        assert_eq!(localizer.text("Automatic ({})"), "Automatisch ({})");
        assert_eq!(localizer.text("Not in any catalog"), "Not in any catalog");
        localizer.set("fr");
        assert_eq!(localizer.t(&NONE), "None");
        localizer.set("en");
        assert_eq!(localizer.t(&AUTOMATIC), "Automatic ({})");
    }

    #[test]
    fn a_language_may_have_several_catalogs() {
        let mut localizer = Localizer::new();
        localizer.add("de", DE).unwrap();
        localizer
            .add(
                "de",
                "msgid \"Another launcher\"\nmsgstr \"Ein anderer Launcher\"\n",
            )
            .unwrap();
        localizer.set("DE");
        assert_eq!(localizer.t(&NONE), "Keine");
        assert_eq!(
            localizer.t(&Msg::new("Another launcher")),
            "Ein anderer Launcher"
        );
    }

    #[test]
    fn a_broken_catalog_is_refused() {
        let mut localizer = Localizer::new();
        assert!(localizer.add("de", "msgid \"open").is_err());
    }
}

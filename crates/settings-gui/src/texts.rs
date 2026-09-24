//! The texts Rust puts into the window: the entries of the lists it fills, and what the parts of
//! Mujina it shows bring (a launcher's name and options, the titles of the doctor's checks),
//! which a `.slint` file cannot translate. They go through mujina-i18n: this app's own `Msg`s
//! from the catalog that holds Slint's texts too (Rust's are the entries without a context), the
//! parts' from the catalogs they bring. A launcher's or a device's texts are looked up in its own
//! catalogs first, so that the same English in another part's does not decide them.

use std::cell::RefCell;
use std::collections::HashMap;

use mujina_app::tool;
use mujina_application::device::DeviceDescriptor;
use mujina_application::launcher::LauncherDescriptor;
use mujina_i18n::{Localizer, Msg};

/// This app's catalogs, by the language `lang/` names them in.
pub const CATALOGS: [(&str, &str); 1] = [(
    "de",
    include_str!("../lang/de/LC_MESSAGES/mujina-settings-gui.po"),
)];

pub const AUTOMATIC: Msg = Msg::new("Automatic");
/// With what the button is called on the device found.
pub const AUTOMATIC_WITH: Msg = Msg::new("Automatic ({})");
pub const NONE: Msg = Msg::new("None");
// The buttons of the System page's rows.
pub const WINDOWS_SETTINGS: Msg = Msg::new("Windows settings");
pub const START_IT: Msg = Msg::new("Start it");

thread_local! {
    /// The lists are filled on the window's thread, as is everything else it shows.
    static LOCALIZER: RefCell<Localizer> = RefCell::new(localizer());
    /// The language set, for the words of a part made after it was set.
    static LANGUAGE: RefCell<String> = RefCell::new(String::from("en"));
    /// The words of each launcher and device shown, by its section (`launcher.steam`): made
    /// once, when first shown.
    static PARTS: RefCell<HashMap<String, Localizer>> = RefCell::new(HashMap::new());
}

fn localizer() -> Localizer {
    let mut localizer = Localizer::new();
    // This app's own first, so that its words win; then those of the parts whose texts it shows.
    let parts = tool::catalogs();
    let catalogs = CATALOGS.iter().chain(&parts);
    for (language, po) in catalogs {
        // A test reads every catalog; one that did not read would leave its texts English.
        let _ = localizer.add(language, po);
    }
    localizer
}

/// The texts from now on in `language`, the one the window shows.
pub fn set(language: &str) {
    LOCALIZER.with(|localizer| localizer.borrow_mut().set(language));
    LANGUAGE.with(|set| language.clone_into(&mut set.borrow_mut()));
    PARTS.with(|parts| {
        for localizer in parts.borrow_mut().values_mut() {
            localizer.set(language);
        }
    });
}

/// `msg` in the language set.
pub fn t(msg: &Msg) -> String {
    LOCALIZER.with(|localizer| localizer.borrow().t(msg).to_string())
}

/// `msg` in the language set, with `value` in place of its `{}`.
pub fn t_with(msg: &Msg, value: &str) -> String {
    t(msg).replacen("{}", value, 1)
}

/// `english`, a text of the launcher or device whose section `part` is (`launcher.steam`) and
/// which brings `catalogs`, in the language set: from its own catalogs first, then from this
/// app's, which has the words Mujina Settings adds to its rows.
fn part_text(part: &str, catalogs: &[(&str, &str)], english: &str) -> String {
    PARTS.with(|parts| {
        let mut parts = parts.borrow_mut();
        let localizer = parts.entry(part.to_string()).or_insert_with(|| {
            let mut localizer = Localizer::new();
            for (language, po) in catalogs.iter().chain(&CATALOGS) {
                // A test reads every catalog, as for `localizer`.
                let _ = localizer.add(language, po);
            }
            LANGUAGE.with(|language| localizer.set(&language.borrow()));
            localizer
        });
        localizer.text(english).to_string()
    })
}

/// The words of `launcher`: its name and its options' texts, in the language set.
pub fn launcher_words(launcher: &dyn LauncherDescriptor) -> impl Fn(&str) -> String {
    let (part, catalogs) = (format!("launcher.{}", launcher.id()), launcher.catalogs());
    move |english| part_text(&part, catalogs, english)
}

/// The words of `device`: its name, its buttons' labels and its options' texts, in the
/// language set.
pub fn device_words(device: &dyn DeviceDescriptor) -> impl Fn(&str) -> String {
    let (part, catalogs) = (format!("device.{}", device.id()), device.catalogs());
    move |english| part_text(&part, catalogs, english)
}

#[cfg(test)]
mod tests {
    use mujina_application::launcher::LauncherCaps;
    use mujina_application::testing::FakeLauncherDescriptor;
    use mujina_i18n::Catalog;

    use super::*;
    use crate::rows::REQUIRED;

    #[test]
    fn every_catalog_reads() {
        // The parts' too: the Localizer leaves one that does not read out, English.
        let parts = tool::catalogs();
        assert!(!parts.is_empty());
        for (language, po) in CATALOGS.iter().chain(&parts) {
            if let Err(error) = Catalog::parse(po) {
                panic!("a \"{language}\" catalog does not read: {error}");
            }
        }
    }

    #[test]
    fn every_launcher_speaks_each_language_of_this_app() {
        // English is the texts as written.
        let languages = crate::form::choices("interface.language")
            .iter()
            .filter(|language| !matches!(**language, "auto" | "en"));
        for launcher in tool::launchers().all {
            for language in languages.clone() {
                assert!(
                    launcher.catalogs().iter().any(|(of, _)| of == language),
                    "launcher {} has no \"{language}\" catalog",
                    launcher.id()
                );
            }
        }
    }

    #[test]
    fn the_texts_follow_the_language_set() {
        let generic = launcher_words(tool::launchers().get("generic"));
        set("de");
        assert_eq!(t(&NONE), "Keine");
        assert_eq!(
            t_with(&AUTOMATIC_WITH, "Desktop button"),
            "Automatisch (Desktop button)"
        );
        // From the catalogs of the parts: a launcher's, the doctor's.
        assert_eq!(generic("Another launcher"), "Ein anderer Launcher");
        assert_eq!(t(&Msg::new("Developer Mode")), "Entwicklermodus");
        assert_eq!(t(&Msg::new("Xbox mode")), "Xbox-Modus");
        // What Mujina Settings adds to a launcher's rows.
        assert_eq!(generic(REQUIRED.english()), "Erforderlich.");
        set("en");
        assert_eq!(t(&AUTOMATIC), "Automatic");
        assert_eq!(generic("Another launcher"), "Another launcher");
    }

    /// A launcher whose German for "None" is not Mujina Settings' own.
    static ODD: FakeLauncherDescriptor = FakeLauncherDescriptor {
        catalogs: &[(
            "de",
            "msgid \"Odd\"\nmsgstr \"Seltsam\"\n\nmsgid \"None\"\nmsgstr \"Nichts davon\"\n",
        )],
        ..FakeLauncherDescriptor::named("odd", "Odd", LauncherCaps::ALL)
    };

    #[test]
    fn a_launcher_s_texts_come_from_its_own_catalog_first() {
        set("de");
        let odd = launcher_words(&ODD);
        assert_eq!(odd("None"), "Nichts davon");
        // Mujina Settings' own "None" stays its own, and another launcher's German is not odd's.
        assert_eq!(t(&NONE), "Keine");
        assert_eq!(odd("Another launcher"), "Another launcher");
        // Words made before the language changes follow it.
        set("en");
        assert_eq!(odd("None"), "None");
        set("de");
        assert_eq!(odd("Odd"), "Seltsam");
        set("en");
    }
}

//! Windows' display languages, and which of its own languages an app shows.

use std::ptr::null_mut;

use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

/// The language an app shows, of those it has (`available`, the one its texts are written in
/// first): `chosen` where the app has it, else the first of the Windows display languages it
/// has, else the first.
pub fn language(chosen: &str, available: &[&'static str]) -> &'static str {
    pick(chosen, &ui_languages(), available)
}

/// The user's display languages as names, most preferred first, e.g. `["de-DE", "en-US"]`.
/// Empty if Windows does not say.
pub fn ui_languages() -> Vec<String> {
    let mut count: u32 = 0;
    let mut length: u32 = 0;
    // SAFETY: a null buffer with length 0 is the documented way to query the required length.
    let sized = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &raw mut count,
            null_mut(),
            &raw mut length,
        )
    };
    if sized == 0 || length == 0 {
        return Vec::new();
    }
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: `buffer` is writable for `length` UTF-16 units.
    let filled = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &raw mut count,
            buffer.as_mut_ptr(),
            &raw mut length,
        )
    };
    if filled == 0 {
        return Vec::new();
    }
    // One name after the other, each ended by a NUL, the list by a second one.
    buffer
        .split(|&unit| unit == 0)
        .take_while(|name| !name.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// [`language`], with the display languages given.
fn pick(chosen: &str, preferred: &[String], available: &[&'static str]) -> &'static str {
    let find = |wanted: &str| {
        available
            .iter()
            .copied()
            .find(|language| language.eq_ignore_ascii_case(wanted))
    };
    // "de-AT" is shown in "de" where the app has no "de-AT".
    let base = |tag: &str| tag.split(['-', '_']).next().unwrap_or(tag).to_string();
    find(chosen)
        .or_else(|| {
            preferred
                .iter()
                .find_map(|tag| find(tag).or_else(|| find(&base(tag))))
        })
        .or_else(|| available.first().copied())
        .unwrap_or("en")
}

#[cfg(test)]
mod tests {
    use super::*;

    const AVAILABLE: [&str; 2] = ["en", "de"];

    fn tags(tags: &[&str]) -> Vec<String> {
        tags.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_language_chosen_comes_first() {
        assert_eq!(pick("en", &tags(&["de-DE"]), &AVAILABLE), "en");
        assert_eq!(pick("de", &tags(&["en-US"]), &AVAILABLE), "de");
    }

    #[test]
    fn otherwise_the_first_display_language_the_app_has() {
        assert_eq!(pick("auto", &tags(&["de-AT", "en-US"]), &AVAILABLE), "de");
        assert_eq!(pick("", &tags(&["fr-FR", "de-CH"]), &AVAILABLE), "de");
        assert_eq!(pick("", &tags(&["en-GB", "de-DE"]), &AVAILABLE), "en");
        // A language Mujina does not have is no choice.
        assert_eq!(pick("fr", &tags(&["de-DE"]), &AVAILABLE), "de");
    }

    #[test]
    fn otherwise_the_language_the_texts_are_written_in() {
        assert_eq!(pick("", &tags(&["fr-FR", "ja-JP"]), &AVAILABLE), "en");
        assert_eq!(pick("", &[], &AVAILABLE), "en");
        assert_eq!(pick("", &[], &[]), "en");
    }

    #[test]
    fn windows_names_its_display_languages() {
        let languages = ui_languages();
        assert!(!languages.is_empty());
        assert!(
            languages
                .iter()
                .all(|name| name.len() >= 2 && !name.contains('\0')),
            "{languages:?}"
        );
    }
}

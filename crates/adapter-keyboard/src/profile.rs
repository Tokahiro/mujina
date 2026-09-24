//! Device profiles: the files in `profiles/devices/`, one per device whose buttons arrive as key
//! chords, each a device descriptor.

use std::sync::LazyLock;

use mujina_application::device::{
    ButtonId, ButtonSpec, DeviceDescriptor, Suppression, SystemIdentity,
};
use mujina_application::settings::schema::SettingSpec;
use mujina_domain::chord::TriggerChord;
use serde::Deserialize;

use crate::MAX_BUTTONS;

include!(concat!(env!("OUT_DIR"), "/profiles.rs"));

/// A profile file, as written.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    id: String,
    name: String,
    #[serde(rename = "match")]
    matcher: MatcherFile,
    buttons: Vec<ButtonFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MatcherFile {
    manufacturer: String,
    product: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ButtonFile {
    key: String,
    label: String,
    chord: String,
    #[serde(default = "yes")]
    injected_only: bool,
    #[serde(default)]
    suppression: SuppressionFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SuppressionFile {
    #[default]
    Swallowed,
    /// Refused for now: the hook holds every chord back until it knows which button it is, so a
    /// button whose keys are to reach other programs too would need a matcher that passes them
    /// while still telling buttons apart. Read, so that the refusal says why.
    Observed,
}

fn yes() -> bool {
    true
}

/// A device whose buttons are key chords, as its profile says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordProfile {
    id: String,
    name: String,
    manufacturer: String,
    product: String,
    buttons: Vec<(ButtonSpec, TriggerChord)>,
}

impl ChordProfile {
    /// Reads a profile file; what is wrong with it, otherwise. Besides the shape, it refuses what
    /// the hook could not serve: more buttons than it catches, and two chords it cannot tell
    /// apart, so that the folder's test names the file rather than a button never firing.
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: ProfileFile =
            toml::from_str(text).map_err(|error| error.message().to_string())?;
        if file.buttons.len() > MAX_BUTTONS {
            return Err(format!(
                "{} buttons; Mujina catches at most {MAX_BUTTONS} of one device",
                file.buttons.len()
            ));
        }
        let mut buttons: Vec<(ButtonSpec, TriggerChord)> = Vec::new();
        for (index, button) in file.buttons.into_iter().enumerate() {
            let chord =
                TriggerChord::parse(&button.chord, button.injected_only).map_err(|error| {
                    format!("button {}: chord \"{}\": {error}", button.key, button.chord)
                })?;
            if let SuppressionFile::Observed = button.suppression {
                return Err(format!(
                    "button {}: suppression \"observed\" is not supported for a key chord, which \
                     Mujina always holds back until it knows which button it is",
                    button.key
                ));
            }
            for (other, theirs) in &buttons {
                if let Some(problem) = clash(&chord, theirs) {
                    return Err(format!(
                        "buttons {} and {}: {problem}",
                        other.key, button.key
                    ));
                }
            }
            let id = u8::try_from(index)
                .map_err(|_| "more buttons than Mujina can number".to_string())?;
            let spec = ButtonSpec {
                id: ButtonId(id),
                key: button.key,
                label: button.label,
                suppression: Suppression::Swallowed,
            };
            buttons.push((spec, chord));
        }
        Ok(Self {
            id: file.id,
            name: file.name,
            manufacturer: file.matcher.manufacturer,
            product: file.matcher.product,
            buttons,
        })
    }

    /// Each button with the chord it arrives as.
    pub fn chords(&self) -> Vec<(ButtonId, TriggerChord)> {
        self.buttons
            .iter()
            .map(|(spec, chord)| (spec.id, *chord))
            .collect()
    }
}

impl DeviceDescriptor for ChordProfile {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    fn matches(&self, identity: &SystemIdentity) -> bool {
        wildcard_match(&self.manufacturer, &identity.manufacturer)
            && wildcard_match(&self.product, &identity.product)
    }

    fn buttons(&self) -> Vec<ButtonSpec> {
        self.buttons.iter().map(|(spec, _)| spec.clone()).collect()
    }

    fn settings(&self) -> &[SettingSpec] {
        &[]
    }
}

/// Why the matcher could not tell two buttons' chords apart, if it could not: the same keys
/// with the same trigger, or one chord made only of keys the other holds. The held keys come in
/// whatever order the firmware presses them, so the second would fire as the first the moment
/// its keys were down (the matcher takes the shorter chord).
fn clash(ours: &TriggerChord, theirs: &TriggerChord) -> Option<&'static str> {
    let same = ours.trigger() == theirs.trigger()
        && ours.held().len() == theirs.held().len()
        && ours.held().iter().all(|key| theirs.held().contains(key));
    if same {
        return Some("they send the same chord");
    }
    let within = |short: &TriggerChord, long: &TriggerChord| {
        short
            .keys
            .keys()
            .iter()
            .all(|key| long.held().contains(key))
    };
    (within(ours, theirs) || within(theirs, ours))
        .then_some("one chord is the beginning of the other, which would never fire")
}

/// The profiles shipped with Mujina, sorted by file name. `auto` takes the first that matches, so
/// a profile for some of the machines another one's patterns cover has to sort before it (a
/// OneXPlayer model's before `onexplayer.toml`, which takes every ONE-NETBOOK machine); the
/// folder's test checks that. One that does not parse is left out here and fails the tests.
pub fn builtin() -> &'static [ChordProfile] {
    static PROFILES: LazyLock<Vec<ChordProfile>> = LazyLock::new(|| {
        BUILTIN
            .iter()
            .filter_map(|(_, text)| ChordProfile::parse(text).ok())
            .collect()
    });
    &PROFILES
}

/// Case-insensitive match where `*` stands for any run of characters.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let text = text.to_lowercase();
    let mut parts = pattern.split('*');
    let Some(first) = parts.next() else {
        return text.is_empty();
    };
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let mut last_was_literal = true;
    let mut parts = parts.peekable();
    while let Some(part) = parts.next() {
        last_was_literal = false;
        if part.is_empty() {
            continue;
        }
        let found = if parts.peek().is_none() {
            // The last literal has to sit at the very end.
            rest.ends_with(part).then(|| rest.len() - part.len())
        } else {
            rest.find(part)
        };
        let Some(at) = found else {
            return false;
        };
        rest = &rest[at + part.len()..];
        last_was_literal = true;
    }
    // Without any `*` the whole text must have been consumed; with a trailing `*` anything goes.
    !last_was_literal || rest.is_empty()
}

#[cfg(test)]
mod tests {
    use mujina_domain::keys::VirtualKey;

    use super::*;

    fn machine(manufacturer: &str, product: &str) -> SystemIdentity {
        SystemIdentity {
            manufacturer: manufacturer.to_string(),
            product: product.to_string(),
        }
    }

    #[test]
    fn every_file_in_the_folder_is_built_in_and_parses() {
        let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/../../profiles/devices");
        let mut on_disk: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            // As build.rs picks them.
            .filter(|name| {
                std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext == "toml")
            })
            .collect();
        on_disk.sort();
        let built_in: Vec<&str> = BUILTIN.iter().map(|(name, _)| *name).collect();
        assert_eq!(built_in, on_disk);
        for (name, text) in BUILTIN {
            if let Err(error) = ChordProfile::parse(text) {
                panic!("profiles/devices/{name}: {error}");
            }
        }
        assert_eq!(builtin().len(), BUILTIN.len());
    }

    #[test]
    fn every_built_in_profile_keeps_the_rules_under_an_id_of_its_own() {
        for (index, profile) in builtin().iter().enumerate() {
            mujina_application::testing::device_conformance(profile);
            assert!(
                builtin()[..index]
                    .iter()
                    .all(|other| other.id != profile.id),
                "{} is there twice",
                profile.id
            );
            let file = BUILTIN[index].0;
            assert_eq!(
                file,
                format!("{}.toml", profile.id),
                "a profile's file is named after its id"
            );
        }
    }

    #[test]
    fn the_onexplayer_is_recognised_by_its_firmware_strings() {
        let oxp = builtin()
            .iter()
            .find(|profile| profile.id == "onexplayer")
            .unwrap();
        assert!(oxp.matches(&machine("ONE-NETBOOK", "ONEXPLAYER 3")));
        assert!(oxp.matches(&machine(
            "ONE-NETBOOK TECHNOLOGY CO., LTD.",
            "ONEXPLAYER X1"
        )));
        assert!(!oxp.matches(&machine("ASUSTeK COMPUTER INC.", "ROG Ally RC71L")));
        // Its button, exactly as before profiles had several: an injected LWIN+D.
        let chords = oxp.chords();
        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].0, ButtonId(0));
        assert_eq!(chords[0].1.keys.keys(), [VirtualKey::LWIN, VirtualKey::D]);
        assert!(chords[0].1.injected_only);
        assert_eq!(oxp.buttons()[0].label, "Desktop button");
        assert_eq!(oxp.buttons()[0].suppression, Suppression::Swallowed);
    }

    #[test]
    fn a_profile_may_have_several_buttons_and_longer_chords() {
        let text = r#"
            id = "onexplayer-mini"
            name = "OneXPlayer Mini"
            [match]
            manufacturer = "ONE-NETBOOK*"
            product = "ONEXPLAYER Mini*"
            [[buttons]]
            key = "orange"
            label = "Orange button"
            chord = "LCTRL+LWIN+LALT"
            [[buttons]]
            key = "keyboard"
            label = "Keyboard button"
            chord = "LWIN+0xA3+O"
            suppression = "swallowed"
            [[buttons]]
            key = "desktop"
            label = "Desktop button"
            chord = "LWIN+D"
            injected_only = false
        "#;
        let mini = ChordProfile::parse(text).unwrap();
        mujina_application::testing::device_conformance(&mini);
        let chords = mini.chords();
        assert_eq!(
            chords.iter().map(|(id, _)| id.0).collect::<Vec<_>>(),
            [0, 1, 2],
            "numbered in order"
        );
        assert_eq!(chords[0].1.trigger(), VirtualKey::LMENU);
        assert_eq!(mini.buttons()[1].suppression, Suppression::Swallowed);
        assert!(!chords[2].1.injected_only);
        assert!(mini.matches(&machine("ONE-NETBOOK", "ONEXPLAYER Mini Pro")));
    }

    /// A profile of `buttons`, each `(key, chord)`.
    fn with_buttons(buttons: &[(&str, &str)]) -> String {
        let head = "id = \"x\"\nname = \"X\"\n[match]\nmanufacturer = \"*\"\nproduct = \"*\"\n";
        buttons.iter().fold(head.to_string(), |text, (key, chord)| {
            text + &format!("[[buttons]]\nkey = \"{key}\"\nlabel = \"B\"\nchord = \"{chord}\"\n")
        })
    }

    #[test]
    fn what_the_hook_could_not_serve_is_refused() {
        // A chord Mujina holds back cannot also reach the companion software.
        let observed = with_buttons(&[("b", "LWIN+D")]) + "suppression = \"observed\"\n";
        assert_eq!(
            ChordProfile::parse(&observed),
            Err(
                "button b: suppression \"observed\" is not supported for a key chord, which \
                 Mujina always holds back until it knows which button it is"
                    .to_string()
            )
        );

        let keys = ["b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8"];
        let chords = [
            "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21",
        ];
        let many: Vec<(&str, &str)> = keys.into_iter().zip(chords).collect();
        assert!(ChordProfile::parse(&with_buttons(&many[..MAX_BUTTONS])).is_ok());
        assert_eq!(
            ChordProfile::parse(&with_buttons(&many)),
            Err("9 buttons; Mujina catches at most 8 of one device".to_string())
        );

        // The same keys, the held ones in another order.
        let twice = with_buttons(&[("a", "LCTRL+LWIN+D"), ("b", "LWIN+LCTRL+D")]);
        assert_eq!(
            ChordProfile::parse(&twice),
            Err("buttons a and b: they send the same chord".to_string())
        );
        // LWIN+D fires as soon as LWIN and D are down, whichever order the other's held keys
        // come in; so does a one-key button that another holds.
        for (short, long) in [("LWIN+D", "D+LWIN+E"), ("F24", "F24+E")] {
            for pair in [[("a", short), ("b", long)], [("a", long), ("b", short)]] {
                assert_eq!(
                    ChordProfile::parse(&with_buttons(&pair)),
                    Err(
                        "buttons a and b: one chord is the beginning of the other, which would \
                         never fire"
                            .to_string()
                    ),
                    "{pair:?}"
                );
            }
        }
        // Sharing keys is fine as long as the next key tells them apart.
        let apart = with_buttons(&[("a", "LWIN+D"), ("b", "LWIN+0xA3+O"), ("c", "D+LWIN")]);
        assert!(ChordProfile::parse(&apart).is_ok());
    }

    /// Whether `a`'s patterns match the machine `b` names in its patterns' literal text: then
    /// `auto`, taking the first profile that matches, finds `a` on `b`'s own machine.
    fn covers(a: &ChordProfile, b: &ChordProfile) -> bool {
        let literal = |pattern: &str| pattern.replace('*', "");
        wildcard_match(&a.manufacturer, &literal(&b.manufacturer))
            && wildcard_match(&a.product, &literal(&b.product))
    }

    /// For `profiles` in the order `auto` tries them, each with its file name: every pair of
    /// which the earlier hides the later.
    fn hidden(profiles: &[(&str, ChordProfile)]) -> Vec<String> {
        let mut found = Vec::new();
        for (index, (later, theirs)) in profiles.iter().enumerate() {
            for (earlier, ours) in &profiles[..index] {
                if covers(ours, theirs) {
                    found.push(format!(
                        "profiles/devices/{earlier} matches every machine {later} is for and \
                         comes first, so {later} is never chosen: name the more specific one \
                         so that it sorts first"
                    ));
                }
            }
        }
        found
    }

    #[test]
    fn a_profile_for_some_of_anothers_machines_sorts_before_it() {
        let shipped: Vec<(&str, ChordProfile)> = BUILTIN
            .iter()
            .filter_map(|(name, text)| Some((*name, ChordProfile::parse(text).ok()?)))
            .collect();
        let found = hidden(&shipped);
        assert!(found.is_empty(), "{found:#?}");

        // The check itself: a OneXPlayer Mini's file sorting after the family's is found.
        let family = shipped
            .iter()
            .find(|(name, _)| *name == "onexplayer.toml")
            .map(|(_, profile)| profile.clone())
            .unwrap();
        let mini = ChordProfile::parse(
            &with_buttons(&[("b", "LCTRL+LWIN+LALT")])
                .replace("manufacturer = \"*\"", "manufacturer = \"ONE-NETBOOK*\"")
                .replace("product = \"*\"", "product = \"ONEXPLAYER Mini*\""),
        )
        .unwrap();
        assert!(mini.matches(&machine("ONE-NETBOOK", "ONEXPLAYER Mini Pro")));
        let before = [
            ("onexplayer-mini.toml", mini.clone()),
            ("onexplayer.toml", family.clone()),
        ];
        assert!(hidden(&before).is_empty());
        let after = [
            ("onexplayer.toml", family.clone()),
            ("onexplayer_mini.toml", mini),
        ];
        assert_eq!(hidden(&after).len(), 1);
        assert!(hidden(&after)[0].starts_with(
            "profiles/devices/onexplayer.toml matches every machine onexplayer_mini.toml is for"
        ));
        // Two files for the same machines hide one another, whichever comes first.
        assert_eq!(
            hidden(&[("a.toml", family.clone()), ("b.toml", family)]).len(),
            1
        );
    }

    #[test]
    fn a_broken_profile_says_what_is_wrong() {
        let v1 = "id = \"x\"\nname = \"X\"\nbutton_name = \"B\"\n[match]\nmanufacturer = \"*\"\n\
                  product = \"*\"\n[button]\nmodifier = \"LWIN\"\nkey = \"D\"\n";
        assert!(
            ChordProfile::parse(v1).is_err(),
            "the old shape is not read"
        );
        let bad = "id = \"x\"\nname = \"X\"\n[match]\nmanufacturer = \"*\"\nproduct = \"*\"\n\
                   [[buttons]]\nkey = \"b\"\nlabel = \"B\"\nchord = \"LWIN+NOPE\"\n";
        assert_eq!(
            ChordProfile::parse(bad),
            Err("button b: chord \"LWIN+NOPE\": chord contains an unknown key name".to_string())
        );
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("one-netbook*", "ONE-NETBOOK Technology"));
        assert!(wildcard_match("*ally*", "ROG Ally RC71L"));
        assert!(wildcard_match("rog*rc71l", "ROG Ally RC71L"));
        assert!(wildcard_match("exact", "EXACT"));
        assert!(!wildcard_match("exact", "exactly"));
        assert!(!wildcard_match("rog*x", "ROG Ally RC71L"));
        assert!(!wildcard_match("one*", "someone"));
    }
}

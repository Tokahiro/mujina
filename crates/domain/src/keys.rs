//! Virtual keys and key chords, independent of any OS API.

use core::fmt;

/// A virtual-key code. The numeric values follow the Windows `VK_*` table so adapters can pass
/// them through unchanged, but nothing in the domain depends on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VirtualKey(pub u16);

impl VirtualKey {
    pub const TAB: Self = Self(0x09);
    pub const ESCAPE: Self = Self(0x1B);
    pub const INSERT: Self = Self(0x2D);
    pub const D: Self = Self(0x44);
    pub const DIGIT_1: Self = Self(0x31);
    pub const DIGIT_2: Self = Self(0x32);
    pub const LWIN: Self = Self(0x5B);
    pub const RWIN: Self = Self(0x5C);
    pub const F1: Self = Self(0x70);
    pub const LSHIFT: Self = Self(0xA0);
    pub const LCONTROL: Self = Self(0xA2);
    pub const LMENU: Self = Self(0xA4);

    /// Parses a key name as used in profile files: `LWIN`, `LCTRL`, `TAB`, `F12`, `D`, `1`, …
    /// Any other key can be given by its code, `0xE8`.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        if let Some((_, key)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
            return Some(*key);
        }
        if let Some(hex) = name.strip_prefix("0x").or_else(|| name.strip_prefix("0X")) {
            return u16::from_str_radix(hex, 16)
                .ok()
                .filter(|code| (1..=0xFE).contains(code))
                .map(Self);
        }
        if let Some(number) = name.strip_prefix(['F', 'f'])
            && let Ok(n @ 1..=24) = number.parse::<u16>()
        {
            return Some(Self(Self::F1.0 + n - 1));
        }
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphanumeric() => {
                let byte = u8::try_from(c).ok()?;
                Some(Self(u16::from(byte.to_ascii_uppercase())))
            }
            _ => None,
        }
    }
}

/// Key names; where a key has several, the first is the one used when printing.
const NAMED: &[(&str, VirtualKey)] = &[
    ("TAB", VirtualKey::TAB),
    ("ESC", VirtualKey::ESCAPE),
    ("ESCAPE", VirtualKey::ESCAPE),
    ("INSERT", VirtualKey::INSERT),
    ("LWIN", VirtualKey::LWIN),
    ("RWIN", VirtualKey::RWIN),
    ("LSHIFT", VirtualKey::LSHIFT),
    ("LCTRL", VirtualKey::LCONTROL),
    ("LCONTROL", VirtualKey::LCONTROL),
    ("LALT", VirtualKey::LMENU),
    ("LMENU", VirtualKey::LMENU),
    // What Windows itself synthesizes from a game controller. Seeing these in `mujinactl probe`
    // means "controller input", not a device button.
    ("GAMEPAD_A", VirtualKey(0xC3)),
    ("GAMEPAD_B", VirtualKey(0xC4)),
    ("GAMEPAD_X", VirtualKey(0xC5)),
    ("GAMEPAD_Y", VirtualKey(0xC6)),
    ("GAMEPAD_RB", VirtualKey(0xC7)),
    ("GAMEPAD_LB", VirtualKey(0xC8)),
    ("GAMEPAD_LT", VirtualKey(0xC9)),
    ("GAMEPAD_RT", VirtualKey(0xCA)),
    ("GAMEPAD_UP", VirtualKey(0xCB)),
    ("GAMEPAD_DOWN", VirtualKey(0xCC)),
    ("GAMEPAD_LEFT", VirtualKey(0xCD)),
    ("GAMEPAD_RIGHT", VirtualKey(0xCE)),
    ("GAMEPAD_MENU", VirtualKey(0xCF)),
    ("GAMEPAD_VIEW", VirtualKey(0xD0)),
];

/// Prints the name [`VirtualKey::from_name`] understands, falling back to the code (`0xE8`).
impl fmt::Display for VirtualKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some((name, _)) = NAMED.iter().find(|(_, key)| key == self) {
            return f.write_str(name);
        }
        match self.0 {
            code @ 0x70..=0x87 => write!(f, "F{}", code - 0x6F),
            code @ (0x30..=0x39 | 0x41..=0x5A) => {
                write!(f, "{}", char::from(u8::try_from(code).unwrap_or(b'?')))
            }
            code => write!(f, "0x{code:02X}"),
        }
    }
}

/// Maximum number of keys in a [`KeyChord`].
pub const MAX_CHORD_KEYS: usize = 4;

/// Keys pressed in order and released in reverse order, e.g. `LCTRL+1`.
///
/// Only left-hand modifiers are nameable on purpose: games and Steam's overlay sample keyboard
/// state per frame and several of them only look at the left-hand variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    keys: [VirtualKey; MAX_CHORD_KEYS],
    len: usize,
}

/// Why a chord description could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordParseError {
    Empty,
    TooManyKeys,
    UnknownKey,
    /// A key named twice; only a device button's chord refuses it, since its keys are held
    /// together.
    RepeatedKey,
}

impl fmt::Display for ChordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "chord is empty",
            Self::TooManyKeys => "chord has more than four keys",
            Self::UnknownKey => "chord contains an unknown key name",
            Self::RepeatedKey => "chord names a key twice",
        })
    }
}

impl core::error::Error for ChordParseError {}

impl KeyChord {
    /// Parses `KEY+KEY+…`.
    pub fn parse(text: &str) -> Result<Self, ChordParseError> {
        let mut keys = [VirtualKey(0); MAX_CHORD_KEYS];
        let mut len = 0;
        for part in text.split('+') {
            if part.trim().is_empty() {
                return Err(ChordParseError::Empty);
            }
            if len == MAX_CHORD_KEYS {
                return Err(ChordParseError::TooManyKeys);
            }
            keys[len] = VirtualKey::from_name(part).ok_or(ChordParseError::UnknownKey)?;
            len += 1;
        }
        Ok(Self { keys, len })
    }

    /// A chord from keys in press order; `None` if there are none or too many.
    pub fn from_keys(pressed: &[VirtualKey]) -> Option<Self> {
        if pressed.is_empty() || pressed.len() > MAX_CHORD_KEYS {
            return None;
        }
        let mut keys = [VirtualKey(0); MAX_CHORD_KEYS];
        keys[..pressed.len()].copy_from_slice(pressed);
        Some(Self {
            keys,
            len: pressed.len(),
        })
    }

    /// A modifier plus a key, the shape of nearly every shortcut.
    pub const fn pair(modifier: VirtualKey, key: VirtualKey) -> Self {
        Self {
            keys: [modifier, key, VirtualKey(0), VirtualKey(0)],
            len: 2,
        }
    }

    /// The keys in press order.
    pub fn keys(&self) -> &[VirtualKey] {
        &self.keys[..self.len]
    }
}

/// Prints the form [`KeyChord::parse`] reads, e.g. `LSHIFT+TAB`.
impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, key) in self.keys().iter().enumerate() {
            if index > 0 {
                f.write_str("+")?;
            }
            write!(f, "{key}")?;
        }
        Ok(())
    }
}

/// How long synthesized chords are held. Too short and frame-sampled input misses them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldTiming {
    /// Pause after each modifier press, in milliseconds.
    pub modifier_gap_ms: u16,
    /// How long the final key stays down, in milliseconds.
    pub key_hold_ms: u16,
}

impl Default for HoldTiming {
    fn default() -> Self {
        Self {
            modifier_gap_ms: 20,
            key_hold_ms: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_single_and_function_keys() {
        assert_eq!(VirtualKey::from_name("lwin"), Some(VirtualKey::LWIN));
        assert_eq!(VirtualKey::from_name("d"), Some(VirtualKey::D));
        assert_eq!(VirtualKey::from_name("1"), Some(VirtualKey::DIGIT_1));
        assert_eq!(VirtualKey::from_name("F12"), Some(VirtualKey(0x7B)));
        assert_eq!(VirtualKey::from_name("F25"), None);
        assert_eq!(VirtualKey::from_name("RCTRL"), None);
    }

    #[test]
    fn every_key_prints_as_a_name_that_parses_back() {
        use alloc::string::ToString;

        for code in 1..=0xFE {
            let key = VirtualKey(code);
            assert_eq!(VirtualKey::from_name(&key.to_string()), Some(key), "{key}");
        }
        assert_eq!(VirtualKey::LCONTROL.to_string(), "LCTRL");
        assert_eq!(VirtualKey(0x7B).to_string(), "F12");
        assert_eq!(VirtualKey::D.to_string(), "D");
        assert_eq!(VirtualKey(0xE8).to_string(), "0xE8");
        assert_eq!(VirtualKey::from_name("0x00"), None);
    }

    #[test]
    fn parses_chords_in_press_order() {
        let chord = KeyChord::parse("LCTRL+1").unwrap();
        assert_eq!(chord.keys(), [VirtualKey::LCONTROL, VirtualKey::DIGIT_1]);
        let chord = KeyChord::parse(" LSHIFT + TAB ").unwrap();
        assert_eq!(chord.keys(), [VirtualKey::LSHIFT, VirtualKey::TAB]);
    }

    #[test]
    fn rejects_bad_chords() {
        assert_eq!(KeyChord::parse(""), Err(ChordParseError::Empty));
        assert_eq!(KeyChord::parse("LCTRL+"), Err(ChordParseError::Empty));
        assert_eq!(
            KeyChord::parse("LCTRL+NOPE"),
            Err(ChordParseError::UnknownKey)
        );
        assert_eq!(
            KeyChord::parse("A+B+C+D+E"),
            Err(ChordParseError::TooManyKeys)
        );
    }
}

//! The package this installer carries: the signed MSIX, its certificate (while the package is
//! self-signed) and a few lines about them. They are not compiled in: `packaging/attach-payload.ps1`
//! adds them to the finished executable as data resources, before it is signed, so that the job
//! that holds the signing key needs no compiler (docs/signing.md). A build without them is a
//! working installer that says it carries no package.
//!
//! What the lines say is read here and checked where it can be: the family must follow from the
//! name and the publisher, as Windows derives it. The certificate is trusted as it is: it and the
//! lines come from the same file, so a check of one against the other would prove nothing.

use std::fmt;

use crate::plan::Version;

/// The resources `packaging/attach-payload.ps1` adds, by name, all raw data (`RT_RCDATA`).
pub const MSIX_RESOURCE: &str = "MUJINA_MSIX";
pub const CER_RESOURCE: &str = "MUJINA_CER";
pub const ABOUT_RESOURCE: &str = "MUJINA_ABOUT";

/// What the attached package is, as `mujina-setup.exe --about` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct About {
    /// The manifest's `Identity Name`, `Mujina`.
    pub name: String,
    /// The manifest's `Identity Publisher`, exactly as written there, e.g. `CN=Mujina`.
    pub publisher: String,
    /// The package family Windows derives from the two, e.g. `Mujina_k2veznmcx4n98`.
    pub family: String,
    pub version: Version,
    /// The package's file name, e.g. `Mujina_0.27.0.0_x64.msix`: what it is unpacked as.
    pub msix: String,
    /// The SHA-1 thumbprint of the certificate carried, in hex, as Windows shows it.
    pub certificate: String,
}

impl About {
    /// Reads the lines `key=value`, one each; every key must be there once. A publisher holds
    /// `=` itself, so only the first one separates.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut values: Vec<(&str, &str)> = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("not a key=value line: {line}"))?;
            if values.iter().any(|(seen, _)| *seen == key) {
                return Err(format!("{key} is given twice"));
            }
            values.push((key, value));
        }
        let get = |key: &str| {
            values
                .iter()
                .find(|(seen, _)| *seen == key)
                .map(|(_, value)| value.to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("{key} is missing"))
        };
        let version = get("version")?;
        let about = Self {
            name: get("name")?,
            publisher: get("publisher")?,
            family: get("family")?,
            version: Version::parse(&version).ok_or_else(|| format!("version {version}"))?,
            msix: get("msix")?,
            certificate: get("certificate")?,
        };
        // Written into a folder of Setup's own as this name: a bare file name, nothing else.
        let plain = about
            .msix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !plain || !about.msix.to_ascii_lowercase().ends_with(".msix") {
            return Err(format!("msix {} is no plain .msix file name", about.msix));
        }
        if let Some(key) = values
            .iter()
            .map(|(key, _)| *key)
            .find(|key| !KEYS.contains(key))
        {
            return Err(format!("unknown key {key}"));
        }
        Ok(about)
    }
}

const KEYS: [&str; 6] = [
    "name",
    "publisher",
    "family",
    "version",
    "msix",
    "certificate",
];

/// As `--about` prints it, one fact per line.
impl fmt::Display for About {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Package:     {}", self.msix)?;
        writeln!(f, "Name:        {}", self.name)?;
        writeln!(f, "Version:     {}", self.version)?;
        writeln!(f, "Publisher:   {}", self.publisher)?;
        writeln!(f, "Family:      {}", self.family)?;
        write!(f, "Certificate: {} (SHA-1)", self.certificate)
    }
}

/// The package, as attached to this executable.
#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct Payload {
    pub about: About,
    pub msix: &'static [u8],
    pub cer: &'static [u8],
}

/// Why this executable has no usable package.
#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// Nothing was attached: a build straight from cargo.
    NotAttached,
    /// Something was, but not all of it, or not what it says it is.
    Damaged(String),
}

#[cfg(windows)]
impl fmt::Display for Missing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAttached => f.write_str("this build of Mujina Setup carries no package"),
            Self::Damaged(why) => write!(f, "the package this installer carries is damaged: {why}"),
        }
    }
}

/// The package attached to this executable, read once.
#[cfg(windows)]
pub fn attached() -> Result<&'static Payload, Missing> {
    static PAYLOAD: std::sync::OnceLock<Result<Payload, Missing>> = std::sync::OnceLock::new();
    PAYLOAD.get_or_init(read).as_ref().map_err(Clone::clone)
}

#[cfg(windows)]
fn read() -> Result<Payload, Missing> {
    use mujina_winutil::{package, resource};

    let parts = [ABOUT_RESOURCE, MSIX_RESOURCE, CER_RESOURCE].map(resource::own_data);
    let [about, msix, cer] = match parts {
        [None, None, None] => return Err(Missing::NotAttached),
        [Some(about), Some(msix), Some(cer)] if !msix.is_empty() && !cer.is_empty() => {
            [about, msix, cer]
        }
        _ => return Err(Missing::Damaged("a part is missing".to_string())),
    };
    let text = std::str::from_utf8(about).map_err(|error| Missing::Damaged(error.to_string()))?;
    let about = About::parse(text).map_err(Missing::Damaged)?;
    let derived = package::family_name_from_id(&about.name, &about.publisher);
    if !derived.is_some_and(|derived| derived.eq_ignore_ascii_case(&about.family)) {
        return Err(Missing::Damaged(format!(
            "family {} does not follow from {} and {}",
            about.family, about.name, about.publisher
        )));
    }
    Ok(Payload { about, msix, cer })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &str = "name=Mujina\r\npublisher=CN=Mujina\r\nfamily=Mujina_k2veznmcx4n98\r\n\
                         version=0.27.0.0\r\nmsix=Mujina_0.27.0.0_x64.msix\r\n\
                         certificate=C21089F232C29920F2A17D78E389E130F3FD589A\r\n";

    #[test]
    fn the_lines_say_what_the_package_is() {
        let about = About::parse(LINES).unwrap();
        assert_eq!(about.publisher, "CN=Mujina");
        assert_eq!(about.version, Version::parse("0.27.0.0").unwrap());
        assert_eq!(about.msix, "Mujina_0.27.0.0_x64.msix");
        let shown = about.to_string();
        assert!(
            shown.contains("Family:      Mujina_k2veznmcx4n98"),
            "{shown}"
        );
        assert!(shown.contains("Version:     0.27.0\n"), "{shown}");
    }

    #[test]
    fn incomplete_or_strange_lines_are_refused() {
        let without = |key: &str| {
            LINES
                .lines()
                .filter(|line| !line.starts_with(key))
                .collect::<Vec<_>>()
                .join("\n")
        };
        for key in KEYS {
            assert_eq!(
                About::parse(&without(key)),
                Err(format!("{key} is missing"))
            );
        }
        assert!(About::parse(&format!("{LINES}name=Other\n")).is_err());
        assert!(About::parse(&format!("{LINES}colour=blue\n")).is_err());
        assert!(About::parse(&format!("{LINES}just words\n")).is_err());
        let path = LINES.replace("msix=Mujina", r"msix=..\..\Mujina");
        assert!(About::parse(&path).is_err());
        assert!(About::parse(&LINES.replace(".msix", ".exe")).is_err());
        assert!(About::parse(&LINES.replace("0.27.0.0\r", "0.27\r")).is_err());
    }
}

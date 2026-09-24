//! Puts `packaging/mujina.ico` into an executable. Included by the build scripts of the crates
//! that build one: it writes a compiled resource file (`.res`) with the icon, which the MSVC
//! linker takes like an object file, so no resource compiler is needed.

use std::path::{Path, PathBuf};

/// `RT_ICON`, one image of the icon.
const IMAGE: u16 = 3;
/// `RT_GROUP_ICON`, the directory of the images; the first group is the executable's icon.
const GROUP: u16 = 14;

/// Embeds `ico` into this crate's executables.
///
/// # Panics
///
/// If the icon cannot be read or is not one; the build should fail then.
pub fn embed(ico: &Path) {
    println!("cargo:rerun-if-changed={}", ico.display());
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let icon = match std::fs::read(ico) {
        Ok(icon) => icon,
        Err(error) => panic!("{}: {error}", ico.display()),
    };
    let Some(compiled) = resources(&icon) else {
        panic!("{}: not an icon", ico.display());
    };
    let Some(out) = std::env::var_os("OUT_DIR") else {
        panic!("OUT_DIR is not set");
    };
    let path = PathBuf::from(out).join("mujina-icon.res");
    if let Err(error) = std::fs::write(&path, compiled) {
        panic!("{}: {error}", path.display());
    }
    println!("cargo:rustc-link-arg-bins={}", path.display());
}

/// The `.res` file for an `.ico` file: every image as an icon resource, and their directory.
fn resources(icon: &[u8]) -> Option<Vec<u8>> {
    let count = usize::from(u16::from_le_bytes([*icon.get(4)?, *icon.get(5)?]));
    let mut out = Vec::new();
    // A resource file starts with an empty entry.
    resource(&mut out, 0, 0, 0, &[]);
    let mut directory = icon.get(..6)?.to_vec();
    for index in 0..count {
        let entry = icon.get(6 + 16 * index..6 + 16 * (index + 1))?;
        let size = u32::from_le_bytes(entry.get(8..12)?.try_into().ok()?);
        let offset = u32::from_le_bytes(entry.get(12..16)?.try_into().ok()?);
        let start = usize::try_from(offset).ok()?;
        let image = icon.get(start..start + usize::try_from(size).ok()?)?;
        let id = u16::try_from(index + 1).ok()?;
        resource(&mut out, IMAGE, id, 0x1010, image);
        // The directory entry is the file's, with the resource id in place of the offset.
        directory.extend_from_slice(entry.get(..12)?);
        directory.extend_from_slice(&id.to_le_bytes());
    }
    resource(&mut out, GROUP, 1, 0x1030, &directory);
    Some(out)
}

/// One resource: its header (numeric type and name, neutral language), its data, padding.
fn resource(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for part in [0xFFFF, kind, 0xFFFF, name] {
        out.extend_from_slice(&u16::to_le_bytes(part));
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

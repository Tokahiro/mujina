//! Where Mujina keeps its own files.

use std::path::PathBuf;

use mujina_winutil::package;

/// The copy of Mujina Setup kept for the package of `family`, run by the sign-in check and by
/// "Remove Mujina". Outside the package so that it outlives it; one per family so that a CI build
/// beside a release removes only itself. `None` without `LOCALAPPDATA`; the file may not exist.
pub fn retained_setup(family: &str) -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("Mujina")
            .join(family)
            .join("mujina-setup.exe"),
    )
}

/// The directory for logs and configuration.
///
/// Packaged: the package's `LocalState` folder, which Windows removes on uninstall. Unpackaged
/// (development): next to the executable.
pub fn data_dir() -> PathBuf {
    if let (Some(family), Some(local)) = (package::family_name(), std::env::var_os("LOCALAPPDATA"))
    {
        return PathBuf::from(local)
            .join("Packages")
            .join(family)
            .join("LocalState");
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

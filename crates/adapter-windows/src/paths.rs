//! Where Mujina keeps its own files.

use std::path::PathBuf;

use mujina_winutil::package;

/// Mujina Setup's copy for `family`, kept outside the package to outlive it, and per family so a
/// CI build beside a release removes only itself. `None` without `LOCALAPPDATA`; may not exist.
pub fn retained_setup(family: &str) -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("Mujina")
            .join(family)
            .join("mujina-setup.exe"),
    )
}

/// For logs and configuration: the package's `LocalState` folder, which Windows removes on
/// uninstall, or next to the executable when unpackaged (development).
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

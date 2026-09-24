//! Where a launcher is installed, from its executable.

use std::path::{Path, PathBuf};

use mujina_application::ports::{LauncherInstall, PortError, PortResult};

/// The launcher runs in the executable's directory.
pub fn install_from_executable(executable: PathBuf) -> PortResult<LauncherInstall> {
    if !executable.is_file() {
        return Err(PortError::NotFound(format!(
            "{} does not exist",
            executable.display()
        )));
    }
    let directory = executable
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Ok(LauncherInstall {
        executable,
        directory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_runs_where_its_executable_lies() {
        let executable = std::env::current_exe().unwrap();
        let install = install_from_executable(executable.clone()).unwrap();
        assert_eq!(install.directory, executable.parent().unwrap());
        assert_eq!(install.executable, executable);
    }

    #[test]
    fn a_missing_executable_is_reported_by_its_path() {
        let executable = std::env::temp_dir()
            .join("mujina-no-such-launcher")
            .join("launcher.exe");
        assert_eq!(
            install_from_executable(executable.clone()),
            Err(PortError::NotFound(format!(
                "{} does not exist",
                executable.display()
            )))
        );
    }

    #[test]
    fn a_directory_is_no_executable() {
        let error = install_from_executable(std::env::temp_dir()).unwrap_err();
        assert!(matches!(error, PortError::NotFound(_)));
    }
}

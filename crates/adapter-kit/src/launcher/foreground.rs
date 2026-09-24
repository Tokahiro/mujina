//! Starting a launcher and bringing its windows to the front.

use std::ffi::OsStr;

use mujina_application::ports::{LauncherInstall, PortError, PortResult};
use mujina_winutil::window::{self, Focused, WindowHandle};

/// Starts the launcher with `args` and hands it the foreground right, so its window may come to
/// the front by itself.
pub fn launch<S: AsRef<OsStr>>(install: &LauncherInstall, args: &[S]) -> PortResult<()> {
    match window::spawn_with_foreground(&install.executable, args, &install.directory) {
        Ok(handed_over) => {
            if !handed_over {
                log::debug!("could not pass on the foreground right (we do not hold it)");
            }
            Ok(())
        }
        Err(error) => Err(PortError::Failed(format!(
            "{}: {error}",
            command_line(install, args)
        ))),
    }
}

fn command_line<S: AsRef<OsStr>>(install: &LauncherInstall, args: &[S]) -> String {
    let mut line = install.executable.display().to_string();
    for arg in args {
        line.push(' ');
        line.push_str(&arg.as_ref().to_string_lossy());
    }
    line
}

/// Brings the launcher's UI to the front; `what` names it in the log.
pub fn focus_ui(handle: WindowHandle, what: &str) -> PortResult<()> {
    match window::focus_with_fallbacks(handle) {
        Ok(Focused::Directly | Focused::BehindLockScreen) => Ok(()),
        Ok(Focused::AfterKeyTap) => {
            log::info!("brought {what} to the front after a synthetic key tap");
            Ok(())
        }
        Err(in_front) => Err(refused(&in_front)),
    }
}

/// Brings a game's window back to the front. Home role only: the synthetic key tap this may need
/// passes through the agent's own keyboard hook, whose thread would be the one waiting here.
pub fn focus_game(handle: WindowHandle) -> PortResult<()> {
    if window::claim_foreground(handle) {
        Ok(())
    } else {
        Err(refused(&window::describe_foreground()))
    }
}

/// Naming what is in front tells a harmless refusal from a real one.
fn refused(in_front: &str) -> PortError {
    PortError::Failed(format!(
        "Windows refused the foreground change; in front: {in_front}"
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn install(executable: PathBuf) -> LauncherInstall {
        LauncherInstall {
            directory: std::env::temp_dir(),
            executable,
        }
    }

    #[test]
    fn the_command_line_is_the_executable_and_its_arguments() {
        let steam = install(PathBuf::from(r"C:\Steam\steam.exe"));
        assert_eq!(
            command_line(&steam, &["-gamepadui"]),
            r"C:\Steam\steam.exe -gamepadui"
        );
        let frontend = install(PathBuf::from(r"C:\Frontend\frontend.exe"));
        assert_eq!(
            command_line(&frontend, &["--fullscreen", "--profile=couch"]),
            r"C:\Frontend\frontend.exe --fullscreen --profile=couch"
        );
        assert_eq!(
            command_line::<&str>(&frontend, &[]),
            r"C:\Frontend\frontend.exe"
        );
    }

    #[test]
    fn a_failed_start_names_the_command_line() {
        let executable = std::env::temp_dir().join("mujina-no-such-launcher.exe");
        let error = launch(&install(executable.clone()), &["-gamepadui"]).unwrap_err();
        let PortError::Failed(message) = error else {
            panic!("not a failure: {error:?}");
        };
        let expected = format!("{} -gamepadui: ", executable.display());
        assert!(message.starts_with(&expected), "{message}");
    }

    #[test]
    fn the_refusal_names_what_is_in_front() {
        assert_eq!(
            refused("explorer.exe (class \"Shell_TrayWnd\", title \"\")"),
            PortError::Failed(
                "Windows refused the foreground change; in front: explorer.exe (class \
                 \"Shell_TrayWnd\", title \"\")"
                    .to_string()
            )
        );
    }
}

//! Having Windows activate the home role.

use mujina_application::ports::{HomeActivator, PortError, PortResult};
use mujina_winutil::shell;

/// The URI scheme registered in the package manifest, with what the home role is to show.
const HOME_URI: &str = "mujina://home";
const GAME_URI: &str = "mujina://game";

/// Activates the home role through the package's own URI scheme. The shell performs the
/// activation, so the new process gets the standing the agent does not have: in the console
/// experience the shell lets the home app it just activated decide what is in front, and takes
/// the foreground back from anything a background process brought forward.
#[derive(Debug, Default)]
pub struct ProtocolHomeActivator;

fn open(uri: &str) -> PortResult<()> {
    if shell::open(uri) {
        log::info!("requested activation through {uri}");
        Ok(())
    } else {
        log::warn!("activation through {uri} failed (not installed as package?)");
        Err(PortError::Failed(format!("could not open {uri}")))
    }
}

impl HomeActivator for ProtocolHomeActivator {
    fn activate_home(&self) -> PortResult<()> {
        open(HOME_URI)
    }

    fn activate_game(&self) -> PortResult<()> {
        open(GAME_URI)
    }
}

//! Entry point of the home role: what runs when Windows activates Mujina as the console home.

use std::process::ExitCode;

use log::LevelFilter;
use mujina_adapter_windows::agent_control::{self, HomeInstance};
use mujina_adapter_windows::fse::WindowsFse;
use mujina_adapter_windows::launch_screen::WindowsLaunchScreen;
use mujina_adapter_windows::{log_file, paths};
use mujina_application::home::HomeActivation;
use mujina_application::ports::{FseState, FullScreenExperience};
use mujina_domain::activation::{ActivationSource, HomeDestination};

use crate::compose::{Adapters, Role};

pub fn run() -> ExitCode {
    // Without a log there is still a launcher to start, so a logging failure is not fatal.
    let _ = log_file::init(&paths::data_dir(), "home", LevelFilter::Info);

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let argument = arguments.first().map(String::as_str);
    // Any program or web page can open `mujina:`, so only what Windows passes is taken as it
    // comes. Checked before the home turn is claimed, so an ignored activation never holds one up.
    let source = ActivationSource::of(argument);
    if source == ActivationSource::Anyone {
        let console = WindowsFse::bind().state() == FseState::Active;
        if !source.accepts(agent_control::agent_is_running(), console) {
            // Whoever asked chose the text, so only its beginning goes into the log.
            let shown: String = argument.unwrap_or_default().chars().take(80).collect();
            log::warn!(
                "ignored activation {shown:?}: the agent is not running and Xbox mode is off"
            );
            return ExitCode::SUCCESS;
        }
    }

    // Windows may activate the home app several times in a row; only one at a time brings the
    // launcher up.
    let Some(_turn) = HomeInstance::claim_home() else {
        log::info!("activated while another activation is still bringing the launcher up");
        return ExitCode::SUCCESS;
    };

    let adapters = Adapters::new(Role::Home);
    log::info!(
        "activated: version {}, full screen experience {:?}, arguments {arguments:?}",
        env!("CARGO_PKG_VERSION"),
        adapters.fse.state(),
    );
    // Windows says which page it wants: `windows.gaming:///home`, `windows.gaming:///library`.
    let destination = argument.map_or(HomeDestination::Home, HomeDestination::from_activation);

    // Always a window (Windows wants to see one), black only on request.
    let screen = if adapters.settings.settings.launch_screen {
        log::info!("black launch screen switched on in the configuration");
        WindowsLaunchScreen::black()
    } else {
        WindowsLaunchScreen::invisible()
    };
    let activation = HomeActivation::new(adapters.launcher.as_ref(), &adapters.agent_control)
        .with_screen(&screen)
        .showing(destination);
    match activation.run() {
        Ok(report) => {
            for warning in &report.warnings {
                log::warn!("{warning}");
            }
            log::info!(
                "{}: {:?}, showing {destination:?}",
                adapters.launcher.display_name(),
                report.outcome
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            log::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

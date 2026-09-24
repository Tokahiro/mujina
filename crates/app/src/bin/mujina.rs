//! `mujina.exe`: the console home, or with `agent [--standalone]` the resident agent.
//! Built for the GUI subsystem so that an activation never flashes a console window.

#![windows_subsystem = "windows"]

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    use mujina_adapter_windows::agent_control::AGENT_ARGUMENT;

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|argument| argument == AGENT_ARGUMENT)
    {
        let standalone = arguments.iter().any(|argument| argument == "--standalone");
        mujina_app::agent::run(standalone)
    } else {
        mujina_app::home::run()
    }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("Mujina only runs on Windows.");
    ExitCode::FAILURE
}

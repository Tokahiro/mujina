//! `mujinactl.exe`: console companion for setup and diagnosis.

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    mujina_app::ctl::run()
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("Mujina only runs on Windows.");
    ExitCode::FAILURE
}

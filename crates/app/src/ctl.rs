//! `mujinactl`: the console companion for setup and diagnosis.

use std::process::ExitCode;

use mujina_adapter_windows::paths;
use mujina_application::doctor::{self, Severity};
use mujina_application::register::{RegisterOutcome, UnregisterOutcome};
use mujina_application::settings::schema;
use mujina_application::settings::{SettingChange, SettingValue, SettingsSource};

use crate::compose::{self, Adapters, Role};
use crate::{registry, tool};

const USAGE: &str = "\
mujinactl - set up and diagnose Mujina

USAGE:
    mujinactl <COMMAND>

COMMANDS:
    doctor        Check everything Mujina depends on (also written to doctor.txt)
    probe         Show what the keyboard sees for 30 s, to find out what a device button sends
                  (--seconds N to change the duration; also written to probe.txt)
    capture       Wait up to 10 s (--seconds N) for one key combination and print it, e.g.
                  'LWIN+D injected'; keys are held back from other programs meanwhile, except
                  those sent by a program with administrator rights
    register      Make Mujina the Xbox mode home app, remembering the previous one
    unregister    Give the home app setting back to the previous app
    config show   Show config.toml and what Mujina made of it
    config set <section.name> <value>
                  Change one setting, e.g. `config set timing.key_hold_ms 60`; refused,
                  with the reason, if Mujina would ignore it. A running agent applies it
                  at once, except the launcher and what it was started with (it says so)
    config unset <section.name>
                  Back to the default (the line is commented out again); also removes a
                  setting that no longer exists
    paths         Show where Mujina keeps its log and configuration
    help          Show this text
    version       Show the version
";

enum CommandLine {
    Doctor,
    Probe { seconds: u64 },
    Capture { seconds: u64 },
    Register,
    Unregister,
    ConfigShow,
    ConfigSet { key: String, value: String },
    ConfigUnset { key: String },
    Paths,
    Help,
    Version,
}

fn parse() -> Result<CommandLine, lexopt::Error> {
    use lexopt::prelude::*;

    let mut parser = lexopt::Parser::from_env();
    let mut words = Vec::new();
    let mut seconds = None;
    while let Some(argument) = parser.next()? {
        match argument {
            Long("seconds") => seconds = Some(parser.value()?.parse()?),
            Short('h') | Long("help") => return Ok(CommandLine::Help),
            Short('V') | Long("version") => return Ok(CommandLine::Version),
            Value(value) => words.push(value.string()?),
            other => return Err(other.unexpected()),
        }
    }
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    Ok(match words.as_slice() {
        [] | ["help"] => CommandLine::Help,
        ["doctor"] => CommandLine::Doctor,
        ["probe"] => CommandLine::Probe {
            seconds: seconds.unwrap_or(30),
        },
        ["capture"] => CommandLine::Capture {
            seconds: seconds.unwrap_or(10),
        },
        ["register"] => CommandLine::Register,
        ["unregister"] => CommandLine::Unregister,
        ["config"] | ["config", "show"] => CommandLine::ConfigShow,
        ["config", "set", key, value] => CommandLine::ConfigSet {
            key: (*key).to_string(),
            value: (*value).to_string(),
        },
        ["config", "unset", key] => CommandLine::ConfigUnset {
            key: (*key).to_string(),
        },
        ["paths"] => CommandLine::Paths,
        ["version"] => CommandLine::Version,
        _ => {
            return Err(lexopt::Error::from(format!(
                "unexpected command: {}",
                words.join(" ")
            )));
        }
    })
}

pub fn run() -> ExitCode {
    let command = match parse() {
        Ok(command) => command,
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match command {
        CommandLine::Help => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        CommandLine::Version => {
            println!("mujinactl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        CommandLine::Paths => {
            println!("{}", paths::data_dir().display());
            ExitCode::SUCCESS
        }
        CommandLine::Doctor => run_doctor(&Adapters::new(Role::Tool)),
        CommandLine::Probe { seconds } => crate::probe::run(seconds),
        CommandLine::Capture { seconds } => crate::capture::run(seconds),
        CommandLine::Register => register(),
        CommandLine::Unregister => unregister(),
        CommandLine::ConfigShow => config_show(),
        CommandLine::ConfigSet { key, value } => match mujina_adapter_config::parse_value(&value) {
            Ok(value) => config_apply(&SettingChange::set(&key, value)),
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::FAILURE
            }
        },
        CommandLine::ConfigUnset { key } => config_apply(&SettingChange::unset(&key)),
    }
}

fn config_show() -> ExitCode {
    let config = compose::config();
    config.ensure_template();
    println!("{}\n", config.path().display());
    match std::fs::read_to_string(config.path()) {
        Ok(text) => print!("{text}"),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    }
    let notes = config.load().notes;
    if !notes.is_empty() {
        println!();
    }
    for note in notes {
        println!("note: {note}");
    }
    ExitCode::SUCCESS
}

fn config_apply(change: &SettingChange) -> ExitCode {
    let changes = std::slice::from_ref(change);
    // Whether config.toml held the key at all before this change.
    let was_stored = compose::config().snapshot().stored(&change.key).is_some();
    match tool::change(changes) {
        Ok(applied) => {
            let (launchers, devices) = (registry::launchers(), registry::devices());
            match &change.value {
                Some(_) => println!("{} set.", change.key),
                // No setting any more, so it has no default to go back to.
                None if schema::find(&change.key, launchers.all, devices.all).is_none() => {
                    // A key that was never in the file is most likely a typo: nothing changed,
                    // so nothing takes effect either.
                    if !was_stored {
                        println!("{} was not set; nothing changed.", change.key);
                        return ExitCode::SUCCESS;
                    }
                    println!("{} removed.", change.key);
                }
                None => println!("{} is back to its default.", change.key),
            }
            match applied {
                tool::Applied::Now => println!("Applied at once."),
                tool::Applied::NextSession => {
                    println!("Takes effect the next time Xbox mode is entered.");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_doctor(adapters: &Adapters) -> ExitCode {
    let findings = tool::examine(adapters);
    let mut report = doctor::render(&findings);
    report.push_str(&describe_configuration(adapters));
    print!("{report}");

    // The device has no keyboard; a file can be opened with a tap.
    let path = paths::data_dir().join("doctor.txt");
    if let Err(error) = std::fs::write(&path, &report) {
        eprintln!("could not write {}: {error}", path.display());
    }

    if findings
        .iter()
        .any(|finding| finding.severity == Severity::Problem)
    {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The device strings a profile is matched against, and what the configuration resolved to.
pub fn describe_configuration(adapters: &Adapters) -> String {
    let settings = &adapters.settings.settings;
    let device = settings
        .device
        .id
        .as_deref()
        .and_then(|id| registry::devices().find(id));
    let button = match (device, settings.button_remap) {
        (Some(device), true) => {
            let keys = registry::button_keys(&settings.device);
            if keys.is_empty() {
                device.name()
            } else {
                format!("{} ({keys})", device.name())
            }
        }
        (Some(_), false) => "switched off".to_string(),
        (None, _) => "no profile for this device (see docs/new-device.md)".to_string(),
    };
    let mut text = format!(
        "\ndevice:   \"{}\" / \"{}\"\nbutton:   {button}\nlauncher: {}\nconfig:   {}\n",
        adapters.system.manufacturer,
        adapters.system.product,
        describe_launcher(adapters),
        paths::data_dir().join("config.toml").display()
    );
    for note in &adapters.settings.notes {
        text.push_str("note:     ");
        text.push_str(note);
        text.push('\n');
    }
    text
}

/// The launcher in use and what its options come to, defaults included: `Steam Big Picture:
/// ui_link = false, wifi_indicator = true (inactive: needs ui_link)`.
fn describe_launcher(adapters: &Adapters) -> String {
    let options = &adapters.settings.settings.launcher.options;
    let specs = adapters.plugin.descriptor.settings();
    let values: Vec<String> = specs
        .iter()
        .filter_map(|spec| {
            let value = spec.value_in(options)?;
            // Set, yet without the toggle it needs it does nothing.
            let inactive = spec
                .requires
                .filter(|needed| !schema::flag(specs, options, needed))
                .map(|needed| format!(" (inactive: needs {needed})"))
                .unwrap_or_default();
            Some(format!("{} = {}{inactive}", spec.key, as_toml(&value)))
        })
        .collect();
    let name = adapters.launcher.display_name();
    if values.is_empty() {
        name
    } else {
        format!("{name}: {}", values.join(", "))
    }
}

/// A value as `config.toml` would spell it.
fn as_toml(value: &SettingValue) -> String {
    match value {
        SettingValue::Bool(flag) => flag.to_string(),
        SettingValue::Integer(number) => number.to_string(),
        SettingValue::Text(text) => format!("{text:?}"),
        SettingValue::TextList(items) => format!("{items:?}"),
    }
}

fn register() -> ExitCode {
    match tool::make_home_app() {
        Ok(RegisterOutcome::Registered) => println!("Mujina is now the home app."),
        Ok(RegisterOutcome::AlreadyRegistered) => println!("Mujina already is the home app."),
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn unregister() -> ExitCode {
    match tool::give_home_app_back() {
        Ok(UnregisterOutcome::Restored(previous)) => println!("Home app restored to {previous}."),
        Ok(UnregisterOutcome::Cleared) => println!("Home app setting removed."),
        Ok(UnregisterOutcome::NotRegistered) => {
            println!("Mujina is not the home app; nothing changed.");
        }
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

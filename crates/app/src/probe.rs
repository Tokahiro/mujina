//! `mujinactl probe`: shows what the keyboard sees, so that the chord behind a device button can
//! be read off and turned into a `[device.button]` entry.

use std::fmt::Write as _;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use mujina_adapter_keyboard::probe::{self, Keys, Observation};
use mujina_adapter_windows::paths;
use mujina_domain::chord::{Direction, KeyEvent, Origin, suggest_trigger};

fn describe(observation: Observation, since: Instant) -> String {
    let KeyEvent {
        key,
        direction,
        origin,
        ..
    } = observation.event;
    format!(
        "{:>7.3}s  {:<4}  {:<13} code 0x{:02X}  scan 0x{:02X}  {}",
        since.elapsed().as_secs_f64(),
        match direction {
            Direction::Down => "down",
            Direction::Up => "up",
        },
        key.to_string(),
        key.0,
        observation.scan_code,
        match origin {
            Origin::Physical => "keyboard",
            Origin::Injected => "injected by a program",
            Origin::Own => "sent by Mujina",
            Origin::Replayed => "sent on by Mujina",
        }
    )
}

pub fn run(seconds: u64) -> ExitCode {
    println!(
        "Watching the keyboard for {seconds} s. Press the device button a few times now.\n\
         Nothing is blocked or changed; keys reach their programs as usual.\n"
    );

    let started = Instant::now();
    let mut transcript = String::new();
    let seen = probe::observe(
        Duration::from_secs(seconds),
        Keys::PassOn,
        &mut |observation| {
            let line = describe(*observation, started);
            println!("{line}");
            // Writing into a String cannot fail.
            let _ = writeln!(transcript, "{line}");
            true
        },
    );
    let Some(seen) = seen else {
        eprintln!("error: the keyboard hook could not be installed");
        return ExitCode::FAILURE;
    };

    let events: Vec<KeyEvent> = seen.iter().map(|observation| observation.event).collect();
    let config = paths::data_dir().join("config.toml");
    let verdict = match suggest_trigger(&events) {
        Some(button) => match button.keys.keys() {
            [modifier, key] => format!(
                "\nThe first chord seen was {modifier}+{key}. If that was the button, add this to\n\
                 {config}:\n\n\
                 [device.button]\n\
                 modifier = \"{modifier}\"\n\
                 key = \"{key}\"\n\
                 injected_only = {injected}\n",
                injected = button.injected_only,
                config = config.display(),
            ),
            _ => format!(
                "\nThe first chord seen was {chord}, more keys than [device.button] takes. A \
                 device profile can have it (see docs/new-device.md):\n\n\
                 [[buttons]]\n\
                 key = \"button\"\n\
                 label = \"...\"\n\
                 chord = \"{chord}\"\n\
                 injected_only = {injected}\n",
                chord = button.keys,
                injected = button.injected_only,
            ),
        },
        None if seen.is_empty() => {
            "\nNo key events at all. The button is not reported as a keyboard key; it \
             needs a device adapter of its own (see docs/new-device.md).\n"
                .to_string()
        }
        None => "\nOnly single keys were seen, no chord (a key pressed while another is held). \
             Mujina maps chords; see docs/new-device.md.\n"
            .to_string(),
    };
    print!("{verdict}");
    transcript.push_str(&verdict);

    let path = paths::data_dir().join("probe.txt");
    match std::fs::write(&path, &transcript) {
        Ok(()) => println!("\nAlso written to {}", path.display()),
        Err(error) => eprintln!("could not write {}: {error}", path.display()),
    }
    ExitCode::SUCCESS
}

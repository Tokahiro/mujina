//! `mujinactl capture`: waits for one key combination, such as the device button, and prints it.
//! Keys are held back from other programs meanwhile, except what a program with administrator
//! rights sends (OneXConsole), which Windows delivers anyway.
//!
//! Mujina Settings runs it with [`watch`]: a hook on a thread of the app got no key events on a
//! device where this console program's hook did.

use std::io::Read as _;
use std::os::windows::process::CommandExt as _;
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mujina_adapter_keyboard::probe::{self, Keys};
use mujina_domain::chord::{Direction, KeyEvent, Origin, TriggerChord, suggest_trigger};
use mujina_domain::keys::VirtualKey;

#[derive(Debug, Default)]
struct Capture {
    events: Vec<KeyEvent>,
    held: Vec<VirtualKey>,
}

impl Capture {
    /// Takes one event; `false` once a chord was seen and all its keys are up again, so that no
    /// key release reaches other programs without its press. Mujina's own keys are no part of it.
    fn see(&mut self, event: KeyEvent) -> bool {
        if matches!(event.origin, Origin::Own | Origin::Replayed) {
            return true;
        }
        match event.direction {
            Direction::Down if !self.held.contains(&event.key) => self.held.push(event.key),
            Direction::Down => {}
            Direction::Up => self.held.retain(|key| *key != event.key),
        }
        self.events.push(event);
        !(self.held.is_empty() && suggest_trigger(&self.events).is_some())
    }
}

/// How a captured chord is printed, one line: `LWIN+D injected` or `LCTRL+LWIN+LALT physical`,
/// every key held at once in the order pressed.
pub fn line(button: TriggerChord) -> String {
    let origin = if button.injected_only {
        "injected"
    } else {
        "physical"
    };
    format!("{} {origin}", button.keys)
}

/// A chord as [`line()`] prints it; `None` for anything else.
pub fn parse(line: &str) -> Option<TriggerChord> {
    let (chord, origin) = line.split_once(' ')?;
    let injected_only = match origin {
        "injected" => true,
        "physical" => false,
        _ => return None,
    };
    TriggerChord::parse(chord, injected_only).ok()
}

/// Runs `mujinactl capture` next to the running program, in a process of its own without a
/// window, for `time`, and reads its line: the chord, `Ok(None)` if none was pressed in time or
/// `cancel` was set meanwhile, `Err` with what went wrong otherwise.
pub fn watch(time: Duration, cancel: &AtomicBool) -> Result<Option<TriggerChord>, String> {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    /// How often a waiting capture looks whether it was cancelled.
    const POLL: Duration = Duration::from_millis(50);

    let program = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .with_file_name("mujinactl.exe");
    let mut child = Command::new(&program)
        .args(["capture", "--seconds", &time.as_secs().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("{}: {error}", program.display()))?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            // Ending the watcher also ends its hold on the keyboard.
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => return Err(error.to_string()),
        }
    };
    // One line each at most, read once the watcher is done.
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        let _ = pipe.read_to_string(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    match status.code() {
        Some(0) => parse(stdout.trim())
            .map(Some)
            .ok_or_else(|| format!("mujinactl capture printed \"{}\"", stdout.trim())),
        Some(1) => Ok(None),
        _ => Err(stderr.trim().to_string()),
    }
}

/// Exit code 0 with the chord on standard output, 1 if none was pressed in time, 2 if the
/// keyboard could not be watched.
pub fn run(seconds: u64) -> ExitCode {
    let mut capture = Capture::default();
    let watched = probe::observe(
        Duration::from_secs(seconds),
        Keys::Hold,
        &mut |observation| capture.see(observation.event),
    );
    if watched.is_none() {
        eprintln!("error: the keyboard hook could not be installed");
        return ExitCode::from(2);
    }
    if let Some(button) = suggest_trigger(&capture.events) {
        println!("{}", line(button));
        ExitCode::SUCCESS
    } else {
        println!("none");
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(key: VirtualKey, direction: Direction) -> KeyEvent {
        KeyEvent {
            key,
            direction,
            origin: Origin::Injected,
            scan: mujina_domain::chord::ScanCode::default(),
        }
    }

    #[test]
    fn a_capture_ends_once_the_chord_is_released() {
        let mut capture = Capture::default();
        assert!(capture.see(event(VirtualKey::LWIN, Direction::Down)));
        assert!(capture.see(event(VirtualKey::D, Direction::Down)));
        assert!(
            capture.see(event(VirtualKey::D, Direction::Up)),
            "LWIN still held"
        );
        assert!(!capture.see(event(VirtualKey::LWIN, Direction::Up)));
        let button = suggest_trigger(&capture.events).unwrap();
        assert_eq!(line(button), "LWIN+D injected");
    }

    #[test]
    fn a_chord_of_three_keys_is_captured_whole() {
        let mut capture = Capture::default();
        let keys = [VirtualKey::LCONTROL, VirtualKey::LWIN, VirtualKey::LMENU];
        for key in keys {
            assert!(capture.see(event(key, Direction::Down)));
        }
        assert!(capture.see(event(VirtualKey::LMENU, Direction::Up)));
        assert!(capture.see(event(VirtualKey::LWIN, Direction::Up)));
        assert!(!capture.see(event(VirtualKey::LCONTROL, Direction::Up)));
        let button = suggest_trigger(&capture.events).unwrap();
        assert_eq!(line(button), "LCTRL+LWIN+LALT injected");
    }

    #[test]
    fn the_printed_line_is_read_back() {
        for printed in [
            "LWIN+D injected",
            "LCTRL+F24 physical",
            "LCTRL+LWIN+LALT injected",
        ] {
            let chord = parse(printed).unwrap();
            assert_eq!(line(chord), printed);
        }
        assert!(parse("LWIN+D injected").unwrap().injected_only);
        assert!(!parse("LCTRL+F24 physical").unwrap().injected_only);
        assert_eq!(parse("none"), None);
        assert_eq!(parse("LWIN+NOPE injected"), None);
        assert_eq!(parse("LWIN+D sideways"), None);
    }

    #[test]
    fn single_keys_and_mujinas_own_do_not_end_it() {
        let mut capture = Capture::default();
        assert!(capture.see(event(VirtualKey::D, Direction::Down)));
        assert!(capture.see(event(VirtualKey::D, Direction::Up)));
        let own = KeyEvent {
            origin: Origin::Own,
            ..event(VirtualKey::LWIN, Direction::Down)
        };
        assert!(capture.see(own));
        assert_eq!(capture.events.len(), 2, "Mujina's keys are not part of it");
    }
}

//! The thread that sends keys: the launcher's shortcuts, a button passed on, and what the hook
//! held back and sends on. One for the process, started on first use; blocked on its channel
//! while nothing is to be sent.

use std::collections::VecDeque;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use mujina_domain::chord::{Direction, KeyEvent, Replay};
use mujina_domain::keys::{HoldTiming, KeyChord, VirtualKey};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VK_APPS, VK_CANCEL, VK_DELETE,
    VK_DIVIDE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_NEXT, VK_PACKET, VK_PRIOR,
    VK_RCONTROL, VK_RIGHT, VK_RMENU, VK_RWIN, VK_SNAPSHOT, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

use crate::hook::{WM_REINSTALL_HOOK, WM_REPLAY_LOST};

// Marks keystrokes synthesized by Mujina; defined next to the one other place that sends them.
pub(crate) use mujina_winutil::window::OWN_INPUT_TAG;

/// Marks what the hook held back and sends on ("MUJR"), apart from Mujina's own keystrokes, so
/// that the hook can count it coming back.
pub(crate) const REPLAY_TAG: usize = 0x4D55_4A52;

/// How long after a chord the hook must have seen our own keystrokes.
const HOOK_PROOF_DELAY: Duration = Duration::from_millis(30);

/// Set by the hook whenever it sees keystrokes of ours, either tag.
pub(crate) static SAW_OWN_INPUT: AtomicBool = AtomicBool::new(false);

/// The thread whose hook is installed, 0 while there is none: whom to tell that the hook is gone.
pub(crate) static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);

pub(crate) enum Job {
    /// A chord held as `timing` says: a launcher's shortcut, or a button passed on.
    Chord(KeyChord, HoldTiming),
    /// What the hook held back, sent on at once and in one go; the hook thread `from` hears how
    /// much of it Windows refused. Boxed, being far larger than a chord; made outside the hook
    /// callback, which must not allocate.
    Replay { events: Box<Replay>, from: u32 },
}

/// The channel to the sender thread, which is started the first time it is asked for; `None`
/// if it could not be started.
fn jobs() -> Option<&'static Sender<Job>> {
    static JOBS: OnceLock<Option<Sender<Job>>> = OnceLock::new();
    JOBS.get_or_init(|| {
        let (jobs, receiver) = channel();
        thread::Builder::new()
            .name("sender".into())
            .spawn(move || run(&receiver))
            .inspect_err(|error| log::error!("the sender thread could not be started: {error}"))
            .ok()
            .map(|_| jobs)
    })
    .as_ref()
}

/// Starts the sender thread if it is not running yet; whether it runs.
pub(crate) fn start() -> bool {
    jobs().is_some()
}

/// Hands `job` to the sender thread; `false` when there is none.
pub(crate) fn send(job: Job) -> bool {
    // The thread lives as long as the process; a failed send means there is none.
    jobs().is_some_and(|jobs| jobs.send(job).is_ok())
}

/// Whether `key` has an extended scan code (`0xE0` first), which a key Mujina sends of its own
/// has to say: the right Alt and Ctrl, Insert, Delete, Home, End, Page Up, Page Down, the arrows,
/// Break, Print Screen, the keypad's Divide, and the Windows and Application keys
/// ([Keyboard Input Overview, Extended-Key Flag](https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input#extended-key-flag)).
/// Num Lock is not one, the same page says. The keypad's Enter is, but has the main Enter's
/// virtual key, so it cannot be told from it here; a key held back keeps the flag it came with.
fn is_extended(key: VirtualKey) -> bool {
    matches!(
        key.0,
        VK_RCONTROL
            | VK_RMENU
            | VK_INSERT
            | VK_DELETE
            | VK_HOME
            | VK_END
            | VK_PRIOR
            | VK_NEXT
            | VK_LEFT
            | VK_UP
            | VK_RIGHT
            | VK_DOWN
            | VK_CANCEL
            | VK_SNAPSHOT
            | VK_DIVIDE
            | VK_LWIN
            | VK_RWIN
            | VK_APPS
    )
}

fn keyboard_input(key: u16, scan: u16, flags: u32, tag: usize) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: tag,
            },
        },
    }
}

/// One key event of Mujina's own, tagged as `tag` says.
fn key_input(key: VirtualKey, up: bool, tag: usize) -> INPUT {
    // SAFETY: plain call; an unknown key yields scan code 0.
    let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC) };
    let mut flags = if up { KEYEVENTF_KEYUP } else { 0 };
    if is_extended(key) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    // Some input stacks read the scan code rather than the virtual key.
    keyboard_input(key.0, u16::try_from(scan).unwrap_or(0), flags, tag)
}

/// A key event the hook held back, sent on as it came: with its own scan code and extended flag,
/// which programs that read scan codes go by. A character a program typed as such (`VK_PACKET`,
/// as the touch keyboard does; SendInput, Remarks) goes on as that character: `KEYEVENTF_UNICODE`
/// with the virtual key 0 and the UTF-16 unit as the scan code
/// ([KEYBDINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput)).
/// The hook reports that unit as the event's scan code, which the hook's own page does not say;
/// checked on Windows 11 (build 26200) with a scratch hook, which saw `VK_PACKET` with the
/// character as its scan code, and the extended flag as sent.
fn replay_input(event: KeyEvent) -> INPUT {
    let mut flags = if event.direction == Direction::Up {
        KEYEVENTF_KEYUP
    } else {
        0
    };
    if event.key.0 == VK_PACKET {
        return keyboard_input(0, event.scan.code, flags | KEYEVENTF_UNICODE, REPLAY_TAG);
    }
    if event.scan.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    keyboard_input(event.key.0, event.scan.code, flags, REPLAY_TAG)
}

/// Puts `inputs` into the input stream in one go, which nothing else is interspersed with
/// (SendInput, Remarks); how many went in.
fn inject(inputs: &[INPUT]) -> usize {
    let (Ok(count), Ok(size)) = (
        u32::try_from(inputs.len()),
        i32::try_from(size_of::<INPUT>()),
    ) else {
        return 0;
    };
    // SAFETY: `inputs` holds `count` valid INPUT structures of the stated size.
    let sent = unsafe { SendInput(count, inputs.as_ptr(), size) };
    usize::try_from(sent).unwrap_or(0)
}

/// Presses or releases one key of ours; whether Windows took it.
fn press(key: VirtualKey, up: bool) -> bool {
    let sent = inject(&[key_input(key, up, OWN_INPUT_TAG)]) == 1;
    if !sent {
        log::warn!("SendInput was blocked (elevated window in front?)");
    }
    sent
}

fn run(jobs: &Receiver<Job>) {
    // Chords that came while the hook's proof of life was awaited, oldest first.
    let mut waiting = VecDeque::new();
    loop {
        let job = match waiting.pop_front() {
            Some(job) => job,
            None => match jobs.recv() {
                Ok(job) => job,
                Err(_) => return,
            },
        };
        match job {
            Job::Chord(chord, timing) => {
                // On this thread, never in the hook: describing a window may take a moment.
                if log::log_enabled!(log::Level::Debug) {
                    log::debug!(
                        "sending {chord} to {}; input {}",
                        mujina_winutil::window::describe_foreground(),
                        mujina_winutil::window::describe_input_focus()
                    );
                }
                SAW_OWN_INPUT.store(false, Ordering::Relaxed);
                if hold(chord, timing) {
                    verify_hook(jobs, &mut waiting);
                }
            }
            Job::Replay { events, from } => replay(&events, from),
        }
    }
}

/// Sends on what the hook held back, in one go; tells the hook thread `from` how much of it
/// Windows refused, since that will not come back through the hook.
fn replay(events: &Replay, from: u32) {
    let inputs: Vec<INPUT> = events.events().iter().copied().map(replay_input).collect();
    let lost = inputs.len().saturating_sub(inject(&inputs));
    if lost > 0 {
        log::warn!("{lost} held-back key events could not be sent on (elevated window in front?)");
        // SAFETY: plain call; a stale thread id makes it fail harmlessly.
        unsafe { PostThreadMessageW(from, WM_REPLAY_LOST, lost, 0) };
    }
}

/// Presses `chord`'s keys in order, holds the last, and releases them in reverse, pausing as
/// `timing` says; whether Windows took every key.
fn hold(chord: KeyChord, timing: HoldTiming) -> bool {
    let keys = chord.keys();
    let Some((&last, modifiers)) = keys.split_last() else {
        return false;
    };
    let gap = Duration::from_millis(u64::from(timing.modifier_gap_ms));
    let mut all = true;
    for &modifier in modifiers {
        all &= press(modifier, false);
        thread::sleep(gap);
    }
    all &= press(last, false);
    thread::sleep(Duration::from_millis(u64::from(timing.key_hold_ms)));
    all &= press(last, true);
    for &modifier in modifiers.iter().rev() {
        thread::sleep(gap);
        all &= press(modifier, true);
    }
    all
}

/// Our own keystrokes pass back through our own hook, so not seeing them means Windows dropped
/// it. Runs only right after a chord that Windows took, so it costs nothing while idle, and only
/// while there is a hook: a device whose buttons come another way has none to prove.
///
/// While it waits, what the hook held back goes out at once: the hook holds back every key that
/// follows until that comes back, and the chord is released by now, so none of its keys can mix
/// in. Another chord waits in `waiting` until the check is done.
fn verify_hook(jobs: &Receiver<Job>, waiting: &mut VecDeque<Job>) {
    if HOOK_THREAD.load(Ordering::Relaxed) == 0 {
        return;
    }
    let deadline = Instant::now() + HOOK_PROOF_DELAY;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match jobs.recv_timeout(left) {
            Ok(Job::Replay { events, from }) => replay(&events, from),
            Ok(chord) => waiting.push_back(chord),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }
    let thread = HOOK_THREAD.load(Ordering::Relaxed);
    if thread != 0 && !SAW_OWN_INPUT.load(Ordering::Relaxed) {
        // SAFETY: plain call; a stale thread id makes it fail harmlessly.
        unsafe { PostThreadMessageW(thread, WM_REINSTALL_HOOK, 0, 0) };
    }
}

#[cfg(test)]
mod tests {
    use mujina_domain::chord::{Origin, ScanCode};

    use super::*;

    fn keyboard(input: &INPUT) -> KEYBDINPUT {
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        // SAFETY: every INPUT built here is a keyboard one, as just checked.
        unsafe { input.Anonymous.ki }
    }

    fn held_back(key: u16, direction: Direction, code: u16, extended: bool) -> KeyEvent {
        KeyEvent {
            key: VirtualKey(key),
            direction,
            origin: Origin::Injected,
            scan: ScanCode { code, extended },
        }
    }

    #[test]
    fn a_key_held_back_goes_on_with_its_own_scan_code_and_extended_flag() {
        // The right arrow: without the flag, a program reading scan codes sees the keypad's 6.
        let right = keyboard(&replay_input(held_back(
            VK_RIGHT,
            Direction::Down,
            0x4D,
            true,
        )));
        assert_eq!(
            (right.wVk, right.wScan, right.dwFlags),
            (VK_RIGHT, 0x4D, KEYEVENTF_EXTENDEDKEY)
        );
        assert_eq!(right.dwExtraInfo, REPLAY_TAG);

        // The keypad's 6 itself keeps its plain scan code.
        let six = keyboard(&replay_input(held_back(0x66, Direction::Up, 0x4D, false)));
        assert_eq!((six.wScan, six.dwFlags), (0x4D, KEYEVENTF_KEYUP));
    }

    #[test]
    fn a_character_typed_as_such_goes_on_as_that_character() {
        let a_umlaut = keyboard(&replay_input(held_back(
            VK_PACKET,
            Direction::Up,
            0x00E4,
            false,
        )));
        assert_eq!(
            (a_umlaut.wVk, a_umlaut.wScan, a_umlaut.dwFlags),
            (0, 0x00E4, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)
        );
    }

    #[test]
    fn keys_of_our_own_say_whether_they_are_extended() {
        let flags = |key: u16| keyboard(&key_input(VirtualKey(key), false, OWN_INPUT_TAG)).dwFlags;
        for key in [
            VK_LWIN,
            VK_RCONTROL,
            VK_RMENU,
            VK_LEFT,
            VK_DELETE,
            VK_INSERT,
        ] {
            assert_eq!(flags(key), KEYEVENTF_EXTENDEDKEY, "{key:#04x}");
        }
        // Num Lock is not, nor is the left Ctrl.
        for key in [0x90, VirtualKey::LCONTROL.0, VirtualKey::D.0] {
            assert_eq!(flags(key), 0, "{key:#04x}");
        }
    }
}

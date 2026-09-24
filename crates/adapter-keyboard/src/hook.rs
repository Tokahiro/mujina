//! The low-level keyboard hook, on a thread of its own.
//!
//! Windows calls a `WH_KEYBOARD_LL` hook "in the context of the thread that installed it … by
//! sending a message to the thread", which "must have a message loop"; a hook that answers slower
//! than `LowLevelHooksTimeout` (at most one second since Windows 10 1709) "is silently removed";
//! and an application "should run the hooks on a dedicated thread that passes the work off to a
//! worker thread and then immediately returns"
//! ([LowLevelKeyboardProc, Remarks](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc)).
//! So this thread does nothing but wait in `GetMessageW`, run the allocation-free matcher, and
//! take messages from the rest of Mujina: new buttons, a replay to hand to the sender, a hook to
//! install again. A press sets a bit and signals an auto-reset event the agent's event loop waits
//! on ([`ButtonSource`]), so however long the agent's main thread takes over something, the hook
//! answers at once.

use std::cell::RefCell;
use std::os::windows::io::{AsHandle, BorrowedHandle};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use mujina_application::agent::AgentEvent;
use mujina_application::device::ButtonId;
use mujina_domain::chord::{
    ChordSetMatcher, Direction, KeyEvent, Origin, REPLAY_GIVE_UP_MS, ScanCode, TriggerChord,
    Verdict,
};
use mujina_domain::keys::VirtualKey;
use mujina_winutil::event::Event;
use mujina_winutil::wait::WaitSource;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    LLKHF_INJECTED, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN, WM_USER,
};

use crate::MAX_BUTTONS;
use crate::sender::{self, HOOK_THREAD, Job, OWN_INPUT_TAG, REPLAY_TAG, SAW_OWN_INPUT};

/// Posted to the hook thread when the hook stopped delivering our own keystrokes.
pub(crate) const WM_REINSTALL_HOOK: u32 = WM_APP + 1;
/// Posted to the hook thread when new buttons wait in [`Shared::next`].
const WM_SET_BUTTONS: u32 = WM_APP + 2;
/// Posted by the hook to its own thread: what it held back waits to be sent on. The callback
/// must not do that itself, and only a posted message ends the thread's `GetMessageW`.
const WM_FLUSH_REPLAY: u32 = WM_APP + 3;
/// Posted by the sender: Windows refused `wParam` events of a replay, which will not come back.
pub(crate) const WM_REPLAY_LOST: u32 = WM_APP + 4;
/// Posted to the hook thread when it is to end.
const WM_STOP: u32 = WM_APP + 5;

/// `HC_ACTION`: the hook code that carries a key event.
const HC_ACTION: i32 = 0;

/// What the hook thread and the rest of Mujina share.
struct Shared {
    /// Signalled on a press; the agent's event loop waits for it.
    fired: Event,
    /// A bit per button pressed since the event loop last looked.
    pressed: AtomicU32,
    /// The buttons to catch, handed over with [`WM_SET_BUTTONS`]. Only the thread's message
    /// handler locks it, never the hook callback.
    next: Mutex<Vec<(ButtonId, TriggerChord)>>,
}

/// The hook thread, as the rest of Mujina holds it. Dropping it ends the thread, which removes
/// its hook.
pub(crate) struct KeyboardHook {
    thread: u32,
    shared: Arc<Shared>,
}

impl KeyboardHook {
    /// Starts the thread, with no buttons and so no hook yet.
    pub(crate) fn spawn() -> Result<Self, String> {
        let fired = Event::new().map_err(|error| error.to_string())?;
        let shared = Arc::new(Shared {
            fired,
            pressed: AtomicU32::new(0),
            next: Mutex::new(Vec::new()),
        });
        let (ready, started) = sync_channel(1);
        let theirs = Arc::clone(&shared);
        thread::Builder::new()
            .name("keyboard-hook".into())
            .spawn(move || run(&theirs, &ready))
            .map_err(|error| format!("the keyboard hook thread could not be started: {error}"))?;
        // Posting to a thread fails until it has a message queue (PostThreadMessageW, Remarks):
        // wait until the thread has made one.
        let thread = started
            .recv()
            .map_err(|_| "the keyboard hook thread ended at once".to_string())?;
        Ok(Self { thread, shared })
    }

    /// Catches `buttons` from now on, none removing the hook; whether the thread was told.
    pub(crate) fn set_buttons(&self, buttons: Vec<(ButtonId, TriggerChord)>) -> bool {
        *self
            .shared
            .next
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = buttons;
        post(self.thread, WM_SET_BUTTONS)
    }

    /// What the agent's event loop waits on for the presses.
    pub(crate) fn source(&self) -> ButtonSource {
        ButtonSource {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        post(self.thread, WM_STOP);
    }
}

fn post(thread: u32, message: u32) -> bool {
    // SAFETY: plain call; a thread that has ended makes it fail harmlessly.
    unsafe { PostThreadMessageW(thread, message, 0, 0) != 0 }
}

/// The presses the hook caught, as the agent's event loop waits for them: one
/// [`AgentEvent::ButtonPressed`] per button.
pub struct ButtonSource {
    shared: Arc<Shared>,
}

impl WaitSource<AgentEvent> for ButtonSource {
    fn name(&self) -> &'static str {
        "device buttons (keyboard)"
    }

    fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
        Some(self.shared.fired.as_handle())
    }

    /// The wait consumed the auto-reset event. Two presses of one button before this runs come
    /// out as one: rare for a button a person presses, and the second would repeat the first.
    fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
        let pressed = self.shared.pressed.swap(0, Ordering::AcqRel);
        for id in 0..32u8 {
            if pressed & (1 << id) != 0 {
                out.push(AgentEvent::ButtonPressed(ButtonId(id)));
            }
        }
    }
}

/// The hook thread's own state; the callback reaches it through the thread-local below, since a
/// low-level hook takes no context.
struct Context {
    matcher: ChordSetMatcher<MAX_BUTTONS>,
    shared: Arc<Shared>,
    thread: u32,
    hook: Option<HHOOK>,
    /// A flush is posted and not yet handled; one is enough.
    flush_posted: bool,
    /// The callback stopped waiting for keys sent on that never came back; the flush says so in
    /// the log, which the callback must not write.
    gave_up: bool,
}

thread_local! {
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

fn run(shared: &Arc<Shared>, ready: &SyncSender<u32>) {
    // SAFETY: MSG is plain data for which all-zero is a valid value.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: `message` is writable. Asking for any message makes Windows create this thread's
    // queue, as PostThreadMessageW's Remarks advise; nothing is taken out.
    unsafe { PeekMessageW(&raw mut message, null_mut(), WM_USER, WM_USER, PM_NOREMOVE) };
    // SAFETY: plain call.
    let thread = unsafe { GetCurrentThreadId() };
    CONTEXT.with(|slot| {
        *slot.borrow_mut() = Some(Context {
            matcher: ChordSetMatcher::new(),
            shared: Arc::clone(shared),
            thread,
            hook: None,
            flush_posted: false,
            gave_up: false,
        });
    });
    if ready.send(thread).is_err() {
        return;
    }
    loop {
        // SAFETY: `message` is writable; a null window means any message of this thread. The
        // hook's calls are delivered in here, as sent messages.
        let got = unsafe { GetMessageW(&raw mut message, null_mut(), 0, 0) };
        // 0 is WM_QUIT; -1 is an error, after which waiting again would spin (GetMessageW,
        // Return value).
        if got == 0 || got == -1 {
            break;
        }
        match message.message {
            WM_SET_BUTTONS => with_context(Context::take_buttons),
            WM_FLUSH_REPLAY => with_context(Context::flush),
            WM_REPLAY_LOST => with_context(|context| {
                context.matcher.replay_lost(message.wParam);
            }),
            WM_REINSTALL_HOOK => with_context(Context::reinstall),
            WM_STOP => break,
            _ => {
                // SAFETY: `message` was filled in by GetMessageW.
                unsafe { DispatchMessageW(&raw const message) };
            }
        }
    }
    with_context(Context::remove_hook);
    CONTEXT.with(|slot| slot.borrow_mut().take());
}

/// Runs `work` on the thread's state, outside the callback. Nothing in here pumps messages, so
/// the callback cannot run meanwhile; if it did, it would find the state busy and let the key
/// through.
fn with_context(work: impl FnOnce(&mut Context)) {
    CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow_mut().as_mut() {
            work(context);
        }
    });
}

impl Context {
    fn take_buttons(&mut self) {
        let buttons = self
            .shared
            .next
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let kept = &buttons[..buttons.len().min(MAX_BUTTONS)];
        if kept.len() < buttons.len() {
            log::warn!(
                "the device has {} buttons; only the first {MAX_BUTTONS} are caught",
                buttons.len()
            );
        }
        // `kept` fits, so this cannot fail.
        let _ = self.matcher.set(kept);
        if kept.is_empty() {
            self.remove_hook();
        } else if self.hook.is_none() {
            self.install_hook();
        }
        // What a chord under way had held back.
        self.flush();
    }

    fn install_hook(&mut self) {
        // SAFETY: `keyboard_proc` matches HOOKPROC and stays valid for the life of the process;
        // the module handle of the executable is what low-level hooks expect, and thread 0 means
        // every thread of the desktop.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_proc),
                GetModuleHandleW(null()),
                0,
            )
        };
        if hook.is_null() {
            let error = std::io::Error::last_os_error();
            log::error!(
                "keyboard hook could not be installed ({error}); the device button stays unmapped"
            );
            return;
        }
        self.hook = Some(hook);
        // What was sent on before cannot come back through a hook that was not there.
        self.matcher.forget_in_flight();
        HOOK_THREAD.store(self.thread, Ordering::Relaxed);
    }

    fn remove_hook(&mut self) {
        if let Some(hook) = self.hook.take() {
            HOOK_THREAD.store(0, Ordering::Relaxed);
            // SAFETY: the handle came from SetWindowsHookExW on this thread.
            unsafe { UnhookWindowsHookEx(hook) };
        }
    }

    fn reinstall(&mut self) {
        if self.hook.is_none() {
            return;
        }
        log::warn!("keyboard hook was dropped by Windows; reinstalling");
        self.remove_hook();
        self.matcher.reset();
        self.install_hook();
        self.flush();
    }

    /// Hands what the matcher held back to the sender, which sends it on in one go.
    fn flush(&mut self) {
        self.flush_posted = false;
        if std::mem::take(&mut self.gave_up) {
            log::warn!(
                "keys sent on did not come back through the hook within {REPLAY_GIVE_UP_MS} ms \
                 (another program's hook took them, or Windows dropped them); no longer waiting"
            );
        }
        if !self.matcher.replay_pending() {
            return;
        }
        // SAFETY: plain call.
        let now = unsafe { GetTickCount() };
        let events = self.matcher.take_replay(now);
        let count = events.events().len();
        let from = self.thread;
        if !sender::send(Job::Replay {
            events: Box::new(events),
            from,
        }) {
            self.matcher.replay_lost(count);
        }
    }

    /// Decides about one key event. Allocation-free and lock-free: this runs in the callback.
    fn swallow(&mut self, info: &KBDLLHOOKSTRUCT, wparam: WPARAM) -> bool {
        let event = key_event(info, wparam);
        if matches!(event.origin, Origin::Own | Origin::Replayed) {
            SAW_OWN_INPUT.store(true, Ordering::Relaxed);
        }
        if self.matcher.in_flight() > 0 {
            // SAFETY: plain call.
            let now = unsafe { GetTickCount() };
            if self.matcher.give_up_if_late(now) {
                self.gave_up = true;
                self.post_flush();
            }
        }
        match self.matcher.feed(event) {
            Verdict::Pass => false,
            Verdict::Swallow => true,
            Verdict::SwallowAndFire(button) => {
                if let Some(bit) = 1u32.checked_shl(u32::from(button.0)) {
                    self.shared.pressed.fetch_or(bit, Ordering::AcqRel);
                }
                self.shared.fired.set();
                true
            }
            Verdict::SwallowAndReplay => {
                self.post_flush();
                true
            }
        }
    }

    /// Asks the thread to flush once the callback has returned; one request is enough.
    fn post_flush(&mut self) {
        if !self.flush_posted {
            self.flush_posted = post(self.thread, WM_FLUSH_REPLAY);
        }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION {
        // SAFETY: for HC_ACTION Windows passes a valid KBDLLHOOKSTRUCT in `lparam`.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let swallowed = CONTEXT.with(|slot| match slot.try_borrow_mut() {
            Ok(mut context) => context
                .as_mut()
                .is_some_and(|context| context.swallow(info, wparam)),
            Err(_) => false,
        });
        if swallowed {
            return 1;
        }
    }
    // SAFETY: forwarding the unmodified arguments is the documented contract of a hook.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// A key event as a low-level hook reports it. Mujina's own keystrokes are told apart by their
/// tag: those it sent of its own accord, and those it sent on after holding them back.
pub(crate) fn key_event(info: &KBDLLHOOKSTRUCT, wparam: WPARAM) -> KeyEvent {
    let injected = info.flags & LLKHF_INJECTED != 0;
    let origin = match info.dwExtraInfo {
        OWN_INPUT_TAG if injected => Origin::Own,
        REPLAY_TAG if injected => Origin::Replayed,
        _ if injected => Origin::Injected,
        _ => Origin::Physical,
    };
    let message = u32::try_from(wparam).unwrap_or(0);
    let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
    KeyEvent {
        key: VirtualKey(u16::try_from(info.vkCode).unwrap_or(0)),
        direction: if down { Direction::Down } else { Direction::Up },
        origin,
        scan: ScanCode {
            code: u16::try_from(info.scanCode).unwrap_or(0),
            extended: info.flags & LLKHF_EXTENDED != 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_button_pressed_comes_out_once_in_order() {
        let shared = Arc::new(Shared {
            fired: Event::new().unwrap(),
            pressed: AtomicU32::new(0b101),
            next: Mutex::new(Vec::new()),
        });
        let mut source = ButtonSource { shared };
        assert!(source.handle().is_some());
        let mut out = Vec::new();
        source.signalled(&mut out);
        assert_eq!(
            out,
            [
                AgentEvent::ButtonPressed(ButtonId(0)),
                AgentEvent::ButtonPressed(ButtonId(2))
            ]
        );
        source.signalled(&mut out);
        assert_eq!(out.len(), 2, "taken, not seen again");
    }

    #[test]
    fn the_thread_takes_messages_as_soon_as_it_is_started() {
        // No buttons, so no hook: nothing here needs a desktop.
        let hook = KeyboardHook::spawn().unwrap();
        assert_ne!(hook.thread, 0);
        assert!(
            hook.set_buttons(Vec::new()),
            "its queue exists before spawn returns"
        );
        let mut source = hook.source();
        assert!(source.handle().is_some());
        let thread = hook.thread;
        drop(hook);
        // Once the thread has taken WM_STOP, posting to it fails; it may take a moment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while post(thread, WM_SET_BUTTONS) {
            assert!(
                std::time::Instant::now() < deadline,
                "the thread did not end"
            );
            thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn our_own_keys_are_told_apart_by_their_tag() {
        let info = |flags, tag| KBDLLHOOKSTRUCT {
            vkCode: 0x5B,
            scanCode: 0,
            flags,
            time: 0,
            dwExtraInfo: tag,
        };
        let origin = |flags, tag| key_event(&info(flags, tag), WM_KEYDOWN as WPARAM).origin;
        assert_eq!(origin(LLKHF_INJECTED, OWN_INPUT_TAG), Origin::Own);
        assert_eq!(origin(LLKHF_INJECTED, REPLAY_TAG), Origin::Replayed);
        assert_eq!(origin(LLKHF_INJECTED, 0), Origin::Injected);
        // A tag on a real key means nothing.
        assert_eq!(origin(0, OWN_INPUT_TAG), Origin::Physical);
    }

    #[test]
    fn the_scan_code_and_extended_flag_travel_with_the_event() {
        let right_ctrl = KBDLLHOOKSTRUCT {
            vkCode: 0xA3,
            scanCode: 0x1D,
            flags: LLKHF_INJECTED | LLKHF_EXTENDED,
            time: 0,
            dwExtraInfo: 0,
        };
        let event = key_event(&right_ctrl, WM_KEYDOWN as WPARAM);
        assert_eq!(
            event.scan,
            ScanCode {
                code: 0x1D,
                extended: true
            }
        );
        assert_eq!(event.direction, Direction::Down);
    }
}

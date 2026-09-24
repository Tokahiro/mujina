//! The agent's event loop: one thread, blocked in the kernel, nothing polled. It wakes for Xbox
//! mode, the caller's sources, the launcher's process ending and window messages, in that order.

use std::cell::RefCell;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use mujina_application::agent::{AgentEvent, Flow};
use mujina_application::ports::{FseState, FullScreenExperience, SessionLauncher};
use mujina_winutil::wait::{self, EventLoop, WaitSource};
use mujina_winutil::window;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_SYSTEM_FOREGROUND, MSG, PM_REMOVE, PeekMessageW, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS,
};

use crate::fse::{FseSource, WindowsFse};
use crate::process_exit::{Lookout, ProcessExitSource};
use crate::session_end;

thread_local! {
    /// Set by the foreground hook, which runs on this thread while it pumps messages.
    static FOREGROUND: RefCell<Option<ForegroundNote>> = const { RefCell::new(None) };
}

/// The process name is `None` when it could not be determined.
struct ForegroundNote(Option<String>);

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let name = window::owner_process_name(hwnd as isize);
    FOREGROUND.with(|slot| *slot.borrow_mut() = Some(ForegroundNote(name)));
}

/// Installs the foreground hook for the rest of the process.
fn observe_foreground() {
    // SAFETY: `on_foreground` matches WINEVENTPROC; out-of-context hooks need no module.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            null_mut(),
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if hook.is_null() {
        log::warn!("foreground changes cannot be observed");
    }
}

pub struct AgentLoop<'a> {
    pub fse: &'a WindowsFse,
    pub launcher: &'a dyn SessionLauncher,
    /// The session's named events, the device's sources and the launcher's own.
    pub sources: Vec<Box<dyn WaitSource<AgentEvent> + 'a>>,
    /// How long to wait for the full screen experience to become active after start-up.
    pub grace: Duration,
}

impl AgentLoop<'_> {
    /// Runs until `handle` returns [`Flow::Exit`] or waiting fails. At the session's end
    /// `last_words` runs first ([`session_end::watch`]); anything after it is best effort.
    pub fn run(self, handle: &mut dyn FnMut(AgentEvent) -> Flow, last_words: Box<dyn FnOnce()>) {
        let fse = self.fse;
        let mut events = EventLoop::new();

        // Held by its source for the whole run: dropping it unregisters.
        let fse_watch = fse
            .watch()
            .inspect_err(|error| {
                log::warn!(
                    "no full screen experience notifications ({error}); the agent will not exit \
                     by itself"
                );
            })
            .ok();
        let fse_watched = fse_watch.is_some();
        let lookout = Lookout::default();
        let launcher = self.launcher;
        let mut sources: Vec<Box<dyn WaitSource<AgentEvent> + '_>> = Vec::new();
        if let Some(watch) = fse_watch {
            sources.push(Box::new(FseSource::new(fse, watch)));
        }
        sources.extend(self.sources);
        // Last: MsgWaitForMultipleObjectsEx reports the first of handles signalled together, so
        // what the launcher says about its process is heard before that process's end.
        sources.push(Box::new(ProcessExitSource::new(
            move || launcher.process_id(),
            move |name| launcher.owns_process(name),
            lookout.clone(),
        )));
        for source in sources {
            if let Err(full) = events.add(source) {
                log::warn!("{full}; it is not waited for");
            }
        }

        // Windows may report Xbox mode a little after it started the home app: once the grace
        // period has passed without a switch reported, the agent looks for itself.
        let state = fse.state();
        if fse_watched && state != FseState::Active {
            events.wake_at(Instant::now() + self.grace, AgentEvent::FseChanged(state));
        }

        observe_foreground();
        if !session_end::watch(last_words) {
            log::warn!("the end of the session (sign-out, shutdown) will not be noticed");
        }
        // The hooks only report changes, so start from what is in front now.
        let process_name = window::foreground_process_name();
        if handle(AgentEvent::ForegroundChanged { process_name }) == Flow::Exit {
            return;
        }

        let result = events.run(&mut pump, &mut |event| {
            // Read the state now: the grace period's event was made at start-up, and another
            // switch may have followed the signalled one.
            let event = match event {
                AgentEvent::FseChanged(_) => AgentEvent::FseChanged(fse.state()),
                other => other,
            };
            lookout.note(&event, |name| launcher.owns_process(name));
            match handle(event) {
                Flow::Continue => wait::Flow::Continue,
                Flow::Exit => wait::Flow::Exit,
            }
        });
        if let Err(error) = result {
            log::error!("waiting for events failed ({error}); leaving");
        }
    }
}

/// Drains the message queue (which is when the WinEvent hook and the window procedures run), then
/// reports what they noted.
fn pump(out: &mut Vec<AgentEvent>) {
    // SAFETY: MSG is plain data for which all-zero is a valid value.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: `message` is writable; a null window means "any message of this thread".
    while unsafe { PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
        // SAFETY: `message` was filled in by PeekMessageW.
        unsafe { DispatchMessageW(&raw const message) };
    }

    // Nothing else matters once the session ends.
    if session_end::take_ending() {
        out.push(AgentEvent::SessionEnding);
        return;
    }
    if let Some(ForegroundNote(process_name)) = FOREGROUND.with(|slot| slot.borrow_mut().take()) {
        out.push(AgentEvent::ForegroundChanged { process_name });
    }
}

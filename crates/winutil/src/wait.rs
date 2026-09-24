//! A one-thread event loop that blocks until a handle, the message queue or a deadline wakes it.

use std::fmt;
use std::os::windows::io::{AsRawHandle, BorrowedHandle};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    HANDLE, WAIT_ABANDONED_0, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, QS_ALLINPUT,
};

/// MsgWaitForMultipleObjectsEx waits for at most MAXIMUM_WAIT_OBJECTS minus one handles.
pub const MAX_WAIT_HANDLES: usize = 63;

/// Something an [`EventLoop`] waits for. A handle that stays signalled after a wait (manual-reset
/// event, ended process) must be reset, re-armed or withdrawn in [`signalled`](Self::signalled).
pub trait WaitSource<E> {
    fn name(&self) -> &'static str;

    /// Asked before every wait; must not block. Needs SYNCHRONIZE; never another source's handle.
    fn handle(&mut self) -> Option<BorrowedHandle<'_>>;

    /// Adds what the signal means to `out`. Must not block: the same thread pumps the messages.
    fn signalled(&mut self, out: &mut Vec<E>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

/// [`EventLoop::add`] was refused: the loop already holds [`MAX_WAIT_HANDLES`] sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopFull {
    pub rejected: &'static str,
}

impl fmt::Display for LoopFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no room for {}: one event loop waits for at most {MAX_WAIT_HANDLES} handles",
            self.rejected
        )
    }
}

impl std::error::Error for LoopFull {}

/// Waits for its sources, the thread's message queue and one deadline, on the calling thread.
pub struct EventLoop<'a, E> {
    sources: Vec<Box<dyn WaitSource<E> + 'a>>,
    deadline: Option<(Instant, E)>,
}

impl<E> Default for EventLoop<'_, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, E> EventLoop<'a, E> {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            deadline: None,
        }
    }

    /// Refused past [`MAX_WAIT_HANDLES`] sources, as any source may offer a handle at any time.
    pub fn add(&mut self, source: Box<dyn WaitSource<E> + 'a>) -> Result<(), LoopFull> {
        if self.sources.len() >= MAX_WAIT_HANDLES {
            return Err(LoopFull {
                rejected: source.name(),
            });
        }
        self.sources.push(source);
        Ok(())
    }

    /// Hands `event` to the handler once `when` has passed; replaces (drops) a pending deadline.
    pub fn wake_at(&mut self, when: Instant, event: E) {
        self.deadline = Some((when, event));
    }

    /// Runs until `handle` returns [`Flow::Exit`] or waiting fails; events after an exit are
    /// dropped. `pump` drains the message queue into events; it may be called with nothing queued.
    pub fn run(
        &mut self,
        pump: &mut dyn FnMut(&mut Vec<E>),
        handle: &mut dyn FnMut(E) -> Flow,
    ) -> Result<(), String> {
        // Filled again in every round, so that waking up allocates nothing.
        let mut handles: Vec<HANDLE> = Vec::with_capacity(MAX_WAIT_HANDLES);
        let mut owners: Vec<usize> = Vec::with_capacity(MAX_WAIT_HANDLES);
        let mut events: Vec<E> = Vec::new();

        loop {
            let now = Instant::now();
            let due = self.deadline.as_ref().map(|(when, _)| *when);
            if due.is_some_and(|when| when <= now) {
                if let Some((_, event)) = self.deadline.take()
                    && handle(event) == Flow::Exit
                {
                    return Ok(());
                }
                continue;
            }
            let timeout = due.map_or(INFINITE, |when| {
                timeout_for(when.saturating_duration_since(now))
            });

            match self.wait(&mut handles, &mut owners, timeout)? {
                // A wait may end just before the deadline by `Instant`'s clock; check again.
                Woken::Timeout => continue,
                Woken::Input => {}
                Woken::Source(index) => {
                    if let Some(source) = self.sources.get_mut(index) {
                        source.signalled(&mut events);
                    }
                    if deliver(&mut events, handle) == Flow::Exit {
                        return Ok(());
                    }
                }
            }
            // Also after a source: Windows does not say how input ranks among signalled handles.
            pump(&mut events);
            if deliver(&mut events, handle) == Flow::Exit {
                return Ok(());
            }
        }
    }

    /// Waits once. Sources are neither called nor dropped until it returns, so handles stay open.
    fn wait(
        &mut self,
        handles: &mut Vec<HANDLE>,
        owners: &mut Vec<usize>,
        timeout: u32,
    ) -> Result<Woken, String> {
        handles.clear();
        owners.clear();
        for (index, source) in self.sources.iter_mut().enumerate() {
            if let Some(handle) = source.handle() {
                handles.push(handle.as_raw_handle());
                owners.push(index);
            }
        }
        // `add` keeps this at MAX_WAIT_HANDLES or below.
        let count = u32::try_from(handles.len())
            .map_err(|_| format!("{} handles to wait for", handles.len()))?;

        // SAFETY: `handles` holds `count` handles, whose sources are not called during the wait.
        let woken = unsafe {
            MsgWaitForMultipleObjectsEx(
                count,
                handles.as_ptr(),
                timeout,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if woken == WAIT_FAILED {
            // Read before anything else can overwrite it.
            let error = std::io::Error::last_os_error();
            return Err(format!(
                "MsgWaitForMultipleObjectsEx on {}: {error}",
                self.names(owners)
            ));
        }

        let owner = |first: u32| {
            woken
                .checked_sub(first)
                .filter(|&index| index < count)
                .and_then(|index| owners.get(index as usize).copied())
        };
        if woken == WAIT_TIMEOUT {
            Ok(Woken::Timeout)
        } else if woken == WAIT_OBJECT_0 + count {
            Ok(Woken::Input)
        } else if let Some(index) = owner(WAIT_OBJECT_0).or_else(|| owner(WAIT_ABANDONED_0)) {
            Ok(Woken::Source(index))
        } else {
            // Nothing else is documented without MWMO_ALERTABLE; going round again would spin.
            Err(format!(
                "MsgWaitForMultipleObjectsEx on {} returned {woken:#x}",
                self.names(owners)
            ))
        }
    }

    fn names(&self, owners: &[usize]) -> String {
        let names: Vec<&str> = owners
            .iter()
            .filter_map(|&index| self.sources.get(index))
            .map(|source| source.name())
            .collect();
        if names.is_empty() {
            "the message queue alone".to_string()
        } else {
            names.join(", ")
        }
    }
}

enum Woken {
    /// The source at this index in the loop's list.
    Source(usize),
    Input,
    Timeout,
}

/// Hands `events` to `handle` in order until it says to exit; the rest are dropped then.
fn deliver<E>(events: &mut Vec<E>, handle: &mut dyn FnMut(E) -> Flow) -> Flow {
    for event in events.drain(..) {
        if handle(event) == Flow::Exit {
            return Flow::Exit;
        }
    }
    Flow::Continue
}

/// Milliseconds, rounded up so the wait does not end early, and kept below INFINITE.
fn timeout_for(left: Duration) -> u32 {
    u32::try_from(left.as_nanos().div_ceil(1_000_000))
        .map_or(INFINITE - 1, |millis| millis.min(INFINITE - 1))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
    use std::process::{Child, Command, Stdio};
    use std::ptr::{null, null_mut};
    use std::rc::Rc;

    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateWaitableTimerW, SetWaitableTimer, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostMessageW, WM_USER,
    };

    use super::*;

    const WATCHDOG: Duration = Duration::from_secs(10);

    const WM_TEST: u32 = WM_USER + 7;

    #[derive(Debug, PartialEq)]
    enum Ev {
        Fired,
        Skipped,
        First,
        Second,
        Deadline,
        Early,
        Posted,
        Exited(Option<i32>),
        Restarted,
        Bail,
    }

    fn ends(event: &Ev) -> bool {
        !matches!(event, Ev::Deadline)
    }

    type OnSignal = Box<dyn FnMut(&mut Vec<Ev>)>;

    struct Probe {
        name: &'static str,
        object: OwnedHandle,
        /// Shared, so that a test can offer the object later.
        offered: Rc<Cell<bool>>,
        on_signal: OnSignal,
    }

    impl Probe {
        fn new(
            name: &'static str,
            object: OwnedHandle,
            on_signal: impl FnMut(&mut Vec<Ev>) + 'static,
        ) -> Self {
            Self {
                name,
                object,
                offered: Rc::new(Cell::new(true)),
                on_signal: Box::new(on_signal),
            }
        }
    }

    fn source(
        name: &'static str,
        object: OwnedHandle,
        on_signal: impl FnMut(&mut Vec<Ev>) + 'static,
    ) -> Box<dyn WaitSource<Ev>> {
        Box::new(Probe::new(name, object, on_signal))
    }

    impl WaitSource<Ev> for Probe {
        fn name(&self) -> &'static str {
            self.name
        }

        fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
            self.offered.get().then(|| self.object.as_handle())
        }

        fn signalled(&mut self, out: &mut Vec<Ev>) {
            (self.on_signal)(out);
        }
    }

    fn event(manual_reset: bool, signalled: bool) -> OwnedHandle {
        // SAFETY: null attributes and a null name are valid; failure is a null handle.
        let raw = unsafe { CreateEventW(null(), manual_reset.into(), signalled.into(), null()) };
        assert!(!raw.is_null(), "{}", std::io::Error::last_os_error());
        // SAFETY: `raw` is a new, open handle that nothing else owns.
        unsafe { OwnedHandle::from_raw_handle(raw) }
    }

    fn is_signalled(object: &OwnedHandle) -> bool {
        // SAFETY: an open handle with SYNCHRONIZE access; a zero timeout only tests it.
        unsafe { WaitForSingleObject(object.as_raw_handle(), 0) == WAIT_OBJECT_0 }
    }

    fn drain(out: &mut Vec<Ev>) {
        // SAFETY: MSG is plain data for which all-zero is a valid value.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: `message` is writable; a null window means any message of this thread.
        while unsafe { PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if message.message == WM_TEST {
                out.push(Ev::Posted);
            }
        }
    }

    /// Makes sure this thread has a message queue, which posting to it needs.
    fn make_queue() {
        // SAFETY: MSG is plain data for which all-zero is a valid value.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: `message` is writable; PM_NOREMOVE leaves the queue as it is.
        unsafe { PeekMessageW(&raw mut message, null_mut(), WM_USER, WM_USER, PM_NOREMOVE) };
    }

    fn post() {
        // SAFETY: a null window posts to this thread's queue; the message carries no pointers.
        let posted = unsafe { PostMessageW(null_mut(), WM_TEST, 0, 0) };
        assert_ne!(posted, 0, "{}", std::io::Error::last_os_error());
    }

    fn record(events: &mut EventLoop<'_, Ev>, pumped: &Cell<u32>) -> Vec<Ev> {
        let mut handled = Vec::new();
        events
            .run(
                &mut |out| {
                    pumped.set(pumped.get() + 1);
                    drain(out);
                },
                &mut |event| {
                    let flow = if ends(&event) {
                        Flow::Exit
                    } else {
                        Flow::Continue
                    };
                    handled.push(event);
                    flow
                },
            )
            .unwrap();
        handled
    }

    fn with_watchdog<'a>() -> EventLoop<'a, Ev> {
        let mut events = EventLoop::new();
        events.wake_at(Instant::now() + WATCHDOG, Ev::Bail);
        events
    }

    #[test]
    fn a_signalled_source_is_dispatched_and_its_signal_consumed() {
        let mut events = with_watchdog();
        let object = event(false, true);
        let kept = object.try_clone().unwrap();
        events
            .add(source("probe", object, |out| out.push(Ev::Fired)))
            .unwrap();
        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::Fired]);
        assert!(!is_signalled(&kept), "the wait took the auto-reset signal");
    }

    /// The agent relies on this to see a launcher's restart before the old process's end.
    #[test]
    fn sources_signalled_together_go_in_the_order_they_were_added() {
        let mut events = with_watchdog();
        events
            .add(source("first", event(false, true), |out| {
                out.push(Ev::First);
            }))
            .unwrap();
        events
            .add(source("second", event(false, true), |out| {
                out.push(Ev::Second);
            }))
            .unwrap();
        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::First]);
        assert_eq!(record(&mut events, &pumped), [Ev::Second]);
    }

    #[test]
    fn a_source_without_a_handle_is_skipped() {
        let mut events = with_watchdog();
        // Signalled for good, and first in the list, but not offered.
        let hidden = Probe::new("hidden", event(true, true), |out| out.push(Ev::Skipped));
        hidden.offered.set(false);
        events.add(Box::new(hidden)).unwrap();
        events
            .add(source("shown", event(false, true), |out| {
                out.push(Ev::Fired);
            }))
            .unwrap();
        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::Fired]);
    }

    #[test]
    fn the_last_deadline_set_fires_once_and_not_early() {
        let mut events = EventLoop::new();
        // The watchdog is a waitable timer here, as the deadline is under test.
        // SAFETY: null attributes and a null name are valid; failure is a null handle.
        let raw = unsafe { CreateWaitableTimerW(null(), 1, null()) };
        assert!(!raw.is_null(), "{}", std::io::Error::last_os_error());
        // SAFETY: `raw` is a new, open handle that nothing else owns.
        let timer = unsafe { OwnedHandle::from_raw_handle(raw) };
        // Relative, in units of 100 ns.
        let due = -i64::try_from(WATCHDOG.as_nanos() / 100).unwrap();
        // SAFETY: an open timer handle; `due` outlives the call; no completion routine.
        let set =
            unsafe { SetWaitableTimer(timer.as_raw_handle(), &raw const due, 0, None, null(), 0) };
        assert_ne!(set, 0, "{}", std::io::Error::last_os_error());
        events
            .add(source("watchdog", timer, |out| out.push(Ev::Bail)))
            .unwrap();

        // Offered only once the deadline has been handled, so a second one would come first.
        let after = Probe::new("after", event(false, true), |out| out.push(Ev::Fired));
        let offered = Rc::clone(&after.offered);
        offered.set(false);
        events.add(Box::new(after)).unwrap();

        let started = Instant::now();
        let delay = Duration::from_millis(30);
        // Earlier than its replacement, so keeping the first or earliest deadline would end on it.
        events.wake_at(started + Duration::from_millis(5), Ev::Early);
        events.wake_at(started + delay, Ev::Deadline);

        let mut handled = Vec::new();
        events
            .run(&mut drain, &mut |event| {
                if event == Ev::Deadline {
                    assert!(started.elapsed() >= delay, "{:?}", started.elapsed());
                    offered.set(true);
                }
                let flow = if ends(&event) {
                    Flow::Exit
                } else {
                    Flow::Continue
                };
                handled.push(event);
                flow
            })
            .unwrap();
        assert_eq!(handled, [Ev::Deadline, Ev::Fired]);
    }

    #[test]
    fn the_64th_source_is_refused_and_63_handles_are_waited_for() {
        let mut events = with_watchdog();
        for _ in 1..MAX_WAIT_HANDLES {
            events
                .add(source("quiet", event(false, false), |out| {
                    out.push(Ev::Skipped);
                }))
                .unwrap();
        }
        events
            .add(source("last", event(false, true), |out| {
                out.push(Ev::Fired);
            }))
            .unwrap();
        let refused = events.add(source("one too many", event(false, true), |out| {
            out.push(Ev::Skipped);
        }));
        assert_eq!(
            refused,
            Err(LoopFull {
                rejected: "one too many"
            })
        );
        assert!(refused.unwrap_err().to_string().contains("63"));

        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::Fired]);
    }

    #[test]
    fn a_source_signalled_every_round_does_not_hold_up_the_queue() {
        make_queue();
        let mut events = with_watchdog();
        // Stays signalled; the message posted on the second dispatch must be pumped right after it.
        let dispatched = Rc::new(Cell::new(0_u32));
        let count = Rc::clone(&dispatched);
        events
            .add(source("level", event(true, true), move |out| {
                count.set(count.get() + 1);
                if count.get() == 2 {
                    post();
                }
                if count.get() >= 100 {
                    out.push(Ev::Bail);
                }
            }))
            .unwrap();

        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::Posted]);
        assert_eq!(dispatched.get(), 2);
        assert_eq!(pumped.get(), 2, "pumped after every dispatch");
    }

    #[test]
    fn input_in_the_queue_alone_wakes_the_loop() {
        make_queue();
        post();
        let mut events = with_watchdog();
        // Never signalled, so only the queue can end the wait.
        events
            .add(source("quiet", event(false, false), |out| {
                out.push(Ev::Skipped);
            }))
            .unwrap();
        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::Posted]);
        assert_eq!(pumped.get(), 1);
    }

    #[test]
    fn input_already_seen_but_not_taken_wakes_the_loop() {
        make_queue();
        post();
        // Looking at the queue marks what is in it as old, which a wait without
        // MWMO_INPUTAVAILABLE ignores until more input arrives.
        // SAFETY: MSG is plain data for which all-zero is a valid value.
        let mut message: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: `message` is writable; PM_NOREMOVE leaves the message in the queue.
        let seen = unsafe { PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_NOREMOVE) };
        assert_ne!(seen, 0, "the posted message is in the queue");

        let mut events = with_watchdog();
        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::Posted]);
        assert_eq!(pumped.get(), 1);
    }

    #[test]
    fn exit_stops_the_loop_at_once() {
        let mut events = with_watchdog();
        events
            .add(source("pair", event(false, true), |out| {
                out.extend([Ev::First, Ev::Second]);
            }))
            .unwrap();
        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::First]);
        assert_eq!(pumped.get(), 0, "nothing runs after the exit");
    }

    /// A child process, withdrawn once it has ended, since its handle stays signalled.
    struct ChildExit(Option<Child>);

    impl WaitSource<Ev> for ChildExit {
        fn name(&self) -> &'static str {
            "child"
        }

        fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
            self.0.as_ref().map(AsHandle::as_handle)
        }

        fn signalled(&mut self, out: &mut Vec<Ev>) {
            if let Some(mut child) = self.0.take() {
                let code = child
                    .try_wait()
                    .ok()
                    .flatten()
                    .and_then(|status| status.code());
                out.push(Ev::Exited(code));
            }
        }
    }

    #[test]
    fn a_child_process_is_a_source() {
        let cmd = crate::library::system_directory().unwrap().join("cmd.exe");
        let child = Command::new(cmd)
            .args(["/c", "exit 3"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut events = with_watchdog();
        events.add(Box::new(ChildExit(Some(child)))).unwrap();
        let pumped = Cell::new(0);
        assert_eq!(record(&mut events, &pumped), [Ev::Exited(Some(3))]);
    }

    /// Reopens its object once told it is stale, like a source that follows a restarted process.
    struct Reopening {
        object: OwnedHandle,
        stale: Rc<Cell<bool>>,
    }

    impl WaitSource<Ev> for Reopening {
        fn name(&self) -> &'static str {
            "reopening"
        }

        fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
            if self.stale.take() {
                // Signalled from the start, so the next wait ends on it.
                self.object = event(false, true);
            }
            Some(self.object.as_handle())
        }

        fn signalled(&mut self, out: &mut Vec<Ev>) {
            out.push(Ev::Fired);
        }
    }

    #[test]
    fn a_source_can_replace_its_handle_before_a_wait() {
        let mut events = with_watchdog();
        let stale = Rc::new(Cell::new(false));
        events
            .add(Box::new(Reopening {
                object: event(false, false),
                stale: Rc::clone(&stale),
            }))
            .unwrap();
        events
            .add(source("restart", event(false, true), |out| {
                out.push(Ev::Restarted);
            }))
            .unwrap();

        // The handler cannot reach a source, so it tells it through what they share.
        let mut handled = Vec::new();
        events
            .run(&mut drain, &mut |event| {
                let flow = if event == Ev::Restarted {
                    stale.set(true);
                    Flow::Continue
                } else {
                    Flow::Exit
                };
                handled.push(event);
                flow
            })
            .unwrap();
        assert_eq!(handled, [Ev::Restarted, Ev::Fired]);
    }

    #[test]
    fn a_timeout_never_ends_before_the_deadline() {
        assert_eq!(timeout_for(Duration::from_nanos(1)), 1);
        assert_eq!(timeout_for(Duration::from_micros(1500)), 2);
        assert_eq!(timeout_for(Duration::from_millis(20)), 20);
        assert_eq!(timeout_for(Duration::MAX), INFINITE - 1);
    }
}

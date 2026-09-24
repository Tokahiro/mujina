//! The launcher's process, as a wait source of the agent's event loop: a process handle is
//! signalled once the process has ended, and the agent supervises the launcher by that.
//!
//! Which process that is changes during a session. The launcher may be started after the agent
//! (the home role starts the agent first), brought back by the home role after a crash, or
//! restarted by anything else. So the source looks the process up again whenever it watches none
//! and there is a reason to believe the launcher may be there now: the home role started it, a
//! window of its own came to the front, or its state changed ([`Lookout`]). While it watches one,
//! only the launcher's state changing makes it ask again, since that is when a launcher that
//! keeps its process id where it can be read (Steam, in the registry) writes a new one. Never on
//! another wake, and never on a timer: looking up may take a snapshot of every process (the
//! generic launcher finds its process by name).
//!
//! An id is taken for the launcher's only if the process it names has not ended and runs an
//! image the launcher owns: an id the launcher left behind, after a crash say, may have been
//! given to another process since, as Windows reuses them.

use std::cell::Cell;
use std::os::windows::io::{AsHandle, BorrowedHandle};
use std::rc::Rc;

use mujina_application::agent::AgentEvent;
use mujina_domain::supervision::LauncherExit;
use mujina_winutil::process::ProcessWatch;
use mujina_winutil::wait::WaitSource;

/// Whether an event is a reason to look for the launcher's process again: the home role started
/// the launcher, one of its windows came to the front (a launcher started by anything shows
/// one), or it said its state changed (Steam writes its new process id then). `owns` tells the
/// launcher's processes by image name (`SessionLauncher::owns_process`). A window whose process
/// is not known counts too; another program's does not, so that switching between other
/// programs never takes a snapshot of every process.
pub fn is_reason_to_look(event: &AgentEvent, owns: impl Fn(&str) -> bool) -> bool {
    match event {
        AgentEvent::LauncherStarted | AgentEvent::LauncherStateChanged => true,
        AgentEvent::ForegroundChanged { process_name } => process_name.as_deref().is_none_or(owns),
        _ => false,
    }
}

/// Tells a [`ProcessExitSource`] when to look for the process again. Shared with whoever hands
/// the loop's events on, since the loop's handler cannot reach a source.
#[derive(Clone, Default)]
pub struct Lookout(Rc<Reasons>);

#[derive(Default)]
struct Reasons {
    /// A reason to look while no process is watched. One seen while the old process is still
    /// watched counts for the next look, so that a launcher which restarted at once is found as
    /// soon as its old process is gone.
    look: Cell<bool>,
    /// The launcher's state changed: where its process is is asked even while one is watched.
    ask_again: Cell<bool>,
}

impl Lookout {
    /// Takes note of `event`, if it is a reason to look; `owns` as for [`is_reason_to_look`].
    pub fn note(&self, event: &AgentEvent, owns: impl Fn(&str) -> bool) {
        if is_reason_to_look(event, owns) {
            self.0.look.set(true);
        }
        if *event == AgentEvent::LauncherStateChanged {
            self.0.ask_again.set(true);
        }
    }
}

/// Reports [`AgentEvent::LauncherExited`] when the launcher's process ends.
pub struct ProcessExitSource<'a> {
    /// Where the launcher says its process is: `SessionLauncher::process_id`.
    process_id: Box<dyn Fn() -> Option<u32> + 'a>,
    /// Whether an image name is one of the launcher's: `SessionLauncher::owns_process`.
    owns: Box<dyn Fn(&str) -> bool + 'a>,
    watch: Option<ProcessWatch>,
    lookout: Lookout,
}

impl<'a> ProcessExitSource<'a> {
    /// Looks for the process before the first wait, and then as `lookout` says.
    pub fn new(
        process_id: impl Fn() -> Option<u32> + 'a,
        owns: impl Fn(&str) -> bool + 'a,
        lookout: Lookout,
    ) -> Self {
        lookout.0.look.set(true);
        Self {
            process_id: Box::new(process_id),
            owns: Box::new(owns),
            watch: None,
            lookout,
        }
    }

    /// A watch on the process with this id, if it is the launcher's and has not ended.
    fn open(&self, pid: u32) -> Option<ProcessWatch> {
        let watch = ProcessWatch::open(pid)?;
        // A process id the launcher left behind may name a process that has ended, whose object
        // lives on while someone holds a handle: it would be reported as ending once more.
        if watch.exit_code().is_some() {
            return None;
        }
        // Or one that Windows has given the id since. Asked through the watch, which keeps the
        // id from being given away once more meanwhile.
        let Some(name) = watch.image_name() else {
            log::debug!("process {pid}: its image name cannot be read; not watched");
            return None;
        };
        if !(self.owns)(&name) {
            log::debug!("process {pid} is {name}, not the launcher; not watched");
            return None;
        }
        Some(watch)
    }
}

impl WaitSource<AgentEvent> for ProcessExitSource<'_> {
    fn name(&self) -> &'static str {
        "launcher process"
    }

    fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
        let reasons = &self.lookout.0;
        let found = match self.watch.as_ref().map(ProcessWatch::pid) {
            // What the launcher said meanwhile is answered by this look.
            None if reasons.look.take() => {
                reasons.ask_again.set(false);
                Some((self.process_id)())
            }
            // The launcher may name another process now: one that restarted itself may name
            // its new process before the old one has ended, whose end is then not the
            // launcher's and not reported. One that names none has no process to supervise.
            // Both as before stage 5.
            Some(watched) if reasons.ask_again.take() => {
                let pid = (self.process_id)();
                (pid != Some(watched)).then(|| {
                    log::debug!("process {watched} is no longer the launcher's");
                    pid
                })
            }
            _ => None,
        };
        if let Some(pid) = found {
            // As good as a look: a reason noted before is answered by it.
            reasons.look.set(false);
            self.watch = pid.and_then(|pid| self.open(pid));
            if let Some(watch) = &self.watch {
                log::debug!("watching launcher process {}", watch.pid());
            }
        }
        self.watch.as_ref().map(AsHandle::as_handle)
    }

    fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
        // An ended process stays signalled, so its handle is let go of here: until there is a
        // reason to look again, there is nothing to wait for.
        if let Some(process) = self.watch.take() {
            let exit = LauncherExit {
                exit_code: process.exit_code(),
                uptime_secs: process.observed_for().as_secs(),
            };
            log::info!("launcher process {} ended: {exit:?}", process.pid());
            out.push(AgentEvent::LauncherExited(exit));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use mujina_application::device::ButtonId;
    use mujina_winutil::event::Event;
    use mujina_winutil::library::system_directory;
    use mujina_winutil::wait::{EventLoop, Flow};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{CREATE_SUSPENDED, WaitForSingleObject};

    use super::*;

    /// A launcher stand-in that never ends by itself: it is created suspended, so it runs
    /// nothing, until the test ends it. Ended when dropped too, so that a failing test leaves no
    /// process behind.
    struct StandIn(Child);

    impl StandIn {
        fn start() -> Self {
            let child = Command::new(system_directory().unwrap().join("cmd.exe"))
                .creation_flags(CREATE_SUSPENDED)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            Self(child)
        }

        fn pid(&self) -> u32 {
            self.0.id()
        }

        /// Ends it with TerminateProcess, whose exit code `kill` makes 1.
        fn end(&mut self) {
            self.0.kill().unwrap();
            self.0.wait().unwrap();
        }
    }

    impl Drop for StandIn {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }

    /// The launcher's processes, as the tests' launcher tells them: its stand-ins.
    fn stand_in(name: &str) -> bool {
        name.eq_ignore_ascii_case("cmd.exe")
    }

    /// Where the launcher is, as the test says, and how often it was asked.
    #[derive(Clone, Default)]
    struct Whereabouts {
        pid: Rc<Cell<Option<u32>>>,
        asked: Rc<Cell<u32>>,
    }

    impl Whereabouts {
        fn source(&self, lookout: &Lookout) -> ProcessExitSource<'static> {
            self.source_owning(lookout, stand_in)
        }

        fn source_owning(
            &self,
            lookout: &Lookout,
            owns: fn(&str) -> bool,
        ) -> ProcessExitSource<'static> {
            let (pid, asked) = (Rc::clone(&self.pid), Rc::clone(&self.asked));
            ProcessExitSource::new(
                move || {
                    asked.set(asked.get() + 1);
                    pid.get()
                },
                owns,
                lookout.clone(),
            )
        }
    }

    /// The process the source watches, if any.
    fn watched(source: &ProcessExitSource<'_>) -> Option<u32> {
        source.watch.as_ref().map(ProcessWatch::pid)
    }

    /// Whether the source's handle is signalled right now.
    fn signalled_now(source: &mut ProcessExitSource<'_>) -> bool {
        let handle = source.handle().unwrap().as_raw_handle();
        // SAFETY: a process handle the source holds until it is next asked for one; a zero
        // timeout only tests it.
        unsafe { WaitForSingleObject(handle, 0) == WAIT_OBJECT_0 }
    }

    fn exit_code(events: &[AgentEvent]) -> Option<u32> {
        match events {
            [AgentEvent::LauncherExited(exit)] => exit.exit_code,
            other => panic!("not one exit: {other:?}"),
        }
    }

    #[test]
    fn the_process_is_looked_for_only_when_there_is_reason_to() {
        let at = Whereabouts::default();
        let lookout = Lookout::default();
        let mut source = at.source(&lookout);
        let mut first = StandIn::start();
        at.pid.set(Some(first.pid()));

        assert!(
            source.handle().is_some(),
            "looked for before the first wait"
        );
        // A window of its own is a reason only while none is watched.
        let in_front = AgentEvent::ForegroundChanged {
            process_name: Some("cmd.exe".into()),
        };
        for wake in [
            AgentEvent::ButtonPressed(ButtonId(0)),
            AgentEvent::LauncherStarted,
            in_front,
        ] {
            lookout.note(&wake, stand_in);
            assert!(source.handle().is_some());
        }
        assert_eq!(at.asked.get(), 1, "not again while one is watched");

        first.end();
        let mut out = Vec::new();
        source.signalled(&mut out);
        assert_eq!(exit_code(&out), Some(1));
        // The reasons seen while it ran have their look now, which finds it gone.
        assert!(source.handle().is_none());
        assert_eq!(at.asked.get(), 2);

        let mut second = StandIn::start();
        at.pid.set(Some(second.pid()));
        let elsewhere = AgentEvent::ForegroundChanged {
            process_name: Some("notepad.exe".into()),
        };
        for wake in [
            AgentEvent::ButtonPressed(ButtonId(0)),
            AgentEvent::SettingsChanged,
            elsewhere,
        ] {
            lookout.note(&wake, stand_in);
            assert!(source.handle().is_none());
        }
        assert_eq!(at.asked.get(), 2, "no reason, no look");

        lookout.note(&AgentEvent::LauncherStarted, stand_in);
        assert!(source.handle().is_some(), "the new process is watched");
        assert_eq!(at.asked.get(), 3);
        second.end();
        out.clear();
        source.signalled(&mut out);
        assert_eq!(exit_code(&out), Some(1), "and its end is reported too");
    }

    /// A launcher that restarted itself may name its new process before the old one has ended
    /// (launcher-4): the new one is watched, and the old one's end is none of the launcher's.
    #[test]
    fn a_process_the_launcher_names_anew_is_watched_in_place_of_the_old() {
        let at = Whereabouts::default();
        let lookout = Lookout::default();
        let mut source = at.source(&lookout);
        let mut old = StandIn::start();
        at.pid.set(Some(old.pid()));
        assert!(source.handle().is_some());

        let new = StandIn::start();
        at.pid.set(Some(new.pid()));
        lookout.note(&AgentEvent::LauncherStateChanged, stand_in);
        assert!(source.handle().is_some());
        assert_eq!(watched(&source), Some(new.pid()));
        assert_eq!(at.asked.get(), 2, "asked once, on the launcher's word");

        old.end();
        assert!(
            !signalled_now(&mut source),
            "the old one's end is not waited for"
        );
        lookout.note(&AgentEvent::LauncherStateChanged, stand_in);
        assert!(!signalled_now(&mut source));
        assert_eq!(watched(&source), Some(new.pid()), "the same one, still");
        assert_eq!(at.asked.get(), 3);
    }

    /// As before stage 5: a launcher that names no process any more has none to supervise.
    #[test]
    fn a_launcher_that_names_no_process_any_more_has_none_watched() {
        let at = Whereabouts::default();
        let lookout = Lookout::default();
        let mut source = at.source(&lookout);
        let running = StandIn::start();
        at.pid.set(Some(running.pid()));
        assert!(source.handle().is_some());

        at.pid.set(None);
        lookout.note(&AgentEvent::LauncherStateChanged, stand_in);
        assert!(source.handle().is_none());
        // Its word was the reason, and it has been answered.
        lookout.note(&AgentEvent::ButtonPressed(ButtonId(0)), stand_in);
        assert!(source.handle().is_none());
        assert_eq!(at.asked.get(), 2);
    }

    #[test]
    fn a_process_that_has_ended_is_not_watched() {
        let at = Whereabouts::default();
        let mut gone = StandIn::start();
        gone.end();
        // The test still holds its handle, so the process object and its id are still there.
        at.pid.set(Some(gone.pid()));
        let mut source = at.source(&Lookout::default());
        assert!(source.handle().is_none());
        assert_eq!(at.asked.get(), 1);
    }

    /// An id left behind may have been given to another program since.
    #[test]
    fn a_process_the_launcher_does_not_own_is_not_watched() {
        let at = Whereabouts::default();
        let other = StandIn::start();
        at.pid.set(Some(other.pid()));
        let lookout = Lookout::default();
        let mut source = at.source_owning(&lookout, |name| name == "steam.exe");
        assert!(source.handle().is_none());
        assert_eq!(at.asked.get(), 1);
    }

    #[test]
    fn the_launchers_own_windows_and_word_are_reasons_to_look_but_other_wakes_are_not() {
        let foreground = |name: Option<&str>| AgentEvent::ForegroundChanged {
            process_name: name.map(str::to_string),
        };
        for reason in [
            AgentEvent::LauncherStarted,
            AgentEvent::LauncherStateChanged,
            foreground(Some("CMD.EXE")),
            // Whose window it is could not be told: it may be the launcher's.
            foreground(None),
        ] {
            assert!(is_reason_to_look(&reason, stand_in), "{reason:?}");
        }
        for other in [
            AgentEvent::ButtonPressed(ButtonId(0)),
            AgentEvent::SettingsChanged,
            AgentEvent::SessionEnding,
            foreground(Some("notepad.exe")),
        ] {
            assert!(!is_reason_to_look(&other, stand_in), "{other:?}");
        }
    }

    /// A source on an event the test sets, standing for a wake of the kind it reports.
    struct Nudge(Rc<Event>, AgentEvent);

    impl WaitSource<AgentEvent> for Nudge {
        fn name(&self) -> &'static str {
            "nudge"
        }

        fn handle(&mut self) -> Option<BorrowedHandle<'_>> {
            Some(self.0.as_handle())
        }

        fn signalled(&mut self, out: &mut Vec<AgentEvent>) {
            out.push(self.1.clone());
        }
    }

    /// Run as the agent's loop runs it, whose handler takes note of every event before handing
    /// it on: a launcher that ends is reported, and one the home role brings back is watched
    /// again, however often. The test ends a stand-in only on a wake after the one
    /// before which the source looked, so the process is always watched by then.
    #[test]
    fn a_launcher_brought_back_is_supervised_again_every_time() {
        let at = Whereabouts::default();
        let lookout = Lookout::default();
        // The home role's signal, and any other wake (the device button, say).
        let started = Rc::new(Event::new().unwrap());
        let pressed = Rc::new(Event::new().unwrap());
        let mut events = EventLoop::new();
        events.add(Box::new(at.source(&lookout))).unwrap();
        let nudges = [
            (&started, AgentEvent::LauncherStarted),
            (&pressed, AgentEvent::ButtonPressed(ButtonId(0))),
        ];
        for (event, means) in nudges {
            events
                .add(Box::new(Nudge(Rc::clone(event), means)))
                .unwrap();
        }
        // Ends a test whose loop is broken, which would otherwise wait for ever.
        let watchdog = Instant::now() + Duration::from_secs(10);
        events.wake_at(watchdog, AgentEvent::SessionEnding);

        let mut running = StandIn::start();
        at.pid.set(Some(running.pid()));
        pressed.set();
        let mut exits = 0;
        let mut handled = String::new();
        events
            .run(&mut |_| {}, &mut |event| {
                lookout.note(&event, stand_in);
                handled.push(match event {
                    AgentEvent::ButtonPressed(_) => 'P',
                    AgentEvent::LauncherExited(_) => 'X',
                    AgentEvent::LauncherStarted => 'S',
                    _ => '?',
                });
                match event {
                    // The source has looked since the last start: its process is watched.
                    AgentEvent::ButtonPressed(_) if at.asked.get() == exits + 1 => running.end(),
                    AgentEvent::ButtonPressed(_) => {
                        assert_eq!(at.asked.get(), exits, "looked on a wake without reason");
                        started.set();
                    }
                    AgentEvent::LauncherExited(_) if exits < 2 => {
                        exits += 1;
                        // Brought back, and some other wake before the home role says so.
                        running = StandIn::start();
                        at.pid.set(Some(running.pid()));
                        pressed.set();
                    }
                    AgentEvent::LauncherStarted => pressed.set(),
                    _ => return Flow::Exit,
                }
                Flow::Continue
            })
            .unwrap();
        assert_eq!(handled, "PXPSPXPSPX");
        assert_eq!(at.asked.get(), 3, "one look per start, none in between");
    }
}

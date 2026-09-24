# ADR-0014: Threads and COM

Status: accepted. Supersedes ADR-0002's limit of "at most three threads" (its decision against an
async runtime stands) and amends ADR-0003, which said Mujina needs no COM: it initialises COM
where a Win32 call it makes asks for it.

## Context

ADR-0002 chose kernel waits over a runtime so that idle means idle, and counted threads to keep
it so: "message passing between at most three threads". The count no longer described the agent.
With Steam's Wi-Fi fix on it had a main thread, the input sender, a WLAN reader and a Steam UI
worker, and every direct menu press started a thread of its own. Nor does a count measure what
matters: a thread blocked in the kernel costs nothing while nothing happens, and a single thread
that polls costs something all the time. Some work needs a thread of its own, too: the WLAN
service's first question may show the location prompt and block the caller until the user answers
(`crates/adapter-steam/src/wlan.rs`); Microsoft asks for a low-level keyboard hook to run on a
thread that does nothing else ([LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc),
done in stage 6); Steam's debugging port has a pace of its own.

ADR-0003 said Mujina needs no COM. The agent brings the home role back through `ShellExecuteW`,
which "can delegate execution to Shell extensions … that are activated using COM", so "COM should
be initialized before ShellExecute is called", as an STA with
`COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE`
([ShellExecuteW, Remarks](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecutew)).
Since stage 2 Mujina does so (`winutil::com::Apartment`).

## Decision

- **The invariant replaces the count:** every thread Mujina starts is blocked in the kernel while
  nothing happens (in a wait function, on its message queue, or receiving from a channel, which
  parks the thread) and wakes only for an event that concerns it. No timer runs while idle and
  nothing is polled while idle. A wait with a time limit is a deadline after something happened
  (the start-up grace period for Xbox mode), never a period.
- **Bounded bursts of polling after an event are allowed**, where the other side offers nothing
  to wait on. The one there is: on a sign of life from Steam (its UI in front, its state changed,
  the home role started it, a press that found the link down), unless the link answered within
  the last 30 s, the `steam-ui` worker probes the debugging port every 100 ms for 30 s, checks
  readiness every 250 ms once connected, and otherwise backs off to at most 8 s between
  attempts, for at most 120 s (`crates/adapter-steam/src/indicator.rs`). Then it is blocked on
  its channel again. Another burst needs the same shape (started by an event, bounded in time,
  cut short when it succeeds) and a line here.
- **It is checked the way ADR-0002 promised:** the agent's `cost:` line at the end of a session
  gives the CPU time of the whole process, every thread included, against the length of the
  session, and the number of events it handled. A share of one core that grows with idle time
  means something polls (`docs/on-device-checklist.md`, "Idle cost").
- **A thread may be added** where the work needs one and it keeps the invariant: a call that may
  block for long, or a protocol with its own pace. It is named, started only while the feature it
  serves is on, and listed below. Changing the list needs no new ADR.
- **COM:** a thread that calls what Win32 documents as needing COM initialises it as that call's
  documentation says, and only then. The agent's main thread holds an STA for its whole life: it
  activates the home role through `ShellExecuteW` and pumps messages anyway, as an STA thread must.
  Other callers enter one around each call (`winutil::shell`). Mujina implements no COM objects,
  and the binding stays `windows-sys` (ADR-0003).

The agent's threads after stage 6:

| Thread | Exists | Blocked in, while idle | Wakes for |
|---|---|---|---|
| main | always | `MsgWaitForMultipleObjectsEx` over the wait set below and the message queue (`winutil::wait::EventLoop`) | the wait set; the foreground WinEvent hook and `WM_ENDSESSION`, delivered while it pumps messages |
| `keyboard-hook` | for the whole session unless the configuration names a device of a crate of its own (`adapter-keyboard`): so also with `profile = "none"` or the button switched off, which makes switching it on apply at once; its hook is installed only while there is a chord to catch | `GetMessageW` on its own queue | every key event on the machine while its hook is installed (the callback runs the allocation-free matcher, signals the device's event on a press, and posts itself a flush, whose handler gives the held-back keys to `sender`); also woken by new chords from the main thread, a request from `sender` to install the hook again, `sender` reporting keys Windows refused, and the stop request |
| `sender` | always (`adapter-keyboard`), for the launcher's shortcuts whatever the device | receiving from its channel | a chord to send, a key-chord button to pass on, or keys the hook held back to send on in one go; holds each key of a chord for tens of milliseconds, then waits 30 ms for the hook's proof of life, sending held-back keys meanwhile at once |
| `wlan` | Steam, with `ui_link` and `wifi_indicator` on | `WaitForSingleObject` on its own event | WLAN notifications that change what the icon shows: connect, disconnect, a signal in another bar bucket (the signal only where Windows grants its notifications: the package's wiFiControl capability and the location permission; otherwise connect and disconnect only, and a refusal that came before the permission is asked again once, at the first reading with the connection's details) |
| `steam-ui` | Steam, with `ui_link` and `wifi_indicator` on | receiving from its channel | a sign of life from Steam, a Wi-Fi status to show, a press of the device button to carry out |
| `steam-running-screen` | for a moment after a game starts, with Steam's `ui_link` and the core setting `game_start_screen` on | ends after one call, two seconds after the start | – |
| `steam-after-game` | for a moment after a game ends, with Steam's `ui_link` on and only with the detailed log | ends after three looks within seven seconds | – |

Stage 6 added `keyboard-hook`. Until then the hook ran on the main thread, so every key event on
the machine woke the main thread, and a slow moment there (a process snapshot, a file read,
`ShellExecuteW`) could make Windows remove the hook for the rest of the session. Now key
events wake only `keyboard-hook`, whose callback answers at once, and the main thread wakes for a
press of the device button. Before stage 5 the other four lasting threads ran as they do now (the
WLAN reader in `adapter-windows`), and each direct menu press added a `steam-menu` thread with a
debugging session of its own; the Steam UI worker carries presses out now. Threads that Windows
runs in the process to deliver callbacks (the WLAN service's notifications, the Xbox mode change
notification) are not Mujina's; their CPU time is in the `cost:` line all the same.

The main thread's wait set after stage 6, every entry a `WaitSource`:

| Handle | Source | Owner |
|---|---|---|
| Xbox mode's change event | `FseSource` | `adapter-windows` |
| `Local\Mujina.settings` | `SettingsChangedSource` (ADR-0010) | `adapter-windows` |
| `Local\Mujina.launcher-started`, signalled by the home role | `LauncherStartedSource` | `adapter-windows` |
| the device's own: a press of a key-chord button, signalled by `keyboard-hook` (a device crate brings what its buttons need) | `ButtonSource`, in `DeviceParts::sources` | `adapter-keyboard` |
| the launcher's own: Steam's registry key | `SteamState`, in `SessionParts::sources` | `adapter-steam` |
| the launcher's process, while one is watched | `ProcessExitSource`. While it watches none, it looks the process up again only when the home role started the launcher, a window of the launcher's came to the front, or the launcher's state changed; while it watches one, only the launcher's state changing makes it ask again. It takes only a running process whose image the launcher owns | `adapter-windows` |

In this order: of the handles signalled together, the wait reports the first
([MsgWaitForMultipleObjectsEx, Remarks](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-msgwaitformultipleobjectsex)),
so what the launcher says about its process is heard before that process's end, as before
stage 5.

Before stage 5 it held Xbox mode's event, the WLAN reader's event, Steam's registry watch (from
`StateSignal`), the settings event and the launcher's process. Wi-Fi changes no longer wake the
main thread: the reader feeds the Steam UI worker directly. Before stage 6 a press was found in the
message pump, after the hook had run on the main thread.

## Consequences

- Idle cost is stated as a number anyone can read in their own log, not as a thread count.
- Since stage 6 the keyboard hook has a thread of its own, one more than in stage 5, blocked in
  `GetMessageW` while nothing is typed. Idle still means idle; the `cost:` line after a session
  with the hook thread is what checks it (`docs/on-device-checklist.md`).
- `MsgWaitForMultipleObjectsEx` waits for at most `MAXIMUM_WAIT_OBJECTS - 1` handles
  ([MsgWaitForMultipleObjectsEx](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-msgwaitformultipleobjectsex)),
  so the event loop refuses a 64th source with an error rather than waiting for fewer.

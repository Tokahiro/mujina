# ADR-0001: One package, two roles — short-lived home, resident agent

Status: accepted, confirmed on device (2026-09-20)

## Context

Windows activates the home app on boot into Xbox mode and whenever the user presses the home
button. Only the process Windows just activated holds the right to set the foreground window.
Microsoft's guidance for launchers on handhelds is to get out of the way after launching.
Button mapping and the Wi-Fi indicator, on the other hand, need something resident.

## Decision

`mujina.exe` has two roles. **home** runs on every activation, brings the launcher up, passes on
the foreground right and exits. **agent** is started by home if it is not running, holds no
window, and exits when Xbox mode ends.

## Consequences

- Re-activation needs no single-instance redirection or IPC: it is just another home run.
- A crash in the agent cannot break the home button, and vice versa.
- The foreground hand-over uses the documented mechanism instead of focus-stealing tricks.
- Confirmed on a OneXPlayer (Windows 11 26200, 2026-09-20): Windows activates the home app with
  the argument `windows.gaming:///home`; the home process switched Steam to Big Picture within
  17 ms and exited, and Xbox mode stayed stable. The `--stay` fallback was not needed and was
  not built.
- Recovery after a launcher crash is the one case without a sanctioned route to the foreground.
  Tried on the device (2026-09-20): the agent opening `mujina://home` does start the home role,
  but Windows grants it no foreground right, and the launcher came up behind whatever had taken
  over when it died. `windows.gaming:///home`, the argument Windows itself uses, is not a
  registered protocol and cannot be invoked. So, only when Windows has refused the launch
  screen's request for the foreground, the home role sends one synthetic Alt tap, after which
  Windows accepts the request. Observed on the device: Windows sometimes reports the request as
  refused on a normal activation too although the window did come to the front, so the tap is
  only sent when the launch screen really is not the foreground window.

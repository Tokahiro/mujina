# ADR-0005: Device buttons are keyboard chords, handled by a low-level hook

Status: superseded by [ADR-0013](0013-compile-time-launcher-plug-ins.md) (stage 6). A device is a
plug-in now; a button is not always a keyboard chord, and a chord may have up to four keys. The
hook rules below stand as the rules of `adapter-keyboard`, whose hook runs on a thread of its own
([ADR-0014](0014-threads-and-com.md)); the consequences below describe the design before that.

## Context

Handheld vendors' companion software reports extra hardware buttons as synthesized keyboard
chords. On OneXPlayer devices the "show desktop" button arrives as an injected `LWIN`+`D`, and
OneXConsole keeps sending it in Xbox mode, where Windows ignores it. The controller's Guide
button is not an option: in Xbox mode Windows owns it, and XInput cannot be intercepted without
a filter driver.

## Decision

A `WH_KEYBOARD_LL` hook feeds every key event into the domain's `ChordMatcher`. The rules, each
of them learned on a device:

- **Swallow the whole chord, modifier included.** Letting `LWIN` through makes the shell open
  Start when it is released.
- **Only synthesized input counts** (`injected_only`), so the chord on a real keyboard still
  works, and our own keystrokes are tagged (`dwExtraInfo`) and never reinterpreted.
- **Nothing is ever lost.** If the swallowed modifier turns out to start a different chord, the
  modifier press and the following event are replayed.
- **The callback is allocation-, lock- and log-free.** Windows silently removes a low-level hook
  that answers slowly. Proof of life: our own keystrokes must pass back through the hook; if
  they do not, the hook is reinstalled.
- **Replacement chords use left-hand modifiers and are held ~50 ms**, from a separate thread,
  because games and Steam's overlay sample the keyboard per frame.
- What the button means is decided in the domain (`button::decide`): launcher UI in front →
  menu; a game running → overlay; otherwise the button keeps its default behaviour.
- **Decide when pressed, not ahead of time.** The first design armed the hook only while the
  tracked foreground gave the button a meaning. Booting straight into Xbox mode showed the
  flaw: Steam comes to the front behind the Windows welcome screen, no foreground event is
  delivered, and the button stayed dead. The hook is now always armed; on a press the agent
  asks what is in front right now. When the answer is "nothing of ours", the chord is sent on
  (tagged as ours, so the hook lets it pass) and the button behaves as without Mujina.

## Consequences

- No driver, no injection into other processes, works at medium integrity.
- A device whose button is a different chord needs only a different `TriggerChord`.
- The launcher shortcuts (`Ctrl+1`, the overlay hotkey) are undocumented Steam behaviour and
  live in `adapter-steam`. A user who changed Steam's overlay hotkey needs it configurable
  (planned with profile files).

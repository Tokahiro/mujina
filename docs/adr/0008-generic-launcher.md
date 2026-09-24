# ADR-0008: A generic launcher, configured as data

Status: accepted; its configuration type in the application ring superseded by ADR-0013:
`GenericLauncherConfig` lives in `adapter-generic`, which reads `[launcher.generic]` through its
descriptor. The network indicator and the state signal below are gone since stage 5 (ADR-0013,
ADR-0014): a launcher brings wait sources and hears the agent's events through `observe`, and the
agent looks for the generic launcher's process again whenever the home role starts it or one of
its windows comes to the front.

## Context

Steam is the only launcher with a dedicated adapter, and every further one costs a crate, an
owner with the device and launcher at hand, and reverse engineering (ADR-0007 made the choice
of adapter a run-time decision). Most of what the home role needs from a launcher is generic,
though: start an executable, tell whether its full-screen UI is up, bring it to the front.
Full-screen frontends (Playnite, emulator frontends, another store's client) are all launched
the same way.

## Decision

`kind = "generic"` under `[launcher]` selects `mujina-adapter-generic`, an adapter driven
entirely by `[launcher.generic]`: `executable`, `arguments`, an optional `window_class`, and
the `process` name (defaulting to the executable's file name). The kind carries its
configuration (`LauncherKind::Generic(GenericLauncherConfig)`), so a selected kind is always a
usable one; anything less falls back to Steam with a note.

What stays at the port defaults, deliberately: game detection (the device button then always
leads to the launcher), menus and navigation (`ESC` is the shortcut answer, overridable via
`[launcher] menu` / `overlay`), network indicator, doctor checks, and a state signal (the
agent watches only the launcher process itself).

Without a `window_class`, a running process counts as "UI visible" and its main window is what
gets focused — the best a generic adapter can honestly claim.

## Consequences

- Any full-screen frontend can be booted into today; a launcher that deserves game detection
  or menu integration still gets a dedicated adapter, and the generic adapter is the measure
  of what such an adapter must beat.
- `switch_to_ui` re-runs the executable, relying on frontends fronting their existing
  instance; a launcher where that spawns a second instance needs a dedicated adapter.
- The settings GUI can offer the generic launcher as a form (path, arguments, window class)
  without any code per launcher.

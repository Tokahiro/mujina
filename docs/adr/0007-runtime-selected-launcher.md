# ADR-0007: The launcher is selected at run time

Status: accepted; the `LauncherKind` enum and its match arm, and `StateSignal`, superseded by
ADR-0013 (compile-time launcher plug-ins): a launcher that can signal its state brings a wait
source of its own. Launchers stay compiled in, and the launcher is still read once per session.

## Context

Until 0.14.0 the composition root returned Steam Big Picture unconditionally, and the agent's
event loop was wired to Steam's registry key as the one way a launcher signals "my state
changed". Supporting a second launcher — a dedicated adapter or the planned generic one that a
settings GUI configures — needs both to become choices: which `LauncherBundle` to build, and
what to wait on for its state.

The alternative of a plug-in system (launchers discovered at run time) was not considered
seriously: adapters are compiled in, reviewed, and covered by the same tests and lints as the
rest; a launcher is one crate and one match arm, not a loadable module.

## Decision

- `Settings` carries a `launcher_kind`, parsed from `kind = "..."` under `[launcher]` in
  `config.toml` (default `steam`; an unknown name falls back to the default with a note, like
  every other setting).
- `launcher()` in `crates/app/src/compose.rs` matches on the kind and boxes per arm. It stays
  the only place that names concrete launcher types.
- `LauncherBundle` gains `state_signal() -> StateSignal`, replacing the hard-wired
  `launcher_state_key()`. `StateSignal::RegistryKey` is what Steam uses;
  `StateSignal::None` means the agent watches only the launcher process itself. New signal
  kinds are added to the enum when a launcher needs them, keeping the agent loop free of
  launcher names.

## Consequences

- A new launcher is: an adapter crate, a `LauncherKind` variant with its `from_name` spelling,
  one match arm in `launcher()`, and a template line — `docs/new-launcher.md` describes the
  path end to end.
- The launcher choice is read once at process start, like every setting; changing it takes
  effect the next time the home role runs. It is deliberately not live-reloaded: home and
  agent must agree on the launcher within one Xbox-mode session.
- The agent loop keeps its wait-set shape (`MsgWaitForMultipleObjectsEx`, no polling); a
  launcher without a state signal simply contributes no handle.

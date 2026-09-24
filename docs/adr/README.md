# Architecture decision records

Why Mujina is the way it is: one short record per decision that shaped it, with the context it
was made in. A record is not rewritten when a later one changes its decision; its status line
says what changed and where to read on.

| ADR | Decision | Status |
|---|---|---|
| [0001](0001-process-model.md) | One package, two roles: a short-lived home, a resident agent | Accepted; confirmed on a device |
| [0002](0002-no-async-runtime.md) | No async runtime | Accepted; its limit of three threads superseded by [0014](0014-threads-and-com.md) |
| [0003](0003-windows-sys-and-runtime-binding.md) | `windows-sys`, and run-time binding for the Xbox mode API | Accepted; amended by [0014](0014-threads-and-com.md) (COM where a call asks for it) |
| [0004](0004-plain-msix.md) | A plain, self-contained MSIX | Accepted; "no installer of our own" superseded by [0012](0012-installer.md), the identity clause by [0015](0015-package-identity-and-signing.md) |
| [0005](0005-button-as-keyboard-chord.md) | Device buttons are keyboard chords, handled by a low-level hook | Superseded by [0013](0013-compile-time-launcher-plug-ins.md); its hook rules stand as `adapter-keyboard`'s |
| [0006](0006-wifi-indicator.md) | The Wi-Fi indicator is fed through one persistent debugging session | Accepted; amended in stage 5: the feature is `adapter-steam`'s ([0013](0013-compile-time-launcher-plug-ins.md), [0014](0014-threads-and-com.md)) |
| [0007](0007-runtime-selected-launcher.md) | The launcher is selected at run time | Accepted; `LauncherKind` and `StateSignal` superseded by [0013](0013-compile-time-launcher-plug-ins.md) |
| [0008](0008-generic-launcher.md) | A generic launcher, configured as data | Accepted; its configuration type in the application ring superseded by [0013](0013-compile-time-launcher-plug-ins.md) |
| [0009](0009-config-file-stays-the-source-of-truth.md) | `config.toml` stays the source of truth, and tools edit it in place | Accepted; amended in the review of September 2026 (strict reading per key) |
| [0010](0010-live-reload-by-named-event.md) | A running agent reloads its settings when a named event is signalled | Accepted; the fixed list of settings that wait superseded by [0013](0013-compile-time-launcher-plug-ins.md) |
| [0011](0011-settings-app.md) | A settings app of its own, built with Slint | Accepted; amended in 0.23.0, 0.26.0, 0.28.0 and stage 9 |
| [0012](0012-installer.md) | An installer that carries the package | Accepted; amended in stages 1 and 8; how it gets its package and what a release publishes superseded by [0015](0015-package-identity-and-signing.md) |
| [0013](0013-compile-time-launcher-plug-ins.md) | Launchers and devices are compile-time plug-ins | Accepted (stages 4 and 6); amended in 0.28.0 (no migration of the removed key) |
| [0014](0014-threads-and-com.md) | Threads and COM | Accepted |
| [0015](0015-package-identity-and-signing.md) | Package identity, signing and what a release publishes | Accepted (stage 8); amended in 0.28.0: a new, public repository, and the release key rotated into an environment |

The records name the steps in which 0.28.0 was made, stages 1 to 9. Each was merged on its own
after 0.27.0, none was released on its own, and all of them are in 0.28.0:

1. Mujina Setup's security, above all its administrator part; signing without cargo; CI's jobs.
2. Correctness fixes: Steam's button path, Win32 contracts, the configuration, Mujina Settings.
3. Shared plumbing: owned handles in `winutil`, the event loop, `adapter-kit`, and an
   arch-check that fails for a crate without a ring.
4. The launcher plug-in point.
5. The agent on the event loop: launchers bring their own wake sources, and the Wi-Fi fix
   becomes Steam's.
6. The device plug-in point: `adapter-keyboard`, chords of up to four keys.
7. One translation system; Mujina Settings shows launchers' and devices' options from their
   descriptors.
8. Mujina Setup restructured: one file for users, the package attached after linking.
9. Contributor scaffolding (`cargo xtask new-launcher`, `new-device`), issue forms, this index.

## Writing one

A decision gets a record when it constrains later work: a dependency or a toolkit, a process or
thread model, a seam between rings, what Mujina changes on a user's machine, or a rule a reviewer
would otherwise have to remember. Write it in the pull request that makes the decision.

- Take the next number, and a file name of a few words: `docs/adr/0016-<what-it-decides>.md`.
- Keep it short. Say what was considered and why it lost, with sources for any fact about
  Windows, a launcher or a device that can change.
- Add its row to the table above.
- A later decision that changes an earlier one is a new record. It says in its status line what
  it supersedes or amends; the earlier record's status line gets "superseded by" or "amended by"
  with a link, and its row above changes to match. Small corrections to a record still in force
  go into it as a dated or versioned *Amended* paragraph under the part they change.

```markdown
# ADR-0016: <The decision, as a short sentence>

Status: accepted. <Supersedes or amends ADR-00NN: what of it, if anything.>

## Context

What forces the decision: the problem, the constraints (no polling, no injection, one signed
installer, …), and the options considered, each with why it lost.

## Decision

What Mujina does, stated so that a reviewer can tell whether a change keeps to it.

## Consequences

What follows, good and bad: what becomes easy, what becomes harder, what has to be checked on a
device, and what is left to do.
```

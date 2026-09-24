# ADR-0012: An installer that carries the package

Status: accepted. Partly supersedes ADR-0004 ("no installer of our own"); the package itself
stays a plain, self-contained MSIX. Amended in stage 1 (the administrator part) and stage 8 (see
the amendments at the end); how the installer gets its package and what a release publishes are
superseded by ADR-0015.

## Context

Installing Mujina took more than the package: Developer Mode on, Mujina's certificate trusted
by the machine, the package installed, Mujina selected as home app, and on removal the home app
given back before the package goes. `Install.ps1` did this, started through `Install.cmd`
because Windows blocks scripts by default. People expect one file that installs, and an entry
under Settings → Apps that removes.

Options were WiX (an MSI whose every step here would be a custom action), Inno Setup (a second
language and a tool no longer on CI's Windows image), an `.appinstaller` file (no way to do the
steps around the package), and a small program of Mujina's own.

## Decision

- `mujina-setup.exe`, built in this workspace (`crates/setup`) with the same UI toolkit as the
  settings app, carrying the signed MSIX and its certificate. The release workflow builds it
  after the package and signs it with the same certificate; other builds carry no package and
  say so. *Superseded by ADR-0015: the package is attached after linking, and the installer's
  signature is its own.*
- The steps and their order are data (`plan.rs`, tested); each is done with the tool
  `Install.ps1` used and that is proven on devices: `reg`/the registry, `certutil`,
  `Add-AppxPackage`, and `mujinactl register` / `unregister` from the freshly installed package.
  *Amended: see below.*
- Administrator rights once, for the two machine-wide steps (Developer Mode, certificate), by
  starting itself again with `--elevated` behind one prompt; skipped when both are done
  already, so updates need no prompt. Everything else runs as the user, because the package and
  the home app setting are per user.
- Settings → Apps shows the package itself, and Windows runs nothing of Mujina's when it is
  uninstalled there, so the home app setting cannot be given back *before* removal on that
  path. 0.24.0 added a second entry of the installer's own that did; two entries for one app
  confused (seen on the device: removing the package's entry first left the other one unable to
  finish). Since 0.24.1 there is one entry, the package's, and the installer leaves a check at
  sign-in instead (`HKCU\...\Run`, a copy of itself under `%LOCALAPPDATA%\Mujina` with
  `--cleanup <family>`): while Mujina is installed it asks Windows once and ends; once Mujina is
  gone it gives the home app setting back, removes itself and ends. Not resident; nothing runs
  in between. `mujina-setup.exe --uninstall` still removes in the other order.
- Developer Mode and the certificate stay on removal.
- `Install.cmd` and the scripts remain in the release zip for now. *Superseded by ADR-0015: the
  zip and the scripts are gone; `--quiet` and `--uninstall --quiet` replace them.*

## Consequences

- One download; SmartScreen still warns, since the installer is signed with Mujina's own
  certificate, not one Windows knows. The README and the installer say so.
- The installer is as small as its UI toolkit allows and has no logic of its own beyond the
  order of the steps: what installing means stays defined by the package and `mujinactl`.

## Amendments

**Stage 1.** The administrator part starts no program and takes no argument: it trusts only the
certificate attached to its own file and imports it in-process (`winutil::certstore`), and a
certificate counts as trusted only if it is exactly that one. Every other program is started by
its full path, and the package family is the exact one of the carried package's publisher.

**Stage 8.**
- The crate is cut into `plan` (pure), `run` (a runner over small host traits, tested with a
  fake host), `host_windows`, `cli` and `ui`. The order puts the safety net first: pre-flight,
  prepare, the package, the check at sign-in, then the home app.
- The home app is made and given back in-process, with `HomeAppRegistration` of
  `mujina-application` and `adapter-windows`' registry adapter, by the family's app ID; no
  `mujinactl.exe` is started. The rule for giving it back is the one `mujinactl unregister`
  uses.
- The installer reads the installed version and installs, updates, reinstalls, or replaces a
  newer version only when that is confirmed.
- The check at sign-in is written as `--cleanup` alone (the family is the attached package's).
  It keeps itself while the home app cannot be given back, for up to five sign-ins, and logs
  every failure to `%LOCALAPPDATA%\Mujina Setup\setup.log`, which outlasts the removal. The
  check installers up to 0.27.0 left, and 0.24.0's entry, are not taken over: a device that had
  them is cleaned up by hand once (the on-device checklist has the commands).
- Removing in the right order is reachable: Mujina Settings → System → Remove Mujina starts the
  kept copy with `--uninstall`, outside Mujina's package.
- `--quiet [--no-home-app]`, `--uninstall [--quiet]`, `--log <path>`, `--about` and `--help`,
  with documented exit codes.
- The installer is as small as before, and still has no logic of its own about launchers or
  devices; what installing means is the package's and mujina-application's.

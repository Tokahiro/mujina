# Architecture

Mujina is a Rust workspace built in rings (onion architecture). Dependencies point inward only:
the inner rings know nothing about Windows, Steam or files.

## Rings

```
┌─────────────────────────────────────────────────────────────────────┐
│ app (wiring, mujina.exe, mujinactl.exe) · settings-gui · setup      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │ adapter-windows · -keyboard · -steam · -generic · -config …   │  │
│  │  ┌─────────────────────────────────────────────────────────┐  │  │
│  │  │ adapter-kit   (what several adapters share)             │  │  │
│  │  │  ┌───────────────────────────────────────────────────┐  │  │  │
│  │  │  │ application   (use cases and their ports)         │  │  │  │
│  │  │  │  ┌─────────────────────────────────────────────┐  │  │  │  │
│  │  │  │  │ domain   (pure rules)                       │  │  │  │  │
│  │  │  │  └─────────────────────────────────────────────┘  │  │  │  │
│  │  │  └───────────────────────────────────────────────────┘  │  │  │
│  │  └─────────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

`winutil` (Win32 plumbing, knowing no Mujina type) sits beside the rings: the kit, the adapters
and the executables use it; `domain` and `application` never do.

`i18n` (the leaf ring) sits beside them too: it knows no Mujina type and uses nothing, and holds
the texts Rust puts into a window in the user's language (see [Languages](#languages)).
`application` builds on it for `Msg`, the titles of the doctor's findings, and re-exports it, so
that the checks of the adapters are titled without a dependency of their own.

<!-- arch-check: begin -->
| Ring | Crates | May depend on | Crates from outside | Contains |
|---|---|---|---|---|
| `domain` | `mujina-domain` | nothing | none | pure rules (`no_std`, no `unsafe`): the chord matcher for several buttons, button decisions, exit policy |
| `application` | `mujina-application` | `domain`, `leaf` | only `thiserror` | use cases (`HomeActivation`, `HomeAppRegistration`, `Doctor`, `AgentService`), the ports they call, and settings, launchers and devices as data (`SettingSpec`, `LauncherDescriptor`, `DeviceDescriptor`, the choice of device); the doctor's findings are titled with `Msg`s |
| `plumbing` | `mujina-winutil` | nothing | any | safe wrappers around Win32 calls (`windows-sys`): handles, errors, window rules, the event loop; knows no Mujina type |
| `leaf` | `mujina-i18n` | nothing | none | texts from Rust in the user's language: gettext `.po` catalogs compiled in, `Msg`, `Localizer`; knows no Mujina type |
| `adapter-support` | `mujina-adapter-kit` | `domain`, `application`, `plumbing` | any | what several adapters share, in Mujina's types: starting and focusing a launcher, the launcher and device runtimes (`LauncherRuntime`, `LauncherPlugin`, `DeviceRuntime`, `DevicePlugin`) |
| `adapter` | `mujina-adapter-windows`, `-keyboard`, `-steam`, `-generic`, `-config` | `domain`, `application`, `plumbing`, `adapter-support` | any | implementations of the ports; never another adapter |
| `root` | `mujina-app` | `domain`, `application`, `plumbing`, `adapter-support`, `adapter` | any | wiring (`compose.rs`), the launcher and device lists (`registry.rs`), what the tools share (`tool.rs`), `mujina.exe` and `mujinactl.exe` |
| `settings-app` | `mujina-settings-gui` | `domain`, `application`, `plumbing`, `leaf`, `root` | any | `mujina-settings.exe`; the rest of Mujina through `mujina-app`'s `tool` module, never an adapter |
| `installer` | `mujina-setup` | `domain`, `application`, `plumbing`, `leaf`, `adapter` | any | `mujina-setup.exe`, the installer: the home app rule of `application` in-process, with `adapter-windows`' registry adapter (the only adapter it uses); knows no launcher and no device |
| `tool` | `xtask` | nothing | any | `arch-check`, `i18n-check`, `version-check`, `package`, `dist`, `release-notes`, and the skeletons `new-launcher` and `new-device` |
<!-- arch-check: end -->

Every crate names its ring in its `Cargo.toml`:

```toml
[package.metadata.mujina]
ring = "adapter"
```

`cargo xtask arch-check` fails CI when a crate names no ring or one the table does not have, or
when it depends on a crate whose ring its row does not list. A crate of the workspace counts by
its ring, whatever its name; every other crate is a crate from outside, and `deny.toml` decides
which of those may be used at all (Slint only in the two apps with a window, no async runtime).
Build- and dev-dependencies are not checked, since they do not ship. The table above is the
check's own matrix: a test in `xtask` fails when the two differ.

Mujina Settings reaches the configuration, the doctor, the home app setting, the launchers and
devices and the capture through `mujina_app::tool`, which `mujinactl` shares; it names no
adapter. Every crate is also built and tested on Linux, where no Windows API exists: what needs
Windows is behind `#[cfg(windows)]`, so the rules and the descriptors of the launchers and
devices are tested there as well.

## Ports

Ports are small and cut by who needs them. What a launcher or a device cannot do it leaves at
the port's default.

| Port | Implemented by | Purpose |
|---|---|---|
| `FullScreenExperience` | `adapter-windows::fse` | is Xbox mode available, is it on |
| `PackageIdentity` | `adapter-windows::identity` | Mujina's app id |
| `HomeAppRegistry` | `adapter-windows::home_registry` | the per-user home app setting |
| `HomeLauncher` | each launcher's runtime (`adapter-steam`, `adapter-generic`) | what the home role needs: find, prepare, start, switch to, focus the launcher and show a page |
| `SessionLauncher` | each launcher's runtime | what the agent needs: tell the launcher's windows and games apart, where its process is, its shortcuts, open its menus directly, take over options that apply at once, hear of every event the agent handled (`observe`) |
| `DeviceButtons` | each device's runtime (`adapter-keyboard` for every key-chord device) | what the agent asks of the device it runs: take a change of device or options over at once where it can, send a swallowed button on where Mujina has nothing for it |
| `KeySender` | `adapter-keyboard` | send the launcher's shortcuts, whatever the device |
| `SettingsSource`, `SettingsStore` | `adapter-config::ConfigFile` | read `config.toml`, edit it in place ([ADR-0009](adr/0009-config-file-stays-the-source-of-truth.md)) |

The roles add `AgentControl` (the home role starts the agent and tells it when it has started the
launcher), `LaunchScreen` (what the home role shows while the launcher starts), `HomeActivator`
and `ForegroundProbe`, all in `adapter-windows`. The doctor (`application::doctor::Doctor`)
checks Xbox mode and the home app through the ports above, and runs every `Check` it is given:
`adapter-windows`' (Developer Mode, the agent, conflicting tools) and each launcher's and
device's runtime's own.
The agent's use case, `AgentService`, handles the `AgentEvent`s its event loop produces (below).
The ports are in `crates/application/src/ports/`, one file per role: `system.rs`, `home.rs`,
`launcher.rs`, `device.rs` and `input.rs`.

## Launchers

Launchers are compile-time plug-ins
([ADR-0013](adr/0013-compile-time-launcher-plug-ins.md)). Each launcher crate brings two halves:

- a **descriptor** (`application::launcher::LauncherDescriptor`), portable and tested on Linux:
  its id, its options under `[launcher.<id>]` as `SettingSpec`s, its template block, what it
  offers (`LauncherCaps`) and its rules. The configuration reader and `mujinactl` work from
  descriptors; `adapter-config` has no launcher-specific logic (its template names the default
  kind in a comment). Mujina Settings lists the launchers from the registry and draws the Setup
  page's rows of the one chosen from its settings, translated with its catalogs;
- a **runtime** (`adapter-kit::plugin::LauncherRuntime`), on the Windows side: the
  `HomeLauncher` for the home role and the tools, the `SessionParts` for the agent (its
  `SessionLauncher` and the wait sources of its own), its doctor checks.

`crates/app/src/registry.rs` lists the pairs (`LauncherPlugin`); `compose.rs` builds the one the
configuration names, and hands the list to every reader and writer of the configuration.
Whether a change applies at once is part of each setting's description, the core settings'
(`application::settings::schema::CORE`) and the launchers' alike.

## Devices

Devices are compile-time plug-ins as well (ADR-0013), with the same two halves:

- a **descriptor** (`application::device::DeviceDescriptor`), portable and tested on Linux with
  `application::testing::device_conformance`: its id, which machine it is (`matches`, against
  the SMBIOS strings `adapter-windows::smbios` reads), its buttons (`ButtonSpec`, with whether
  Mujina swallows a press or only observes it) and its options under `[device.<id>]`.
  `application::device::select` chooses the device: a `[device.button]` of one's own, else
  `[device] profile`, `auto` being the first device that says it is the machine (the device
  crates first, then the profiles by file name);
- a **runtime** (`adapter-kit::plugin::DeviceRuntime`), on the Windows side: `DeviceParts` for
  the agent (the `DeviceButtons` port and the wait sources that report its presses), its doctor
  checks.

`adapter-keyboard` serves every device whose buttons arrive as key chords, with one runtime: each
file in `profiles/devices/` and the button of one's own. It owns the low-level keyboard hook, on a
thread of its own, the thread that sends keys (`KeySender`, for every device), and the hook's
proof of life, which is the hook seeing those keys. `registry.rs` lists its devices and, in
`DEVICE_PLUGINS`, the crates of devices with a mechanism of their own
([new-device.md](new-device.md)).

## Processes

One package, three programs ([ADR-0001](adr/0001-process-model.md)):

- **Home** (`mujina.exe`), started by Windows when Xbox mode opens its home (boot, home button,
  Game Bar). Brings the launcher up, hands it the foreground and exits within seconds.
- **Agent** (`mujina.exe agent`), running while Xbox mode is on. It sleeps on kernel objects
  only (see below).
- **Settings** (`mujina-settings.exe`), running only while its window is open. It writes
  `config.toml` and signals the named event `Local\Mujina.settings`; the agent then re-reads
  the file ([ADR-0010](adr/0010-live-reload-by-named-event.md)).

`mujinactl.exe` is the command-line tool for the same things (doctor, config, probe, capture,
home app).

The installer, `mujina-setup.exe`, is not part of the package. It carries the package, attached
to it after linking, and installs, updates and removes it ([ADR-0012](adr/0012-installer.md),
[ADR-0015](adr/0015-package-identity-and-signing.md)). Its crate is cut like the rest: `plan`
decides the steps from what it found out about the device, `run` carries them out over small
host traits (tested with a fake host on any system), `host_windows` does the work on the
machine, and `ui` and `cli` are its window and its command line. It makes Mujina the home app
and gives the setting back with `application`'s `HomeAppRegistration` and `adapter-windows`'
registry adapter, the rule `mujinactl` uses, in-process.

## The agent's event loop

The agent's main thread runs `winutil::wait::EventLoop` (`adapter-windows::agent_loop`): one
`MsgWaitForMultipleObjectsEx` over the handles of its wait sources and the thread's message
queue, with no timeout but the start-up grace period for Xbox mode. Each source says what its
handle means as `AgentEvent`s:

- `adapter-windows` brings Xbox mode switching (`FseSource`), the launcher's process ending
  (`ProcessExitSource`: while it watches none, it looks the process up again when the home
  role started the launcher, a window of the launcher's came to the front or the launcher's
  state changed; while it watches one, only the launcher's state changing makes it ask again;
  never otherwise, and it takes only a process whose image the launcher owns), and the named
  events other Mujina processes signal: `Local\Mujina.settings` when the configuration changed
  and `Local\Mujina.launcher-started` when the home role started the launcher.
- The device brings its own in `DeviceParts::sources`, each reporting `ButtonPressed(ButtonId)`:
  a key-chord device the event its hook thread signals on a press.
- The launcher brings its own in `SessionParts::sources`: Steam a change notification on
  `HKCU\Software\Valve\Steam`, the generic launcher none.
- The message pump delivers foreground changes and the end of the session. The keyboard hook has
  a thread of its own.

`AgentService` acts on each event, then hands it to the launcher's `observe`, so that a launcher
can run features of its own beside the ports. Steam's Wi-Fi fix is one: its WLAN reader and its
`steam-ui` worker, which keeps the link to Big Picture, live in `adapter-steam`, and the rings
inside know nothing of Wi-Fi. What runs on which thread, and how idle cost is checked, is in
[ADR-0014](adr/0014-threads-and-com.md).

## Languages

Mujina Settings and Mujina Setup speak English and German; `mujina`, `mujinactl`, the log and what
the doctor finds stay English (Mujina Settings names each check by its title in the window's
language, and its System page shows the sentence a check may have for what it found). Each
window's texts are gettext catalogs, `lang/<language>/LC_MESSAGES/*.po` in its crate, compiled
in:

- **Slint's own texts** are `@tr("…")` in the `.slint` files, looked up by the component they are
  in (msgctxt). Rust passes states and data (`Verdict`, `Notice`, `StepKind`, …), never
  sentences; counts use Slint's plural form.
- **Texts Rust has only at run time**, such as the entries of a list it fills, are `Msg`s of
  `mujina-i18n`: the English is the key, looked up without a context by a `Localizer` fed with
  the same catalog.
- **Texts another crate brings**: the titles of the doctor's checks and their sentences, and a
  launcher's or a device's name and the title, help and choice values of each of its settings.
  Each crate that has such texts writes them as `Msg`s and ships `lang/<language>.po`:
  `mujina-application` and `mujina-adapter-windows` as `CATALOGS`, a launcher or device through
  its descriptor's `catalogs()`. Mujina Settings feeds them all to its `Localizer`, after its own
  catalog; a launcher's or a device's own texts it looks up in that part's catalogs first.

Each app decides its language once at start-up, the same way
(`mujina_winutil::locale::language`): `interface.language` where Mujina Settings has it, else the
first of the Windows display languages the app has, else English. The answer goes to
`slint::select_bundled_translation` and, in Settings, to the `Localizer`.

`cargo xtask i18n-check`, in CI, fails when a catalog lacks a text: every `@tr` as
slint-tr-extractor finds it, with its context, and every `Msg::new("…")` of the crate outside
its test modules. It also fails on a translation with another plural or other placeholders
than the English. It reads the catalogs with rspolib, the reader of Slint's build.

## Extending

- **Another launcher:** try the generic launcher first. For more, write an `adapter-<id>`
  crate with a descriptor and a runtime, and list it in `registry.rs`
  ([new-launcher.md](new-launcher.md)). `cargo xtask new-launcher <id>` makes one that builds
  and passes every check, and lists it.
- **Another device:** a profile in `profiles/devices/` when its buttons send key combinations
  (`cargo xtask new-device <id> --profile`); otherwise an `adapter-<id>` crate with a
  descriptor, a runtime and a wait source of its own, listed in `registry.rs`
  (`cargo xtask new-device <id>`; [new-device.md](new-device.md)).

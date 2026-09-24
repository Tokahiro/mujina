# ADR-0013: Launchers and devices are compile-time plug-ins

Status: accepted; the launcher part in stage 4, the device part in stage 6; amended in 0.28.0
(the *Amended* paragraph below: no migration of the removed key). Supersedes ADR-0005
(a device button is one keyboard chord; its hook rules stand, as the keyboard crate's),
ADR-0007's `LauncherKind` enum and match arm and its `StateSignal`, ADR-0008's configuration
type in the application ring, and ADR-0010's fixed list of settings that wait for the next
session.

## Context

Adding a launcher meant editing every ring: a variant of the closed `LauncherKind` enum in the
application ring, which also held the generic launcher's own configuration; a field in the
configuration reader, whose strict sections rejected `[launcher.playnite]` as a whole; the
template; a match arm in the composition root; `takes_effect_live`'s hard-coded list; and the
settings app, which listed the kinds by index. Counted for a Playnite adapter with one toggle of
its own, 13 to 16 files. Steam's own switch, `features.wifi_indicator`, lived among the core
settings and was shown for every launcher, and it silently switched Steam's whole debugging-port
integration. `LauncherBundle` made six small traits mandatory for every launcher, with keyboard
shortcuts it may not have.

Devices had no plug-in point at all. A device was one keyboard chord of a modifier and a key:
`InputInterceptor::set_trigger(Option<TriggerChord>)` in the application ring, one
`ButtonPressed` without a button, and an event loop welded to adapter-windows' keyboard hook.
The built-in profiles were a hand-kept `include_str!` list in the configuration adapter, which
also read the machine's SMBIOS strings. So nothing else fitted: not a button that arrives as a
vendor HID report (the ROG Ally's), not a second button, not a chord of three keys. The OneXPlayer
Mini already has three buttons, two of them three-key chords
([HandheldCompanion's definition](https://raw.githubusercontent.com/Valkirie/HandheldCompanion/main/HandheldCompanion/Devices/OneXPlayer/OneXPlayerMini.cs)).

The single signed installer, reviewed code and the idle-cost rule stay: no plug-ins loaded at run
time.

## Decision

### Launchers

- **A descriptor per launcher, in the application ring's terms**
  (`mujina_application::launcher::LauncherDescriptor`): its id (`kind = "<id>"`, the section
  `[launcher.<id>]`), its name, its options as `SettingSpec`s (key, kind, title, help, whether a
  change applies at once or next session, what it requires, whether it is required), its template
  block, what it offers (`LauncherCaps`: game detection, menu, overlay, navigation), cross-field
  rules without side effects, and the programs that conflict with it. It is portable: every
  launcher crate tests it on Linux too, with `application::testing::conformance`.
- **A runtime per launcher, on the Windows side** (`mujina_adapter_kit::plugin::LauncherRuntime`):
  what the home role gets (`HomeLauncher`), what the agent gets (`SessionParts`: a
  `SessionLauncher`, and the kernel objects of the launcher's own that the agent's event loop
  should wait on, as `winutil::wait::WaitSource`s), and its doctor checks. A launcher that can
  signal its state brings a `WaitSource`; Steam's is a change notification on its registry key.
  `LauncherPlugin` pairs descriptor and runtime.
- **One list** in the composition root, `crates/app/src/registry.rs`, and its fallback (Steam).
  Everything that reads or writes the configuration gets the list from `compose`.
- **Two role ports** replace `LauncherBundle`: `HomeLauncher` and `SessionLauncher`, with
  defaults for what a launcher may lack. A menu or overlay opened directly reports `Taken` only
  after trying. `reconfigure` passes a running launcher the options that apply at once, and
  `observe` every event the agent handled, for what the launcher does beside the ports (Steam's
  link to Big Picture, for its Wi-Fi fix and its menus).
- **Settings carry `LauncherSelection { id, options }`.** The configuration reads each
  `[launcher.<id>]` key by key against the descriptor; a wrong key is a note of its own, a
  missing required setting or a broken rule falls back to the default launcher with a note, and
  `SettingsStore::apply` refuses what would add a note (ADR-0009). Whether a change applies at
  once comes from the specs, core and launchers'.
- **The button decides by what the launcher offers.** Without a menu the press is swallowed,
  never passed on as the device's own chord; without an overlay it leads home in Xbox mode.
- **Steam owns its options:** `[launcher.steam] ui_link`, the consent for its unauthenticated
  debugging port, and `wifi_indicator`, which needs it.
- **Clean break, one exception.** `features.wifi_indicator` is removed without an alias; the key
  is noted as removed. An old `false` is read as `launcher.steam.ui_link = false`, so that a port
  closed per SECURITY.md stays closed. Its note says so whichever launcher is chosen: a note that
  came and went with the launcher would make ADR-0009 refuse the switch back to Steam.

  *Amended in 0.28.0:* no exception and no migration. Mujina's only user is its owner, still
  testing, so the removed key is an unknown one like any other: one note, otherwise ignored. An
  old `false` no longer closes the port; `ui_link = false` does. `migration.rs` is gone, and with
  it the one place `adapter-config`'s logic named Steam.

### Devices

- **A descriptor per device** (`mujina_application::device::DeviceDescriptor`): its id (what
  `[device] profile = "<id>"` names, and `[device.<id>]` for its options), its name, whether it
  is the machine the SMBIOS strings describe (`matches(&SystemIdentity)`), its buttons
  (`ButtonSpec`: a `ButtonId`, a key, a label and its `Suppression`), its options as
  `SettingSpec`s and its rules. Descriptors are trait objects, not tables of functions, because
  most devices are data: a profile file read at start-up is as much a descriptor as a crate.
  Every device is checked with `application::testing::device_conformance`, on Linux too.
- **Choosing a device is one pure function** (`application::device::select`), over the list the
  composition root hands to the configuration: a `[device.button]` that names a usable button
  wins, as it did; otherwise `profile = "none"` is no button, an id is that device, and `auto`
  (the default) is the first device that says it is this machine. What cannot be used is a note,
  and then no button is mapped. Settings carry the result as `DeviceSelection { id, options }`.
- **The list's order is the rule for `auto`**, so `select` needs no ranking: the device crates
  first, then the profiles by file name, then the button of one's own (which `auto` never
  takes). A profile for a whole family (`onexplayer.toml` takes every ONE-NETBOOK machine) thus
  hides no crate, and a profile for one model is named to sort before the family's; the keyboard
  crate's test fails, naming both files, when a profile matches every machine a later one is for.
- **A runtime per device, on the Windows side** (`mujina_adapter_kit::plugin::DeviceRuntime`):
  `start` gives the agent `DeviceParts`, the device's `DeviceButtons` and its wait sources, which
  report `AgentEvent::ButtonPressed(ButtonId)`; a device may bring several sources, one per way
  its buttons reach Windows. `checks` for `doctor`, `prepare` for what the device needs readied.
  `DevicePlugin` pairs descriptor and runtime. `start` and `DeviceButtons::reconfigure` take the
  whole `DeviceSelection` rather than its options alone, because one runtime may serve many
  devices: every key-chord device has the keyboard crate's, so a switch between two profiles, to
  a button of one's own or off takes effect at once, as it did before, while a device another
  runtime runs waits for the next session, which `config set` and Mujina Settings say too. Every
  runtime starts with the button switched off, so that switching it on applies at once;
  `adapter_kit::plugin::runtime_conformance` checks that in a crate's tests.
- **Suppression is said, not assumed.** A `Swallowed` button is Mujina's to catch, and
  `DeviceButtons::pass_on` sends it on where Mujina has nothing for it (a key chord on the
  desktop). An `Observed` button reaches the device's own software whatever Mujina does: ASUS
  says the Ally's Armoury Crate and Command Center buttons cannot be remapped
  ([ROG Ally remapping guide](https://rog.asus.com/us/articles/guides/how-to-remap-buttons-and-create-custom-game-profiles-on-the-rog-ally/)),
  and a user-mode reader of a HID report cannot hold it back without a driver. Mujina can only
  add to such a button, the OEM app reacts to the same press, it is never passed on, and the
  device's `doctor` checks should name the program that also reacts.
- **Key-chord devices are one crate**, `adapter-keyboard`, with one runtime: every
  `profiles/devices/*.toml`, found by its `build.rs`, and the button of one's own from
  `[device.button]`. A profile has `[[buttons]]`, each with a key, a label, a `chord` of one to
  four keys (held keys first, in any order the firmware keeps; the trigger last), `injected_only`
  and `suppression`, which may only be `swallowed` for now: the hook holds every chord back until
  it knows which button it is, and a chord button whose keys also reach other programs would need
  a matcher that passes them while still telling buttons apart. A profile the hook could not
  serve (more than eight buttons, two chords it cannot tell apart) is refused when it is read.
  The crate keeps what shares state together: the low-level hook, on a thread of its own
  (ADR-0014), the sender, and the hook's proof of life. Its matcher
  (`domain::chord::ChordSetMatcher`) matches every button's chord at once without allocating,
  keeps a prefix two chords share undecided, and sends on what it held back in order and as it
  came (scan code and extended flag included), holding back what comes meanwhile until its own
  copies came back through the hook, or for at most a second from the oldest still out.
  ADR-0005's hook rules are its module's rules.
- **`KeySender` stays a port of its own** for the launcher's shortcuts, whatever the device; the
  keyboard crate implements it.
- **One list**: `registry.rs` takes every key-chord device from the keyboard crate and lists the
  crates with a mechanism of their own in `DEVICE_PLUGINS`.
- **Every button means what the one button meant** (`button::decide`); the id travels so that a
  binding per button (menu, overlay, home, off) can come with the first device whose second button
  needs another meaning.

## Consequences

- A launcher is one crate, one line in `registry.rs`, one line each in `[workspace.dependencies]`
  and `crates/app/Cargo.toml`, and a checklist row. Nothing in domain, application,
  adapter-config, adapter-windows or another adapter changes for it (`docs/new-launcher.md`).
- A device whose buttons are key chords is one file in `profiles/devices/`. A device with a
  mechanism of its own is one crate, the same two Cargo lines, one line in `registry.rs` and a
  checklist row. Nothing in domain, application, adapter-config, adapter-windows, another adapter
  or Mujina Settings changes for either (`docs/new-device.md`); a made-up device in the app's
  tests, a wait source on an event the test signals, plugs in that way.
- Mujina Settings lists every launcher and every device, and draws the rows of the launcher it
  shows and of the device in effect from their specs, translated with the catalogs the
  descriptors bring (`catalogs()`); nothing in it changes for a new one. A device's options are
  read and checked under `[device.<id>]`, but the template names none.
- `[device.button]` holds a modifier and a key. `mujinactl capture` and `probe` print every key
  held at once, and a longer chord needs a profile; Mujina Settings' capture says it could not
  store it.
- A device crate reading a HID report must keep the idle-cost rule: a report that streams at the
  controller's rate would wake the agent all the time, so its cost line (ADR-0014) is measured
  before it lands. Which of Raw Input or an overlapped `ReadFile` reaches a report while the
  vendor's app holds the device is unverified, per device.
- Launchers and devices stay compiled in and reviewed, as ADR-0007 wanted; one signed installer
  ships them all.
- Since stage 5 neither a state signal nor the Wi-Fi fix passes through the application ring:
  a launcher that can signal its state brings a wait source, and Steam's Wi-Fi fix, its WLAN
  reader included, lives in the Steam adapter, which hears of the agent's events through
  `observe`. Undoing what a runtime changed outside Mujina (Steam's marker) when a feature is
  switched off is still to come (leftover-1).

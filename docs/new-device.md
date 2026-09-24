# Supporting another device

A device is what Mujina maps an extra hardware button of: a OneXPlayer's "show desktop" button,
say. There are two kinds, and neither needs a change to Mujina's inner rings
([ADR-0013](adr/0013-compile-time-launcher-plug-ins.md)):

- **The button arrives as a key combination** (a chord), which is what most companion software
  sends: a OneXPlayer's is an injected `LWIN+D`. Such a device is **one file**, a profile in
  `profiles/devices/`. No Rust.
- **The button reaches Windows some other way**, such as a vendor HID report (the ROG Ally's
  Armoury Crate and Command Center buttons). Such a device is **a crate** with a few lines to list
  it.

## 1. Find out what the button sends

`mujinactl` comes with Mujina; from a checkout, on Windows, run it as
`cargo run -p mujina-app --bin mujinactl -- probe`.

`mujinactl probe` watches the keyboard for 30 seconds (`--seconds N` for longer). Press the button
a few times. It prints every key with its name and whether it came from a keyboard or from a
program, and ends with what to write down. It writes the same to `probe.txt`, beside
`doctor.txt` (`mujinactl paths` shows the folder); attach it to the pull request or the issue.
`mujinactl doctor` shows the two strings your firmware reports, which a profile is matched
against:

```
device:   "ONE-NETBOOK" / "ONEXPLAYER 3"
```

If `probe` sees no keys at all for the button, it is no chord: go to section 4.

## 2. Use it on your own device

In **Mujina Settings**, choose *Teach Mujina your button* → *Capture*, then press the button.
Mujina stores the combination in `config.toml` and uses it at once. By hand, the same is:

```toml
[device.button]
modifier = "LWIN"
key = "D"
injected_only = true
```

A button of your own wins over any profile. It holds one modifier and one key; a button that sends
more keys at once needs a profile (section 3), and Capture says it could not store it.

Key names: `LWIN`, `RWIN`, `LCTRL`, `LSHIFT`, `LALT`, `TAB`, `ESC`, `INSERT`, `F1` to `F24`,
letters and digits, and any other key by its code, such as `0xA3` (the right Ctrl key).
`injected_only = true` means a real keyboard pressing the same keys is left alone.

## 3. A profile, for everyone with the device

Create `profiles/devices/<id>.toml`, named after its `id`. A device id is written in lower case
letters, digits, `_` and `-`, beginning with a letter and ending with a letter or digit, with no
two of `_` and `-` in a row: `my-handheld`. `cargo xtask new-device <id> --profile` writes one
with a button to fill in; `--name "<name>"` sets its `name`, which otherwise the id gives
(`my-handheld` is "My Handheld"). It refuses an id spelt otherwise, one another profile or a
device crate has, and one that `[device]` gives a meaning of its own (`auto`, `none`, `button`,
`profile`, `custom`). A profile looks like this:

```toml
id = "my-handheld"
name = "My Handheld (home button)"

# Compared with the two firmware strings, ignoring case; `*` matches anything.
[match]
manufacturer = "ACME*"
product = "*"

# One entry per extra button.
[[buttons]]
key = "home"                 # a name for logs: lower case, digits and _
label = "Home button"        # what the button is called on the device; Mujina Settings shows it
chord = "LCTRL+F24"          # the keys it sends: those held first, the one that completes it last
injected_only = true         # the default: only when a program sends it
```

Mujina Settings shows the name and the labels as the file writes them, in every language: a
profile brings no translations.

A device with several buttons, some of them sending three keys, lists each. The OneXPlayer Mini's,
as HandheldCompanion defines them (not tried on the device):

```toml
[[buttons]]
key = "orange"
label = "Orange button"
chord = "LCTRL+LWIN+LALT"

[[buttons]]
key = "keyboard"
label = "Keyboard button"
chord = "LWIN+0xA3+O"

[[buttons]]
key = "desktop"
label = "Desktop button"
chord = "LWIN+D"
```

How a chord is read:

- Up to four keys. The last is the one that completes the chord; the ones before it are held, in
  whatever order the firmware presses them.
- Every key of a chord is held back from other programs until it is clear which button it is.
  Buttons that start with the same keys (`LWIN` above) are told apart by the next key. When the
  keys turn out to be no button at all, they are sent on in the order they came, unchanged, so
  nothing another program sends is lost.
- A profile is refused when the hook could not serve it: more than eight buttons, two buttons that
  send the same keys, or a chord made only of keys another button holds (`LWIN+D` beside
  `D+LWIN+E`), since the shorter would always fire first.
- A key chord is always held back; `suppression` may only be `"swallowed"`, the default.

Every button does what the one button always did: the launcher's menu in its UI, the overlay in a
game, home in Xbox mode. A meaning per button comes with the first device that needs one.

`profile = "auto"` takes the first profile that matches the machine, **in file-name order**. A
profile for some of the machines another one matches has to be named so that it sorts first.
`onexplayer.toml` matches every ONE-NETBOOK machine, so a profile for one OneXPlayer model sorts
before it: `onexplayer-mini.toml` does (`-` sorts before `.`), `onexplayer2.toml` and
`onexplayer_mini.toml` do not. Device crates (section 4) are asked before any profile.

Then run `cargo test -p mujina-adapter-keyboard`, on any system. It reads every file in the folder
and checks that it parses, keeps the rules, is named after its id and is not hidden by a profile
that sorts before it (the test names both files). There is nothing else to edit: the crate's build
script finds the new file. In the pull request, say which companion software and version you
tested with, and paste what `mujinactl doctor` and `mujinactl probe` printed. Not writing the
profile yourself? The *Device report* issue form asks for the same, and someone else can.

Switching between profiles, or to a button of your own, applies at once in a running agent.

## 4. A device crate, for a button that is no chord

When the button sends no keys (`probe` sees nothing), Mujina needs code that knows how the button
reaches Windows. `cargo xtask new-device <id>` makes the crate, `crates/adapter-<id>/`, and lists
it: a descriptor that matches no machine until you give it the firmware strings, one button
marked `Observed`, a runtime whose wait source is a `winutil` event that nothing signals yet,
`lang/de.po` and the tests below. It builds and passes every check as it is; each `TODO` in it is
what only the device can tell. `--name "<name>"` sets what Mujina Settings lists it as. The id is
spelt as in section 3; the command refuses the same ids, and `device` as well, which would name
the crate's types like Mujina's own (`DeviceRuntime`). The crate has `ring = "adapter"` in its
`[package.metadata.mujina]`, and brings:

- **A descriptor** (`mujina_application::device::DeviceDescriptor`): its id, its name, which
  machines it is (`matches`), its buttons (`ButtonSpec`: an id, a key, a label and its
  `Suppression`), and its options under `[device.<id>]`, if it has any. Mujina Settings shows those
  on its Setup page from the settings, as it does a launcher's ([new-launcher.md](new-launcher.md)):
  under the device's button, names, key combinations and numbers under *Advanced*. Their titles,
  help and choice values, the device's name and its buttons' labels are translated from the
  crate's `lang/de.po`, which `catalogs()` returns; write them as `Msg::new("…").english()` so
  that `cargo xtask i18n-check` holds the catalog to them. It builds everywhere; test it on Linux
  with `mujina_application::testing::device_conformance` (the `test-util` feature as a
  dev-dependency), which also checks that each catalog translates those texts. CI's Linux job
  runs clippy on it with `-D warnings`, so what in it only the Windows runtime uses has to be
  `pub` and re-exported from `lib.rs`, or go behind `#[cfg(windows)]`; otherwise clippy on
  Linux calls it never used.
- **A runtime** (`mujina_adapter_kit::plugin::DeviceRuntime`): `start` returns `DeviceParts`, the
  `DeviceButtons` the agent talks to and one or more wait sources (`winutil::wait::WaitSource`)
  whose handle Windows signals when a button is pressed, each reporting
  `AgentEvent::ButtonPressed(ButtonId)`. `start` must also take a selection whose id is `None`:
  the button switched off, which a later change may switch on, and then `DeviceButtons` is asked
  to take the device over. Check that in the crate's tests with
  `mujina_adapter_kit::plugin::runtime_conformance`. `checks` tells `mujinactl doctor` what
  matters ([A doctor check](#a-doctor-check)), `prepare` readies the device.
- **`pub static PLUGIN: DevicePlugin`**, pairing the two.

It is listed with one line under `[workspace.dependencies]` in the root `Cargo.toml`, one in
`crates/app/Cargo.toml` and one in `DEVICE_PLUGINS` in `crates/app/src/registry.rs`, which the
command adds, and a row in `docs/on-device-checklist.md`, which you add. The command also
updates `Cargo.lock` for the new crate; committing it with the crate is yours, since every build
in CI runs with `--locked`. Nothing in `domain`, `application`, `adapter-config`,
`adapter-windows`, another adapter or Mujina Settings changes. `crates/app/src/fake_hid.rs` is a
made-up device of this kind, used by the tests.

What such a crate must keep to:

- **No polling, near-zero idle cost** ([ADR-0014](adr/0014-threads-and-com.md)). A wait source's
  handle is an object Windows signals, such as the event of an overlapped `ReadFile` on the
  vendor's HID collection, or one a thread of the crate's own signals while it is otherwise blocked
  in the kernel. A HID collection that reports the controller's whole state many times a second
  would wake the agent all the time: compare the agent's `cost:` line with and without the device
  before the pull request.
- **Say what cannot be held back.** A program without a driver cannot keep a HID report from the
  vendor's own software. Mark such a button `Suppression::Observed`: the vendor's app reacts to
  the same press (ASUS says the Ally's Armoury Crate and Command Center buttons cannot be
  remapped), Mujina only adds to it and never passes it on. Add a `doctor` check that says which
  program reacts too ([A doctor check](#a-doctor-check)).
- **No driver, no injection** into the vendor's software and no reading of its memory; no filter
  or virtual-bus driver to hide the device.
- Adapters do not depend on each other. Launcher shortcuts go out through the `KeySender` port;
  a device crate sends no keys itself.

Which way a given vendor report can be read while the vendor's app holds the device (Raw Input, or
opening the collection and reading it) is for the crate to find out on the device; nobody has
checked it for the Ally, the Legion Go or the Claw yet. `probe` shows keys only; the vendor and
product ids of a HID device are in Device Manager (the device's *Hardware Ids*).

A device crate is chosen like a profile, by `auto` or its id. `auto` asks the crates first, in
`DEVICE_PLUGINS` order, then the profiles, so that a profile for a whole family (`onexplayer.toml`)
does not hide a crate for one of its machines; keep `matches` as narrow as the machines the crate
knows. Switching to it, or from it to another device, applies the next time Xbox mode is entered:
`mujinactl config set`, Mujina Settings and the agent's log say so. Switching the button off
applies at once.

### A doctor check

`mujinactl doctor` and the Status page of Mujina Settings run the checks a runtime returns: a
device runtime from `checks(&self)`, a launcher runtime from `checks(&self, options:
&OptionTable)`, with its options as read. A check of the device's own software, in the crate's
`runtime.rs`:

```rust
use mujina_application::Msg;
use mujina_application::doctor::{Check, Finding, Severity};
use mujina_winutil::process;

/// ACME Center, the handheld's own app, which reacts to the same press.
struct Companion;

impl Check for Companion {
    fn id(&self) -> &'static str {
        "acme center"
    }

    fn title(&self) -> Msg {
        Msg::new("ACME Center")
    }

    fn examine(&self) -> Finding {
        // Its process, lower case, as Task Manager's Details tab shows it.
        if process::pid_of_image("acmecenter.exe").is_some() {
            self.found(Severity::Warning, "running; it reacts to the press too")
        } else {
            self.found(Severity::Ok, "not running")
        }
    }
}
```

and in `impl DeviceRuntime for …`:

```rust
fn checks(&self) -> Vec<Box<dyn Check>> {
    vec![Box::new(Companion)]
}
```

- **`id`**: English and lower case, words apart, never changed once released: `acme center`,
  like Mujina's `developer mode` and Steam's `steam ui link`. `doctor` prints it, so that reports
  stay comparable, and Mujina Settings looks the finding up by it.
- **`title`**: what Mujina Settings calls the check, a `Msg` with its German in the crate's
  `lang/de.po`. `cargo xtask i18n-check` fails while the catalog lacks it; the conformance tests
  do not notice, since they see only the descriptor.
- **`examine`** returns `self.found(severity, detail)`, the detail in English. `Severity::Ok`:
  as it should be (`doctor` prints `ok`). `Severity::Warning`: Mujina works, but something will
  not behave as expected (`warn`). `Severity::Problem`: Mujina cannot do its job until it is put
  right (`FAIL`, and `mujinactl doctor` exits with an error). `.saying(Msg::new("…"))` adds the
  finding as a sentence for people, translated by the same catalog.

### What a ROG Ally crate would touch

Tried with `cargo xtask new-device rog-ally --name "ROG Ally"`: its crate passed clippy on
Windows and Linux, the tests, `arch-check` and `i18n-check` as made, and was then removed. The
command makes 1 to 3 below; 4 is yours:

1. `crates/adapter-rog-ally/**`: `Cargo.toml`, the descriptor, a runtime with a wait source,
   `lang/de.po`, and the tests (`device_conformance` for the descriptor, `runtime_conformance` for
   the runtime). As made, the descriptor has one button marked `Observed` and a `TODO` where
   ASUS's firmware strings go; the runtime's event is signalled by nothing yet.
2. One line in `[workspace.dependencies]` and one in `crates/app/Cargo.toml`, and the
   `Cargo.lock` that follows from them.
3. `&mujina_adapter_rog_ally::PLUGIN` in `DEVICE_PLUGINS`, `crates/app/src/registry.rs`.
4. A row in `docs/on-device-checklist.md`.

A finished crate fills in the `TODO`s: the descriptor matches ASUS's firmware strings and has two
buttons marked `Observed` (Armoury Crate and Command Center), the wait source is an overlapped
read on the vendor HID collection, and a `doctor` check names Armoury Crate SE.

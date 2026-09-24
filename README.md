<p align="center"><img src="docs/images/mujina.svg" width="112" alt=""></p>

# Mujina 狢

Mujina makes a Windows 11 handheld start like a console. It registers as the home app of Xbox
mode (Windows' "full screen experience") and opens **Steam Big Picture** in its place, so the
device boots straight into Steam.

> A *mujina* (狢) is a shapeshifter from Japanese folklore. It takes another form to get into
> places it could not enter otherwise. Windows only accepts a "gaming home app" as the Xbox mode
> home, so Mujina takes that form and starts Steam instead.

> [!NOTE]
> Mujina is young and tested on one device, a OneXPlayer 3. Other handhelds work, but their
> extra button needs a profile or a one-time capture in the settings.

**Contents:** [Why Mujina](#why-mujina) · [Features](#features) · [Requirements](#requirements) ·
[Install](#install) · [Settings](#settings) · [Troubleshooting](#troubleshooting) ·
[Building](#building) · [AI use](#ai-use) · [License](#license)

## Why Mujina

I got a OneXPlayer 3 and wanted it to work like SteamOS: turn it on, Steam opens. [AnyFSE] and
[OmniConsole] already do this for Xbox mode. I tried both, but neither fit my device and setup
completely. So I wrote my own, heavily inspired by them.

**I recommend AnyFSE for most people.** More people use it, it supports more launchers and
devices, and it is more mature.

### So why did I build Mujina?

- **I only use Steam.** So Mujina is built around it. Other launchers can be started, but most
  [features](#features) need Steam, such as the Wi-Fi icon and the device button.
- **I wanted no idle cost.** Mujina does not poll. It only waits on Windows events and uses
  almost no CPU while idle.
- **I wanted nothing inside other programs.** Mujina does not inject code or read other
  processes' memory. No driver, no service.
- **I wanted one file.** Written in Rust. One signed installer, no runtime needed.

## Features

| Feature | What it does |
|---|---|
| **Boot into Steam** | Starts Big Picture, or switches a running Steam to it, and gives it focus. Restarts it if it crashes. |
| **Device button** | The OneXPlayer "show desktop" button opens the Steam menu, or the Steam overlay in a game. Anywhere else in Xbox mode it goes back to the game or to Big Picture. On the desktop it still shows the desktop. |
| **Wi-Fi icon** | Big Picture on Windows shows Wi-Fi as disconnected (a Steam bug). Mujina shows the real network and signal. This is only cosmetic, nothing more: Wi-Fi works either way, and you still can't change networks in Steam. But the empty icon bothered me more than it probably should have. |
| **Loading screens** | Optionally a black screen while Steam starts, and Steam's "game is starting" screen kept in front while a game loads. |
| **Mujina Settings** | Works with touch and the controller, full screen in Xbox mode. Changes apply without leaving Xbox mode. |

## Requirements

- Windows 11 24H2 or later, with Xbox mode (full screen experience). Windows turns it on only
  for some handhelds. On others, unlock it first, for example with the
  [Xbox Full Screen Experience Tool].
- Steam
- Developer Mode. The installer turns it on. Windows offers an app as the Xbox mode home only
  for a capability claimed in a descriptor file that only Microsoft, the capability's owner, can
  sign, and it honours an unsigned one only in Developer Mode. So this applies to every tool of
  this kind; see [SECURITY.md](SECURITY.md) for what else Developer Mode changes.

## Install

1. Download `Mujina-Setup-vX.Y.Z.exe` from the [releases] and run it. That one file is all you
   need; the `.msix` and `.cer` beside it are the same package for experts.
   If SmartScreen warns, choose *More info → Run anyway* (Mujina uses its own certificate).
   Smart App Control, where it is on, blocks Mujina Setup without that choice: see
   [Blocked by Smart App Control](#blocked-by-smart-app-control).
2. Choose **Install**. The first time, Windows asks for administrator rights once, to turn on
   Developer Mode and trust the certificate.
3. Enter Xbox mode. Steam opens.

- **Update:** run a newer `Mujina-Setup`. It says which version it replaces, and leaves the
  home app as you had it. A Setup of an older version asks before it replaces a newer one.
- **Uninstall:** Mujina Settings → *System* → **Remove Mujina**. Mujina Setup gives your
  previous home app back first, then removes Mujina. If that row is missing (Mujina was not
  installed with Mujina Setup), use *Settings → Apps → Mujina*; your previous home app then
  comes back at the next sign-in. Developer Mode and the certificate stay; see
  [SECURITY.md](SECURITY.md).
- **Without a window:** `mujina-setup.exe --quiet` installs or updates (`--no-home-app` leaves
  the home app alone), `mujina-setup.exe --uninstall --quiet` removes. Windows still asks once
  for administrator rights on a first installation. Start it with
  `Start-Process .\Mujina-Setup-vX.Y.Z.exe -ArgumentList '--quiet' -Wait -PassThru` to read its
  exit code: 0 done, 1 failed, 2 wrong options, 3 the prompt was declined, 4 Windows is too old,
  5 a newer version is installed. `--about` prints what the file carries, `--help` the rest.
- **Something went wrong:** Mujina Setup names the step and the reason, and, run as you, keeps a
  log in `%LOCALAPPDATA%\Mujina Setup\setup.log`. *Copy details* puts both on the clipboard for an
  issue.

### "Allow Mujina to access your location?"

Windows asks this the first time you enter Xbox mode (with Wi-Fi not connected yet, once it is).
Mujina doesn't care where you are. It only reads the name and signal of your Wi-Fi, so Steam
can show the right Wi-Fi icon. Windows counts that as location data, so it shows this prompt.
The data stays on your device. See for yourself: all Wi-Fi code is in
[`wlan.rs`](crates/adapter-steam/src/wlan.rs) and the WLAN calls it makes in
[`winutil/src/wlan.rs`](crates/winutil/src/wlan.rs).

If you say **No**, everything still works. Steam just shows "connected" without the network
name, and the icon no longer follows changes in signal strength (connecting and disconnecting
still show). You can change it later under *Settings → Privacy & security → Location*; the
signal strength is followed again after Wi-Fi next connects, or the next time you enter Xbox
mode.

## Settings

A supported device needs no settings. For everything else, open **Mujina Settings** from the
Start menu. It has four pages: *Status* shows whether everything works, *Setup* has the
launcher, the device button (with *Capture*) and the fixes, *System* the home app and what
Windows allows, and *Help* the log and the language. Every change is saved at once, and a short
message says whether it applies now or the next time you enter Xbox mode.

![Mujina Settings, the Status page](docs/images/settings-status.png)

More pages: [docs/screenshots.md](docs/screenshots.md).

**In Xbox mode:** add Mujina Settings to Steam as a non-Steam game. The program is
`mujina-settings.exe` in `%LOCALAPPDATA%\Microsoft\WindowsApps`; the settings' *System* page shows
the full path to copy. It opens full screen and works with the controller.

**By hand:** the settings live in a plain `config.toml`. Edit it directly, or use
`mujinactl config set launcher.steam.wifi_indicator false`. Mistakes are reported by the doctor
and never stop Steam from starting.

**Another device:** if its extra button sends a key combination, *Capture* is all it takes.
To add a built-in profile for everyone with the device, see
[docs/new-device.md](docs/new-device.md), or open a
[device report](https://github.com/Tokahiro/mujina/issues/new/choose).

## Troubleshooting

```
mujinactl doctor    # checks everything Mujina depends on, also writes doctor.txt
mujinactl paths     # where the config, log and doctor.txt are
```

For a bug report, switch on *Write down everything* (Mujina Settings → Help, or
`mujinactl config set logging.level debug`), reproduce the problem, and attach `mujina.log`
(and `mujina.log.1`, if there is one) and `doctor.txt` to the
[issue](https://github.com/Tokahiro/mujina/issues).

### Blocked by Smart App Control

Windows says it blocked Mujina Setup, and offers no *Run anyway*. Where Microsoft's cloud
service cannot vouch for an app, Smart App Control looks at its signature and blocks it if it is
unsigned or its signature is invalid. Mujina Setup is signed with Mujina's own certificate, which
Windows does not know yet, so expect Smart App Control to block it (not yet confirmed on a
device). Microsoft: "There is currently no way to bypass Smart App Control protection for
individual apps"
([Smart App Control FAQ](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions)).

- **See its state:** Windows Security → *App & browser control* → *Smart App Control settings*.
  It is *On*, *Off*, or in *Evaluation*, where it blocks nothing
  ([Microsoft](https://support.microsoft.com/en-us/windows/security/windows-security/app-browser-control-in-the-windows-security-app)).
- **To install Mujina** it has to be off. That is your call: it guards against every app it
  does not know, not only Mujina. The FAQ says recent Windows updates let you turn it back on
  "without requiring a clean installation"; for some cases the same page still says turning it
  on means resetting the PC or reinstalling Windows.
- Mujina needs Developer Mode, and Microsoft lists "developer-mode has been configured" among
  the reasons Smart App Control is off. So expect it to stay off on a PC with Mujina
  ([SECURITY.md](SECURITY.md)).

## Building

Rust, in the version `rust-toolchain.toml` pins. Every crate builds and tests on any OS, the
Windows parts only on Windows; the package needs the Windows SDK (`makeappx`, `signtool`).

```
rustup toolchain install
cargo test --locked --workspace
cargo xtask arch-check
cargo xtask i18n-check
cargo xtask package
```

`cargo xtask dist --pfx <file>` builds what a release publishes, signed with a development
certificate of your own: see [CONTRIBUTING.md](CONTRIBUTING.md#a-release-shaped-build).
`cargo xtask new-launcher <id>` and `cargo xtask new-device <id>` start a launcher or a device.

| Document | Contents |
|---|---|
| [docs/architecture.md](docs/architecture.md) | How the crates fit together |
| [docs/adr](docs/adr/README.md) | Why things are the way they are |
| [docs/new-launcher.md](docs/new-launcher.md) | Adding a dedicated launcher |
| [docs/new-device.md](docs/new-device.md) | Adding a device: a profile, or a crate |
| [docs/on-device-checklist.md](docs/on-device-checklist.md) | What to test on a real device |
| [docs/signing.md](docs/signing.md) | Certificates, and how builds are signed |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Ground rules and workflow |
| [SECURITY.md](SECURITY.md) | What Mujina changes on your system, and reporting a vulnerability |
| [CHANGELOG.md](CHANGELOG.md) | Every release |

## AI use

- Of course I use AI, like every other tool that helps me develop. Algorithms have helped with
  coding for a long time: autocomplete, refactoring tools that rename and extract code, linters
  that suggest better code, formatters, the compiler's "did you mean …?". I know coding agents
  have a certain image, but to me they are the next step of the same thing. And instead of
  searching the web (sorry, Stack Overflow), I ask an AI chat to find things and to help me
  debug.
- Also for naming things: variables, functions, types, branches. I hate naming things and sit
  on it way too long.
- Definitely for commits and docs. I don't like writing them, and dyslexia doesn't make it any
  easier.
- I am a software engineer, not a designer, so Claude definitely helped me with the design.

## License

[MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option. Third-party notices are in
[NOTICE](NOTICE).

Mujina Settings and Mujina Setup are built with [Slint](https://slint.dev), under its
royalty-free licence, which asks for this badge:

<a href="https://slint.dev"><img src="https://slint.dev/logo/MadeWithSlint-logo-whitebg.png" height="80" alt="Made with Slint"></a>

Mujina is not affiliated with Valve, Microsoft or any device manufacturer.

[AnyFSE]: https://github.com/ashpynov/AnyFSE
[OmniConsole]: https://github.com/8bit2qubit/OmniConsole
[Xbox Full Screen Experience Tool]: https://github.com/8bit2qubit/XboxFullScreenExperienceTool
[releases]: https://github.com/Tokahiro/mujina/releases

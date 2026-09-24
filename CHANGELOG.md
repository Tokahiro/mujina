# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

Mujina is published in a new repository, whose history starts during the development of 0.28.0.
The versions before 0.28.0 are described here but not published any more.

## [0.28.0] - 2026-09-24

### Breaking

- `features.wifi_indicator` is gone; Steam's Wi-Fi fix is `launcher.steam.wifi_indicator` now.
  There is no migration: the old key is ignored like any key Mujina does not know (one note;
  `mujinactl config unset features.wifi_indicator` clears it), and an old
  `features.wifi_indicator = false` no longer closes Steam's debugging port. To keep it closed,
  set `launcher.steam.ui_link = false` before entering Xbox mode with this version; set later,
  also delete the marker file it left and restart Steam (SECURITY.md).
- Updating no longer replaces the sign-in check of Setup 0.24.1 to 0.27.0 (the Run value
  `MujinaCleanup`) nor removes 0.24.0's entry under Settings → Apps, and leaves their copy
  `%LOCALAPPDATA%\Mujina\mujina-setup.exe`. Right after updating from one of those versions, and
  before removing Mujina (until then the old check and this version's would both give the home
  app back at the same sign-in), remove them by hand in PowerShell, as you; keep
  `%LOCALAPPDATA%\Mujina\<family>\`, this version's own copy:
  - `Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'MujinaCleanup'`
  - `Remove-Item -LiteralPath "$env:LOCALAPPDATA\Mujina\mujina-setup.exe"`
  - only when updating from 0.24.0 itself: `Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Mujina'`

### Added

- Mujina Settings → System → *Remove Mujina*: gives the home app back first, then removes Mujina
  with Mujina Setup. The way to uninstall; Settings → Apps remains the fallback.
- `mujina-setup.exe --quiet [--no-home-app]` installs or updates without a window, and
  `--uninstall --quiet` removes; both with exit codes for scripts (see the README). `--log`,
  `--about` and `--help` too.
- CI builds on `main` come with `Mujina-Setup-dev.exe`, which installs the build beside a
  release for testing.
- `cargo xtask dist` builds what a release publishes, with a certificate of your own.
- `cargo xtask new-launcher <id>` and `cargo xtask new-device <id>` make the skeleton of a
  launcher or a device crate, one that builds and passes the checks, and list it in the three
  places Mujina lists one; `new-device <id> --profile` makes a key-chord device's profile.
  `--help` says how an id is spelt and which ids they refuse.
- Each press of the device button leaves one line in the log saying what it did and why, without
  naming what you had open, so a press that seems to do nothing can be told from one that never
  arrived.

### Changed

- On the desktop (the agent started with `--standalone`) the device button shows the desktop
  anywhere but Big Picture and a game, also while a game runs; it used to send the Steam
  overlay's shortcut into whatever was in front.
- Steam's debugging port has a switch of its own, `launcher.steam.ui_link` (*Use Steam's
  debugging port* in Mujina Settings, on by default). Big Picture's Home and Library pages, the
  "game is starting" screen and the Wi-Fi fix go through it; turning the Wi-Fi fix off no longer
  turns the others off. SECURITY.md says what the port powers and how to close it.
- Mujina Settings shows Steam's rows only while Steam is the launcher, and the starting-screen
  row only for a launcher that notices games. The Status page's Wi-Fi tile says "Not used" with
  another launcher.
- `mujinactl doctor` shows the launcher and what its options come to in place of the Wi-Fi line.
  A generic launcher is called by its program's name in the log. `steamwififeeder.exe` is only
  a conflict while Steam is the launcher, and the location permission is only checked while
  Steam's Wi-Fi fix, the one feature that needs it, is on.
- Launchers are compile-time plug-ins ([ADR-0013](docs/adr/0013-compile-time-launcher-plug-ins.md)):
  adding one is a crate and a line in the registry, see `docs/new-launcher.md`.
- Devices are plug-ins too. A device profile (`profiles/devices/*.toml`) may have several
  buttons, each a chord of up to four keys, such as a three-key `LCTRL+LWIN+LALT`; adding a
  profile is adding the file, nothing else. A device whose buttons are no key chords, such as a
  vendor HID report, can come as a crate of its own. See `docs/new-device.md`.
- `mujinactl capture` and `mujinactl probe` show every key held at once, e.g.
  `LCTRL+LWIN+LALT injected`. `[device.button]` holds two keys; for a longer chord `probe`
  prints a profile entry, and Mujina Settings' capture says it cannot store it.
- `mujinactl doctor` and the log name the device by its name, with the keys its button sends.
- Mujina Settings and Mujina Setup are built with Slint 1.18.
- Built with toml 1.1, toml_edit 0.25, tungstenite 0.30 and windows-sys 0.61. `config.toml` is
  read and written as before, and a file is no longer refused as not valid when it uses TOML 1.1
  (`\e`, a trailing comma in `{ }`, a time without seconds) or a dotted key into a table only
  implied so far, such as `steam.ui_link = false` under `[launcher]` after `[launcher.steam.x]`.
- One mistyped or unknown key in `config.toml` is now skipped with a note of its own; every other
  setting still applies. Before, it reset the whole file to the defaults.
- Mujina Settings answers at once when a setting is switched, also while Steam runs: its checks
  run in the background and show "Checking…" briefly.
- The log is kept as `mujina.log` and one older `mujina.log.1`, instead of being emptied when it
  grows too big. A crash is now written to the log.
- Mujina Settings and Mujina Setup speak the first of your Windows display languages they have,
  English or German, where before they looked at the first one only. Setup decides the way
  Settings does, so its steps and its buttons can no longer come in two languages. The Status
  page counts in digits: "2 things need fixing".
- Mujina Setup says in German too that the administrator prompt was declined.
- The Status page of Mujina Settings names its checks in German too; what each check found
  stays English, as in `mujinactl doctor`.
- Mujina Settings shows a launcher's and a device's own options from what they say about them:
  a new launcher's switches, program and other options appear on the Setup page, in its own
  words and translations, with no change to Mujina Settings. Any launcher that needs a value
  before it can be used (the generic launcher's program) is shown as chosen until that value is
  entered, as the generic launcher alone was before. The generic launcher's window class and
  process name are shown under Advanced only while it is chosen, their empty fields say "Not
  set", and the starting-screen row names the setting it needs ("Needs “Use Steam's debugging
  port”").
- The System page shows what the doctor finds for Xbox mode, Developer Mode, the location
  permission and the background agent, in the window's language and in the sentences it had
  before; the location permission only while Steam's Wi-Fi fix, which needs it, is on.
- `mujinactl doctor` counts a background agent that does not run in Xbox mode as a problem, as
  the System page did.
- Mujina Settings and Mujina Setup share their buttons, focus frames, cards and spinner, and
  name them, their switches, choices and fields to screen readers.
- Mujina Setup's window appears at once and says it is checking the device, then which version
  it installs over which: install, update, the same version again, or, for a newer version
  already installed, whether to replace it.
- An update leaves the home app as you had it: the switch starts on only if Mujina is the home
  app. A first installation still starts with it on.
- Mujina Setup arranges its check at sign-in before it makes Mujina the home app, and cannot be
  closed while a step runs, so that no removal through Settings → Apps can leave the home app
  pointing at a missing package.
- When a step fails, Mujina Setup says why in a sentence of its own (a newer version installed,
  the certificate not trusted, Mujina in use, …) with the code Windows gave, and offers *Try
  again*, *Copy details* and *Open log*. Only a declined prompt is called declined. Run as
  you, it writes what it does to `%LOCALAPPDATA%\Mujina Setup\setup.log`, which stays after a
  removal; run as administrator it writes a log only where `--log` says.
- Mujina Setup refuses Windows builds older than 24H2 before it asks for anything.
- The check at sign-in keeps itself when giving the home app back fails, and tries again at up
  to five sign-ins. Its copy of Setup and its Run value are per package family now
  (`%LOCALAPPDATA%\Mujina\<family>\`, `MujinaCleanup.<family>`), so a CI build installed beside
  a release no longer takes the release's.
- Mujina Setup makes Mujina the home app itself, with the rule `mujinactl` uses, instead of
  starting `mujinactl.exe`.
- Mujina Setup carries its package as resources attached after it is built, and the release
  signs them with it. `mujina-setup.exe --about` prints what a Setup carries.
- The agent's log says "Xbox mode is off" when that is why it leaves, before its closing lines.

### Removed

- The zip with `Install.ps1`, `Uninstall.ps1`, `Install.cmd` and `Uninstall.cmd`. A release has
  `Mujina-Setup-vX.Y.Z.exe`, the bare package and its certificate beside it, and
  `SHA256SUMS.txt`.

### Fixed

- The device button no longer does nothing after you quit a game that had opened a browser (or
  another app) from a link in it: with that app in front, the button now brings Big Picture up.
  Steam can keep counting such a game as running while the app is open, and the button used to
  send the Steam overlay's shortcut into the app. Mujina now finds a Steam game by the folder
  Steam installed it in, so it knows when nothing of the game runs any more, and a browser the
  game opened no longer counts as the game.
- The device button also finds a Steam game started through a launcher of its own (the EA app,
  Ubisoft Connect) or one whose own launcher has exited: in the game it opens the Steam overlay,
  elsewhere it leads back to the game. A game Steam did not install (a shortcut you added) that
  Mujina cannot find gets the overlay only when it fills its screen without a frame; in a window
  with a title bar, maximised or not exactly the screen's size, the button leads to Big Picture.
  Any other app in a full screen of its own (a browser with F11 or a video, a video player, a
  streaming client) is taken for such a game then, and gets the overlay's shortcut.
- The device button's keys are watched on a thread of their own, so a slow moment in the agent
  (a busy disk while the button is handled, say) can no longer make Windows drop the keyboard
  hook, after which the button did nothing until Xbox mode was entered again.
- Keys another program sends that start like the device button's chord (its `Win`+`E`, say) are
  sent on in the order they came, and as they came: arrows, the right `Ctrl` and `Alt` keep
  telling games they are those keys, and the touch keyboard's characters stay characters. Before,
  its later keys could overtake them, and a key could stay held down.
- Another launcher than Steam is brought back after every crash (`[launcher] on_exit`), not only
  after the first one, and also when it was started after Xbox mode was entered or restarted by
  something else than Mujina.
- A program that Windows has given the process id of a crashed launcher is no longer taken for
  the launcher: when that program ended, the home screen could come back unasked.
- The device button never does nothing in Big Picture: when Steam's UI link is down (for example
  Steam was started before Mujina's marker existed) it opens the menu by shortcut. Mujina now
  makes sure of the marker on every activation and says in the log when Steam needs a restart.
- Turning the Wi-Fi icon fix off no longer delays the Game Bar's "Library" by 10 seconds.
- Launcher arguments with spaces in quotes keep their quotes in Mujina Settings.
- Mujina Settings and `mujinactl` no longer lose each other's changes to `config.toml`: writes are
  locked, synced to disk and swapped in whole.
- A `[device.button]` that only says `injected_only`, left by clearing a captured button in
  0.22 to 0.27, is now noted as ignored. Remove it with
  `mujinactl config unset device.button.injected_only`.
- Copy in Mujina Settings works reliably (the clipboard is opened with its window as owner).
- With another launcher than Steam, Mujina no longer reads the Wi-Fi status, so Windows no longer
  asks for location access.
- Big Picture's Wi-Fi icon follows Wi-Fi changes on Windows 11 24H2 and later: connecting,
  disconnecting and the signal strength. Before, it kept the value it first showed. Without the
  location permission the icon shows a generic connection and follows connecting and
  disconnecting. The log says which changes it follows.
- The agent's last log lines are written before Windows ends the session.

### Security

- A `mujina:` link opened by a web page or another program on the desktop no longer starts
  Steam: Mujina only follows it while its agent runs or Xbox mode is on.
- The log no longer contains the name of your Wi-Fi network, so it can be attached to an issue
  as it is. With `level = "debug"` the name appears shortened, such as `H…(8)`.
- Mujina notes in `HKCU\Software\Mujina\Created` the files it creates outside its own folders
  (Steam's debugging marker), and uninstalling removes them.
- Mujina Setup's administrator part starts no other program and takes nothing from outside: it
  trusts only the certificate built into Setup itself. Before, a program named like a Windows
  tool placed next to Setup (in Downloads) could have run with administrator rights.
- Mujina Setup starts every program by its full path, loads DLLs only from Windows' system
  folder, and recognises the installed Mujina and its certificate exactly, not by name.
- The package and Mujina Setup carry a timestamp with their signature, so they keep installing
  after the certificate has expired.
- The package declares the `wiFiControl` device capability: Windows reports changes in Wi-Fi
  signal strength to a packaged app only if it declares this capability, and Microsoft says the
  capability needs the location consent. Mujina uses it for those reports alone (see
  SECURITY.md).
- Releases are signed with a new certificate of the same subject, `CN=Mujina`, so the package
  family stays. If a build signed with the earlier certificate (thumbprint
  `C21089F232C29920F2A17D78E389E130F3FD589A`) is installed, Mujina Setup asks once for
  administrator rights to trust the new one, and the old one can be removed in an administrator
  PowerShell with
  `Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Thumbprint -eq C21089F232C29920F2A17D78E389E130F3FD589A | Remove-Item`.
- Mujina Setup and the package come with a build attestation:
  `gh attestation verify Mujina-Setup-vX.Y.Z.exe --repo Tokahiro/mujina --signer-workflow Tokahiro/mujina/.github/workflows/release.yml`
  shows that the file was built by this repository's release workflow.

## [0.27.0] - 2026-09-23

### Changed

- Mujina Setup redesigned to match Mujina Settings: a spinner on the step at work and on the
  button, and a failure says "Setup stopped" above its reason.

### Fixed

- The icon shows 狢 at every size, also on the installer file and in title bars, and the
  taskbar and Start menu draw it without a plate behind it.
- Mujina Setup mentions the administrator prompt only on a first installation; updates do not
  ask.
- Installed without making it the home app, Mujina Setup no longer says to enter Xbox mode to
  start with Mujina.

## [0.26.0] - 2026-09-22

### Added

- Device profiles name their button (`button_name`); Mujina Settings shows it.
- Mujina Settings: B cancels capturing the device button.
- Mujina Settings: its language can be chosen on the Help page instead of following Windows.

### Changed

- Mujina Settings redesigned: four pages instead of eight, made for the controller and touch.
  Every change says whether it applied at once, applies next time, or was refused.
- Mujina's mark is the icon of the apps, the installer and the Start menu entry.
- Mujina Settings is added to Steam by its own program, `mujina-settings.exe` in
  `%LOCALAPPDATA%\Microsoft\WindowsApps`, instead of Explorer with a launch option.
- Release pages show the version's changelog entry instead of a list of commits.
- README and the other documents rewritten and brought up to date.

## [0.25.0] - 2026-09-22

### Added

- Mujina Settings and Mujina Setup in German when Windows is set to German. Doctor findings and
  error details stay English, as in `mujinactl` and the log.

## [0.24.1] - 2026-09-22

### Fixed

- Mujina appeared twice under Settings → Apps, and removing the package first left the second
  entry unable to uninstall. Now there is one entry; a check at sign-in gives the home app
  setting back once Mujina is gone, then deletes itself.

## [0.24.0] - 2026-09-22

### Added

- Mujina Setup: one file that installs, updates and uninstalls Mujina, instead of the zip with
  `Install.cmd`. It asks for administrator rights only the first time. The zip stays for now.

## [0.23.1] - 2026-09-22

### Fixed

- Capturing the device button worked, but the result was hidden: the button also showed the
  desktop. The app now comes back to the front with the result and ignores the controller
  while it waits.

## [0.23.0] - 2026-09-22

### Fixed

- The controller could only move through the menu of Mujina Settings, not the pages. Up and
  down now move between rows, left and right change values, A presses, B goes back.
- Capturing the device button saw no keys. It now runs as `mujinactl capture` in a hidden
  process.
- Starting Mujina Settings twice opened a second window; it now brings the first one forward.

## [0.22.0] - 2026-09-21

### Added

- Capture the device button in Mujina Settings: press it within ten seconds and it is stored as
  your button.

## [0.21.1] - 2026-09-21

### Changed

- The Start menu only lists "Mujina Settings". The "Mujina" entry booted Big Picture on the
  desktop, and Windows does not need it to offer Mujina as home app.

## [0.21.0] - 2026-09-21

### Added

- Mujina Settings works with the controller: D-pad or left stick to move, A to choose, B to go
  back, LB and RB to switch pages.
- In Xbox mode it opens full screen. The About page shows how to add it to Steam.

### Changed

- Drop-down lists and number boxes became rows of buttons, which work the same with touch,
  mouse and controller.

## [0.20.0] - 2026-09-21

### Added

- Mujina Settings can change settings: launcher, device button, timing, features, detailed log
  and home app. Invalid values are refused with the reason; the app says whether a change
  applies now or on the next Xbox mode start.

## [0.19.0] - 2026-09-21

### Added

- Mujina Settings, a new app in the Start menu. This first version shows status, doctor
  findings, the log and the version.

## [0.18.0] - 2026-09-21

### Added

- Settings changed with `mujinactl config` apply at once in Xbox mode. Only the launcher and
  the Wi-Fi icon wait for the next start.

### Changed

- The keyboard hook only exists while a device button is mapped.

## [0.17.0] - 2026-09-21

### Added

- `mujinactl config show`, `set` and `unset` change `config.toml` and keep its comments. Values
  Mujina would ignore are refused and the file stays unchanged.

## [0.16.0] - 2026-09-21

### Added

- `kind = "generic"` with a `[launcher.generic]` section boots into launchers Mujina does not
  know. They get no menu, game detection or Wi-Fi icon; the device button sends `ESC`.

## [0.15.0] - 2026-09-21

### Added

- `kind` under `[launcher]` selects the launcher. Steam is the default and so far the only one.

## [0.14.0] - 2026-09-21

### Changed

- In a game, the device button opens Steam's overlay directly instead of sending `Shift+Tab`
  into the game. `Shift+Tab` remains the fallback.

## [0.13.0] - 2026-09-21

### Fixed

- After the device button had brought Big Picture forward, it no longer opened Steam's menu,
  because `Ctrl+1` went to a window without keyboard focus. The menu is now opened directly.

## [0.12.1] - 2026-09-21

### Fixed

- Booting into Xbox mode kept the Windows welcome screen up again. Windows needs the home app to
  have a window, so there is one again, but it is see-through.

## [0.12.0] - 2026-09-21

### Changed

- The black launch screen is off by default. Xbox mode already shows its own backdrop, and
  every hand-over from the black screen to the launcher flashed. `launch_screen = true` brings
  it back.

## [0.11.0] - 2026-09-21

### Changed

- The launch screen now covers the launcher's window for 0.4 s while it is still empty, to hide
  a grey flash.

## [0.10.4] - 2026-09-21

### Added

- `launch_screen = false` under `[features]` switches the black launch screen off.

## [0.10.3] - 2026-09-21

### Fixed

- A flash of the desktop just before Steam's start-up animation: the launch screen closed
  before Steam's window had drawn anything.

## [0.10.2] - 2026-09-21

### Added

- Detailed log: where input goes in the seconds after a game ends, to find out why Big Picture
  sometimes ignores the controller then.

## [0.10.1] - 2026-09-21

### Fixed

- Entering Xbox mode with Steam already running made Big Picture vanish and reappear. The Wi-Fi
  icon no longer needs to reload Steam's UI.

## [0.10.0] - 2026-09-21

### Added

- While a game loads, Big Picture stays on its "game is running" screen instead of showing the
  game's page, which looked like a failed start. Switch off with `game_start_screen = false`.

## [0.9.2] - 2026-09-21

### Fixed

- The device button did not bring the game back: Windows took the foreground straight back. It
  now goes through a home activation, which Windows allows.

## [0.9.1] - 2026-09-21

### Added

- With a game running and something else in front, the device button brings the game back.

## [0.9.0] - 2026-09-21

### Added

- Anywhere else in Xbox mode (Xbox app, settings), the device button leads back to the
  launcher, like the home button.

## [0.8.2] - 2026-09-21

### Changed

- The home button takes a running Big Picture back to its home page, not only to the front.

## [0.8.1] - 2026-09-20

### Fixed

- The Game Bar's library entry did nothing, because Big Picture ignores `steam://` library
  links. The library is now opened through Big Picture itself.

## [0.8.0] - 2026-09-20

### Added

- The Game Bar's library entry opens Steam's library instead of its home page.

## [0.7.5] - 2026-09-20

### Fixed

- No "console UI not focused" warning while the lock screen is in front after booting.

## [0.7.4] - 2026-09-20

### Added

- Detailed log: every device button press with the keys sent and the window in front.

## [0.7.3] - 2026-09-20

### Fixed

- A "Windows refused the foreground change" warning when the launcher was in front already.

## [0.7.2] - 2026-09-20

### Added

- `level = "debug"` under `[logging]` switches the detailed log on.

### Changed

- The launcher starts before the launch screen, which saves about 0.1 s on boot.

## [0.7.1] - 2026-09-20

### Fixed

- Steam showed a black screen. The start-up change of 0.7.0 is withdrawn.

## [0.7.0] - 2026-09-20 [withdrawn]

### Changed

- The Wi-Fi icon was set up before Steam's UI loaded, to avoid a reload. Do not use this
  version: Steam stays black.
- No warning when Windows refuses the foreground behind the lock screen.

## [0.6.1] - 2026-09-20

### Fixed

- Steam seemed to start twice about 20 s after booting: the Wi-Fi icon reloaded Steam's UI too
  late. It now connects while Steam starts and reloads, if at all, before Big Picture shows.

## [0.6.0] - 2026-09-20

### Fixed

- Booting straight into Xbox mode:
  - Windows kept its welcome screen up until Steam had loaded and started Mujina three times.
  - Steam's start-up video was cut off by a reload for the Wi-Fi icon.
  - The device button did nothing until a game had been started once.
  - The agent's cost line was missing after shutting down from Xbox mode.

## [0.5.1] - 2026-09-20

### Added

- The agent logs what its session cost: CPU time, events handled, peak memory.
- `docs/new-launcher.md`: how to add another launcher.

### Changed

- `mujinactl probe` names the keys Windows creates from a controller, so they are not mistaken
  for a device button.

## [0.5.0] - 2026-09-20

### Added

- `mujinactl probe` shows which keys a button sends and suggests a `[device.button]` block.
- `mujinactl doctor` checks more: Developer Mode, location permission, the agent, conflicting
  tools, Steam's debugging port and the overlay shortcut.

## [0.4.3] - 2026-09-20

### Fixed

- The launch screen could close before Big Picture was back from its reload.
- The key tap that claims the foreground is only sent when it is really needed.

## [0.4.2] - 2026-09-20

### Changed

- The launch screen hides the reload of Big Picture that the Wi-Fi icon needs.
- The agent connects to a starting Steam about 3 s sooner.

## [0.4.1] - 2026-09-20

### Fixed

- After a crash, the restarted launcher came up behind other windows.

## [0.4.0] - 2026-09-20

### Added

- `config.toml` for features, launcher behaviour, shortcuts, timing and the device button.
  Mistakes are reported, never fatal.
- Device profiles as data files, chosen by the device's firmware strings. OneXPlayer is the
  first.
- The in-game overlay shortcut is read from Steam's settings.

## [0.3.0] - 2026-09-20

### Added

- A crashed launcher is started again. A normal exit or a crash within 30 s is left alone.
- A black launch screen covers the desktop while the launcher starts.

## [0.2.1] - 2026-09-20

### Fixed

- Windows' location prompt blocked the agent and cost it the keyboard hook.
- On a cold Steam start the Wi-Fi icon gave up connecting too early.

### Added

- Without location permission, the Wi-Fi icon shows a generic connection instead of nothing.

## [0.2.0] - 2026-09-20

### Added

- The agent, running while Xbox mode is on, waiting only on Windows events.
- Device button: the OneXPlayer "show desktop" button opens the Steam menu, or the overlay in a
  game.
- Wi-Fi icon: Big Picture shows the real network and signal.
- `Install.cmd` and `Uninstall.cmd`, which work with a double-click.

## [0.1.0] - 2026-09-20

### Added

- A package Windows accepts as Xbox mode home app. It starts Steam Big Picture, switches a
  running Steam to it, or brings it to the front.
- `mujinactl` with `doctor`, `register`, `unregister` and `paths`.
- `Install.ps1` and `Uninstall.ps1`; uninstalling gives the previous home app back first.

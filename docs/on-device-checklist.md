# On-device checklist

CI cannot enter Xbox mode, run Steam or press hardware buttons. Before a release, and for every
pull request touching the behaviour in question, walk through the relevant section on a real
device and note the result (Windows build, device, Steam client version).

## Mujina Setup

- [ ] The window appears at once, says "Checking this device…" for a moment, then the version
      line: "Make Mujina the console home…", "Update Mujina x to this version.", "This version is
      installed already…" or "A newer Mujina (x) is installed…"
- [ ] `Mujina-Setup-vX.Y.Z.exe --about | Out-String` names the package, its family and version
- [ ] Update over an existing installation: no administrator prompt; the plan shows "Update
      Mujina"; the home app switch is on only if Mujina is the home app now (switch the home app
      to the Xbox app in Mujina Settings first: after the update it is still the Xbox app);
      Xbox mode works as before
- [ ] Update while in Xbox mode with the agent running: the window says installing stops it;
      after the update the device button works again once Xbox mode is entered again
- [ ] The same version again: "Reinstall" runs without an error
- [ ] An older Setup over a newer version: the window asks "Replace it with this version?";
      "Replace" installs the older one; `--quiet` refuses it with exit code 5 and changes nothing
- [ ] While a step runs, the window's close button and Alt+F4 do nothing
- [ ] A failure (e.g. decline the prompt): "Setup stopped" names the reason; *Copy details* puts
      the step, the error and the log's path on the clipboard; *Open log* opens
      `%LOCALAPPDATA%\Mujina Setup\setup.log`; *Try again* checks the device again
- [ ] Settings → Apps lists "Mujina" once (the package, with its icon)
- [ ] Uninstall there, sign out and in again: the home app is no longer Mujina (the previous one
      is back), `HKCU\...\Run\MujinaCleanup.<family>` and `%LOCALAPPDATA%\Mujina` are gone;
      Task View works; `setup.log` has a "sign-in:" line for it
- [ ] The same with a give-back that fails: before uninstalling, set
      `HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration\GamingHomeApp` to a
      REG_DWORD by hand. Each sign-in logs the failure and the Run value stays; after the fifth
      the check removes itself anyway and the log says so. Installing again before then removes
      `HKCU\Software\Mujina\Setup\<family>`, so the next removal gets all five
- [ ] Mujina Settings → System → *Remove Mujina*: Settings closes, Mujina Setup's removal window
      appears; *Remove* gives the home app back, removes the package and finishes (Setup is not
      stopped by the removal: this checks that it runs outside the package); a few seconds after
      *Close*, `%LOCALAPPDATA%\Mujina\<family>` is gone (with `%LOCALAPPDATA%\Mujina`, if nothing
      else is in it); the row is missing for a package installed without Mujina Setup
- [ ] Updated over 0.27.0 or earlier: Setup adds `MujinaCleanup.<family>` and
      `%LOCALAPPDATA%\Mujina\<family>\mujina-setup.exe` and leaves the old version's check alone:
      the Run value `MujinaCleanup`, `"%LOCALAPPDATA%\Mujina\mujina-setup.exe" --cleanup <family>`,
      which does nothing while Mujina is installed. Right after the update, before Mujina is
      removed (until then 0.27.0's check and this version's would both give the home app back at
      the same sign-in), remove it in PowerShell as you, not as administrator:
      `Remove-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'MujinaCleanup'`
      and `Remove-Item -LiteralPath "$env:LOCALAPPDATA\Mujina\mujina-setup.exe"`. Over 0.24.0
      itself there is no Run value (the first command says so), but 0.24.0's own entry under
      Settings → Apps, which 0.24.1 to 0.27.0 removed:
      `Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Mujina'`.
      If Mujina was removed already and you have signed in since, 0.27.0's check has removed its
      Run value and copy itself, and the first two commands report them missing. Not the folder
      `%LOCALAPPDATA%\Mujina`: this version's copy is in it. Then
      `(Get-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run').GetValueNames() -like 'MujinaCleanup*'`
      lists no `MujinaCleanup` without a family, only `MujinaCleanup.<family>`, one per installed
      family; `Get-ChildItem -Force "$env:LOCALAPPDATA\Mujina"` lists no `mujina-setup.exe`, only
      one folder per installed family; and Mujina Settings → System still offers *Remove Mujina*
- [ ] `Start-Process .\Mujina-Setup-vX.Y.Z.exe -ArgumentList '--quiet' -Wait -PassThru` on a
      fresh device: one administrator prompt, exit code 0, Mujina is the home app;
      `--quiet --no-home-app` leaves the home app; declining the prompt gives exit code 3
- [ ] `--uninstall --quiet`: exit code 0, the home app back, the package gone
- [ ] `Mujina-Setup-dev.exe` from CI installs beside a release as a second "Mujina"; removing one
      (from its own Settings or Settings → Apps) leaves the other and its sign-in check. With the
      release installed first and the dev build made the home app over it: remove the release,
      then the dev build, and the app before both (e.g. the Xbox app) comes back; the other
      order ends the same way, never with a removed Mujina as the home app
- [ ] Fresh install after an uninstall: one administrator prompt only if Developer Mode was off
      or the certificate missing; afterwards the home app is Mujina and Xbox mode boots Steam
- [ ] "Open Mujina Settings" at the end opens the app
- [ ] Declining the administrator prompt says it was declined; accepting it continues
- [ ] Installed from a folder whose path has an apostrophe (e.g. `C:\Users\O'Brien\Downloads`)
- [ ] Smart App Control: note its state before installing (Windows Security → *App & browser
      control* → *Smart App Control settings*). Where it is *On*, note whether the release's
      Setup is blocked with no *Run anyway*, as the README's troubleshooting expects, and what
      Windows says. Where it is in *Evaluation*, Setup runs: note its state after Setup turned
      Developer Mode on. SECURITY.md expects it *Off* then, which no device has confirmed yet
- [ ] `mujina-setup.exe --uninstall`: the home app is back at once, the package is gone, and
      Steam's `.cef-enable-remote-debugging` is gone if Mujina created it (listed under
      `HKCU\Software\Mujina\Created`)

## Package and activation

- [ ] Mujina appears under Settings → Gaming → Xbox mode as home app (or, on builds without the
      picker, `mujinactl doctor` reports "Mujina is the home app")
- [ ] Entering Xbox mode starts Steam in Big Picture, focused and full screen
- [ ] Home button from inside a game returns to Big Picture
- [ ] With Steam running in desktop mode before entering Xbox mode: switches to Big Picture
- [ ] `mujina.log` shows one `activated:` line per activation — note the logged arguments for
      boot, home button and Game Bar "Home"
- [ ] Xbox mode stays stable although `mujina.exe` has exited (confirmed once, ADR-0001; keep
      checking, since a Windows update could change it)
- [ ] On the desktop without Xbox mode, opening `mujina://game` (Win+R) starts nothing; the log
      says "ignored activation". Inside Xbox mode the device button still returns to a game
- [ ] `mujinactl doctor`: no `FAIL`

## Booting straight into Xbox mode

- [ ] With "enter Xbox mode at start-up" on: the Windows welcome screen gives way promptly to a
      black screen, then Steam's start-up video plays uninterrupted, then Big Picture
- [ ] `mujina.log` shows one activation that does the work; any further one says "another
      activation is still bringing the launcher up"
- [ ] The device button works in Big Picture right away, without starting a game first
- [ ] Wi-Fi icon correct when Big Picture appears, and Steam's start-up plays exactly once. If
      the log has a "reloading" line, it must come before Big Picture is in front, never after
- [ ] Shutting down from Xbox mode leaves a `cost:` line in the log

## Agent

- [ ] After entering Xbox mode, `mujina.log` has an `[agent] started` line and exactly one
      `mujina.exe agent` process runs; pressing Home again does not start a second one
- [ ] The agent exits within a moment of leaving Xbox mode (`[agent] Xbox mode is off`, then
      `[agent] leaving after … s`)
- [ ] Task Manager: agent CPU time does not grow while the device sits idle in Big Picture

## Device button (OneXPlayer: show desktop)

- [ ] In Big Picture: opens the Steam menu; Start menu does **not** open
- [ ] In Big Picture, two quick presses: the menu opens and closes again, in that order
- [ ] In a game: opens the Steam overlay
- [ ] A link or button in a game opens a browser (or another app), with the browser **already
      running**: the button in the browser goes back to the game. `mujina.log` has one
      `device button pressed:` line per press, here "back to the game (in front: another app; the
      game's window is behind it; Xbox mode)"
- [ ] The same with the browser **not running** before, so that the game starts it: the button in
      the browser goes back to the game as well (the browser does not count as the game)
- [ ] Quit that game from inside it while the browser stays open, so that the browser comes to the
      front: the button brings Big Picture up. The log line says "Steam Big Picture (in front:
      another app; the launcher counts a game as running, but none of it runs; Xbox mode)". If it
      says "a game runs but its window cannot be found" instead, something Steam started is
      still alive and taken for the game: note what Task Manager shows below `steam.exe`. Also
      note whether `reg query HKCU\Software\Valve\Steam /v RunningAppID` is still non-zero while
      the browser is open, and 0x0 once it is closed
- [ ] Settings (or the Store) in front while a game counts as running (a game started, Settings
      opened from the Xbox mode's menu): the button leads back to the game, or to Big Picture;
      it never does nothing
- [ ] A Steam game started through a launcher of its own (EA app, Ubisoft Connect), or whose own
      launcher closes once the game runs: the button in the game opens the Steam overlay, and the
      log says "in front: the game"
- [ ] A shortcut you added to Steam for a program it did not install (an emulator, say), started
      through a launcher of its own so that Mujina cannot find its window: the button in the game
      opens the Steam overlay only when the game fills its screen without a frame, and the log
      says "in front: another app taken for the game". Note the facts the log names about the
      window if Big Picture comes instead: a game in a window with a title bar, a maximised one,
      or one not exactly its screen's size leads to Big Picture by design. Known limit, only
      there: any other app in a full screen of its own (a browser with F11 or a video, a video
      player, a streaming client) is taken for the game and gets the overlay's shortcut, which a
      streaming client passes on to the machine it shows
- [ ] No `device button pressed:` line names a program or a window title
- [ ] On the desktop (agent started with `mujina.exe agent --standalone`): still shows the desktop
- [ ] `Win`+`D` on a real keyboard still works everywhere
- [ ] Still works after sleep/resume and after a long session
- [ ] `mujina.log` says `device button: LWIN+D` once at start. Under the same settings the agent
      runs one thread more than 0.27.0, whose hook ran on the main thread (`keyboard-hook`,
      ADR-0014): compare
      `(Get-Process -Id <agent's pid>).Threads.Count` a few times, since Windows starts and ends
      pool threads of its own. Task Manager shows no thread names, and Process Explorer's
      Threads tab had no column for them either (a feature request of 2023); a debugger's
      thread list (Visual Studio, WinDbg) shows `keyboard-hook` and `sender`
- [ ] A program's `Win`+`E` (AutoHotkey `Send "#e"`, or the on-screen keyboard) still opens
      Explorer, and a program's lone `Win` still opens Start: what the hook held back is sent on
      in order, no key stays stuck
- [ ] With the agent running, *Teach Mujina your button* → *Capture* in Mujina Settings stores the
      button and it applies at once; *Which button your handheld has* → "None", then
      "Automatic", applies at once too
- [ ] In a game that loads heavily, the button still works right after the load (the hook no
      longer shares a thread with the agent's work)
- [ ] Idle cost: the `cost:` line of a session left idle in Big Picture is no higher than with
      0.27.0, before the hook got its own thread (ADR-0014)

## Devices with a crate of their own

One row per device crate listed in `DEVICE_PLUGINS` (`crates/app/src/registry.rs`): the device,
its buttons in Big Picture, in a game and on the desktop, what its own software does on the same
press, and the agent's `cost:` line after an idle session.

| Device | Big Picture | In a game | Desktop | Its own software | `cost:` when idle |
|---|---|---|---|---|---|

## Wi-Fi indicator

- [ ] Fresh install: Windows shows its location prompt once. While it is open the device button
      already works (the prompt must not block the agent). Answer **yes**: real network name.
      Answer **no**: generic "Wi-Fi" with medium signal, one warning in the log
- [ ] Cold start (Steam not running when Xbox mode is entered): icon lit without pressing
      anything, even when Steam first shows its update window
- [ ] First Xbox mode start after install: Steam must have been started by Mujina (marker file);
      if Steam was already running, restart Steam once
- [ ] Icon lit with plausible bars a few seconds after Big Picture appears (one UI reload)
- [ ] Log shows `Wi-Fi indicator: <n> bars` once, not repeatedly, and never the network name
      (with `level = "debug"` it appears shortened, e.g. `H…(8)`)
- [ ] Steam already running (desktop) when Xbox mode is entered for the first time with Mujina:
      the log says Steam must restart once; after a Steam restart menus and the icon work
- [ ] Device button in Big Picture while the Wi-Fi fix is on but Steam was started without the
      marker: the Steam menu still opens (by shortcut), it never does nothing
- [ ] Steam started by Mujina for the first time: `HKCU\Software\Mujina\Created` lists the marker
      file
- [ ] Switching network or turning Wi-Fi off and on updates the icon
- [ ] Location allowed: the log has no `Wi-Fi change notifications unavailable` line and no
      `although location is allowed` warning, and says `Wi-Fi changes followed: connecting,
      disconnecting and signal strength` once
- [ ] Walking away from the router and back changes the icon's bars within seconds; turning
      Wi-Fi off and on changes the icon within seconds
- [ ] Location denied (*Settings → Privacy & security → Location*, Mujina off), Xbox mode entered
      again: the log says once which changes are followed (note which: with `wiFiControl`
      Windows may grant signal changes all the same, which is harmless as the icon is generic),
      and the only warning is `Windows denies Wi-Fi details …`; turning Wi-Fi off and on still
      changes the icon
- [ ] On a first run the location prompt is visible, not hidden behind the lock screen or Big
      Picture; note when it appeared (at the agent's start, or when Wi-Fi first connected)
- [ ] First run with Wi-Fi not connected when Xbox mode is entered: after connecting and
      answering **yes**, the log's `Wi-Fi changes followed: connecting and disconnecting for now`
      is followed by `Wi-Fi changes followed: connecting, disconnecting and signal strength`
- [ ] Updating over an installed build that had location allowed: Windows does not ask for
      location again, and *Settings → Privacy & security → Location* still shows Mujina's choice
- [ ] Survives restarting Steam inside Xbox mode

## Steam UI link (`[launcher.steam] ui_link`)

- [ ] `launcher.steam.ui_link = false`, marker deleted, Steam restarted: `127.0.0.1:8080` does
      not answer; the Wi-Fi fix row is greyed out; the Game Bar's "Library" and the home button
      still bring Big Picture up, without its pages; the device button opens the menu by
      shortcut; `mujinactl doctor` has no "steam ui link" line
- [ ] `ui_link` on, `wifi_indicator = false`: Home and Library pages and the "game is starting"
      screen work; the icon is Steam's own; the menu opens by shortcut
- [ ] An old `config.toml` with `[features] wifi_indicator = false` and no `ui_link`:
      `mujinactl doctor` has one note, `features.wifi_indicator = false ignored: not a setting`,
      and no other; the first entry into Xbox mode creates the marker (the port follows
      `ui_link` alone); `mujinactl config unset features.wifi_indicator` comments the line out
      and the note is gone

## Generic launcher (`kind = "generic"`)

- [ ] With `[launcher] kind = "generic"` and `[launcher.generic]` pointing at a full-screen
      frontend: entering Xbox mode starts it, focused; `mujina.log` names it by its program's
      file name, as the Status page does
- [ ] `kind` back to `"steam"` (or the line commented out): Steam Big Picture as before
- [ ] With the frontend already running: entering Xbox mode brings it to the front instead of
      starting a second instance
- [ ] The device button in the frontend sends `ESC` (or the `[launcher] menu` chord if set)
- [ ] A `kind = "generic"` without an executable falls back to Steam; `mujinactl doctor` shows
      the note
- [ ] With the generic launcher, Mujina Settings shows neither Steam's rows nor the starting
      screen row, and the Status page's Wi-Fi tile says "Not used"
- [ ] Crash relaunch, twice: a minute after the frontend came up, `taskkill /f /im <its exe>`;
      it comes back. A minute later, the same again: it comes back again. `mujina.log` has a
      `launcher process … ended` line for each
- [ ] The same with the frontend started by hand after entering Xbox mode (closed first, then
      started from the Xbox app): the first kill brings it back too

## Settings without leaving Xbox mode

- [ ] In Xbox mode, `mujinactl config set features.button_remap false` says "Applied at once";
      the device button now does what it did without Mujina. The log says "device button: not
      mapped"
- [ ] `mujinactl config unset features.button_remap`: the button opens Steam's menu again
- [ ] `mujinactl config set launcher.steam.wifi_indicator false` says "next time Xbox mode is
      entered"; `mujinactl config set features.surprise true` is refused: not a setting
- [ ] Idle cost after a few changes still close to zero (`cost:` line)

## Mujina Settings

- [ ] Start menu shows "Mujina Settings" and no "Mujina", with the mark as icon; the window has
      the rail on the left and four pages: Status, Setup, System, Help
- [ ] Mujina is still offered as home app (Settings → Gaming → Xbox mode, or `mujinactl doctor`)
      and entering Xbox mode still boots the launcher
- [ ] Status matches `mujinactl doctor`; "Check again" updates it and the time
- [ ] Switching a setting answers at once, also while Steam runs; Status and System show
      "Checking…" briefly on entry, then the results
- [ ] Copy (mouse and controller) puts the value on the clipboard
- [ ] Another launcher: arguments with spaces in quotes (e.g. `--config "C:My Games.cfg"`)
      survive saving and reopening the Setup page
- [ ] Help: the log shows the newest lines first and "Open" opens the data folder; About shows
      the version and the "Made with Slint" badge; the language switches at once
- [ ] Touch: rows react to a tap; pages and the log scroll with a finger
- [ ] With the app closed, the agent's `cost:` line is as before (the app is not resident)
- [ ] Setup: switching "Use the device button" off and on in Xbox mode says "Saved and applied"
      and the button follows at once; `config.toml` shows the change with its comments intact
- [ ] Setup → Advanced: an incomplete shortcut (e.g. `LCTRL+`) is refused in red; the file is
      unchanged
- [ ] Setup → Advanced: `LWIN+D` as "Device button combination" is stored as `[device.button]`;
      emptying the field goes back to the chosen button
- [ ] Setup: "Another launcher" without a program only shows the Program row; nothing is saved
      until a program is entered
- [ ] System: "Give it back" and "Make Mujina the home app" change what `mujinactl doctor` says
- [ ] Added to Steam as a non-Steam game (program from the System page) and started from
      Big Picture: opens full screen. In the menu, up/down change the page, A opens it; on a
      page, up/down move a visible frame between rows, left/right change values, A presses,
      B returns to the menu; B in the menu closes the app; LB/RB turn pages; a held direction
      repeats; no double steps (Steam's own controller layout); Steam's overlay does not cover
      the app
- [ ] Every row of every page is reachable with the controller, including text fields (A starts
      editing) and the log (up/down scroll it)
- [ ] Starting the app a second time brings the open window forward, no second window
- [ ] While Big Picture is in front, controller presses do nothing in the app
- [ ] Close returns to Big Picture; the device button inside the app also leads back to it
- [ ] Device button → Capture: an overlay asks for the button and the controller does nothing
      else meanwhile. Press the button: even if it shows the desktop, the app comes back with
      "Captured LWIN+D. Saved…"; A or B closes the overlay. `config.toml` has
      `[device.button]`; the button works as before. Capture without pressing anything ends
      after 10 s with "No button seen" and no change; B while waiting cancels it at once

## Languages

- [ ] On a German Windows, Mujina Settings and Mujina Setup show German texts: menu, pages,
      hints, buttons, messages, the capture overlay and the installer's steps
- [ ] On an English Windows, everything is English
- [ ] With a display language Mujina lacks first (French, say) and German after it under
      Settings › Time & language › Language & region, both windows are German
- [ ] On a German Windows, the Status page names its checks in German ("Xbox-Modus",
      "Hintergrund-Agent", …); what each check found stays English
- [ ] On a German Windows, the System page says what Windows allows in German ("Verfügbar. Du
      bist gerade auf dem Desktop.", "Läuft nicht. Er startet mit dem Xbox-Modus …")
- [ ] Doctor findings and technical error details are English in both

## Idle cost

- [ ] Leave the device idle in Big Picture for ten minutes, then leave Xbox mode. The
      `[agent] cost:` line should show a CPU share of one core close to zero and only a handful
      of events; a share that grows with idle time means something is polling

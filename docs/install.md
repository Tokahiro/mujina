# Installing Mujina

The short version is in the [README](../README.md#install). This page has the details.

## Developer Mode

Mujina Setup turns on Developer Mode. Windows offers an app as the Xbox mode home only for a
capability claimed in a descriptor file that only Microsoft, the capability's owner, can sign,
and it honours an unsigned one only in Developer Mode. So this applies to every tool of this
kind; see [SECURITY.md](../SECURITY.md) for what else Developer Mode changes.

## Install, update, uninstall

1. Download `Mujina-Setup-vX.Y.Z.exe` from the
   [releases](https://github.com/Tokahiro/mujina/releases) and run it. That one file is all you
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
  [SECURITY.md](../SECURITY.md).
- **Something went wrong:** Mujina Setup names the step and the reason, and, run as you, keeps a
  log in `%LOCALAPPDATA%\Mujina Setup\setup.log`. *Copy details* puts both on the clipboard for an
  issue.

## Without a window

`mujina-setup.exe --quiet` installs or updates (`--no-home-app` leaves the home app alone),
`mujina-setup.exe --uninstall --quiet` removes. Windows still asks once for administrator rights
on a first installation. `--about` prints what the file carries, `--help` the rest.

To read the exit code, start it with
`Start-Process .\Mujina-Setup-vX.Y.Z.exe -ArgumentList '--quiet' -Wait -PassThru`:

| Code | Meaning |
|---|---|
| 0 | Done |
| 1 | Failed |
| 2 | Wrong options |
| 3 | The prompt was declined |
| 4 | Windows is too old |
| 5 | A newer version is installed |

## "Allow Mujina to access your location?"

Windows asks this the first time you enter Xbox mode (with Wi-Fi not connected yet, once it is).
Mujina doesn't care where you are. It only reads the name and signal of your Wi-Fi, so Steam
can show the right Wi-Fi icon. Windows counts that as location data, so it shows this prompt.
The data stays on your device. See for yourself: all Wi-Fi code is in
[`wlan.rs`](../crates/adapter-steam/src/wlan.rs) and the WLAN calls it makes in
[`winutil/src/wlan.rs`](../crates/winutil/src/wlan.rs).

If you say **No**, everything still works. Steam just shows "connected" without the network
name, and the icon no longer follows changes in signal strength (connecting and disconnecting
still show). You can change it later under *Settings → Privacy & security → Location*; the
signal strength is followed again after Wi-Fi next connects, or the next time you enter Xbox
mode.

## Blocked by Smart App Control

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
  ([SECURITY.md](../SECURITY.md)).

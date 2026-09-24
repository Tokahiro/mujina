# Security

## What Mujina changes on your system

### When installing

- **Developer Mode** is turned on: Mujina Setup sets `AllowDevelopmentWithoutDevLicense` under
  `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock`, and nothing else. Windows
  offers an app as the Xbox mode home only for a capability the package claims in a file of its
  own, `CustomCapability.SCCD`. Mujina's is unsigned, with the `<Catalog>FFFF</Catalog>` that
  Microsoft gives for "a desktop PC in Developer Mode" while a signed one is pending
  ([Hardware Support App guide](https://learn.microsoft.com/en-us/windows-hardware/drivers/devapps/hardware-support-app--hsa--steps-for-app-developers)),
  so the capability counts only in Developer Mode. A certificate from a certificate authority
  would not change that. What Developer Mode opens up:
  - Any program that asks for it (`SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE`) may create
    symbolic links without administrator rights
    ([CreateSymbolicLinkW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createsymboliclinkw)).
  - Turned on through that registry value, it "will not enable the SSH server or allow the
    device to be targeted for remote deployment and debugging"
    ([Developer Mode](https://learn.microsoft.com/en-us/windows/advanced-settings/developer-mode)).
  - **Smart App Control:** Microsoft lists "developer-mode has been configured" among the reasons
    Smart App Control is off
    ([Smart App Control FAQ](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions)).
    Expect it to be off on a PC with Mujina. Whether setting the value alone turns it off has
    not been checked on a device yet.
- **Mujina's certificate** is added to the machine's trusted people, so Windows accepts the
  package. Trusting it means trusting packages signed with it. The private key exists as an
  encrypted secret of this repository's `release` environment, which only jobs running for a
  `v*` tag can reach; only the release workflow's signing job uses it, and that job builds
  nothing and runs nothing that was built. Besides, the maintainer keeps one encrypted backup
  offline. CI builds are signed with a separate development certificate (`CN=Mujina Dev`) and
  install as a separate app ([docs/signing.md](docs/signing.md)).
- Both need administrator rights. Mujina Setup asks once, and only if one of the two is
  missing. Nothing else runs elevated. The administrator part takes no arguments and trusts
  only the certificate attached to its own file; while Mujina Setup is self-signed, Windows'
  prompt cannot show who asks, so run only a Setup you downloaded from this repository's
  releases (and compare `SHA256SUMS.txt`, or check its build attestation with
  `gh attestation verify Mujina-Setup-vX.Y.Z.exe --repo Tokahiro/mujina --signer-workflow Tokahiro/mujina/.github/workflows/release.yml`).
- **A sign-in check:** Mujina Setup keeps a copy of itself in
  `%LOCALAPPDATA%\Mujina\<package family>\mujina-setup.exe` and starts it at every sign-in
  (`HKCU\...\Run\MujinaCleanup.<package family>`). It asks Windows whether Mujina is still
  installed and ends. Once Mujina has been removed through Settings → Apps, it gives the home
  app setting back and deletes itself. If giving the home app back fails, it stays and tries
  again at the next sign-in, five times at most (counted in
  `HKCU\Software\Mujina\Setup\<package family>`, value `CleanupAttempts`), then removes itself
  anyway. The same copy is what *Remove Mujina* in Mujina Settings starts; it deletes itself
  once its window is closed.
- **A log:** Mujina Setup writes what it did, each step and each failure with its code, to
  `%LOCALAPPDATA%\Mujina Setup\setup.log` (and `setup.log.1`, at most 256 KB each), as you.
  It lives outside the folders a removal deletes, because it has to explain a removal, and the
  sign-in check's work after it. Delete the folder when you no longer need it. Mujina Setup
  started as administrator (*Run as administrator*, or `--quiet` from an elevated shell) writes
  no log unless `--log` names a file: any program running as you could turn that folder into a
  link to a folder of the system's, and creating or renaming the log through it as
  administrator would change system files.
- **Without a window** (`mujina-setup.exe --quiet`, `--uninstall --quiet`) Mujina Setup does
  exactly what the window would, with the switch's default: a first installation makes Mujina
  the home app unless `--no-home-app` is given, an update leaves the home app as it is, and a
  newer installed version is never replaced. The administrator prompt still appears when
  Developer Mode is off or the certificate is not trusted yet.

Developer Mode, the certificate and the log stay after uninstalling; the sign-in check goes.

### While running

Mujina runs as the logged-in user and never asks for administrator rights.

- **The home app setting**,
  `HKCU\Software\Microsoft\Windows\CurrentVersion\GamingConfiguration\GamingHomeApp`, plus a
  backup of its previous value. Uninstalling and `mujinactl unregister` restore it.
- **A list of the files Mujina created** outside its own folders, in
  `HKCU\Software\Mujina\Created`, so that uninstalling can remove exactly those. A file that
  existed before is never listed. Mujina Setup deletes listed files only without administrator
  rights, as you.
- **Steam's debugging port**, the Steam UI link: `[launcher.steam] ui_link`, on unless you switch
  it off (*Use Steam's debugging port* in Mujina Settings). Mujina creates
  `.cef-enable-remote-debugging` in the Steam folder. Steam then listens on `127.0.0.1:8080`
  with Chromium's debugging protocol. Mujina uses it to open Big Picture's Home and Library
  pages, to keep "game is starting" on screen while a game loads, to correct the Wi-Fi icon and,
  while that fix is on, to open Steam's menus without a keyboard shortcut. That port has no
  password: any program running as you can control Steam's interface through it. Such programs
  can already do worse, but if you do not want it, switch the link off
  (`mujinactl config set launcher.steam.ui_link false`), delete the file and restart Steam.
  Switched off, Mujina no longer creates the file, but one it created before stays until you
  delete it. Uninstalling deletes it if Mujina created it.
- **The `mujina:` link** exists so that Mujina's agent can bring the home role back. Any
  program or web page can open such a link, so Mujina follows it only while its agent runs or
  Xbox mode is on.
- **Location permission** (for the Wi-Fi icon): since 24H2, Windows counts the Wi-Fi network
  name and signal strength as location data and asks once. Mujina calls no location API. It
  reads these two values from the WLAN service and passes them to Steam on the same device,
  only while Steam is the launcher and its Wi-Fi fix is on. Declining makes the icon generic,
  and it then follows connecting and disconnecting but not the signal strength.
- **The `wiFiControl` capability** in Mujina's package: Windows tells a packaged app about
  changes in signal strength only if it declares this capability
  ([WlanRegisterNotification](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlanregisternotification)),
  and Microsoft says the capability needs the same consent to location access; apps are to
  check their location access through it
  ([Wi-Fi access and location](https://learn.microsoft.com/en-us/windows/win32/nativewifi/wi-fi-access-location-changes)).
  In principle it "allows apps to scan and connect to Wi-Fi networks"
  ([App capability declarations](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/app-capability-declarations)).
  Mujina only asks to be told when the connection or its signal changes; it scans for no
  network and connects to none. All of its Wi-Fi code is in two files,
  [`adapter-steam/src/wlan.rs`](crates/adapter-steam/src/wlan.rs) and
  [`winutil/src/wlan.rs`](crates/winutil/src/wlan.rs).
- **Keyboard hook** (for the device button): a low-level hook, on a thread of its own, that only
  reacts to the key combinations of the device's buttons, and by default only when a program
  sent them (`injected_only`). Keys that may begin such a combination are held back until it is
  clear, then sent on in order. It exists only while a device button is mapped. On a press, to
  tell the game Steam runs from other programs, the agent reads Steam's library list and the
  game's manifest, and the program path of each running process; it opens those processes only
  to ask Windows for that path, never to read their memory.
- **Settings changes, and the launcher's start**, are announced to the running agent through
  named events in your session. The agent then re-reads `config.toml`, or looks for the
  launcher's process to watch. Nothing else is passed.

### Mujina Settings

Runs only while its window is open. *Remove Mujina* on its System page starts Mujina Setup's
kept copy with `--uninstall`, outside Mujina's package, and closes. It reads the controller
(XInput) only while its window is in front. *Capture* watches the keyboard for at most ten
seconds and stores only the key combination it saw. Meanwhile it holds keys back from other programs where Windows allows it;
keys sent by a program with administrator rights cannot be held back.

### Never

Code injection, reading or writing other processes' memory, drivers, services, network traffic
beyond `127.0.0.1`.

## Reporting a vulnerability

Please do not describe it in a public issue. Use GitHub's private vulnerability reporting: on
this repository's *Security and quality* tab, choose *Report a vulnerability*, fill in the form
and submit it
([Privately reporting a security vulnerability](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/report-privately)).
You will get an answer there within a week.

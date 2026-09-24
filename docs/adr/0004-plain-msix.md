# ADR-0004: A plain, self-contained MSIX

Status: accepted; "no installer of our own" superseded by ADR-0012 (an installer that carries
this package); the identity clause superseded by ADR-0015 (the package keeps its constant
certificate, the installer gets an identity of its own)

## Context

Windows builds the home app list from package manifests, so an unpackaged executable cannot be a
home app. Eligibility rests on three undocumented pieces that AnyFSE, OmniConsole and
FullScreenExperienceShell arrived at independently: the `windows.gamingApp` app extension, the
custom capability `Microsoft.appCategory.gamingHome_8wekyb3d8bbwe`, and an unsigned
`CustomCapability.SCCD` (hence Developer Mode).

Options were a package with external location (payload in `Program Files`, as AnyFSE does) or a
plain MSIX with the payload inside.

## Decision

Plain MSIX. The executables are statically linked against the C runtime, so the package has no
framework dependencies. Registry and file-system write virtualization are disabled because
Mujina manages real user settings.

## Consequences

- Install, update and uninstall are what Windows already does for packages; no installer of our
  own, nothing left behind in `Program Files`.
- The signing certificate's subject is part of the package family name and therefore of the
  application user model id stored as home app. It must never change. *Superseded by ADR-0015,
  which keeps this for the package (`CN=Mujina`, constant) and gives the installer, the file
  users download, a signature of its own.*
- Everything about the manifest lives in `packaging/` and `xtask`, the one place to adapt when a
  Windows build changes the rules.

## What changed

<!-- What it does, and why. Link the issue it fixes, if there is one. -->

## On a device

<!--
CI cannot enter Xbox mode, run Steam or press hardware buttons. Name the sections of
docs/on-device-checklist.md you walked through, with the Windows build, the device and the
launcher's version, or say why none applies.
-->

- Sections:
- Windows build, device, launcher:

## Checklist

- [ ] `cargo fmt`, `clippy`, `test`, `cargo xtask arch-check`, `cargo xtask i18n-check` and
      `cargo deny --locked check` pass (CONTRIBUTING.md, *Workflow*)
- [ ] A user-visible change has its line under `[Unreleased]` in CHANGELOG.md
- [ ] Docs say what the change changes: README, SECURITY.md, the guides in `docs/`, an ADR for a
      design decision
- [ ] A new text in a window, or in a descriptor, has its German too
- [ ] A dependency change comes with its `Cargo.lock`

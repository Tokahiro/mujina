# Contributing

## Ground rules

- **Dependencies point inward.** `domain` depends on nothing. `application` depends on `domain`.
  Adapters depend on `application`/`domain`/`winutil`/`adapter-kit`, never on each other. Only
  `app` wires adapters: Mujina Settings reaches them through `app`'s `tool` module, and Mujina
  Setup uses `adapter-windows`' home app registry alone. Every crate names its ring in its
  `Cargo.toml` (`[package.metadata.mujina] ring = "…"`); the rings table in
  [docs/architecture.md](docs/architecture.md) says what each ring may use.
  `cargo xtask arch-check` enforces this and fails for a crate without a ring; do not work
  around it.
- **No polling.** If something has to be noticed, find the event for it (registry notification,
  WinEvent hook, process handle, socket event, …). A timer is acceptable only as a bounded,
  one-shot deadline.
- **`unsafe` lives in `winutil` and the adapters**, one Win32 call per block where possible, each
  with a `// SAFETY:` comment stating the contract. `domain` and `application` forbid it.
- **Rules go into the domain, with tests.** If a behaviour can be expressed without an OS call,
  it belongs in `crates/domain` and gets a unit test there.
- **No injection into other processes.** Ever. It is what gets tools like this flagged by
  antivirus and anti-cheat software.

## Workflow

1. Branch from `main`.
2. Before you push, run what CI runs:

   ```
   cargo fmt --all
   cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
   cargo test --locked --workspace --all-features
   cargo xtask arch-check
   cargo xtask i18n-check
   cargo deny --all-features --locked check
   ```

   Every build uses the Rust version `rust-toolchain.toml` pins; `rustup toolchain install`,
   run in the repository, installs it. The Windows parts build only on Windows; on another
   system the same commands check the rest. CI runs clippy on Linux too, where code that only
   Windows code uses is never used. On Windows, check that side as well, without a Linux
   linker; the target is added once with `rustup target add x86_64-unknown-linux-gnu`:

   ```
   cargo clippy --locked --workspace --all-targets --all-features --target x86_64-unknown-linux-gnu -- -D warnings
   ```

   `i18n-check` builds slint-tr-extractor, of the Slint version in `Cargo.lock`, into
   `target/tools` the first time. [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) is
   installed once with `cargo install --locked cargo-deny`.
3. Commit using [Conventional Commits](https://www.conventionalcommits.org/)
   (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`).
4. Describe user-visible changes under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md). That
   entry becomes the text of the release page; a release without one fails.
5. New texts in Mujina Settings or Mujina Setup need a German translation too: `@tr("...")` in
   the `.slint` file plus an entry, with the component as its context, in the crate's
   `lang/de/LC_MESSAGES/*.po`. Rust passes states and data, not sentences; a text Rust has only
   at run time is a `Msg::new("...")` of `mujina-i18n`, with an entry without a context. A
   launcher's or a device's texts (its name, its options' titles and help, its checks' titles)
   are `Msg`s too, in its crate's `lang/de.po`. `cargo xtask i18n-check` lists every text that
   has no translation, and every entry nothing uses any more.
6. Open a pull request. Behaviour that CI cannot exercise (anything that needs Xbox mode, Steam
   or real hardware) must be verified on a device; say in the PR what you checked, following
   [docs/on-device-checklist.md](docs/on-device-checklist.md).

Significant design decisions get a short record in [docs/adr](docs/adr/README.md), which has
the index and a template.

## The lockfile

`Cargo.lock` is committed, and every build, here and in CI, runs with `--locked`: it fails
rather than change the lock. So a change that needs a different lock brings it along, in the
same pull request:

- **A dependency added or changed** in a `Cargo.toml`: any cargo command without `--locked`,
  such as `cargo check`, adds what the change needs and leaves the rest; to move one crate that
  is already there to a newer version, `cargo update -p <crate>`. Commit the lock with the
  manifest.
- **A new crate in the workspace:** Cargo adds it to the lock at the first command without
  `--locked`; `cargo xtask new-launcher` and `new-device` do it for you.
- **Routine updates** come from Dependabot, as one grouped pull request a week for compatible
  versions and one per major version. Leave them to it rather than running a bare
  `cargo update`, which moves every dependency at once.

A pull request whose lock changes more than its manifests explain gets asked why.

## A release-shaped build

A release publishes `Mujina-Setup-vX.Y.Z.exe` with the signed package attached, the package and
its certificate beside it, and `SHA256SUMS.txt` (ADR-0015). To build the same on your machine,
for a device test of the installer itself:

1. Create a development certificate once, as [docs/signing.md](docs/signing.md#the-development-certificate)
   describes (subject `CN=Mujina Dev`), and export it as a PFX. Never use the release
   certificate here: signtool keeps a copy of the key in your user's key store.
2. In PowerShell:

   ```powershell
   $env:SIGNING_PFX_PASSWORD = '<its password>'
   cargo xtask dist --pfx C:\path\to\mujina-dev.pfx
   ```

   `MSIX_PUBLISHER` must be the certificate's subject; unset, it is `CN=Mujina Dev`.
3. `target/dist` then holds `Mujina-Setup-vX.Y.Z.exe` (signed, the package attached), the
   package, `Mujina.cer` and `SHA256SUMS.txt`. The task ends with the installer's own `--about`.

The steps are those of the release workflow: `cargo xtask package`, `cargo build --release -p
mujina-setup`, then `packaging/sign.ps1` for the package, `packaging/attach-payload.ps1` to
attach it, and `packaging/sign.ps1` for the installer. A build of Mujina Setup straight from
cargo carries no package and says so.

## Releasing

Only the maintainer can push a `v*` tag, and pushing one is the decision to release
([docs/signing.md](docs/signing.md#gating-the-release-key)).

1. Bump the version on a branch from `main`, in one commit `chore(release): X.Y.Z`:
   - `version` in the root `Cargo.toml`;
   - `Cargo.lock`, with `cargo update --workspace`, which moves only the workspace's own crates;
   - `CHANGELOG.md`: `## [Unreleased]` becomes `## [X.Y.Z] - YYYY-MM-DD`.

   `cargo xtask version-check vX.Y.Z` and `cargo xtask release-notes vX.Y.Z` show what the
   release workflow will check and print.
2. Open a pull request and merge it once CI is green. `main` takes no direct pushes.
3. Wait until CI has passed on `main` for the merged commit, then tag that commit:

   ```
   git fetch origin
   git tag -a vX.Y.Z origin/main -m "Mujina X.Y.Z"
   git push origin vX.Y.Z
   ```

4. `release.yml` does the rest. Its `preflight` job checks the tag against `Cargo.toml`, the
   CHANGELOG entry, and that CI passed on `main` for exactly this commit (it waits up to 45
   minutes for a CI run still going); then it builds, signs and publishes the release.

A tag on a commit CI has not passed on `main` fails in `preflight`, before anything is built.
Move it to the right commit: `git push origin :refs/tags/vX.Y.Z`, then
`git tag -fa vX.Y.Z <commit> -m "Mujina X.Y.Z"` and push it again.

## Adding a device

If the device's extra buttons arrive as keyboard chords, no code is needed, only a profile file:
`cargo xtask new-device <id> --profile` writes `profiles/devices/<id>.toml` with what to fill in.
A button that reaches Windows some other way (a vendor HID report) needs a device crate:
`cargo xtask new-device <id>` makes `crates/adapter-<id>/`, with a descriptor, a runtime with a
wait source and the tests, and lists it in `[workspace.dependencies]`, `crates/app/Cargo.toml`
and `crates/app/src/registry.rs`. See [docs/new-device.md](docs/new-device.md).

## Adding a launcher

One adapter crate with a descriptor and a runtime, one line each in `[workspace.dependencies]`
and `crates/app/Cargo.toml`, and one entry in `crates/app/src/registry.rs`.
`cargo xtask new-launcher <id>` makes the crate, with an example option, its German catalog and
the tests, and adds the three lines: see [docs/new-launcher.md](docs/new-launcher.md).

Both commands refuse an id that is taken, that the configuration gives a meaning of its own,
that users could not write in `config.toml`, or that would name the crate's types like
Mujina's own (`home`, `session`, `launcher`, `device`); `--help` gives the rules. What they make
builds and passes every check; each `TODO` in it is what only knowledge of the launcher or the
device can fill in.

## License

Contributions are accepted under the project's dual license (MIT OR Apache-2.0), without
additional terms.

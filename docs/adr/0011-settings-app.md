# ADR-0011: A settings app of its own, built with Slint

Status: accepted; amended in 0.23.0, 0.26.0, 0.28.0 and stage 9 (the *Amended* paragraphs
below)

## Context

Everything Mujina can be told goes through `config.toml` and `mujinactl`. A handheld has no
keyboard at hand and its owner lives in Xbox mode, so a settings app is wanted that is modern,
works with touch and later with the controller, and looks at home on Windows 11. Two
constraints come from the project itself: the resident agent must not get any more expensive
while idle (ADR-0002), and CI is the only compiler, so the toolkit must build with plain Cargo.

*Amended in stage 9:* CI is no longer the only compiler. `rust-toolchain.toml` pins the Rust
version, and contributors build, test and package on their own machines with it
([CONTRIBUTING.md](../../CONTRIBUTING.md)). What stays is the constraint that mattered: plain
Cargo, no second toolchain.

Considered: WinUI 3 (the real Windows 11 look and controller focus for free, but realistically
C# and a second toolchain), Tauri with WebView2 (any look, the browser gamepad API, but a web
toolchain in CI and a browser engine per window), iced and egui (pure Rust, but neither looks
like Windows 11 without a lot of styling), and Slint.

## Decision

- **Slint** with its `fluent-dark` style: pure Rust, compiled `.slint` files, touch support,
  one static executable, no runtime to install.
- **A separate executable**, `mujina-settings.exe`, in its own crate (`crates/settings-gui`) and
  with its own Start menu entry (a second `<Application Id="Settings">` in the package). It
  exists only while it is open; `mujina.exe` never links Slint.
- It reuses the composition root (`mujina-app`) rather than wiring adapters again, so it shows
  exactly what `mujinactl doctor` shows. `cargo xtask arch-check` allows that one dependency and
  forbids anything from depending on the settings app.

  *Amended in 0.28.0:* through `mujina_app::tool` only, the module it shares with `mujinactl`:
  the settings app names no adapter, and arch-check forbids it one.
- Changes go through `SettingsStore` and the live-reload signal (ADR-0009, ADR-0010), like
  `mujinactl config set`.
- **Licence**: Slint's Royalty-free Desktop Applications License 2.0, whose condition is the
  Slint attribution in the app; the About page shows Slint's own `AboutSlint` widget. Recorded in
  `NOTICE` and allowed in `deny.toml`.

  *Amended in stage 9:* the licence asks for either of two things (its section 2): (a) the
  `AboutSlint` widget in an About screen reachable from the app's top-level menu, or (b) the
  Slint attribution badge on a public web page, preferably where the binaries are downloaded.
  Since 0.26.0 the Help page shows the badge as an image, which is literally neither, and
  Mujina Setup, the file users download, uses Slint too and has no About screen. Mujina takes
  (b): the README and every release page from 0.28.0 on (`cargo xtask release-notes` adds it)
  show the official badge, for both apps. The releases before 0.28.0, published only in the
  private predecessor of this repository (ADR-0015), were without it. The Help page keeps its
  badge.

- **Controller**: XInput is read every 33 ms, only while the app is open and its window is in
  front, and turned into Tab, Shift+Tab, Space and Escape, which Slint's focus handling already
  understands. Controls that need arrow keys once open (drop-down lists, spin boxes) are not used.
  In Xbox mode the window is full screen.

  *Amended in 0.23.0:* Slint's Tab order does not reach pages shown with `if`, so the controller
  could not get past the menu. Navigation is now the app's own: static pages switched with
  `visible`, and the controller mapped to arrow keys, Enter and Escape. Capturing the device
  button runs as `mujinactl capture` in a hidden process of its own, because a keyboard hook in
  the app's process received no keys.

  *Amended in 0.26.0:* the app follows a design of its own, four pages with a console look,
  instead of the Fluent style. Its controls are drawn by the app; no std widgets are left. Each
  page is one FocusScope that knows its current row, and Rust (`nav::step`) decides where up and
  down go, skipping rows that are switched off. Inter, the Phosphor icons and the mark are
  compiled in (about 1.2 MB); Slint's attribution is its official "Made with Slint" badge on the
  Help page.

- **Languages**: English and German, following the Windows display language. The window's
  texts are Slint `@tr` strings with `.po` files compiled in (`lang/de/LC_MESSAGES/`); texts
  built in Rust follow the same choice. Doctor findings stay English: they are shared with
  `mujinactl` and the log, where one language keeps reports comparable.

  *Amended in 0.28.0:* Rust no longer writes sentences in both languages. It passes states and
  data, and the `.slint` files word them with `@tr`; a text Rust has only at run time is a `Msg`
  of `mujina-i18n`, looked up in the same catalog. Settings and Setup decide the language the
  same way at start-up: `interface.language` (Settings only), else the first of the Windows
  display languages the app has, else English. Each doctor check is named in that language by
  the title it gives itself, from the catalog of its crate; what the doctor found stays English,
  but for a check that also says it in a sentence for people (`Finding::summary`), which the
  System page shows in that language. `cargo xtask i18n-check` keeps the catalogs complete
  ([architecture.md](../architecture.md#languages)).

## Consequences

- A second, larger executable in the package (Slint's renderer and windowing); the agent's size
  and idle cost are unchanged.
- The Fluent style is Slint's rendition of Windows 11, not WinUI itself; close, not identical.
- Controller navigation is Mujina's to add (keyboard focus exists; the gamepad must be mapped to
  it), and so is a full-screen layout for Xbox mode.

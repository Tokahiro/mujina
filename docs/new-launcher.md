# Supporting another launcher

## First: try the generic launcher

In **Mujina Settings → Setup**, choose *Another launcher* and enter its program and arguments
(its window class and process name are under *Advanced*). Mujina then starts it in Xbox mode and
brings it to the front ([ADR-0008](adr/0008-generic-launcher.md)). For many launchers that is
enough.

A dedicated adapter is worth it for what the generic launcher cannot do: detect games, open menus
and an overlay, navigate, wake the agent when its state changes, run features of its own beside
Mujina's (Steam fixes its Wi-Fi icon), add doctor checks, have options of its own.

## What you touch

A launcher is a compile-time plug-in ([ADR-0013](adr/0013-compile-time-launcher-plug-ins.md)):

1. a new crate, `crates/adapter-<id>/`, with its descriptor, its runtime, its checks and
   `lang/de.po`;
2. one line in `[workspace.dependencies]` of the root `Cargo.toml`;
3. one line in `crates/app/Cargo.toml`;
4. one entry in `LAUNCHERS` in `crates/app/src/registry.rs`;
5. `Cargo.lock`, which Cargo updates for the new crate: commit it, since every build in CI runs
   with `--locked`;
6. a section in `docs/on-device-checklist.md`, because CI can neither start your launcher nor
   enter Xbox mode.

`cargo xtask new-launcher <id>` does 1 to 5: it makes the crate, with an example option (where
the program is), its template block, `lang/de.po`, a runtime on the kit's `launch` and
`focus_ui`, and the tests, and lists it. `--name "<name>"` sets what Mujina Settings lists it
as; otherwise the id gives it (`big_box` is "Big Box"). What it makes builds and passes
`cargo xtask arch-check`, `i18n-check` and the tests on Windows and Linux; every `TODO` in it is
what only knowledge of your launcher can fill in. It refuses an id that is taken, one of
`[launcher]`'s own keys, one users could not write (lower case letters, digits and `_`,
beginning with a letter and ending with a letter or digit, no two `_` in a row), and `home`,
`session` and `launcher`, which would name the crate's types like Mujina's own
(`HomeLauncher`). `cargo xtask new-launcher --help` says the same.

Nothing in `domain`, `application`, `adapter-config`, `adapter-windows`, another adapter or
Mujina Settings changes. If something has to, the seam is wrong: open an issue before working
around it.

**In Mujina Settings** your launcher is listed by its descriptor's name, and its options appear
on the Setup page from its settings, with nothing to add there:

- a **switch** (`Toggle`) joins *What Mujina fixes*, with its help and when it takes effect
  (`applies`); one that `requires` another is greyed out while that one is off;
- a **program** or other file (`Text` in `Path`), **arguments** (`TextList`, edited as a command
  line with quotes around an argument with spaces), a **choice** and anything **required** sit
  under the launcher choice. A choice lists its values as `config.toml` holds them, in English
  as written and otherwise as your catalog translates them: make them words that read well,
  written as `Msg::new("…").english()` in the `values`;
- a **name** (`Text` in `Plain`), a **key combination** (`Chord`, checked as it is typed) and a
  **number** go under *Advanced*.

While a `required` option is not set, choosing your launcher stores nothing: the page shows it
with its rows, the required one asking to be entered, and stores the switch together with that
value. The core rows apply as well; the starting screen shows only when your launcher detects
games, and is greyed out, naming the switch that would help, while it cannot show its pages
(`navigation`).

The titles, help and choice values are yours, written in English and translated by your crate:
write each, and your name, as `Msg::new("…").english()`, and ship `lang/de.po` with their German
through `catalogs()` (below). `cargo xtask i18n-check` fails when the catalog lacks one, the
conformance test when a text is missing from it, and Mujina Settings' tests when a launcher has no
catalog for a language Mujina Settings speaks. The same catalog holds the titles of your checks.
Mujina Settings looks your name and options up in your catalogs before any other, so the same
English elsewhere in Mujina does not decide their German.

Still to come, so that nobody is surprised:

- **undoing** what your runtime changes outside Mujina (a file in the launcher's folder) when an
  option is switched off (leftover-1). Record such files the way Steam's marker is recorded
  (`HKCU\Software\Mujina\Created`), so that uninstalling removes them;
- the **System page** lists the checks `SYSTEM_CHECKS` in `crates/app/src/tool.rs` names, the
  core ones and Steam's location permission; your checks show on the Status page. Its hint on
  opening Mujina Settings from Steam shows whenever Mujina is packaged, whichever launcher is
  chosen;
- the **Wi-Fi tile** on the Status page is Steam's: it follows a toggle called `wifi_indicator`,
  with the one it requires, and says what Steam shows. A second launcher with such a fix needs a
  capability for it first.

Read `crates/adapter-steam` (the full example) and `crates/adapter-generic` (the minimal one)
alongside this guide.

## The descriptor: what your launcher is

`LauncherDescriptor` (`crates/application/src/launcher.rs`) is plain data and rules, with no
Windows in it, so it builds and is tested on Linux too. Keep it outside `#[cfg(windows)]`.
CI's Linux job runs clippy on it with `-D warnings`: what in the descriptor only the Windows
runtime uses has to be `pub` and re-exported from `lib.rs`, as the generated `<Type>Options`
is, or go behind `#[cfg(windows)]`; otherwise clippy on Linux calls it never used.

```rust
use mujina_application::Msg;
use mujina_application::launcher::{LauncherCaps, LauncherDescriptor, OptionTable};
use mujina_application::settings::schema::{self, Applies, SettingKind, SettingSpec};

pub struct PlayniteDescriptor;

pub static DESCRIPTOR: PlayniteDescriptor = PlayniteDescriptor;

const SETTINGS: &[SettingSpec] = &[SettingSpec {
    key: "game_detection",
    kind: SettingKind::Toggle { default: false },
    // `Msg`s, so that i18n-check holds lang/de.po to them; the setting keeps the English.
    title: Msg::new("Detect games").english(),
    help: Msg::new("Needs Mujina's extension in Playnite.").english(),
    applies: Applies::NextSession,
    requires: None,
    required: false,
}];

impl LauncherDescriptor for PlayniteDescriptor {
    fn id(&self) -> &'static str {
        "playnite"
    }

    fn name(&self) -> &'static str {
        Msg::new("Playnite").english()
    }

    fn settings(&self) -> &'static [SettingSpec] {
        SETTINGS
    }

    fn template(&self) -> &'static str {
        "# Playnite, with kind = \"playnite\" above:\n\
         # [launcher.playnite]\n\
         # game_detection = false    # needs Mujina's extension in Playnite\n"
    }

    fn capabilities(&self, options: &OptionTable) -> LauncherCaps {
        LauncherCaps {
            game_detection: schema::flag(SETTINGS, options, "game_detection"),
            // No keyboard shortcut for its menu is known; the button then does nothing there.
            menu: false,
            overlay: false,
            navigation: false,
        }
    }

    fn catalogs(&self) -> &'static [(&'static str, &'static str)] {
        &[("de", include_str!("../lang/de.po"))]
    }
}
```

| Member | What it says | Steam's answer |
|---|---|---|
| `id` | what `kind = "..."` names and `[launcher.<id>]` holds; never changes once released | `steam` |
| `name` | what Mujina Settings lists | "Steam Big Picture" |
| `settings` | its options, each a `SettingSpec`: key, kind (`Toggle`, `Choice`, `Text` with a format, `TextList`, `Number`), title, help, whether a change applies at once (`Live`) or next session, the toggle it `requires`, whether it is `required` | `ui_link`, `wifi_indicator` (requires `ui_link`) |
| `template` | its block of the commented configuration template | `[launcher.steam]` with both options |
| `capabilities` | what it offers with these options: game detection, a menu, an overlay, navigation. A running agent asks again when an option that applies at once (`Live`) changes, with the others as the session started | everything; navigation only with `ui_link` |
| `validate` | optional: rules across options; a note means the options cannot be used, and Mujina falls back to Steam. Called whenever the file is read or a change checked, so no side effects | none (the generic launcher: a program without a file name) |
| `conflicting_processes` | optional: programs that do its job the way Mujina does; `doctor` warns while one runs | `steamwififeeder.exe` |
| `catalogs` | optional, but every launcher Mujina ships has one: the translations of its name, its settings' titles, help and choice values and its checks' titles, by language (gettext `.po`, `include_str!`d) | `lang/de.po` |

The configuration reads nothing in `[launcher.<id>]` but your settings: an unknown key or a value
of the wrong kind is a note of its own, a missing `required` setting or a broken rule of yours
falls back to Steam with a note, and `mujinactl config set` and Mujina Settings refuse what would
add a note. Options are flat; a value is never a table.

What the button does follows `capabilities`: without a menu a press in your UI is swallowed
(never passed on as the device's own chord, which is Show Desktop on a OneXPlayer); without an
overlay a press in a game leads home in Xbox mode. `[launcher] menu` and `overlay` in the
configuration count as a menu and an overlay of the user's own.

Test it in your crate, with `mujina-application`'s `test-util` feature as a dev-dependency:

```rust
#[test]
fn the_descriptor_keeps_the_rules() {
    mujina_application::testing::conformance(&DESCRIPTOR);
}
```

It checks that the id and keys are plain, the id is none of `[launcher]`'s own keys (`kind`,
`on_exit`, `menu`, `overlay`), every key is there once with a title, defaults lie within their
kind, only a setting without a default is required, `requires` names a toggle beside it, the
template names exactly your options under `[launcher.<id>]`, your defaults pass your own
rules, and each of your catalogs translates your name and every title and help.

## The runtime: what your launcher does

`LauncherRuntime` (`crates/adapter-kit/src/plugin.rs`) builds, from the options as read, what
each role needs; it is Windows-only. `home()` must not start anything that outlives the call:
the home role builds it on every activation, and the tools build it too.

- `home(options) -> Box<dyn HomeLauncher>`, for the short-lived home role and the tools;
- `session(options) -> SessionParts`, for the resident agent: your `SessionLauncher`, and your
  wait sources (below). It may start threads of your own for the session, each blocked in the
  kernel while idle ([ADR-0014](adr/0014-threads-and-com.md)), and only for options that are on;
- `checks(&self, options: &OptionTable) -> Vec<Box<dyn Check>>`, optional: what
  `mujinactl doctor` should look at. A `Check` (`mujina_application::doctor`) says its `id`
  (English and lower case, words apart, never changed once released: `steam ui link`; what
  `doctor` prints and Mujina Settings looks it up by) and its `title` (a `Msg` your catalog
  translates) before it looks, and `examine` returns `self.found(severity, detail)`, the detail
  in English, the severity `Ok`, `Warning` or `Problem`. A finding may also say it in a sentence
  for people, `.saying(Msg::new("…"))`, and offer a `Remedy`; the System page shows both, for the
  checks it lists. [new-device.md](new-device.md#a-doctor-check) has a whole check, with what
  each severity means.

The ports are in `crates/application/src/ports/launcher.rs`. One type may implement both, as
Steam's does.

| `HomeLauncher` | Question it answers | Steam's answer |
|---|---|---|
| `display_name` | What to call it in logs and on the pages | "Steam Big Picture" (the generic launcher: its program's name) |
| `locate` | Where is it installed? | `SteamExe` from `HKCU\Software\Valve\Steam` |
| `state` | Not running, running without console UI, or console UI on screen? | Big Picture window found / client pid alive / neither |
| `prepare` | Optional: what has to hold whenever it runs. Called on every home activation, so idempotent and cheap; failing is only a warning | make sure the UI debugging marker exists, with `ui_link` on |
| `start_ui` | Start straight into the console UI | `steam.exe -gamepadui` |
| `switch_to_ui` | Ask the running launcher for its console UI | `steam.exe steam://open/bigpicture` |
| `focus_ui` | Bring the existing console UI to the front | the Big Picture window, through the kit's `focus_ui` |
| `navigate` | Optional: show the page Windows asked for (`windows.gaming:///library`) | Big Picture's router over the debugging endpoint |
| `focus_game` | Optional: bring the running game back, from the home role | the game's window, through the kit's `focus_game` |

| `SessionLauncher` | Question it answers | Steam's answer |
|---|---|---|
| `owns_process` | Is process *x* yours? A foreground window of it means "the launcher UI is in front", and supervision watches only a process it accepts | `steamwebhelper.exe`, `steam.exe` |
| `process_id` | Its process id, for supervision. Asked while no process is watched, when the home role has started it, a window of yours came to the front (as `owns_process` says) or your state source fired; while one is watched, only when your state source fired, so that a new id replaces the old one quietly and none ends supervision. Never on a timer. An id is taken only for a running process whose image `owns_process` accepts | `ActiveProcess\pid` |
| `game_running`, `game_in_front`, `game_findable` | Optional: is a game running, in front, to be found? | `RunningAppID`; the game's window |
| `game_whereabouts` | Optional, asked on a button press while a game runs: in front, behind, without a window (and whether you know all its processes, so that nothing else in front is the game), or gone although still counted as running. By default from `game_in_front` and `game_findable` | the programs running from the game's install folder (from `libraryfolders.vdf` and the app's manifest), else the client's descendants |
| `game_started`, `game_ended` | Optional: a game just started or ended; must not block | keep "game is starting" up |
| `menu_shortcut`, `overlay_shortcut` | Optional: the chord that opens the menu in its UI, and the one that opens the overlay in a game; each asked only when a press needs it | `Ctrl+1`; the overlay key from `localconfig.vdf` |
| `open_menu`, `open_overlay` | Optional: open them without a shortcut; `Direct::Taken` only once the request went through | the menu stores over the debugging endpoint, while its link is up |
| `reconfigure` | Optional: the options that apply at once changed. Only those set in the file arrive, so a missing one is back to its default (read them with `SettingSpec::value_in` or `schema::flag`), and only when they differ from what it was last given. It takes `&self`: keep what changes in a `Cell` or behind a lock | none (Steam's options wait for the next session) |
| `observe` | Optional: every event the agent handled, after it did, for features of your own beside the ports. Takes `&self` and must not block: hand the work to a thread of your own | on its signs of life (its UI in front, its state changed, started by the home role) the link to Big Picture is checked, for the Wi-Fi fix and the direct menus |

A launcher that cannot tell whether a game runs leaves `game_running` at `false`; the button then
opens its menu in its UI and leads home everywhere else.

### Waking the agent: wait sources

The agent sleeps in one kernel wait until something happens (`winutil::wait::EventLoop`). A
launcher that can signal its state brings a `WaitSource<AgentEvent>` in `SessionParts::sources`,
and the agent's loop waits on it beside its own:

- `handle()` lends the kernel object to wait on (an event, a change notification, a process, an
  overlapped read's event), or `None` while there is none. It is asked before every wait.
- `signalled()` runs when that object was signalled: re-arm it, reset it or give it up, and add
  what it means, usually `AgentEvent::LauncherStateChanged`. The agent then asks `game_running()`
  again, and `process_id()`: to find your process if it watches none, or to follow a new id if
  it watches one.
- `name()` names it in logs and errors.

Neither may block, and a source owns what it waits on, since it outlives the call that built it.
Steam's (`crates/adapter-steam/src/state.rs`) is a change notification on its registry key,
armed again on every change. A source and your `SessionLauncher` are separate boxes, but both
live on the agent's thread, so they may share what the source learnt through an `Rc<Cell<_>>`
made in `session()`: set it in `signalled()` before adding `LauncherStateChanged`, and answer
`game_running()` from it. A launcher that cannot signal its state brings none: the agent
still sees its process come and go, looked up again when the home role has started it or one of
its windows came to the front. A feature that needs no answer from the agent needs no source either:
Steam's Wi-Fi fix runs on threads of its own and hears of the agent's events through `observe`.

## Rules that are easy to get wrong

- **Recognise the console UI precisely.** `state()` decides whether the home role starts,
  switches or merely focuses, and `focus_ui()` needs the same window. Steam has several
  `SDL_app` windows; Big Picture is the one that wants a taskbar button and has no sizing border
  (`window_rule.rs`, rule kept as data in `winutil::window::WindowRule`). Expect to need a similar
  discriminator.
- **Pass the foreground on before starting the launcher.** The home role owns the foreground
  right only until something else shows a window. Start the launcher with
  `mujina_adapter_kit::launcher::launch`, which hands the right on immediately before spawning,
  and bring it forward with `focus_ui`, which knows the fallbacks Xbox mode needs.
- **Never block, never poll.** `observe`, `game_started`, `reconfigure` and a wait source's
  `signalled` return at once and do their work on a thread that is otherwise blocked. `state()`
  is called from event handlers, so keep it cheap.
- **Keep the descriptor free of side effects.** It is asked whenever the configuration is read or
  a change is checked, in every process.
- **No injection into the launcher or into games.** Talking to a documented or semi-documented
  local interface (Steam's debugging port) is the limit. Anything that changes the launcher's
  own files or opens a port needs an option of its own that the user can switch off, like Steam's
  `ui_link`, and a word in SECURITY.md.
- **Undocumented behaviour stays in the adapter**, with a comment saying what it relies on, so
  that a launcher update breaks one file.

## Wiring it in

`cargo xtask new-launcher <id>` does all of this but the checklist section in step 4; by hand:

1. Create `crates/adapter-<id>`. Its `Cargo.toml` names its ring and depends on
   `mujina-application` and `mujina-adapter-kit`, on `mujina-winutil` for Windows only, and on
   `mujina-application` with `test-util` for its tests; add `mujina-domain` once it uses a
   domain type, such as a `KeyChord` for its menu:

   ```toml
   [package.metadata.mujina]
   ring = "adapter"

   [dependencies]
   mujina-adapter-kit.workspace = true
   mujina-application.workspace = true

   [target.'cfg(windows)'.dependencies]
   mujina-winutil.workspace = true

   [dev-dependencies]
   mujina-application = { workspace = true, features = ["test-util"] }
   ```

   Its `lang/de.po` sits beside `src/`, with a header as in `crates/adapter-steam/lang/de.po`.
   It may not depend on another adapter; `cargo xtask arch-check` enforces both.
2. In its `lib.rs`, the descriptor outside `#[cfg(windows)]`, the rest inside, and the pair:

   ```rust
   #[cfg(windows)]
   pub static PLUGIN: mujina_adapter_kit::plugin::LauncherPlugin =
       mujina_adapter_kit::plugin::LauncherPlugin {
           descriptor: &DESCRIPTOR,
           runtime: &runtime::RUNTIME,
       };
   ```

3. Add `mujina-adapter-<id> = { path = "crates/adapter-<id>" }` to `[workspace.dependencies]`,
   `mujina-adapter-<id>.workspace = true` to `crates/app/Cargo.toml`, and
   `&mujina_adapter_<id>::PLUGIN` to `LAUNCHERS` in `crates/app/src/registry.rs` (an `_` in the
   id is a `-` in the crate's name). The registry's own tests check every launcher's descriptor
   and template again, on Windows. Then run any cargo command without `--locked`, such as
   `cargo check`, and commit the `Cargo.lock` it updates.
4. Tests: the descriptor's conformance and any parsing in the adapter, run on Linux as well;
   behaviour against fakes is already covered in `application`. Add the launcher to
   `docs/on-device-checklist.md`.

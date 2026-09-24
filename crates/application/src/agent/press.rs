//! What a button press came to, for the log. Names roles only, so logs can go into an issue.

use std::fmt;

use mujina_domain::button::WindowShape;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InFront {
    LauncherUi,
    /// A game the launcher recognises as the one it runs.
    Game,
    /// Taken for the game by its full-screen shape: a game runs, but its window was not found.
    TakenForGame,
    /// Anything else, or nothing that could be identified.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameSeen {
    /// The launcher runs none, or cannot tell.
    None,
    /// One runs; where its window is was not needed.
    Running,
    InFront,
    Behind,
    /// No window found, and the launcher knows its processes: the window in front is not the game.
    NoWindow,
    /// No window found, and the launcher does not know its processes: the front one may be it.
    NotFound,
    /// The launcher still counts one as running, but nothing of it runs.
    Gone,
}

impl GameSeen {
    /// Whether a game counts: one that has ended does not, whatever the launcher says.
    pub const fn running(self) -> bool {
        !matches!(self, Self::None | Self::Gone)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub in_front: InFront,
    pub game: GameSeen,
    /// Only asked while the game's window is not found; then `None` means it could not be read.
    pub shape: Option<WindowShape>,
    /// Xbox mode is on.
    pub console: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The button is switched off, or no device runs: the press means nothing.
    Off,
    /// The launcher's menu; `direct`: opened by the launcher itself, not by its shortcut.
    Menu { direct: bool },
    /// `direct` as for [`Menu`](Self::Menu).
    Overlay { direct: bool },
    /// The menu or the overlay was due, but the launcher did not open it and has no shortcut.
    NothingToSend,
    /// The home role was asked to bring the game back to the front.
    BackToGame,
    /// The home role was asked to bring the launcher to the front; `failed` if asking failed.
    Home { failed: bool },
    /// The button's own meaning, replayed since it was swallowed.
    PassedOn,
    /// The button's own meaning; the device's own software saw it already.
    LeftToDevice,
    /// The launcher has nothing for it, and the button's own meaning would lead away from it.
    Swallowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PressReport {
    /// `None` when the press was not looked into ([`Outcome::Off`]).
    pub seen: Option<Seen>,
    pub outcome: Outcome,
}

impl PressReport {
    /// One line for the log; `launcher` is its display name, e.g. "Steam Big Picture".
    pub fn describe(&self, launcher: &str) -> String {
        let outcome = Described {
            outcome: self.outcome,
            launcher,
        };
        match &self.seen {
            Some(seen) => format!("{outcome} ({seen})"),
            None => outcome.to_string(),
        }
    }
}

struct Described<'a> {
    outcome: Outcome,
    launcher: &'a str,
}

impl fmt::Display for Described<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let by = |direct| {
            if direct {
                "opened by the launcher"
            } else {
                "by its shortcut"
            }
        };
        match self.outcome {
            Outcome::Off => f.write_str("nothing, it is switched off"),
            Outcome::Menu { direct } => write!(f, "the menu, {}", by(direct)),
            Outcome::Overlay { direct } => write!(f, "the overlay, {}", by(direct)),
            Outcome::NothingToSend => {
                f.write_str("nothing: the launcher did not open it and has no shortcut for it")
            }
            Outcome::BackToGame => f.write_str("back to the game"),
            Outcome::Home { failed: false } => f.write_str(self.launcher),
            Outcome::Home { failed: true } => {
                write!(f, "{}, but asking for it failed", self.launcher)
            }
            Outcome::PassedOn => f.write_str("its own meaning, passed on"),
            Outcome::LeftToDevice => f.write_str("its own meaning, left to the device"),
            Outcome::Swallowed => f.write_str("nothing, the launcher has nothing for it"),
        }
    }
}

impl fmt::Display for Seen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.in_front {
            InFront::LauncherUi => "in front: the launcher",
            InFront::Game => "in front: the game",
            InFront::TakenForGame => "in front: another app taken for the game",
            InFront::Other => "in front: another app",
        })?;
        // The shape is what the decision rested on; for checking it against what was on screen.
        if self.game == GameSeen::NotFound {
            match self.shape {
                Some(shape) => write!(f, ", {}", Shape(shape))?,
                None => f.write_str(", its window could not be read")?,
            }
        }
        f.write_str(match self.game {
            GameSeen::None => "; no game",
            GameSeen::Running => "; the launcher counts a game as running",
            GameSeen::InFront => "; a game runs",
            GameSeen::Behind => "; the game's window is behind it",
            GameSeen::NoWindow => "; a game runs, without a window to go back to",
            GameSeen::NotFound => "; a game runs but its window cannot be found",
            GameSeen::Gone => "; the launcher counts a game as running, but none of it runs",
        })?;
        f.write_str(if self.console {
            "; Xbox mode"
        } else {
            "; desktop"
        })
    }
}

struct Shape(WindowShape);

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let shape = self.0;
        f.write_str(if shape.framed { "framed" } else { "frameless" })?;
        f.write_str(if shape.fills_monitor {
            ", filling its screen"
        } else {
            ", not exactly its screen's size"
        })?;
        for (set, what) in [
            (shape.maximized, ", maximised"),
            (shape.cloaked, ", cloaked"),
            (shape.shell, ", the shell's"),
            (shape.packaged, ", a packaged app"),
        ] {
            if set {
                f.write_str(what)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_is_described_in_roles_only() {
        let browser = WindowShape {
            framed: true,
            maximized: true,
            ..WindowShape::default()
        };
        let report = PressReport {
            seen: Some(Seen {
                in_front: InFront::Other,
                game: GameSeen::NotFound,
                shape: Some(browser),
                console: true,
            }),
            outcome: Outcome::Home { failed: false },
        };
        assert_eq!(
            report.describe("Steam Big Picture"),
            "Steam Big Picture (in front: another app, framed, not exactly its screen's size, \
             maximised; a game runs but its window cannot be found; Xbox mode)"
        );
    }

    #[test]
    fn the_shape_is_named_only_where_it_was_asked_for() {
        let report = PressReport {
            seen: Some(Seen {
                in_front: InFront::LauncherUi,
                game: GameSeen::Running,
                shape: None,
                console: false,
            }),
            outcome: Outcome::Menu { direct: true },
        };
        assert_eq!(
            report.describe("Steam Big Picture"),
            "the menu, opened by the launcher (in front: the launcher; the launcher counts a game as \
             running; desktop)"
        );
        let unread = PressReport {
            seen: Some(Seen {
                in_front: InFront::Other,
                game: GameSeen::NotFound,
                shape: None,
                console: true,
            }),
            outcome: Outcome::Home { failed: true },
        };
        assert_eq!(
            unread.describe("Launcher"),
            "Launcher, but asking for it failed (in front: another app, its window could not be \
             read; a game runs but its window cannot be found; Xbox mode)"
        );
        let off = PressReport {
            seen: None,
            outcome: Outcome::Off,
        };
        assert_eq!(off.describe("Launcher"), "nothing, it is switched off");
    }

    #[test]
    fn a_game_that_has_ended_is_named_so() {
        let ended = PressReport {
            seen: Some(Seen {
                in_front: InFront::Other,
                game: GameSeen::Gone,
                shape: None,
                console: true,
            }),
            outcome: Outcome::Home { failed: false },
        };
        assert_eq!(
            ended.describe("Steam Big Picture"),
            "Steam Big Picture (in front: another app; the launcher counts a game as running, but \
             none of it runs; Xbox mode)"
        );
        assert!(!GameSeen::Gone.running() && !GameSeen::None.running());
        assert!(GameSeen::NoWindow.running() && GameSeen::NotFound.running());
    }
}

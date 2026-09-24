//! What Windows asks of the console home when it activates it.

/// The scheme Windows activates the console home with. Not a registered protocol, so no web page
/// can open it (ADR-0001); a local program could, but gains nothing over starting the launcher.
const WINDOWS_SCHEME: &str = "windows.gaming:";
/// The package's own scheme, through which the agent activates the home role.
const OWN_SCHEME: &str = "mujina:";

/// What an activation of the console home is to put in front of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeDestination {
    /// The launcher's start page: booting into the console experience, the home button.
    #[default]
    Home,
    /// The user's games: the library entry of the Game Bar.
    Library,
    /// The running game; only the agent asks for it (`mujina://game`). In the console experience
    /// the shell takes the foreground back from a window a background process raised, but not
    /// from one raised by the home app it just activated.
    Game,
}

impl HomeDestination {
    /// Reads an activation argument such as `windows.gaming:///library`. Anything unknown means
    /// home, so the launcher comes up whatever Windows asks for in future.
    pub fn from_activation(argument: &str) -> Self {
        let (own, rest) = if let Some(rest) = strip_scheme(argument, WINDOWS_SCHEME) {
            (false, rest)
        } else if let Some(rest) = strip_scheme(argument, OWN_SCHEME) {
            (true, rest)
        } else {
            return Self::Home;
        };
        let page = rest
            .trim_start_matches('/')
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        if page.eq_ignore_ascii_case("library") {
            Self::Library
        } else if own && page.eq_ignore_ascii_case("game") {
            Self::Game
        } else {
            Self::Home
        }
    }
}

/// Who can have asked for an activation, as far as its argument tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationSource {
    /// Windows itself: `windows.gaming:///home` for the console home, `windows.gaming:///library`
    /// from the Game Bar, or no argument at all.
    Windows,
    /// Anything else, above all `mujina:`. Only the agent has a reason to use that scheme, but
    /// any program or web page can open a registered one, and nothing tells who did.
    Anyone,
}

impl ActivationSource {
    /// Reads the first activation argument. Only what Windows is known to pass counts as
    /// Windows; an argument nobody has seen yet has to pass the same test as `mujina:`.
    pub fn of(argument: Option<&str>) -> Self {
        match argument {
            None => Self::Windows,
            Some(argument) if strip_scheme(argument, WINDOWS_SCHEME).is_some() => Self::Windows,
            Some(_) => Self::Anyone,
        }
    }

    /// Whether the activation may bring the launcher up. `Anyone` counts only while the agent
    /// runs (the one caller that means it) or in the console experience, where that is the home
    /// app's job anyway.
    pub const fn accepts(self, agent_running: bool, console_experience: bool) -> bool {
        match self {
            Self::Windows => true,
            Self::Anyone => agent_running || console_experience,
        }
    }
}

/// What follows `scheme` in `argument`. Schemes are case-insensitive (RFC 3986, section 3.1).
fn strip_scheme<'a>(argument: &'a str, scheme: &str) -> Option<&'a str> {
    let head = argument.get(..scheme.len())?;
    let rest = argument.get(scheme.len()..)?;
    head.eq_ignore_ascii_case(scheme).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_is_recognised() {
        for argument in [
            "windows.gaming:///library",
            "windows.gaming:///Library/",
            "windows.gaming:///library?source=gamebar",
            "Windows.Gaming:///library",
            "mujina://library",
        ] {
            assert_eq!(
                HomeDestination::from_activation(argument),
                HomeDestination::Library,
                "{argument}"
            );
        }
    }

    #[test]
    fn only_mujina_itself_asks_for_the_game() {
        for argument in ["mujina://game", "mujina://game/", "MUJINA://game"] {
            assert_eq!(
                HomeDestination::from_activation(argument),
                HomeDestination::Game,
                "{argument}"
            );
        }
        assert_eq!(
            HomeDestination::from_activation("windows.gaming:///game"),
            HomeDestination::Home
        );
    }

    #[test]
    fn everything_else_is_home() {
        for argument in [
            "windows.gaming:///home",
            "windows.gaming:///",
            "windows.gaming:///libraries",
            "mujina://home",
            "library",
            "",
        ] {
            assert_eq!(
                HomeDestination::from_activation(argument),
                HomeDestination::Home,
                "{argument}"
            );
        }
    }

    #[test]
    fn only_what_windows_passes_is_windows() {
        for argument in [
            None,
            Some("windows.gaming:///home"),
            Some("windows.gaming:///library"),
            Some("WINDOWS.GAMING:///home"),
        ] {
            assert_eq!(
                ActivationSource::of(argument),
                ActivationSource::Windows,
                "{argument:?}"
            );
        }
        for argument in [
            "mujina://home",
            "mujina://game",
            "Mujina://home",
            "MUJINA:home",
            "windows.gaming",
            "https://example.com/?windows.gaming:",
            "agent",
            "\u{e9}",
            "",
        ] {
            assert_eq!(
                ActivationSource::of(Some(argument)),
                ActivationSource::Anyone,
                "{argument}"
            );
        }
    }

    #[test]
    fn what_anyone_can_ask_for_needs_the_agent_or_the_console_experience() {
        use ActivationSource::{Anyone, Windows};

        for agent_running in [false, true] {
            for console in [false, true] {
                assert!(Windows.accepts(agent_running, console));
            }
        }
        // The agent's own `mujina://` activations.
        assert!(Anyone.accepts(true, true));
        assert!(Anyone.accepts(true, false));
        assert!(Anyone.accepts(false, true));
        // A web page on the desktop.
        assert!(!Anyone.accepts(false, false));
    }
}

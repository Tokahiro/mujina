//! What Windows asks of the console home when it activates it.

/// The scheme Windows activates the console home with. Not a registered protocol, so no web page
/// can open it (ADR-0001); a local program could, but gains nothing over starting the launcher.
const WINDOWS_SCHEME: &str = "windows.gaming:";
const OWN_SCHEME: &str = "mujina:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeDestination {
    #[default]
    Home,
    Library,
    /// Only the agent asks for it (`mujina://game`): in the console experience the shell takes the
    /// foreground back from a window a background process raised, not from the home app's.
    Game,
}

impl HomeDestination {
    /// Reads an activation argument such as `windows.gaming:///library`; anything unknown is home.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationSource {
    /// `windows.gaming:` (the console home, the Game Bar's library), or no argument at all.
    Windows,
    /// Anything else, above all `mujina:`: any program or web page can open a registered scheme,
    /// and nothing tells who did.
    Anyone,
}

impl ActivationSource {
    /// Takes the first activation argument; only what Windows is known to pass counts as Windows.
    pub fn of(argument: Option<&str>) -> Self {
        match argument {
            None => Self::Windows,
            Some(argument) if strip_scheme(argument, WINDOWS_SCHEME).is_some() => Self::Windows,
            Some(_) => Self::Anyone,
        }
    }

    /// `Anyone` may bring the launcher up only while the agent (its one real caller) runs, or in
    /// the console experience, where that is the home app's job anyway.
    pub const fn accepts(self, agent_running: bool, console_experience: bool) -> bool {
        match self {
            Self::Windows => true,
            Self::Anyone => agent_running || console_experience,
        }
    }
}

/// Schemes are case-insensitive (RFC 3986, section 3.1).
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
        assert!(Anyone.accepts(true, true));
        assert!(Anyone.accepts(true, false));
        assert!(Anyone.accepts(false, true));
        assert!(!Anyone.accepts(false, false));
    }
}

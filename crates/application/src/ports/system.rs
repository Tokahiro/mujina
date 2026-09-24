//! What the operating system reports: the full screen experience and the window in front.

use mujina_domain::button::WindowShape;

// --- Full screen experience -------------------------------------------------------------------

/// State of the Windows full screen experience ("Xbox mode"), which these comments also call the
/// console experience.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FseState {
    /// This Windows build has no full screen experience API.
    Unavailable,
    /// The API exists, the console experience is off (normal desktop).
    Inactive,
    /// The console experience is on.
    Active,
}

pub trait FullScreenExperience {
    fn state(&self) -> FseState;
}

// --- Foreground -------------------------------------------------------------------------------

pub trait ForegroundProbe {
    /// File name of the process that owns the foreground window right now.
    fn foreground_process(&self) -> Option<String>;

    /// How the foreground window is shown right now; `None` without one, or when it could not be
    /// read. Asked on a button press only, and only while a game runs whose window cannot be
    /// found and whose processes the launcher cannot tell, so it may look things up; it must not
    /// wait on the window's own thread.
    fn foreground_shape(&self) -> Option<WindowShape> {
        None
    }
}

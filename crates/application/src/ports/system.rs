//! What the operating system reports: the full screen experience and the window in front.

use mujina_domain::button::WindowShape;

/// State of the Windows full screen experience ("Xbox mode", also called console experience).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FseState {
    /// This Windows build has no full screen experience API.
    Unavailable,
    /// Supported but off: the normal desktop.
    Inactive,
    Active,
}

pub trait FullScreenExperience {
    fn state(&self) -> FseState;
}

pub trait ForegroundProbe {
    /// File name of the process that owns the foreground window right now.
    fn foreground_process(&self) -> Option<String>;

    /// `None` without a foreground window or when it cannot be read. Asked only on a button press
    /// whose game window cannot be found, so it may look things up, but it must not wait on the
    /// window's own thread.
    fn foreground_shape(&self) -> Option<WindowShape> {
        None
    }
}

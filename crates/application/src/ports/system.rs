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
    fn foreground_process(&self) -> Option<String>;

    /// `None` without a foreground window or when it cannot be read. May look things up (asked on
    /// a button press only), but must not wait on the window's own thread.
    fn foreground_shape(&self) -> Option<WindowShape> {
        None
    }
}

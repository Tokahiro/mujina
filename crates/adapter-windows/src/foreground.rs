//! What is in front right now.

use mujina_application::ports::ForegroundProbe;
use mujina_domain::button::WindowShape;
use mujina_winutil::window::{self, Presentation};

#[derive(Debug, Default)]
pub struct WindowsForeground;

impl ForegroundProbe for WindowsForeground {
    fn foreground_process(&self) -> Option<String> {
        window::foreground_process_name()
    }

    fn foreground_shape(&self) -> Option<WindowShape> {
        window::foreground_presentation().map(shape)
    }
}

fn shape(presentation: Presentation) -> WindowShape {
    let Presentation {
        framed,
        fills_monitor,
        maximized,
        cloaked,
        shell,
        packaged,
    } = presentation;
    WindowShape {
        framed,
        fills_monitor,
        maximized,
        cloaked,
        shell,
        packaged,
    }
}

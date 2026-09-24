//! How to tell Big Picture from every other window Steam opens.
//!
//! The rule (an `SDL_app` window of `steamwebhelper.exe` that wants a taskbar button and has no
//! sizing border) is derived from the Steam launcher profile of AnyFSE, MIT licensed,
//! <https://github.com/ashpynov/AnyFSE>. See `NOTICE`.

use mujina_winutil::window::{self, WindowHandle, WindowRule};

pub fn big_picture_rule() -> WindowRule {
    WindowRule {
        class_name: "SDL_app".to_string(),
        process_name: "steamwebhelper.exe".to_string(),
        require_app_window: true,
        forbid_resizable: true,
    }
}

/// Finds the Big Picture window, if it is on screen.
pub fn find_big_picture() -> Option<WindowHandle> {
    window::find_top_level(&big_picture_rule())
}

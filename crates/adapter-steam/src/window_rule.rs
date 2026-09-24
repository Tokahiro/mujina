//! How to tell Big Picture from every other window Steam opens. Derived from the Steam launcher
//! profile of AnyFSE, MIT licensed, <https://github.com/ashpynov/AnyFSE>. See `NOTICE`.

use mujina_winutil::window::{self, WindowHandle, WindowRule};

pub fn big_picture_rule() -> WindowRule {
    WindowRule {
        class_name: "SDL_app".to_string(),
        process_name: "steamwebhelper.exe".to_string(),
        require_app_window: true,
        forbid_resizable: true,
    }
}

pub fn find_big_picture() -> Option<WindowHandle> {
    window::find_top_level(&big_picture_rule())
}

//! Enumerating and activating top-level windows.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::ptr::null;

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
    GetSystemMetricsForDpi, SetThreadDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_LMENU,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, EnumWindows, GA_ROOT, GUITHREADINFO, GW_OWNER, GWL_EXSTYLE,
    GWL_STYLE, GetAncestor, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetShellWindow,
    GetWindow, GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsIconic, IsWindowVisible,
    IsZoomed, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON,
    SW_RESTORE, SendMessageW, SetForegroundWindow, ShowWindow, WM_SETICON, WS_CAPTION,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_THICKFRAME,
};
use windows_sys::core::BOOL;

use crate::wide::from_wide;

/// Stored as an integer so it can cross threads; Win32 validates it when it is handed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowHandle(isize);

impl WindowHandle {
    fn from_raw(hwnd: HWND) -> Self {
        Self(hwnd as isize)
    }

    pub(crate) fn raw(self) -> HWND {
        self.0 as HWND
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub handle: WindowHandle,
    pub class_name: String,
    pub process_id: u32,
    pub visible: bool,
    pub app_window: bool,
    pub resizable: bool,
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: `lparam` is the `Vec` of `top_level_windows`, alive and unshared during EnumWindows.
    let handles = unsafe { &mut *(lparam as *mut Vec<WindowHandle>) };
    handles.push(WindowHandle::from_raw(hwnd));
    1
}

/// All top-level windows, in Z order.
pub fn top_level_windows() -> Vec<WindowSnapshot> {
    let mut handles: Vec<WindowHandle> = Vec::new();
    // SAFETY: `collect` matches WNDENUMPROC and only dereferences the pointer passed here.
    unsafe { EnumWindows(Some(collect), (&raw mut handles) as LPARAM) };
    handles.into_iter().map(snapshot).collect()
}

fn reinterpret(bits: i32) -> u32 {
    u32::from_ne_bytes(bits.to_ne_bytes())
}

fn snapshot(handle: WindowHandle) -> WindowSnapshot {
    let hwnd = handle.raw();
    let mut class = [0u16; 256];
    let mut process_id: u32 = 0;
    // SAFETY: Win32 validates the handle (a stale one yields zero); buffers fit the stated length.
    let (style, ex_style, visible) = unsafe {
        GetClassNameW(hwnd, class.as_mut_ptr(), 256);
        GetWindowThreadProcessId(hwnd, &raw mut process_id);
        (
            reinterpret(GetWindowLongW(hwnd, GWL_STYLE)),
            reinterpret(GetWindowLongW(hwnd, GWL_EXSTYLE)),
            IsWindowVisible(hwnd) != 0,
        )
    };
    WindowSnapshot {
        handle,
        class_name: from_wide(&class),
        process_id,
        visible,
        app_window: ex_style & WS_EX_APPWINDOW != 0,
        resizable: style & WS_THICKFRAME != 0,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub class_name: String,
    pub process_name: String,
    pub visible: bool,
    /// The window asks for a taskbar button (`WS_EX_APPWINDOW`).
    pub app_window: bool,
    /// The window has a sizing border (`WS_THICKFRAME`).
    pub resizable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRule {
    pub class_name: String,
    pub process_name: String,
    pub require_app_window: bool,
    /// Full-screen console UIs are borderless; a sizing border means "desktop client".
    pub forbid_resizable: bool,
}

impl WindowRule {
    pub fn matches(&self, window: &WindowInfo) -> bool {
        window.visible
            && window.class_name == self.class_name
            && window.process_name.eq_ignore_ascii_case(&self.process_name)
            && (!self.require_app_window || window.app_window)
            && !(self.forbid_resizable && window.resizable)
    }
}

/// The topmost visible top-level window that `rule` describes.
pub fn find_top_level(rule: &WindowRule) -> Option<WindowHandle> {
    top_level_windows()
        .into_iter()
        // Cheap checks first: the process name needs a handle per candidate.
        .filter(|window| window.visible && window.class_name == rule.class_name)
        .find(|window| rule.matches(&info(window)))
        .map(|window| window.handle)
}

fn info(window: &WindowSnapshot) -> WindowInfo {
    WindowInfo {
        class_name: window.class_name.clone(),
        process_name: crate::process::image_name(window.process_id).unwrap_or_default(),
        visible: window.visible,
        app_window: window.app_window,
        resizable: window.resizable,
    }
}

/// Restores and activates the window; works only while this process may set the foreground.
/// A window already in front counts as success, which Windows may report as refused.
pub fn bring_to_foreground(handle: WindowHandle) -> bool {
    let hwnd = handle.raw();
    // SAFETY: Win32 validates the handle; no pointers are involved.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd) != 0 || GetForegroundWindow() == hwnd
    }
}

/// Marks keystrokes synthesized by Mujina ("MUJI"), so its own keyboard hook lets them be.
pub const OWN_INPUT_TAG: usize = 0x4D55_4A49;

/// [`bring_to_foreground`] for a process without the foreground right: taps Alt, as Windows lifts
/// the restriction for the last input's source. Only on the user's explicit request (ADR-0001).
pub fn claim_foreground(handle: WindowHandle) -> bool {
    if bring_to_foreground(handle) {
        return true;
    }
    let key = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_LMENU,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: OWN_INPUT_TAG,
            },
        },
    };
    let tap = [key(false), key(true)];
    let size = i32::try_from(size_of::<INPUT>()).unwrap_or(0);
    // SAFETY: two valid INPUT structures of the stated size.
    unsafe { SendInput(2, tap.as_ptr(), size) };
    // The tap is processed asynchronously; give it a few moments to register as our input.
    (0..10).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        bring_to_foreground(handle)
    })
}

/// Lets the next window shown take the foreground; works only while this process has that right.
pub fn allow_any_foreground() -> bool {
    // SAFETY: plain call without pointers.
    unsafe { AllowSetForegroundWindow(ASFW_ANY) != 0 }
}

/// Starts `executable` without waiting and hands the foreground to the next window shown.
/// `Ok(false)`: started, but without the hand-over, as this process lacked the foreground right.
pub fn spawn_with_foreground<S: AsRef<OsStr>>(
    executable: &Path,
    args: &[S],
    directory: &Path,
) -> std::io::Result<bool> {
    // Right before the start: the next input from the user takes the hand-over back.
    let handed_over = allow_any_foreground();
    Command::new(executable)
        .args(args)
        .current_dir(directory)
        .spawn()
        .map(|_child| handed_over)
}

/// How [`focus_with_fallbacks`] got a window in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focused {
    Directly,
    /// The window shows once the user unlocks.
    BehindLockScreen,
    /// Through [`claim_foreground`], which may have needed its synthetic key tap.
    AfterKeyTap,
}

/// Every means a process without the foreground right has; the error describes what is in front.
/// Like [`claim_foreground`], only on the user's explicit request.
pub fn focus_with_fallbacks(handle: WindowHandle) -> Result<Focused, String> {
    if bring_to_foreground(handle) {
        return Ok(Focused::Directly);
    }
    if lock_screen_in_front() {
        return Ok(Focused::BehindLockScreen);
    }
    // Activated for a background process (device button, crash relaunch), it lacks the right.
    if claim_foreground(handle) {
        return Ok(Focused::AfterKeyTap);
    }
    Err(describe_foreground())
}

/// `hwnd` is a window handle as an integer, e.g. from a WinEvent.
pub fn owner_process_name(hwnd: isize) -> Option<String> {
    let mut process_id: u32 = 0;
    // SAFETY: Win32 validates the handle; `process_id` is writable.
    unsafe { GetWindowThreadProcessId(hwnd as HWND, &raw mut process_id) };
    (process_id != 0)
        .then(|| crate::process::image_name(process_id))
        .flatten()
}

pub fn foreground_process_name() -> Option<String> {
    // SAFETY: plain call without arguments; may return null.
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_null())
        .then(|| owner_process_name(hwnd as isize))
        .flatten()
}

pub fn foreground_process_id() -> Option<u32> {
    // SAFETY: plain call without arguments; may return null.
    let hwnd = unsafe { GetForegroundWindow() };
    let mut process_id: u32 = 0;
    // SAFETY: Win32 validates the handle (null yields 0); `process_id` is writable.
    unsafe { GetWindowThreadProcessId(hwnd, &raw mut process_id) };
    (process_id != 0).then_some(process_id)
}

/// The facts that tell a full-screen game from the windows of other programs.
// Independent facts about one window, not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presentation {
    /// Any bit of `WS_CAPTION` (title bar or border) or `WS_THICKFRAME` (sizing border).
    pub framed: bool,
    /// Its window rectangle is exactly its monitor's, in physical pixels. A maximised window
    /// with a sizing border is larger: its borders lie off the screen.
    pub fills_monitor: bool,
    pub maximized: bool,
    /// DWM hides it (`DWMWA_CLOAKED`), although it counts as visible.
    pub cloaked: bool,
    /// Owned by the process of `GetShellWindow` (desktop, taskbar, task switcher). Xbox mode may
    /// have no shell window; its home is a packaged app, which `packaged` covers.
    pub shell: bool,
    /// Its process has a package identity, or its class is one of [`PACKAGED_APP_FRAMES`].
    pub packaged: bool,
}

/// Window classes a packaged (UWP) app is shown in: the frame `ApplicationFrameHost.exe` draws
/// around it, whose process has no package identity, and the app's own core window.
pub const PACKAGED_APP_FRAMES: [&str; 2] = ["ApplicationFrameWindow", "Windows.UI.Core.CoreWindow"];

/// Empty for a stale handle.
fn class_of(hwnd: HWND) -> String {
    let mut class = [0u16; 256];
    // SAFETY: Win32 validates the handle; the buffer is writable for the stated length.
    unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), 256) };
    from_wide(&class)
}

/// Switches the calling thread to physical pixels while it lives: for a DPI-unaware process,
/// window and monitor rectangles are scaled and may round apart.
struct PhysicalPixels(DPI_AWARENESS_CONTEXT);

impl PhysicalPixels {
    fn enter() -> Self {
        // SAFETY: a predefined context; an invalid one returns null, which `drop` does not restore.
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for PhysicalPixels {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the context the thread had before `enter`.
            unsafe { SetThreadDpiAwarenessContext(self.0) };
        }
    }
}

/// Of the foreground's root window; `None` without one or when a fact cannot be read. Sends no
/// message, so a window that does not answer holds nothing up.
pub fn foreground_presentation() -> Option<Presentation> {
    // SAFETY: plain call without arguments; may return null.
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return None;
    }
    // SAFETY: Win32 validates the handle; null for a stale one.
    let root = unsafe { GetAncestor(foreground, GA_ROOT) };
    presentation_of(if root.is_null() { foreground } else { root })
}

fn presentation_of(hwnd: HWND) -> Option<Presentation> {
    let _physical = PhysicalPixels::enter();

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: Win32 validates the handle; `rect` is writable.
    if unsafe { GetWindowRect(hwnd, &raw mut rect) } == 0 {
        return None;
    }
    // SAFETY: Win32 validates the handle; null when the window is on no monitor.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) };
    if monitor.is_null() {
        return None;
    }
    // SAFETY: MONITORINFO is plain data for which all-zero is a valid value.
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(size_of::<MONITORINFO>()).ok()?;
    // SAFETY: a monitor handle Windows just gave; `info` is writable and carries its size.
    if unsafe { GetMonitorInfoW(monitor, &raw mut info) } == 0 {
        return None;
    }
    let mut cloaked: u32 = 0;
    // SAFETY: Win32 validates the handle; `cloaked` is a writable DWORD, the size stated.
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            u32::try_from(DWMWA_CLOAKED).ok()?,
            (&raw mut cloaked).cast(),
            u32::try_from(size_of::<u32>()).ok()?,
        )
    };
    if read < 0 {
        return None;
    }
    let mut process_id: u32 = 0;
    // SAFETY: Win32 validates the handle; `process_id` is writable.
    unsafe { GetWindowThreadProcessId(hwnd, &raw mut process_id) };
    // SAFETY: plain call without arguments; null without a shell.
    let shell_window = unsafe { GetShellWindow() };
    let mut shell_process: u32 = 0;
    if !shell_window.is_null() {
        // SAFETY: a window handle Windows just gave; `shell_process` is writable.
        unsafe { GetWindowThreadProcessId(shell_window, &raw mut shell_process) };
    }
    // SAFETY: Win32 validates the handle; no pointers are involved.
    let (style, maximized) = unsafe {
        (
            reinterpret(GetWindowLongW(hwnd, GWL_STYLE)),
            IsZoomed(hwnd) != 0,
        )
    };
    let bounds = |r: RECT| (r.left, r.top, r.right, r.bottom);
    // GetClassNameW reads the window's class, kept by Windows; it sends no message either.
    let class = class_of(hwnd);
    let frame = PACKAGED_APP_FRAMES.iter().any(|name| *name == class);
    Some(Presentation {
        framed: style & (WS_CAPTION | WS_THICKFRAME) != 0,
        fills_monitor: bounds(rect) == bounds(info.rcMonitor),
        maximized,
        cloaked: cloaked != 0,
        shell: shell_process != 0 && shell_process == process_id,
        packaged: frame || (process_id != 0 && crate::process::is_packaged(process_id)),
    })
}

/// The topmost window of `process_ids` a user would call "the application".
pub fn main_window_of(process_ids: &[u32]) -> Option<WindowHandle> {
    top_level_windows()
        .into_iter()
        .filter(|window| window.visible && process_ids.contains(&window.process_id))
        .map(|window| window.handle)
        .find(|handle| {
            let hwnd = handle.raw();
            // SAFETY: Win32 validates the handle; a stale one yields zero/null.
            unsafe {
                GetWindowTextLengthW(hwnd) > 0
                    && GetWindow(hwnd, GW_OWNER).is_null()
                    && reinterpret(GetWindowLongW(hwnd, GWL_EXSTYLE)) & WS_EX_TOOLWINDOW == 0
            }
        })
}

/// Nothing can be brought in front of the lock screen; the order behind it shows after unlock.
pub fn lock_screen_in_front() -> bool {
    foreground_process_name().is_some_and(|name| name.eq_ignore_ascii_case("LockApp.exe"))
}

fn describe(hwnd: HWND) -> String {
    if hwnd.is_null() {
        return "none".to_string();
    }
    let mut class = [0u16; 256];
    // SAFETY: Win32 validates the handle; the buffer is writable for the stated length.
    unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), 256) };
    format!(
        "{} \"{}\"",
        owner_process_name(hwnd as isize).unwrap_or_else(|| "unknown process".to_string()),
        from_wide(&class)
    )
}

/// The foreground thread's active window and focused control, for diagnostics. A window in
/// front with neither gets no keys until something (a touch, a click) activates it.
pub fn describe_input_focus() -> String {
    // SAFETY: GUITHREADINFO is plain data for which all-zero is a valid value.
    let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
    info.cbSize = u32::try_from(size_of::<GUITHREADINFO>()).unwrap_or(0);
    // SAFETY: `info` is writable and carries its own size; thread 0 means the foreground thread.
    if unsafe { GetGUIThreadInfo(0, &raw mut info) } == 0 {
        return "unknown (no foreground thread)".to_string();
    }
    format!(
        "active {}, focus {}",
        describe(info.hwndActive),
        describe(info.hwndFocus)
    )
}

pub fn describe_foreground() -> String {
    // SAFETY: plain call without arguments; may return null.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return "no window".to_string();
    }
    let mut class = [0u16; 256];
    let mut title = [0u16; 256];
    // SAFETY: Win32 validates the handle; both buffers are writable for the stated length.
    unsafe {
        GetClassNameW(hwnd, class.as_mut_ptr(), 256);
        GetWindowTextW(hwnd, title.as_mut_ptr(), 256);
    }
    format!(
        "{} (class \"{}\", title \"{}\")",
        owner_process_name(hwnd as isize).unwrap_or_else(|| "unknown process".to_string()),
        from_wide(&class),
        from_wide(&title)
    )
}

/// Sets the executable's first icon on this process's visible windows, at each display's sizes:
/// a toolkit's single large image scales down poorly. Whether any window got it.
pub fn use_own_icon() -> bool {
    let own = std::process::id();
    let mut given = false;
    let sizes = [
        (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
        (ICON_BIG, SM_CXICON, SM_CYICON),
    ];
    for window in top_level_windows() {
        if window.process_id != own || !window.visible {
            continue;
        }
        let hwnd = window.handle.raw();
        for (kind, width, height) in sizes {
            // SAFETY: Win32 validates the handle; the resource name is a MAKEINTRESOURCE id. The
            // icon is never destroyed: the window uses it for as long as it exists.
            unsafe {
                let dpi = GetDpiForWindow(hwnd);
                let icon = LoadImageW(
                    GetModuleHandleW(null()),
                    std::ptr::without_provenance(1),
                    IMAGE_ICON,
                    GetSystemMetricsForDpi(width, dpi),
                    GetSystemMetricsForDpi(height, dpi),
                    LR_DEFAULTCOLOR,
                );
                if !icon.is_null() {
                    SendMessageW(hwnd, WM_SETICON, kind as usize, icon as LPARAM);
                    given = true;
                }
            }
        }
    }
    given
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_does_not_crash() {
        // Headless CI sessions may legitimately have no windows at all.
        let _ = top_level_windows();
    }

    #[test]
    fn the_foreground_is_read_and_the_thread_keeps_its_dpi_awareness() {
        use windows_sys::Win32::UI::HiDpi::{
            AreDpiAwarenessContextsEqual, GetThreadDpiAwarenessContext,
        };
        // SAFETY: plain call without arguments.
        let before = unsafe { GetThreadDpiAwarenessContext() };
        // What is in front depends on the machine; only the DPI awareness is checked.
        let _ = foreground_presentation();
        // SAFETY: plain call without arguments.
        let after = unsafe { GetThreadDpiAwarenessContext() };
        // SAFETY: two contexts Windows gave.
        assert_ne!(unsafe { AreDpiAwarenessContextsEqual(before, after) }, 0);
    }

    /// Never shown, so it takes no foreground and nothing appears on screen.
    struct TestWindow(HWND);

    impl TestWindow {
        fn new(style: u32, area: RECT) -> Self {
            use windows_sys::Win32::UI::WindowsAndMessaging::CreateWindowExW;
            let class = crate::wide::to_wide("STATIC");
            // SAFETY: a predefined class whose name outlives the call; no parent, menu or data.
            // Created in physical pixels, as `presentation_of` measures them.
            let hwnd = unsafe {
                let _physical = PhysicalPixels::enter();
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    null(),
                    style,
                    area.left,
                    area.top,
                    area.right - area.left,
                    area.bottom - area.top,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    GetModuleHandleW(null()),
                    null(),
                )
            };
            assert!(!hwnd.is_null(), "test window not created");
            Self(hwnd)
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            use windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow;
            // SAFETY: a window this thread created, destroyed once.
            unsafe { DestroyWindow(self.0) };
        }
    }

    /// In physical pixels; `None` on a machine without a monitor.
    fn primary_monitor() -> Option<RECT> {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromPoint};
        let _physical = PhysicalPixels::enter();
        // SAFETY: plain call; null without a monitor at the point.
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTONULL) };
        if monitor.is_null() {
            return None;
        }
        // SAFETY: MONITORINFO is plain data for which all-zero is a valid value.
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = u32::try_from(size_of::<MONITORINFO>()).ok()?;
        // SAFETY: a monitor handle Windows just gave; `info` is writable and carries its size.
        (unsafe { GetMonitorInfoW(monitor, &raw mut info) } != 0).then_some(info.rcMonitor)
    }

    #[test]
    fn a_frameless_window_over_its_monitor_is_read_as_one_and_a_framed_one_as_framed() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WS_OVERLAPPEDWINDOW, WS_POPUP};
        let Some(monitor) = primary_monitor() else {
            eprintln!("skipped: no monitor");
            return;
        };
        // Styled as a full-screen game (Unreal's).
        let game = TestWindow::new(WS_POPUP, monitor);
        let Some(read) = presentation_of(game.0) else {
            eprintln!("skipped: the window manager's facts cannot be read here");
            return;
        };
        assert_eq!(
            read,
            Presentation {
                framed: false,
                fills_monitor: true,
                maximized: false,
                cloaked: false,
                shell: false,
                packaged: false,
            }
        );
        let short = RECT {
            right: monitor.right - 1,
            ..monitor
        };
        let narrow = TestWindow::new(WS_POPUP, short);
        assert!(!presentation_of(narrow.0).unwrap().fills_monitor);
        // A desktop app's styles; custom title bars (Chromium's, Firefox's) keep them too.
        let app = TestWindow::new(WS_OVERLAPPEDWINDOW, monitor);
        let read = presentation_of(app.0).unwrap();
        assert!(read.framed && !read.maximized, "{read:?}");
    }

    #[test]
    fn a_packaged_app_s_frame_is_known_by_its_class() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW, WS_POPUP,
        };
        let Some(monitor) = primary_monitor() else {
            eprintln!("skipped: no monitor");
            return;
        };
        // The frame's class, in this process, which like ApplicationFrameHost.exe is unpackaged.
        let name = crate::wide::to_wide(PACKAGED_APP_FRAMES[0]);
        // SAFETY: plain call; GetModuleHandleW(null) is this executable.
        let instance = unsafe { GetModuleHandleW(null()) };
        // SAFETY: WNDCLASSW is plain data for which all-zero is a valid value.
        let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
        class.lpfnWndProc = Some(DefWindowProcW);
        class.hInstance = instance;
        class.lpszClassName = name.as_ptr();
        // SAFETY: the class and its name outlive the call; a second registration fails harmlessly.
        unsafe { RegisterClassW(&raw const class) };
        // SAFETY: the class registered above; no parent, menu or creation data.
        let hwnd = unsafe {
            let _physical = PhysicalPixels::enter();
            CreateWindowExW(
                0,
                name.as_ptr(),
                null(),
                WS_POPUP,
                monitor.left,
                monitor.top,
                monitor.right - monitor.left,
                monitor.bottom - monitor.top,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                null(),
            )
        };
        assert!(!hwnd.is_null(), "test window not created");
        let frame = TestWindow(hwnd);
        let Some(read) = presentation_of(frame.0) else {
            eprintln!("skipped: the window manager's facts cannot be read here");
            return;
        };
        assert!(!read.framed && read.fills_monitor, "{read:?}");
        assert!(read.packaged, "{read:?}");
    }

    fn rule() -> WindowRule {
        WindowRule {
            class_name: "SDL_app".to_string(),
            process_name: "steamwebhelper.exe".to_string(),
            require_app_window: true,
            forbid_resizable: true,
        }
    }

    fn big_picture() -> WindowInfo {
        WindowInfo {
            class_name: "SDL_app".to_string(),
            process_name: "SteamWebHelper.exe".to_string(),
            visible: true,
            app_window: true,
            resizable: false,
        }
    }

    #[test]
    fn matches_big_picture() {
        assert!(rule().matches(&big_picture()));
    }

    #[test]
    fn rejects_desktop_client_hidden_and_foreign_windows() {
        assert!(!rule().matches(&WindowInfo {
            resizable: true,
            ..big_picture()
        }));
        assert!(!rule().matches(&WindowInfo {
            visible: false,
            ..big_picture()
        }));
        assert!(!rule().matches(&WindowInfo {
            app_window: false,
            ..big_picture()
        }));
        assert!(!rule().matches(&WindowInfo {
            process_name: "game.exe".to_string(),
            ..big_picture()
        }));
        assert!(!rule().matches(&WindowInfo {
            class_name: "Chrome".to_string(),
            ..big_picture()
        }));
    }

    #[test]
    fn a_rule_no_window_meets_finds_nothing() {
        let rule = WindowRule {
            class_name: "Mujina test class no window has".to_string(),
            process_name: "no-such-process.exe".to_string(),
            require_app_window: false,
            forbid_resizable: false,
        };
        assert_eq!(find_top_level(&rule), None);
    }

    #[test]
    fn a_missing_executable_is_not_started() {
        let missing = std::env::temp_dir().join("mujina-no-such-program.exe");
        let error =
            spawn_with_foreground(&missing, &["--flag"], &std::env::temp_dir()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}

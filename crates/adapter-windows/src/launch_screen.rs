//! The home app's full-screen window while the launcher starts. Not topmost, so the launcher's
//! windows come in front; the costly readiness check runs only when WinEvent hooks see a window.

use std::cell::Cell;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use mujina_application::ports::LaunchScreen;
use mujina_winutil::wide::to_wide;
use mujina_winutil::window::OWN_INPUT_TAG;
use windows_sys::Win32::Foundation::{HWND, WAIT_FAILED};
use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW,
    SendInput, VK_LMENU,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EVENT_OBJECT_SHOW,
    EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetSystemMetrics, HWND_TOPMOST, IDC_ARROW,
    LWA_ALPHA, LoadCursorW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PeekMessageW, QS_ALLINPUT, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WNDCLASSW, WS_EX_APPWINDOW,
    WS_EX_LAYERED, WS_POPUP,
};

/// `OBJID_WINDOW`: the event concerns a window itself, not a part of one.
const OBJID_WINDOW: i32 = 0;

thread_local! {
    /// Set by the WinEvent hooks, which run on this thread while it pumps messages.
    static SOMETHING_APPEARED: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn on_window_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    _hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    // "Shown" fires for every tooltip, caret and list item; only whole windows matter.
    if event == EVENT_SYSTEM_FOREGROUND || (object == OBJID_WINDOW && child == 0) {
        SOMETHING_APPEARED.with(|flag| flag.set(true));
    }
}

fn hook(event: u32) -> HWINEVENTHOOK {
    // SAFETY: `on_window_event` matches WINEVENTPROC; out-of-context hooks need no module.
    unsafe {
        SetWinEventHook(
            event,
            event,
            null_mut(),
            Some(on_window_event),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    }
}

/// Tagged with `OWN_INPUT_TAG` so that the agent's keyboard hook lets it be.
fn alt(up: bool) -> INPUT {
    // SAFETY: plain call; it only looks the scan code up.
    let scan = unsafe { MapVirtualKeyW(u32::from(VK_LMENU), MAPVK_VK_TO_VSC) };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_LMENU,
                // Some input stacks read the scan code rather than the virtual key.
                wScan: u16::try_from(scan).unwrap_or(0),
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: OWN_INPUT_TAG,
            },
        },
    }
}

/// For when the agent, not Windows, asked for the home role. Windows lets the process that
/// produced the last input take the foreground, hence one synthetic tap of the Alt key.
fn claim_foreground(window: HWND) {
    let tap = [alt(false), alt(true)];
    let size = i32::try_from(size_of::<INPUT>()).unwrap_or(0);
    // SAFETY: two valid INPUT structures of the stated size.
    unsafe { SendInput(2, tap.as_ptr(), size) };
    // The tap is processed asynchronously; give it a few moments to count as this process's.
    let in_front = (0..10).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        // Being in front already counts: behind the lock screen the request keeps being refused
        // although the window has become the foreground window.
        // SAFETY: valid window handle owned by this thread.
        unsafe { GetForegroundWindow() == window || SetForegroundWindow(window) != 0 }
    });
    if in_front {
        log::info!("the launch screen is in front (after a synthetic key tap)");
    } else {
        log::info!("Windows did not grant the foreground (expected behind the lock screen)");
    }
}

fn pump_messages() {
    // SAFETY: MSG is plain data for which all-zero is a valid value.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: `message` is writable; a null window means "any message of this thread".
    while unsafe { PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
        // SAFETY: `message` was filled in by PeekMessageW.
        unsafe { DispatchMessageW(&raw const message) };
    }
}

#[derive(Debug, Default)]
pub struct WindowsLaunchScreen {
    window: Cell<isize>,
    invisible: bool,
}

impl WindowsLaunchScreen {
    pub fn black() -> Self {
        Self::default()
    }

    /// Windows needs the home app to have a window, or it keeps its welcome screen up and activates
    /// the home app again and again. See-through, as a black one adds a flash.
    pub fn invisible() -> Self {
        Self {
            window: Cell::new(0),
            invisible: true,
        }
    }
}

impl Drop for WindowsLaunchScreen {
    fn drop(&mut self) {
        LaunchScreen::close(self);
    }
}

impl LaunchScreen for WindowsLaunchScreen {
    fn show(&self) {
        if self.window.get() != 0 {
            return;
        }
        let class_name = to_wide("MujinaLaunchScreen");
        let title = to_wide("Mujina");
        // SAFETY: the class and both strings outlive the calls that use them; a failed
        // registration makes CreateWindowExW fail, which is handled below.
        let window = unsafe {
            let instance = GetModuleHandleW(null());
            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(DefWindowProcW),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: null_mut(),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: GetStockObject(BLACK_BRUSH),
                lpszMenuName: null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&raw const class);
            CreateWindowExW(
                // A real application window, which Windows waits for (see `invisible`).
                if self.invisible {
                    WS_EX_APPWINDOW | WS_EX_LAYERED
                } else {
                    WS_EX_APPWINDOW
                },
                class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        if window.is_null() {
            log::warn!("launch screen could not be created");
            return;
        }
        if self.invisible {
            // Alpha 1 of 255, not 0: a fully transparent window may not count as one.
            // SAFETY: valid window handle owned by this thread; the colour key is not used.
            unsafe { SetLayeredWindowAttributes(window, 0, 1, LWA_ALPHA) };
        }
        // SAFETY: valid window handle owned by this thread.
        let in_front = unsafe {
            ShowWindow(window, SW_SHOW);
            SetForegroundWindow(window) != 0
        };
        // Showing a first window usually brings it to the front even when the explicit request
        // is reported as refused, so look before resorting to the workaround.
        // SAFETY: plain call without arguments.
        if !in_front && unsafe { GetForegroundWindow() } != window {
            claim_foreground(window);
        }
        self.window.set(window as isize);
        pump_messages();
    }

    fn hold_until(&self, ready: &dyn Fn() -> bool, timeout: Duration) -> bool {
        let hooks = [hook(EVENT_SYSTEM_FOREGROUND), hook(EVENT_OBJECT_SHOW)];
        let deadline = Instant::now() + timeout;

        let mut is_ready = ready();
        while !is_ready {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let milliseconds = u32::try_from(left.as_millis()).unwrap_or(u32::MAX - 1);
            // SAFETY: no handles are passed; the call only waits for messages or the timeout.
            let woken = unsafe {
                MsgWaitForMultipleObjectsEx(
                    0,
                    null(),
                    milliseconds,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                )
            };
            if woken == WAIT_FAILED {
                break;
            }
            pump_messages();
            if SOMETHING_APPEARED.with(|flag| flag.replace(false)) {
                is_ready = ready();
            }
        }

        for hook in hooks {
            if !hook.is_null() {
                // SAFETY: the hook was installed by this thread and is removed exactly once.
                unsafe { UnhookWinEvent(hook) };
            }
        }
        is_ready
    }

    fn raise(&self) {
        let window = self.window.get();
        if window == 0 {
            return;
        }
        // SAFETY: the window was created by this thread.
        unsafe {
            SetWindowPos(
                window as HWND,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    fn close(&self) {
        let window = self.window.replace(0);
        if window != 0 {
            // SAFETY: the window was created by this thread and is destroyed exactly once.
            unsafe { DestroyWindow(window as HWND) };
        }
    }
}

//! The first connected game controller, read from XInput on demand.

use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};

/// XInput knows at most this many controllers.
const SLOTS: u32 = 4;
const ERROR_SUCCESS: u32 = 0;

/// Buttons (XInput's `XINPUT_GAMEPAD_*` bits) and the left stick of one controller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadState {
    pub buttons: u16,
    pub left_x: i16,
    pub left_y: i16,
}

pub fn first_connected() -> Option<PadState> {
    (0..SLOTS).find_map(|slot| {
        // SAFETY: XINPUT_STATE is plain data for which all-zero is a valid value.
        let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
        // SAFETY: `state` is writable; an unused slot is reported as an error code.
        let result = unsafe { XInputGetState(slot, &raw mut state) };
        (result == ERROR_SUCCESS).then_some(PadState {
            buttons: state.Gamepad.wButtons,
            left_x: state.Gamepad.sThumbLX,
            left_y: state.Gamepad.sThumbLY,
        })
    })
}

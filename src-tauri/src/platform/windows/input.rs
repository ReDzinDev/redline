use std::mem::size_of;
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_V,
};

const MODIFIER_KEYS: &[VIRTUAL_KEY] = &[
    VK_CONTROL,
    VK_LCONTROL,
    VK_RCONTROL,
    VK_MENU,
    VK_LMENU,
    VK_RMENU,
    VK_SHIFT,
    VK_LSHIFT,
    VK_RSHIFT,
    VK_LWIN,
    VK_RWIN,
];

fn is_key_down(vk: VIRTUAL_KEY) -> bool {
    // SAFETY: GetAsyncKeyState has no pointer invariants.
    unsafe { GetAsyncKeyState(i32::from(vk.0)) < 0 }
}

fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    // SAFETY: `inputs` is a valid contiguous slice of `INPUT` structs.
    let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        log::warn!("SendInput delivered {sent}/{} events", inputs.len());
    }
}

/// Waits up to `timeout` (spec: 300 ms for capture) for all modifier keys to be physically
/// released, then forces synthetic key-ups for any modifiers still reported down.
pub fn wait_for_modifiers_released(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !MODIFIER_KEYS.iter().any(|&vk| is_key_down(vk)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let forced_ups: Vec<INPUT> = MODIFIER_KEYS
        .iter()
        .copied()
        .filter(|&vk| is_key_down(vk))
        .map(|vk| key_input(vk, true))
        .collect();
    if !forced_ups.is_empty() {
        send_inputs(&forced_ups);
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Simulates `Ctrl+C` (or `Ctrl+Shift+C` in terminal emulators).
pub fn send_copy(ctrl_shift_c: bool) {
    if ctrl_shift_c {
        let inputs = [
            key_input(VK_CONTROL, false),
            key_input(VK_SHIFT, false),
            key_input(VK_C, false),
            key_input(VK_C, true),
            key_input(VK_SHIFT, true),
            key_input(VK_CONTROL, true),
        ];
        send_inputs(&inputs);
    } else {
        let inputs = [
            key_input(VK_CONTROL, false),
            key_input(VK_C, false),
            key_input(VK_C, true),
            key_input(VK_CONTROL, true),
        ];
        send_inputs(&inputs);
    }
}

/// Simulates `Ctrl+V` immediately (caller must wait for modifiers released and verify foreground target first).
pub fn send_paste_keys() {
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ];
    send_inputs(&inputs);
}

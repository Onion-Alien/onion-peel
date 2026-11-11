//! Catches Win+Shift+S before Windows (and Snipping Tool) sees it.
//! Runs on its own thread, so a capture in progress never makes typing lag.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::{main_hwnd, overlay, WM_CANCEL, WM_MODE, WM_SNIP};

/// We swallowed the S key-down, so swallow its key-up too.
static ATE_S: AtomicBool = AtomicBool::new(false);

pub fn held(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn post(msg: u32, wp: usize) {
    unsafe {
        let _ = PostMessageW(main_hwnd(), msg, WPARAM(wp), LPARAM(0));
    }
}

/// Press and let go of a key, as if typed. `extra` marks the two events as ours.
pub fn tap(vk: VIRTUAL_KEY, extra: usize) {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, dwExtraInfo: extra, ..Default::default() } },
    };
    unsafe {
        SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], size_of::<INPUT>() as i32);
    }
}

/// Tap an unassigned key, so letting go of Win doesn't open the Start menu.
fn mask_win_key() {
    tap(VIRTUAL_KEY(0xE8), 0);
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        if k.flags.0 & LLKHF_INJECTED.0 == 0 {
            match VIRTUAL_KEY(k.vkCode as u16) {
                VK_S if down && (held(VK_LWIN) || held(VK_RWIN)) && held(VK_SHIFT) && !held(VK_CONTROL) && !held(VK_MENU) => {
                    if !ATE_S.swap(true, Relaxed) {
                        mask_win_key();
                        post(WM_SNIP, 0);
                    }
                    return LRESULT(1);
                }
                // S still held after Win or Shift was let go: its repeats belong to the hotkey
                // too. Passed on, the app in front would see S go down and never come up.
                VK_S if down && ATE_S.load(Relaxed) => return LRESULT(1),
                VK_S if !down && ATE_S.swap(false, Relaxed) => return LRESULT(1),
                // Esc always closes the snip, even if a game kept keyboard focus.
                VK_ESCAPE if down && overlay::OPEN.load(Relaxed) => {
                    post(WM_CANCEL, 0);
                    return LRESULT(1);
                }
                // 1, 2, 3 (top row or number pad) pick Rectangle, Window, Full screen.
                vk @ (VK_1 | VK_2 | VK_3 | VK_NUMPAD1 | VK_NUMPAD2 | VK_NUMPAD3)
                    if down && overlay::OPEN.load(Relaxed) && !held(VK_CONTROL) && !held(VK_MENU) =>
                {
                    let first = if vk.0 >= VK_NUMPAD1.0 { VK_NUMPAD1 } else { VK_1 };
                    post(WM_MODE, (vk.0 - first.0) as usize);
                    return LRESULT(1);
                }
                _ => {}
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wp, lp)
}

pub fn spawn() {
    std::thread::spawn(|| unsafe {
        if SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), HINSTANCE::default(), 0).is_ok() {
            // Windows calls the hook from inside GetMessageW; nothing is ever posted here.
            // (GetMessageW gives -1 for an error, which must end the loop too.)
            let mut m = MSG::default();
            while GetMessageW(&mut m, HWND::default(), 0, 0).0 > 0 {}
        }
        // No hook, no hotkey: quit rather than sit there doing nothing.
        post(WM_CLOSE, 0);
    });
}

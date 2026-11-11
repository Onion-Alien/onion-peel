//! What's on the desktop right now: the windows a person could snip, and the mouse.

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub fn cursor_pos() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

pub fn class_name(h: HWND) -> String {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut class) } as usize;
    String::from_utf16_lossy(&class[..n])
}

/// The Alt+Tab switcher (Windows 11, then 10).
pub fn is_switcher(h: HWND) -> bool {
    matches!(class_name(h).as_str(), "XamlExplorerHostIslandWindow" | "MultitaskingViewFrame" | "TaskSwitcherWnd")
}

/// Visible top-level windows, top of the z-order first, in screen coordinates.
pub fn visible_windows() -> Vec<RECT> {
    unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp.0 as *mut Vec<RECT>);
        out.extend(pickable(h));
        TRUE
    }
    let mut out = vec![];
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

/// A window's frame on screen, if it's something a person would want to snip.
pub fn pickable(h: HWND) -> Option<RECT> {
    unsafe {
        if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() || dwm::<u32>(h, DWMWA_CLOAKED).unwrap_or(0) != 0 {
            return None;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TRANSPARENT.0 != 0 {
            return None; // click-through overlays
        }
        if ex & WS_EX_LAYERED.0 != 0 {
            let (mut alpha, mut flags) = (0u8, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
            let got = GetLayeredWindowAttributes(h, None, Some(&mut alpha), Some(&mut flags)).is_ok();
            if got && flags.0 & LWA_ALPHA.0 != 0 && alpha == 0 {
                return None; // fully see-through
            }
        }
        match class_name(h).as_str() {
            "Progman" | "WorkerW" => return None, // the desktop itself: that's what Full screen is for
            "OnionPeelOverlay" => return None,      // the snip screen, when the picture is retaken
            _ => {}
        }
        let r = dwm(h, DWMWA_EXTENDED_FRAME_BOUNDS).unwrap_or_else(|| {
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            r
        });
        (r.right - r.left >= 8 && r.bottom - r.top >= 8).then_some(r)
    }
}

unsafe fn dwm<T: Default>(h: HWND, attr: DWMWINDOWATTRIBUTE) -> Option<T> {
    let mut v = T::default();
    DwmGetWindowAttribute(h, attr, &mut v as *mut T as _, size_of::<T>() as u32).ok().map(|_| v)
}

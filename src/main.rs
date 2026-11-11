//! Onion Peel: press Win+Shift+S, drag a box, it's on the clipboard. Nothing else.
//! No window, no tray icon, no files, no history. The installer (installer/OnionPeel.iss)
//! puts it on the PC; this exe never installs itself.
//!
//!   onion-peel.exe              run: from the Start menu, or when Windows starts
//!   onion-peel.exe --quit       stop the running copy (the installer uses it before an update)
#![windows_subsystem = "windows"]

mod capture;
mod clip;
mod desktop;
mod dib;
mod geom;
mod hook;
mod overlay;
#[cfg(feature = "selftest")]
mod selftest;

use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{CreateMutexW, OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Posted by the keyboard hook: open the snip screen / close it.
pub const WM_SNIP: u32 = WM_APP + 1;
pub const WM_CANCEL: u32 = WM_APP + 2;
/// Posted by the keyboard hook: switch the snip screen to mode `wParam` (0, 1, 2).
pub const WM_MODE: u32 = WM_APP + 3;
pub const CLASS: PCWSTR = w!("OnionPeelMain");

static MAIN: AtomicIsize = AtomicIsize::new(0);

/// The hidden message window: the hook posts to it, and it owns the clipboard.
pub fn main_hwnd() -> HWND {
    HWND(MAIN.load(Relaxed) as _)
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SNIP => overlay::open(),
        WM_CANCEL => overlay::close(None),
        WM_MODE => overlay::set_mode(wp.0),
        WM_CLOSE => PostQuitMessage(0),
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    LRESULT(0)
}

/// Stop the running copy, if there is one, and wait until it has gone (so its exe is free).
fn quit_running() {
    unsafe {
        let Ok(hwnd) = FindWindowExW(HWND_MESSAGE, HWND::default(), CLASS, PCWSTR::null()) else { return };
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_SYNCHRONIZE, false, pid);
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        if let Ok(p) = process {
            WaitForSingleObject(p, 5000);
            let _ = CloseHandle(p);
        }
    }
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--quit") => return quit_running(),
        #[cfg(feature = "selftest")]
        Some("--selftest") => return selftest::run(&args.next().unwrap_or(".".into())),
        _ => {}
    }

    unsafe {
        // One copy at a time; the mutex lives until the process exits.
        let _mutex = CreateMutexW(None, true, w!("Local\\OnionPeelSingleInstance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        let module = GetModuleHandleW(None).unwrap_or_default();
        let class = WNDCLASSW { lpfnWndProc: Some(main_proc), hInstance: module.into(), lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CLASS,
            w!("Onion Peel"),
            WINDOW_STYLE(0),
            0, 0, 0, 0,
            HWND_MESSAGE,
            HMENU::default(),
            module,
            None,
        ) else {
            return;
        };
        MAIN.store(hwnd.0 as isize, Relaxed);
        hook::spawn();
        let mut m = MSG::default();
        // GetMessageW gives 0 for WM_QUIT and -1 for an error: both end the program.
        while GetMessageW(&mut m, HWND::default(), 0, 0).0 > 0 {
            DispatchMessageW(&m);
        }
    }
}

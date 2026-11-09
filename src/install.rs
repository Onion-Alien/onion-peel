//! Installing and uninstalling. Everything goes in the user's own folders and registry, so
//! there is no admin prompt:
//!
//!   %LOCALAPPDATA%\Programs\Onion Peel\onion-peel.exe
//!   Start menu: "Onion Peel" (starts it) and "Uninstall Onion Peel"
//!   Settings > Apps > Installed apps: "Onion Peel"
//!   HKCU\...\Run "OnionPeel": starts it when Windows starts, if wanted

use std::path::{Path, PathBuf};

use windows::core::{w, Interface, GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const UNINSTALL_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OnionPeel");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const NAME: PCWSTR = w!("OnionPeel");
const SHORTCUTS: [&str; 2] = ["Onion Peel.lnk", "Uninstall Onion Peel.lnk"];

struct Places {
    dir: PathBuf,
    exe: PathBuf,
    /// The Start menu's Programs folder.
    menu: PathBuf,
}

fn known(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_CREATE, HANDLE::default()).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as _));
        path
    }
}

fn places() -> Option<Places> {
    let dir = known(&FOLDERID_UserProgramFiles)?.join("Onion Peel");
    Some(Places { exe: dir.join("onion-peel.exe"), dir, menu: known(&FOLDERID_Programs)? })
}

/// The same file, whatever case or path form each is written in.
fn same(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// This is the installed copy (the normal case), not one run from Downloads, say.
pub fn is_installed_copy() -> bool {
    match (places(), std::env::current_exe()) {
        (Some(p), Ok(me)) => same(&me, &p.exe),
        _ => true, // nowhere to install to: just run
    }
}

pub fn is_installed() -> bool {
    places().is_some_and(|p| p.exe.exists())
}

pub fn starts_with_windows() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, NAME, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

/// Stop the running copy, if there is one, and wait until it has gone (so its exe is free).
pub fn quit_running() {
    unsafe {
        let Ok(hwnd) = FindWindowExW(HWND_MESSAGE, HWND::default(), crate::CLASS, PCWSTR::null()) else { return };
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

/// Copy this exe into place, add the Start menu and Installed apps entries, set whether it
/// starts with Windows, and start it. Running it again updates it.
pub fn install(startup: bool) -> Result<(), String> {
    let p = places().ok_or("Windows didn't say where programs go.")?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    quit_running();
    std::fs::create_dir_all(&p.dir).map_err(|e| format!("Couldn't make the folder {}: {e}", p.dir.display()))?;
    if !same(&me, &p.exe) {
        std::fs::copy(&me, &p.exe).map_err(|e| format!("Couldn't copy Onion Peel to {}: {e}", p.dir.display()))?;
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        shortcut(&p.menu.join(SHORTCUTS[0]), &p.exe, "", "Snip part of the screen to the clipboard with Win+Shift+S")
            .and_then(|_| shortcut(&p.menu.join(SHORTCUTS[1]), &p.exe, "--uninstall", "Remove Onion Peel from this PC"))
            .map_err(|e| format!("Couldn't add Onion Peel to the Start menu: {e}"))?;
    }
    let exe = p.exe.to_string_lossy();
    let kb = std::fs::metadata(&p.exe).map_or(0, |m| m.len() / 1024) as u32;
    set(UNINSTALL_KEY, &[
        ("DisplayName", Val::Text("Onion Peel")),
        ("DisplayIcon", Val::Text(&exe)),
        ("DisplayVersion", Val::Text(env!("CARGO_PKG_VERSION"))),
        ("InstallLocation", Val::Text(&p.dir.to_string_lossy())),
        ("UninstallString", Val::Text(&format!("\"{exe}\" --uninstall"))),
        ("NoModify", Val::Number(1)),
        ("NoRepair", Val::Number(1)),
        ("EstimatedSize", Val::Number(kb)),
    ])?;
    if startup {
        set(RUN_KEY, &[("OnionPeel", Val::Text(&format!("\"{exe}\"")))])?;
    } else {
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, NAME);
        }
    }
    start(&format!("\"{exe}\""), false).map_err(|e| format!("Onion Peel is installed, but didn't start: {e}"))
}

/// Stop it and remove everything `install` added. When this is the installed exe, it can't
/// delete itself yet: `remove_leftovers` does that once its window has closed.
pub fn uninstall() -> Result<(), String> {
    let p = places().ok_or("Windows didn't say where programs go.")?;
    quit_running();
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, NAME);
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY);
    }
    for name in SHORTCUTS {
        let _ = std::fs::remove_file(p.menu.join(name));
    }
    if !is_installed_copy() {
        match std::fs::remove_file(&p.exe) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("Couldn't delete {}: {e}", p.exe.display())),
            _ => {}
        }
        let _ = std::fs::remove_dir(&p.dir); // only if it's empty
    }
    Ok(())
}

/// After uninstalling from the installed exe: delete it and its folder once this process has
/// exited, with a hidden command that retries for up to half a minute.
pub fn remove_leftovers() {
    let Some(p) = places().filter(|_| is_installed_copy()) else { return };
    let (exe, dir) = (p.exe.display(), p.dir.display());
    let script = format!(
        "cmd /d /s /c \"for /l %i in (1,1,30) do @(ping -n 2 127.0.0.1 >nul & del /f /q \"{exe}\" 2>nul & if not exist \"{exe}\" (rmdir \"{dir}\" 2>nul & exit))\""
    );
    let _ = start(&script, true);
}

/// Start a program and don't wait for it; `hidden` keeps a console program's window away.
fn start(command_line: &str, hidden: bool) -> windows::core::Result<()> {
    let mut line: Vec<u16> = command_line.encode_utf16().chain([0]).collect();
    let flags = if hidden { CREATE_NO_WINDOW } else { PROCESS_CREATION_FLAGS(0) };
    let si = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut pi = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(PCWSTR::null(), PWSTR(line.as_mut_ptr()), None, None, false, flags, None, PCWSTR::null(), &si, &mut pi)?;
        let _ = CloseHandle(pi.hProcess);
        let _ = CloseHandle(pi.hThread);
    }
    Ok(())
}

unsafe fn shortcut(lnk: &Path, exe: &Path, args: &str, about: &str) -> windows::core::Result<()> {
    let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
    link.SetPath(&HSTRING::from(exe))?;
    link.SetArguments(&HSTRING::from(args))?;
    link.SetDescription(&HSTRING::from(about))?;
    link.cast::<IPersistFile>()?.Save(&HSTRING::from(lnk), true)
}

enum Val<'a> {
    Text(&'a str),
    Number(u32),
}

/// Write values under a key in HKEY_CURRENT_USER, making the key if needed.
fn set(key: PCWSTR, values: &[(&str, Val)]) -> Result<(), String> {
    unsafe {
        let mut k = HKEY::default();
        let made = RegCreateKeyExW(HKEY_CURRENT_USER, key, 0, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut k, None);
        made.ok().map_err(|e| format!("Couldn't write to the registry: {e}"))?;
        let mut result = Ok(());
        for (name, v) in values {
            let (kind, data): (_, Vec<u8>) = match v {
                Val::Text(s) => (REG_SZ, s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()),
                Val::Number(n) => (REG_DWORD, n.to_le_bytes().to_vec()),
            };
            let wrote = RegSetValueExW(k, &HSTRING::from(*name), 0, kind, Some(&data)).ok();
            if let (Err(e), Ok(())) = (wrote, &result) {
                result = Err(format!("Couldn't write to the registry: {e}"));
            }
        }
        let _ = RegCloseKey(k);
        result
    }
}

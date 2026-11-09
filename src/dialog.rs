//! The small window that installs and uninstalls Onion Peel, styled like a Windows 11 dialog
//! and light or dark like the user's apps. Drawn into a buffer like the snip screen, so the
//! self-test can picture every page without opening a window.

use std::cell::RefCell;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dib::{rgb, Dib};
use crate::install;

const CLASS: PCWSTR = w!("OnionPeelSetup");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_CAPTION.0 | WS_SYSMENU.0);
const WM_MOUSELEAVE: u32 = 0x02a3; // lives in the Controls bindings, which nothing else here needs

thread_local! {
    static STATE: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Install,
    Uninstall,
    Installed,
    Uninstalled,
    Failed,
}

pub struct Page {
    pub kind: Kind,
    title: String,
    body: String,
    /// The "start when Windows starts" box, on the install page.
    pub startup: Option<bool>,
    /// Left to right. Enter presses the first, Esc the last.
    buttons: Vec<&'static str>,
    /// Shown on the first button while it works.
    pub busy: Option<&'static str>,
}

impl Page {
    pub fn install() -> Page {
        let update = install::is_installed();
        Page::install_as(update, !update || install::starts_with_windows())
    }

    pub fn install_as(update: bool, startup: bool) -> Page {
        Page {
            kind: Kind::Install,
            title: if update { "Update Onion Peel" } else { "Install Onion Peel" }.into(),
            body: "Press Win+Shift+S to snip any part of the screen to the clipboard. \
                   Onion Peel runs quietly in the background, with no window or tray icon."
                .into(),
            startup: Some(startup),
            buttons: vec![if update { "Update" } else { "Install" }, "Cancel"],
            busy: None,
        }
    }

    pub fn uninstall() -> Page {
        Page {
            kind: Kind::Uninstall,
            title: "Uninstall Onion Peel?".into(),
            body: "Onion Peel will stop and be removed from this PC. \
                   Win+Shift+S goes back to Windows' own Snipping Tool."
                .into(),
            startup: None,
            buttons: vec!["Uninstall", "Cancel"],
            busy: None,
        }
    }

    pub fn finished(kind: Kind, title: &str, body: &str) -> Page {
        Page { kind, title: title.into(), body: body.into(), startup: None, buttons: vec!["Close"], busy: None }
    }

    pub fn installed(update: bool) -> Page {
        let title = if update { "Onion Peel is updated" } else { "Onion Peel is installed" };
        Page::finished(Kind::Installed, title, "Press Win+Shift+S to snip. To start or uninstall Onion Peel later, search for it in the Start menu.")
    }

    pub fn uninstalled() -> Page {
        Page::finished(Kind::Uninstalled, "Onion Peel is uninstalled", "It has been removed from this PC.")
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Check,
    Button(usize),
}

struct Colors {
    bg: COLORREF,
    footer: COLORREF,
    line: COLORREF,
    title: COLORREF,
    text: COLORREF,
    /// The main button and the ticked box: normal, hovered, pressed, and what's drawn on it.
    accent: [COLORREF; 3],
    on_accent: COLORREF,
    /// The other buttons: normal, hovered, pressed, then their edge.
    button: [COLORREF; 3],
    button_edge: COLORREF,
    /// The empty box: edge, and its fill when hovered.
    box_edge: COLORREF,
    box_hover: COLORREF,
}

// Windows 11's own dialog colours; the dark accent is the snip screen's.
const DARK: Colors = Colors {
    bg: rgb(0x2b, 0x2b, 0x2b),
    footer: rgb(0x20, 0x20, 0x20),
    line: rgb(0x1a, 0x1a, 0x1a),
    title: rgb(0xff, 0xff, 0xff),
    text: rgb(0xcf, 0xcf, 0xcf),
    accent: [rgb(0x4c, 0xc2, 0xff), rgb(0x47, 0xb1, 0xe8), rgb(0x42, 0x9f, 0xd1)],
    on_accent: rgb(0, 0, 0),
    button: [rgb(0x37, 0x37, 0x37), rgb(0x45, 0x45, 0x45), rgb(0x2c, 0x2c, 0x2c)],
    button_edge: rgb(0x43, 0x43, 0x43),
    box_edge: rgb(0xa0, 0xa0, 0xa0),
    box_hover: rgb(0x33, 0x33, 0x33),
};

const LIGHT: Colors = Colors {
    bg: rgb(0xff, 0xff, 0xff),
    footer: rgb(0xf3, 0xf3, 0xf3),
    line: rgb(0xe5, 0xe5, 0xe5),
    title: rgb(0x1b, 0x1b, 0x1b),
    text: rgb(0x5d, 0x5d, 0x5d),
    accent: [rgb(0x00, 0x5f, 0xb8), rgb(0x19, 0x6e, 0xbf), rgb(0x31, 0x7d, 0xc6)],
    on_accent: rgb(0xff, 0xff, 0xff),
    button: [rgb(0xfb, 0xfb, 0xfb), rgb(0xee, 0xee, 0xee), rgb(0xe3, 0xe3, 0xe3)],
    button_edge: rgb(0xd5, 0xd5, 0xd5),
    box_edge: rgb(0x8a, 0x8a, 0x8a),
    box_hover: rgb(0xf3, 0xf3, 0xf3),
};

#[derive(Default)]
pub struct Layout {
    icon: RECT,
    title: RECT,
    body: RECT,
    check: RECT,
    check_label: RECT,
    footer: RECT,
    pub buttons: Vec<RECT>,
}

pub struct Dialog {
    hwnd: HWND,
    pub page: Page,
    dark: bool,
    dpi: u32,
    pub back: Dib,
    /// Title, text, and the check mark glyph.
    fonts: [HFONT; 3],
    icon: HICON,
    pub lay: Layout,
    pub hover: Option<Hit>,
    pub pressed: Option<Hit>,
}

fn contains(r: &RECT, p: (i32, i32)) -> bool {
    p.0 >= r.left && p.0 < r.right && p.1 >= r.top && p.1 < r.bottom
}

impl Dialog {
    pub fn new(page: Page, dark: bool, dpi: u32) -> Option<Dialog> {
        let mut d = Dialog {
            hwnd: HWND::default(),
            page,
            dark,
            dpi,
            back: Dib::new(1, 1)?,
            fonts: [HFONT::default(); 3],
            icon: HICON::default(),
            lay: Layout::default(),
            hover: None,
            pressed: None,
        };
        d.set_dpi(dpi)?;
        Some(d)
    }

    fn px(&self, v: f32) -> i32 {
        (v * self.dpi as f32 / 96.0).round() as i32
    }

    fn colors(&self) -> &'static Colors {
        if self.dark {
            &DARK
        } else {
            &LIGHT
        }
    }

    /// Fonts, icon and layout for this DPI.
    fn set_dpi(&mut self, dpi: u32) -> Option<()> {
        self.dpi = dpi;
        self.free();
        let font = |size: f32, weight: i32, face: PCWSTR| unsafe {
            CreateFontW(-self.px(size), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET.0 as u32, OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32, CLEARTYPE_QUALITY.0 as u32, (DEFAULT_PITCH.0 | FF_SWISS.0) as u32, face)
        };
        self.fonts = [
            font(20.0, FW_SEMIBOLD.0 as i32, w!("Segoe UI Semibold")),
            font(14.0, FW_NORMAL.0 as i32, w!("Segoe UI")),
            font(15.0, FW_NORMAL.0 as i32, w!("Segoe MDL2 Assets")),
        ];
        let size = self.px(32.0);
        self.icon = unsafe {
            let module = GetModuleHandleW(None).unwrap_or_default();
            LoadImageW(module, PCWSTR(1 as _), IMAGE_ICON, size, size, LR_DEFAULTCOLOR).map(|h| HICON(h.0)).unwrap_or_default()
        };
        self.relayout()
    }

    /// Lay out the page; the buffer (and the window, if there is one) follows its size.
    fn relayout(&mut self) -> Option<()> {
        let w = self.px(440.0);
        let pad = self.px(24.0);
        let icon = RECT { left: pad, top: pad, right: pad + self.px(32.0), bottom: pad + self.px(32.0) };
        let title = RECT { left: icon.right + self.px(12.0), top: icon.top, right: w - pad, bottom: icon.bottom };
        let mut body = RECT { left: pad, top: icon.bottom + self.px(16.0), right: w - pad, bottom: 0 };
        unsafe {
            SelectObject(self.back.dc, self.fonts[1]);
        }
        body.bottom = body.top + self.back.text(body, &self.page.body, DT_WORDBREAK | DT_CALCRECT);
        let mut bottom = body.bottom;
        let (mut check, mut check_label) = (RECT::default(), RECT::default());
        if self.page.startup.is_some() {
            let top = body.bottom + self.px(20.0);
            check = RECT { left: pad, top, right: pad + self.px(20.0), bottom: top + self.px(20.0) };
            check_label = RECT { left: check.right + self.px(12.0), top: top - self.px(4.0), right: w - pad, bottom: check.bottom + self.px(4.0) };
            bottom = check.bottom;
        }
        let footer = RECT { left: 0, top: bottom + pad, right: w, bottom: bottom + pad + self.px(80.0) };
        let (n, gap) = (self.page.buttons.len() as i32, self.px(8.0));
        let bw = (w - 2 * pad - gap * (n - 1)) / n;
        let buttons = (0..n)
            .map(|i| {
                let left = pad + i * (bw + gap);
                RECT { left, top: footer.top + pad, right: left + bw, bottom: footer.top + pad + self.px(32.0) }
            })
            .collect();
        self.lay = Layout { icon, title, body, check, check_label, footer, buttons };

        if (self.back.w, self.back.h) != (w, footer.bottom) {
            self.back = Dib::new(w, footer.bottom)?;
            unsafe {
                SelectObject(self.back.dc, GetStockObject(DC_BRUSH));
                SelectObject(self.back.dc, GetStockObject(DC_PEN));
                SetBkMode(self.back.dc, TRANSPARENT);
            }
            if !self.hwnd.is_invalid() {
                let (ow, oh) = outer_size(w, footer.bottom, self.dpi);
                unsafe {
                    let _ = SetWindowPos(self.hwnd, HWND::default(), 0, 0, ow, oh, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
            }
        }
        Some(())
    }

    fn set_page(&mut self, page: Page) {
        self.page = page;
        (self.hover, self.pressed) = (None, None);
        let _ = self.relayout();
        self.redraw();
    }

    fn redraw(&self) {
        if !self.hwnd.is_invalid() {
            unsafe {
                let _ = InvalidateRect(self.hwnd, None, false);
            }
        }
    }

    pub fn hit(&self, p: (i32, i32)) -> Option<Hit> {
        let l = &self.lay;
        if self.page.startup.is_some() && (contains(&l.check, p) || contains(&l.check_label, p)) {
            return Some(Hit::Check);
        }
        l.buttons.iter().position(|b| contains(b, p)).map(Hit::Button)
    }

    pub fn paint(&self) {
        let (d, c, l) = (&self.back, self.colors(), &self.lay);
        let state = |h: Hit| if self.pressed == Some(h) && self.hover == Some(h) { 2 } else if self.hover == Some(h) { 1 } else { 0 };
        fill(d, d.rect(), c.bg);
        unsafe {
            let size = l.icon.right - l.icon.left;
            let _ = DrawIconEx(d.dc, l.icon.left, l.icon.top, self.icon, size, size, 0, None, DI_NORMAL);
            SelectObject(d.dc, self.fonts[0]);
            SetTextColor(d.dc, c.title);
        }
        d.text(l.title, &self.page.title, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        unsafe {
            SelectObject(d.dc, self.fonts[1]);
            SetTextColor(d.dc, c.text);
        }
        d.text(l.body, &self.page.body, DT_LEFT | DT_WORDBREAK);

        if let Some(on) = self.page.startup {
            let s = state(Hit::Check);
            let radius = self.px(4.0);
            if on {
                d.round_box(l.check, c.accent[s], c.accent[s], radius);
                unsafe {
                    SelectObject(d.dc, self.fonts[2]);
                    SetTextColor(d.dc, c.on_accent);
                }
                d.text(l.check, "\u{E73E}", DT_CENTER | DT_VCENTER | DT_SINGLELINE); // CheckMark
                unsafe {
                    SelectObject(d.dc, self.fonts[1]);
                }
            } else {
                d.round_box(l.check, if s > 0 { c.box_hover } else { c.bg }, c.box_edge, radius);
            }
            unsafe {
                SetTextColor(d.dc, c.title);
            }
            d.text(l.check_label, "Start Onion Peel when Windows starts", DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        }

        fill(d, l.footer, c.footer);
        fill(d, RECT { bottom: l.footer.top + 1, ..l.footer }, c.line);
        for (i, (&b, &label)) in l.buttons.iter().zip(&self.page.buttons).enumerate() {
            let s = state(Hit::Button(i));
            let (label, text) = match (i, self.page.busy) {
                (0, Some(busy)) => (busy, c.on_accent),
                (0, None) => (label, c.on_accent),
                _ if s == 2 => (label, c.text), // pressed: the label dims, like Windows' own
                _ => (label, c.title),
            };
            if i == 0 {
                d.round_box(b, c.accent[s], c.accent[s], self.px(4.0));
            } else {
                d.round_box(b, c.button[s], c.button_edge, self.px(4.0));
            }
            unsafe {
                SetTextColor(d.dc, text);
            }
            d.text(b, label, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
    }

    fn free(&mut self) {
        unsafe {
            SelectObject(self.back.dc, GetStockObject(SYSTEM_FONT));
            for f in self.fonts {
                let _ = DeleteObject(f);
            }
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}

impl Drop for Dialog {
    fn drop(&mut self) {
        self.free();
    }
}

fn fill(d: &Dib, r: RECT, c: COLORREF) {
    unsafe {
        SetDCBrushColor(d.dc, c);
        FillRect(d.dc, &r, HBRUSH(GetStockObject(DC_BRUSH).0));
    }
}

/// Window size for a client area of `w` x `h`.
fn outer_size(w: i32, h: i32, dpi: u32) -> (i32, i32) {
    let mut r = RECT { left: 0, top: 0, right: w, bottom: h };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
    }
    (r.right - r.left, r.bottom - r.top)
}

/// Dark mode for apps, from Settings > Personalization > Colors.
fn apps_dark() -> bool {
    let (mut light, mut size) = (1u32, 4u32);
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    unsafe {
        let _ = RegGetValueW(HKEY_CURRENT_USER, key, w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some(&mut light as *mut u32 as _), Some(&mut size));
    }
    light == 0
}

/// What a click or key asks for, done once the state is no longer borrowed.
enum Act {
    Nothing,
    Press(usize),
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut act = Act::Nothing;
    let handled = STATE.with(|s| {
        let mut s = s.try_borrow_mut().ok()?;
        let d = s.as_mut()?;
        let p = (lp.0 as i16 as i32, (lp.0 >> 16) as i16 as i32);
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                d.paint();
                let r = ps.rcPaint;
                let _ = BitBlt(dc, r.left, r.top, r.right - r.left, r.bottom - r.top, d.back.dc, r.left, r.top, SRCCOPY);
                let _ = EndPaint(hwnd, &ps);
            }
            WM_ERASEBKGND => return Some(LRESULT(1)),
            WM_MOUSEMOVE => {
                let h = d.hit(p);
                if h != d.hover {
                    d.hover = h;
                    d.redraw();
                }
                let mut track = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
            }
            WM_MOUSELEAVE => {
                d.hover = None;
                d.redraw();
            }
            WM_LBUTTONDOWN if d.page.busy.is_none() => {
                d.pressed = d.hit(p);
                SetCapture(hwnd);
                d.redraw();
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                match d.pressed.take().filter(|&h| d.hit(p) == Some(h)) {
                    Some(Hit::Check) => d.page.startup = d.page.startup.map(|on| !on),
                    Some(Hit::Button(i)) => act = Act::Press(i),
                    None => {}
                }
                d.redraw();
            }
            WM_KEYDOWN if d.page.busy.is_none() => match VIRTUAL_KEY(wp.0 as u16) {
                VK_RETURN => act = Act::Press(0),
                VK_ESCAPE => act = Act::Press(d.page.buttons.len() - 1),
                VK_SPACE if d.page.startup.is_some() => {
                    d.page.startup = d.page.startup.map(|on| !on);
                    d.redraw();
                }
                _ => {}
            },
            WM_CLOSE if d.page.busy.is_some() => {} // not while it's working
            WM_DPICHANGED => {
                let r = *(lp.0 as *const RECT);
                let _ = d.set_dpi((wp.0 & 0xffff) as u32);
                let (ow, oh) = outer_size(d.back.w, d.back.h, d.dpi);
                let _ = SetWindowPos(hwnd, HWND::default(), r.left, r.top, ow, oh, SWP_NOZORDER | SWP_NOACTIVATE);
                d.redraw();
            }
            WM_DESTROY => PostQuitMessage(0),
            _ => return None,
        }
        Some(LRESULT(0))
    });
    if let Act::Press(i) = act {
        press(hwnd, i);
    }
    handled.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wp, lp))
}

/// A button: the first one does the page's job, any other closes.
fn press(hwnd: HWND, i: usize) {
    let Some((kind, startup)) = STATE.with(|s| s.borrow().as_ref().map(|d| (d.page.kind, d.page.startup))) else { return };
    let busy = match (i, kind) {
        (0, Kind::Install) => "Installing\u{2026}",
        (0, Kind::Uninstall) => "Uninstalling\u{2026}",
        _ => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return;
        }
    };
    // Show it's working before the work starts.
    STATE.with(|s| s.borrow_mut().as_mut().map(|d| d.page.busy = Some(busy)));
    unsafe {
        let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
    }
    let next = if kind == Kind::Install {
        let update = install::is_installed();
        match install::install(startup.unwrap_or(false)) {
            Ok(()) => Page::installed(update),
            Err(e) => Page::finished(Kind::Failed, "Couldn't install Onion Peel", &e),
        }
    } else {
        match install::uninstall() {
            Ok(()) => Page::uninstalled(),
            Err(e) => Page::finished(Kind::Failed, "Couldn't uninstall Onion Peel", &e),
        }
    };
    STATE.with(|s| s.borrow_mut().as_mut().map(|d| d.set_page(next)));
}

/// Show the window until it's closed; returns the page it ended on.
pub fn run(page: Page) -> Option<Kind> {
    unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        let icon = |metric| {
            let n = GetSystemMetrics(metric);
            LoadImageW(module, PCWSTR(1 as _), IMAGE_ICON, n, n, LR_DEFAULTCOLOR).map(|h| HICON(h.0)).unwrap_or_default()
        };
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            hCursor: LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default(),
            hIcon: icon(SM_CXICON),
            hIconSm: icon(SM_CXSMICON),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);

        // Centred on the monitor under the mouse, at that monitor's scale.
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
        let (mut dpi, mut unused) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let dark = apps_dark();
        let mut d = Dialog::new(page, dark, dpi)?;
        let (ow, oh) = outer_size(d.back.w, d.back.h, dpi);
        let work = info.rcWork;
        let (x, y) = ((work.left + work.right - ow) / 2, (work.top + work.bottom - oh) / 2);
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("Onion Peel"), STYLE, x, y, ow, oh, HWND::default(), HMENU::default(), module, None).ok()?;
        // The title bar in the same colour as the page (Windows 11), dark in dark mode.
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &BOOL::from(dark) as *const BOOL as _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &d.colors().bg as *const COLORREF as _, 4);
        d.hwnd = hwnd;
        STATE.with(|s| *s.borrow_mut() = Some(d));
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let mut m = MSG::default();
        while GetMessageW(&mut m, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
        STATE.with(|s| s.borrow_mut().take()).map(|d| d.page.kind)
    }
}

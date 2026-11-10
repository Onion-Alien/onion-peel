//! The snip screen: the frozen picture, dimmed, with a Snipping-Tool-style bar on top.
//! Rectangle (drag), Window (click a window) and Full screen (click a monitor) modes.
//! Everything here is in picture coordinates, which are also the window's client coordinates.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Once;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::capture::{self, Shot};
use crate::dib::{rgb, wide, Dib};
use crate::hook::held;
use crate::{clip, main_hwnd};

/// The snip screen is up (the keyboard hook reads this from its own thread).
pub static OPEN: AtomicBool = AtomicBool::new(false);
const CLASS: PCWSTR = w!("OnionPeelOverlay");
/// Ticks every 50 ms while another window is in front (see `on_tick`).
const AWAY_TIMER: usize = 1;
/// From the mouse hook while the Alt+Tab switcher is up: a drag started at (x, y) on screen.
const WM_SWITCHER_DRAG: u32 = WM_APP + 0x40;
/// From the mouse hook: a click with no drag, to hand on to the switcher.
const WM_SWITCHER_CLICK: u32 = WM_APP + 0x41;
/// Marks the clicks we hand on ourselves.
const OURS: usize = 0x4f50;

thread_local! {
    static STATE: RefCell<Option<Overlay>> = const { RefCell::new(None) };
    /// Opens in the mode used last time, like Snipping Tool.
    static LAST_MODE: Cell<Mode> = const { Cell::new(Mode::Rect) };
    /// The mouse hook, only while the snip screen is open and the Alt+Tab switcher is in front.
    static MOUSE: Cell<Option<(HHOOK, HWND)>> = const { Cell::new(None) };
    /// Where the left button went down over the switcher (swallowed until we know what it is).
    static PRESS: Cell<Option<POINT>> = const { Cell::new(None) };
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Rect,
    Window,
    Screen,
}

/// The bar's buttons, left to right; the one without a mode is the close (X) button.
const BUTTONS: [(Option<Mode>, &str); 4] =
    [(Some(Mode::Rect), "Rectangle"), (Some(Mode::Window), "Window"), (Some(Mode::Screen), "Full screen"), (None, "")];

const BAR_BG: COLORREF = rgb(0x2b, 0x2b, 0x2b);
const BAR_EDGE: COLORREF = rgb(0x4a, 0x4a, 0x4a);
const BTN_HOVER: COLORREF = rgb(0x3a, 0x3a, 0x3a);
const BTN_ON: COLORREF = rgb(0x5c, 0x5c, 0x5c);
const WHITE: COLORREF = rgb(0xff, 0xff, 0xff);
/// Labels of the modes not chosen, so the chosen one stands out.
const DIM: COLORREF = rgb(0xa0, 0xa0, 0xa0);
const BLACK: COLORREF = rgb(0, 0, 0);

// ---- rectangle helpers (right and bottom are exclusive, like Windows) ----

/// The box spanned by two corner pixels.
fn span(a: (i32, i32), b: (i32, i32)) -> RECT {
    RECT { left: a.0.min(b.0), top: a.1.min(b.1), right: a.0.max(b.0) + 1, bottom: a.1.max(b.1) + 1 }
}

fn inflate(r: RECT, n: i32) -> RECT {
    RECT { left: r.left - n, top: r.top - n, right: r.right + n, bottom: r.bottom + n }
}

fn intersect(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) }
}

/// Smallest box around both; an empty box counts as nothing.
fn union(a: RECT, b: RECT) -> RECT {
    let empty = |r: RECT| r.right <= r.left || r.bottom <= r.top;
    match (empty(a), empty(b)) {
        (true, _) => b,
        (_, true) => a,
        _ => RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) },
    }
}

fn contains(r: &RECT, p: (i32, i32)) -> bool {
    p.0 >= r.left && p.0 < r.right && p.1 >= r.top && p.1 < r.bottom
}

/// The same spot of `r` in `b` as it has in `a`, scaled.
fn map_rect(r: RECT, a: RECT, b: RECT) -> RECT {
    let f = |v: i32, a0: i32, a1: i32, b0: i32, b1: i32| b0 + ((v - a0) as i64 * (b1 - b0) as i64 / (a1 - a0).max(1) as i64) as i32;
    RECT {
        left: f(r.left, a.left, a.right, b.left, b.right),
        top: f(r.top, a.top, a.bottom, b.top, b.bottom),
        right: f(r.right, a.left, a.right, b.left, b.right),
        bottom: f(r.bottom, a.top, a.bottom, b.top, b.bottom),
    }
}

fn blit(dst: HDC, src: HDC, r: RECT) {
    unsafe {
        let _ = BitBlt(dst, r.left, r.top, r.right - r.left, r.bottom - r.top, src, r.left, r.top, SRCCOPY);
    }
}

pub struct Overlay {
    hwnd: HWND,
    /// `shot.img` is the bright picture shown inside the selection.
    pub shot: Shot,
    dim: Dib,
    /// Each paint is drawn here first and then copied to the window, so nothing flickers.
    pub back: Dib,
    font: HFONT,
    pub mode: Mode,
    /// Where a rectangle drag started.
    pub drag: Option<(i32, i32)>,
    pub cur: (i32, i32),
    /// Visible windows, top of the z-order first.
    windows: Vec<RECT>,
    scale: f32,
    pub bar: RECT,
    pub buttons: [RECT; 4],
    /// The left button went down on this bar button.
    press_bar: Option<usize>,
    /// The left button went down outside the bar (Window / Full screen modes).
    press_pick: bool,
    /// The area the last paint drew on, so the next one also wipes it.
    last_dirty: RECT,
    /// While another window is in front: the one seen last tick, and for how many ticks.
    away: (HWND, u32),
    /// When `shot` is stretched to the screen: the picture at the game's own resolution, and
    /// each monitor's rectangle in `shot` and in it. Snips copy from it.
    original: Option<(Shot, Vec<(RECT, RECT)>)>,
    /// Where the windows and the cursor were on screen when the picture was taken.
    taken: Option<(Vec<RECT>, POINT)>,
}

/// What the window should do after an input event.
enum Act {
    Nothing,
    Snip(RECT),
    Cancel,
    /// Take a new picture: another window came to the front.
    Retake,
    /// The screen changed size: fit the picture to it again.
    Refit,
    /// A drag started over the Alt+Tab switcher, here on screen.
    SwitcherDrag(POINT),
}

impl Overlay {
    /// `windows` and `cursor` are in screen coordinates.
    pub fn new(shot: Shot, windows: Vec<RECT>, cursor: POINT, mode: Mode) -> Option<Overlay> {
        let (w, h) = (shot.img.w, shot.img.h);
        let mut dim = Dib::new(w, h)?;
        for (d, s) in dim.pixels_mut().iter_mut().zip(shot.img.pixels()) {
            *d = s >> 1;
        }
        let back = Dib::new(w, h)?;
        let scale = unsafe {
            let (mut dx, mut dy) = (96, 96);
            let _ = GetDpiForMonitor(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            dx.max(96) as f32 / 96.0
        };
        let font = unsafe {
            CreateFontW(
                -((14.0 * scale).round() as i32),
                0, 0, 0,
                FW_NORMAL.0 as i32,
                0, 0, 0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
                w!("Segoe UI"),
            )
        };
        unsafe {
            // The back buffer only ever draws with these, so set them up once.
            SelectObject(back.dc, font);
            SelectObject(back.dc, GetStockObject(DC_BRUSH));
            SelectObject(back.dc, GetStockObject(DC_PEN));
            SetBkMode(back.dc, TRANSPARENT);
        }
        let (ox, oy) = (shot.x, shot.y);
        let mut o = Overlay {
            hwnd: HWND::default(),
            shot,
            dim,
            back,
            font,
            mode,
            drag: None,
            cur: (cursor.x - ox, cursor.y - oy),
            windows: windows.into_iter().map(|r| RECT { left: r.left - ox, top: r.top - oy, right: r.right - ox, bottom: r.bottom - oy }).collect(),
            scale,
            bar: RECT::default(),
            buttons: [RECT::default(); 4],
            press_bar: None,
            press_pick: false,
            last_dirty: RECT::default(),
            away: (HWND::default(), 0),
            original: None,
            taken: None,
        };
        o.layout_bar();
        o.last_dirty = o.dirty();
        Some(o)
    }

    /// Design pixels to real pixels on the cursor's monitor.
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    fn monitor_at(&self, p: (i32, i32)) -> RECT {
        let m = &self.shot.monitors;
        *m.iter().find(|m| contains(m, p)).unwrap_or(&m[0])
    }

    fn text_width(&self, s: &str) -> i32 {
        let mut size = SIZE::default();
        if !s.is_empty() {
            unsafe {
                let _ = GetTextExtentPoint32W(self.back.dc, &wide(s), &mut size);
            }
        }
        size.cx
    }

    /// The bar sits top-centre on the cursor's monitor.
    fn layout_bar(&mut self) {
        let mon = self.monitor_at(self.cur);
        let (pad, gap, bh, hpad) = (self.px(6.0), self.px(4.0), self.px(32.0), self.px(14.0));
        let widths = BUTTONS.map(|(mode, label)| if mode.is_some() { self.text_width(label) + hpad * 2 } else { bh });
        let total = widths.iter().sum::<i32>() + gap * (BUTTONS.len() as i32 - 1) + pad * 2;
        let left = mon.left + (mon.right - mon.left - total) / 2;
        let top = mon.top + self.px(16.0);
        self.bar = RECT { left, top, right: left + total, bottom: top + bh + pad * 2 };
        let mut x = left + pad;
        for (b, w) in self.buttons.iter_mut().zip(widths) {
            *b = RECT { left: x, top: top + pad, right: x + w, bottom: top + pad + bh };
            x += w + gap;
        }
    }

    fn button_at(&self, p: (i32, i32)) -> Option<usize> {
        self.buttons.iter().position(|b| contains(b, p))
    }

    fn over_bar(&self) -> bool {
        contains(&self.bar, self.cur) && self.drag.is_none()
    }

    /// The area that would be snipped right now.
    pub fn highlight(&self) -> Option<RECT> {
        if self.over_bar() {
            return None;
        }
        let at_cursor = |r: &&RECT| contains(r, self.cur);
        match self.mode {
            Mode::Rect => self.drag.map(|d| span(d, self.cur)),
            Mode::Window => self.windows.iter().find(at_cursor).map(|&w| intersect(w, self.shot.img.rect())),
            Mode::Screen => self.shot.monitors.iter().find(at_cursor).copied(),
        }
    }

    /// The white part of the selection frame; a 1-pixel black line runs outside it.
    fn frame_width(&self) -> i32 {
        self.px(1.0).max(1)
    }

    /// The picture and the part of it a snip of `r` copies: the game's own pixels when the
    /// picture on screen is stretched.
    fn source(&self, r: RECT) -> (&Dib, RECT) {
        let r = intersect(r, self.shot.img.rect());
        match &self.original {
            None => (&self.shot.img, r),
            Some((orig, pairs)) => {
                let mid = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                let (on_screen, own) = pairs.iter().find(|(n, _)| contains(n, mid)).unwrap_or(&pairs[0]);
                (&orig.img, map_rect(intersect(r, *on_screen), *on_screen, *own))
            }
        }
    }

    /// The "W × H" label: below the selection, or tucked inside it when there's no room.
    fn label(&self, r: RECT) -> (RECT, String) {
        let s = self.source(r).1;
        let text = format!("{} \u{00d7} {}", s.right - s.left, s.bottom - s.top);
        let (pad, h, gap) = (self.px(8.0), self.px(24.0), self.px(8.0));
        let w = self.text_width(&text) + pad * 2;
        let mon = self.monitor_at((r.left.max(0), r.bottom.min(self.shot.img.h - 1)));
        let (mut left, mut top) = (r.left, r.bottom + gap);
        if top + h > mon.bottom {
            // Inside the bottom-left corner, clear of the border.
            let inset = gap + self.frame_width();
            top = (r.bottom - h - inset).max(r.top + inset);
            left = r.left + inset;
        }
        let left = left.clamp(mon.left + gap, (mon.right - w - gap).max(mon.left + gap));
        (RECT { left, top, right: left + w, bottom: top + h }, text)
    }

    /// Everything the current state draws on top of the dimmed picture.
    fn dirty(&self) -> RECT {
        let mut d = inflate(self.bar, 2);
        if let Some(r) = self.highlight() {
            d = union(d, inflate(r, self.frame_width() + 2));
            d = union(d, inflate(self.label(r).0, 2));
        }
        d
    }

    /// Repaint what changed: what was drawn last time plus what's drawn now.
    fn refresh(&mut self) {
        let now = self.dirty();
        let area = union(self.last_dirty, now);
        self.last_dirty = now;
        if !self.hwnd.is_invalid() {
            unsafe {
                let _ = InvalidateRect(self.hwnd, Some(&area), false);
            }
        }
    }

    // ---- drawing, all into the back buffer ----

    /// Draw everything inside `clip`.
    pub fn paint(&self, clip: RECT) {
        let dc = self.back.dc;
        blit(dc, self.dim.dc, clip);
        unsafe {
            IntersectClipRect(dc, clip.left, clip.top, clip.right, clip.bottom);
        }
        if let Some(r) = self.highlight() {
            blit(dc, self.shot.img.dc, r);
            // A white frame with a thin black outline. At a monitor edge it moves inside the
            // selection, so it stays on screen.
            let inside = |m: &&RECT| r.left >= m.left && r.top >= m.top && r.right <= m.right && r.bottom <= m.bottom;
            let bounds = self.shot.monitors.iter().find(inside).copied().unwrap_or(self.shot.img.rect());
            let mut f = intersect(inflate(r, self.frame_width() + 1), bounds);
            self.outline(f, BLACK);
            for _ in 0..self.frame_width() {
                f = inflate(f, -1);
                self.outline(f, WHITE);
            }
            let (lr, text) = self.label(r);
            self.round_box(lr, BAR_BG, BAR_EDGE, self.px(6.0));
            self.text(lr, &text, WHITE);
        }
        self.paint_bar();
        unsafe {
            SelectClipRgn(dc, HRGN::default());
        }
    }

    fn paint_bar(&self) {
        self.round_box(self.bar, BAR_BG, BAR_EDGE, self.px(8.0));
        let hover = if self.drag.is_none() { self.button_at(self.cur) } else { None };
        for (i, (&b, (mode, label))) in self.buttons.iter().zip(BUTTONS).enumerate() {
            let (on, hovered) = (mode == Some(self.mode), hover == Some(i));
            if on {
                self.round_box(b, BTN_ON, BTN_ON, self.px(4.0));
            } else if hovered {
                self.round_box(b, BTN_HOVER, BTN_HOVER, self.px(4.0));
            }
            let color = if on || hovered { WHITE } else { DIM };
            match mode {
                Some(_) => self.text(b, label, color),
                None => self.draw_x(b, color),
            }
        }
    }

    fn outline(&self, r: RECT, c: COLORREF) {
        unsafe {
            SetDCBrushColor(self.back.dc, c);
            FrameRect(self.back.dc, &r, HBRUSH(GetStockObject(DC_BRUSH).0));
        }
    }

    fn round_box(&self, r: RECT, fill: COLORREF, edge: COLORREF, radius: i32) {
        self.back.round_box(r, fill, edge, radius);
    }

    fn text(&self, r: RECT, s: &str, color: COLORREF) {
        unsafe {
            SetTextColor(self.back.dc, color);
        }
        self.back.text(r, s, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }

    fn draw_x(&self, b: RECT, color: COLORREF) {
        let dc = self.back.dc;
        let s = self.px(5.0);
        let (cx, cy) = ((b.left + b.right) / 2, (b.top + b.bottom) / 2);
        unsafe {
            let pen = CreatePen(PS_SOLID, self.px(1.5).max(1), color);
            let old = SelectObject(dc, pen);
            let _ = MoveToEx(dc, cx - s, cy - s, None);
            let _ = LineTo(dc, cx + s + 1, cy + s + 1);
            let _ = MoveToEx(dc, cx + s, cy - s, None);
            let _ = LineTo(dc, cx - s - 1, cy + s + 1);
            SelectObject(dc, old);
            let _ = DeleteObject(pen);
        }
    }

    // ---- input ----

    fn on_move(&mut self, p: (i32, i32)) {
        self.cur = p;
        self.refresh();
    }

    fn on_down(&mut self, p: (i32, i32)) {
        self.cur = p;
        self.press_bar = self.button_at(p);
        self.press_pick = false;
        if contains(&self.bar, p) {
            return;
        }
        match self.mode {
            Mode::Rect => {
                self.drag = Some(p);
                unsafe {
                    SetCapture(self.hwnd);
                }
            }
            _ => self.press_pick = true,
        }
        self.refresh();
    }

    fn on_up(&mut self, p: (i32, i32)) -> Act {
        self.cur = p;
        if let Some(i) = self.press_bar.take() {
            if self.button_at(p) == Some(i) {
                match BUTTONS[i].0 {
                    Some(m) => self.mode = m,
                    None => return Act::Cancel,
                }
            }
        } else if let Some(d) = self.drag.take() {
            unsafe {
                let _ = ReleaseCapture();
            }
            let r = span(d, p);
            if r.right - r.left >= 3 && r.bottom - r.top >= 3 {
                return Act::Snip(r);
            }
        } else if std::mem::take(&mut self.press_pick) {
            return self.highlight().map_or(Act::Nothing, Act::Snip);
        } else {
            return Act::Nothing;
        }
        self.refresh();
        Act::Nothing
    }

    /// Right-click drops a drag in progress, otherwise closes.
    fn on_right(&mut self) -> Act {
        if self.drag.take().is_none() {
            return Act::Cancel;
        }
        unsafe {
            let _ = ReleaseCapture();
        }
        self.refresh();
        Act::Nothing
    }

    /// Another window is in front (Alt+Tab, say, to a VM that keeps Win+Shift+S for itself).
    /// Once Alt is up and the same window has stayed in front for about 150 ms, so the switch
    /// is over and it has drawn itself, take a new picture of it.
    fn on_tick(&mut self) -> Act {
        let fg = unsafe { GetForegroundWindow() };
        // The switcher takes every click while it's up, so watch the mouse ourselves meanwhile.
        mouse_hook((held(VK_MENU) && is_switcher(fg)).then_some(self.hwnd));
        if held(VK_MENU) || fg == self.hwnd || fg != self.away.0 {
            self.away = (fg, 0);
            return Act::Nothing;
        }
        self.away.1 += 1;
        if self.away.1 < 3 {
            return Act::Nothing;
        }
        unsafe {
            let _ = KillTimer(self.hwnd, AWAY_TIMER);
        }
        Act::Retake
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            // A font still selected into a DC can't be deleted.
            SelectObject(self.back.dc, GetStockObject(SYSTEM_FONT));
            let _ = DeleteObject(self.font);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut act = Act::Nothing;
    let handled = STATE.with(|s| {
        // Already borrowed: Windows is calling back from inside one of our own calls.
        let mut s = s.try_borrow_mut().ok()?;
        let o = s.as_mut()?;
        let p = (lp.0 as i16 as i32, (lp.0 >> 16) as i16 as i32);
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                o.paint(ps.rcPaint);
                blit(dc, o.back.dc, ps.rcPaint);
                let _ = EndPaint(hwnd, &ps);
            }
            WM_ERASEBKGND => return Some(LRESULT(1)),
            WM_SETCURSOR => {
                let cursor = if o.over_bar() { IDC_ARROW } else { IDC_CROSS };
                SetCursor(LoadCursorW(HINSTANCE::default(), cursor).unwrap_or_default());
                return Some(LRESULT(1));
            }
            WM_MOUSEMOVE => o.on_move(p),
            WM_LBUTTONDOWN => o.on_down(p),
            WM_LBUTTONUP => act = o.on_up(p),
            WM_RBUTTONUP => act = o.on_right(),
            WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => act = Act::Cancel,
            WM_ACTIVATE => {
                if wp.0 & 0xffff == WA_INACTIVE as usize {
                    o.away = (HWND::default(), 0);
                    SetTimer(hwnd, AWAY_TIMER, 50, None);
                } else {
                    let _ = KillTimer(hwnd, AWAY_TIMER);
                    mouse_hook(None);
                }
                return None;
            }
            WM_TIMER if wp.0 == AWAY_TIMER => act = o.on_tick(),
            WM_DISPLAYCHANGE => act = Act::Refit,
            WM_SWITCHER_DRAG => act = Act::SwitcherDrag(POINT { x: wp.0 as i32, y: lp.0 as i32 }),
            WM_SWITCHER_CLICK => replay_click(),
            _ => return None,
        }
        Some(LRESULT(0))
    });
    // These replace or destroy the state, so they run only after it is no longer borrowed.
    match act {
        Act::Snip(r) => close(Some(r)),
        Act::Cancel => close(None),
        Act::Retake => retake(),
        Act::Refit => refit(),
        Act::SwitcherDrag(p) => switcher_drag(p),
        Act::Nothing => {}
    }
    handled.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wp, lp))
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
unsafe fn pickable(h: HWND) -> Option<RECT> {
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
    let mut class = [0u16; 64];
    let n = GetClassNameW(h, &mut class) as usize;
    match String::from_utf16_lossy(&class[..n]).as_str() {
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

unsafe fn dwm<T: Default>(h: HWND, attr: DWMWINDOWATTRIBUTE) -> Option<T> {
    let mut v = T::default();
    DwmGetWindowAttribute(h, attr, &mut v as *mut T as _, size_of::<T>() as u32).ok().map(|_| v)
}

/// Switch to the bar's mode number `i`, as if its button were clicked.
pub fn set_mode(i: usize) {
    let Some(mode) = BUTTONS.get(i).and_then(|b| b.0) else { return };
    STATE.with(|s| {
        if let Some(o) = s.try_borrow_mut().ok().as_mut().and_then(|s| s.as_mut()) {
            o.mode = mode;
            if o.drag.take().is_some() {
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            o.refresh();
        }
    });
}

/// A picture taken over a game in exclusive fullscreen, waiting for the game to leave the front.
struct Pending {
    shot: Shot,
    windows: Vec<RECT>,
    cursor: POINT,
    /// The window in front when it was taken: the game.
    game: HWND,
}

thread_local! {
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}
/// Ticks every 250 ms while a picture is waiting (see `pending_tick`).
const PENDING_TIMER: usize = 2;

/// A game is in exclusive fullscreen. Windows drops such a game to the desktop the moment any
/// window is drawn over it (Snipping Tool's too), so then nothing is shown yet.
fn game_in_front() -> bool {
    matches!(unsafe { SHQueryUserNotificationState() }, Ok(QUNS_RUNNING_D3D_FULL_SCREEN))
}

fn grab_now() -> Option<Pending> {
    // Freeze first, so the snip screen itself never ends up in the picture.
    let shot = capture::grab(true)?;
    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    Some(Pending { shot, windows: visible_windows(), cursor, game: unsafe { GetForegroundWindow() } })
}

/// Freeze the screen and put the snip screen over every monitor. Over a game in exclusive
/// fullscreen, only take the picture: the snip screen opens with it once the game is no longer
/// in front (Alt+Tab, or the game closes). Pressing again before then takes a newer picture.
pub fn open() {
    if OPEN.load(Relaxed) {
        return;
    }
    if game_in_front() {
        if let Some(p) = grab_now() {
            PENDING.with(|s| *s.borrow_mut() = Some(p));
            unsafe {
                SetTimer(main_hwnd(), PENDING_TIMER, 250, Some(pending_tick));
            }
        }
        return;
    }
    PENDING.with(|s| s.borrow_mut().take());
    if let Some(p) = grab_now() {
        OPEN.store(true, Relaxed);
        if show(p).is_none() {
            OPEN.store(false, Relaxed);
        }
    }
}

unsafe extern "system" fn pending_tick(hwnd: HWND, _: u32, id: usize, _: u32) {
    // The game has left the front once it is no longer fullscreen, or once another window has
    // focus and Alt+Tab is let go (a DX12 game can stay "fullscreen" for seconds while minimized).
    let game = PENDING.with(|s| s.borrow().as_ref().map(|p| p.game)).unwrap_or_default();
    let fg = GetForegroundWindow();
    let pid = |h| {
        let mut p = 0;
        GetWindowThreadProcessId(h, Some(&mut p));
        p
    };
    // Another window of the game itself doesn't count.
    let switched = !fg.is_invalid() && fg != game && pid(fg) != pid(game) && !held(VK_MENU);
    if game_in_front() && !switched {
        return;
    }
    let _ = KillTimer(hwnd, id);
    let Some(p) = PENDING.with(|s| s.borrow_mut().take()) else { return };
    if !OPEN.swap(true, Relaxed) && show(p).is_none() {
        OPEN.store(false, Relaxed);
    }
}

/// The snip screen for a picture. When the monitors changed size since it was taken (a game in
/// exclusive fullscreen runs at its own resolution), the picture is shown stretched to the screen
/// as it is now, and snips copy from the game's own pixels.
fn build(shot: Shot, windows: Vec<RECT>, cursor: POINT, mode: Mode) -> Option<Overlay> {
    let taken = (windows.clone(), cursor);
    let (shot, windows, cursor, original) = match shot.fit_to_screen() {
        Some((fit, pairs)) => {
            let to_screen = |r: RECT| {
                let r = RECT { left: r.left - shot.x, top: r.top - shot.y, right: r.right - shot.x, bottom: r.bottom - shot.y };
                let mid = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                let (on_screen, own) = pairs.iter().find(|(_, o)| contains(o, mid)).unwrap_or(&pairs[0]);
                let n = map_rect(r, *own, *on_screen);
                RECT { left: n.left + fit.x, top: n.top + fit.y, right: n.right + fit.x, bottom: n.bottom + fit.y }
            };
            let windows = windows.into_iter().map(to_screen).collect();
            let c = to_screen(RECT { left: cursor.x, top: cursor.y, right: cursor.x, bottom: cursor.y });
            (fit, windows, POINT { x: c.left, y: c.top }, Some((shot, pairs)))
        }
        None => (shot, windows, cursor, None),
    };
    let mut o = Overlay::new(shot, windows, cursor, mode)?;
    o.original = original;
    o.taken = Some(taken);
    Some(o)
}

fn show(p: Pending) -> Option<()> {
    let Pending { shot, windows, cursor, .. } = p;
    let mut o = build(shot, windows, cursor, LAST_MODE.get())?;
    let (x, y, w, h) = (o.shot.x, o.shot.y, o.shot.img.w, o.shot.img.h);
    static REGISTER: Once = Once::new();
    unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        REGISTER.call_once(|| {
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: module.into(), lpszClassName: CLASS, ..Default::default() });
        });
        let style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
        o.hwnd = CreateWindowExW(style, CLASS, w!("Snip"), WS_POPUP, x, y, w, h, HWND::default(), HMENU::default(), module, None).ok()?;
        let hwnd = o.hwnd;
        // Left out of every screen capture, ours included, so a retake sees what's behind it.
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        STATE.with(|s| *s.borrow_mut() = Some(o));
        let _ = ShowWindow(hwnd, SW_SHOW);
        force_foreground(hwnd);
        let _ = UpdateWindow(hwnd);
    }
    Some(())
}

/// Take a new picture behind the open snip screen, keeping its mode.
fn retake() {
    let Some((hwnd, mode)) = STATE.with(|s| s.borrow().as_ref().map(|o| (o.hwnd, o.mode))) else { return };
    unsafe {
        // Windows older than 10 (2004) can't leave it out of captures: hide it meanwhile.
        let mut affinity = 0;
        let hide = GetWindowDisplayAffinity(hwnd, &mut affinity).is_err() || affinity != WDA_EXCLUDEFROMCAPTURE.0;
        if hide {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let o = capture::grab(true).and_then(|shot| {
            let o = build(shot, visible_windows(), cursor, mode)?;
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, o.shot.x, o.shot.y, o.shot.img.w, o.shot.img.h, SWP_NOACTIVATE);
            Some(o)
        });
        if let Some(mut o) = o {
            o.hwnd = hwnd;
            STATE.with(|s| *s.borrow_mut() = Some(o));
            let _ = InvalidateRect(hwnd, None, false);
        }
        if hide {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        force_foreground(hwnd);
    }
}

/// The screen changed size with the snip screen up (a game switching back from its own
/// resolution, say): fit the same picture to it again.
fn refit() {
    let Some(mut o) = STATE.with(|s| s.borrow_mut().take()) else { return };
    let blank = Dib::new(1, 1).map(|img| Shot { x: 0, y: 0, img, monitors: vec![RECT::default()], how: vec![] });
    let (Some((windows, cursor)), Some(blank)) = (o.taken.take(), blank) else {
        STATE.with(|s| *s.borrow_mut() = Some(o));
        return;
    };
    let shot = match o.original.take() {
        Some((own, _)) => own,
        None => std::mem::replace(&mut o.shot, blank),
    };
    let (hwnd, mode) = (o.hwnd, o.mode);
    if o.drag.is_some() {
        unsafe {
            let _ = ReleaseCapture();
        }
    }
    drop(o);
    unsafe {
        match build(shot, windows, cursor, mode) {
            Some(mut n) => {
                n.hwnd = hwnd;
                let (x, y, w, h) = (n.shot.x, n.shot.y, n.shot.img.w, n.shot.img.h);
                STATE.with(|s| *s.borrow_mut() = Some(n));
                let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
                let _ = InvalidateRect(hwnd, None, false);
            }
            None => {
                let _ = DestroyWindow(hwnd);
                OPEN.store(false, Relaxed);
            }
        }
    }
}

/// Windows only lets the active app hand out focus, so borrow its input queue for a moment.
unsafe fn force_foreground(hwnd: HWND) {
    let fg = GetWindowThreadProcessId(GetForegroundWindow(), None);
    let me = GetCurrentThreadId();
    let attached = fg != 0 && fg != me && AttachThreadInput(me, fg, true).as_bool();
    let _ = SetForegroundWindow(hwnd);
    let _ = BringWindowToTop(hwnd);
    let _ = SetFocus(hwnd);
    if attached {
        let _ = AttachThreadInput(me, fg, false);
    }
}

/// Close the snip screen; with a rectangle, copy that part to the clipboard first.
pub fn close(snip: Option<RECT>) {
    mouse_hook(None);
    let Some(o) = STATE.with(|s| s.borrow_mut().take()) else { return };
    unsafe {
        let _ = DestroyWindow(o.hwnd);
    }
    LAST_MODE.set(o.mode);
    if let Some(r) = snip {
        let (img, part) = o.source(r);
        clip::copy(main_hwnd(), img, part);
    }
    drop(o);
    OPEN.store(false, Relaxed);
}

// ---- snipping the Alt+Tab switcher ----
// With the snip screen open, Alt+Tab lets you change what's on screen. The switcher itself covers
// the whole screen and takes every click while Alt is held, so to snip it, a mouse hook watches
// for a drag over it: the picture is retaken with the switcher in it, the switcher is closed and
// the drag carries on in the snip screen. A plain click still goes to the switcher.

/// The Alt+Tab switcher (Windows 11, then 10).
fn is_switcher(h: HWND) -> bool {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut class) } as usize;
    matches!(String::from_utf16_lossy(&class[..n]).as_str(), "XamlExplorerHostIslandWindow" | "MultitaskingViewFrame" | "TaskSwitcherWnd")
}

/// Watch the mouse for `overlay`, or stop watching.
fn mouse_hook(overlay: Option<HWND>) {
    match (MOUSE.get(), overlay) {
        (None, Some(hwnd)) => unsafe {
            PRESS.set(None);
            if let Ok(h) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), HINSTANCE::default(), 0) {
                MOUSE.set(Some((h, hwnd)));
            }
        },
        (Some((h, _)), None) => unsafe {
            let _ = UnhookWindowsHookEx(h);
            MOUSE.set(None);
            // A press we swallowed and never handed on: hand it on now.
            if PRESS.take().is_some() {
                replay_click();
            }
        },
        _ => {}
    }
}

/// Runs on this thread (from inside its message loop), so it only posts to the snip screen.
unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let m = &*(lp.0 as *const MSLLHOOKSTRUCT);
    if code == HC_ACTION as i32 && m.flags & LLMHF_INJECTED == 0 {
        if let Some((_, hwnd)) = MOUSE.get() {
            match wp.0 as u32 {
                WM_LBUTTONDOWN => {
                    PRESS.set(Some(m.pt));
                    return LRESULT(1);
                }
                WM_MOUSEMOVE => {
                    if let Some(p) = PRESS.get() {
                        if (m.pt.x - p.x).abs() > 4 || (m.pt.y - p.y).abs() > 4 {
                            PRESS.set(None);
                            let _ = PostMessageW(hwnd, WM_SWITCHER_DRAG, WPARAM(p.x as isize as usize), LPARAM(p.y as isize));
                        }
                    }
                }
                WM_LBUTTONUP if PRESS.take().is_some() => {
                    let _ = PostMessageW(hwnd, WM_SWITCHER_CLICK, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
                _ => {}
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wp, lp)
}

/// Hand a swallowed click on to whatever is under the mouse (the switcher).
fn replay_click() {
    let button = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: flags, dwExtraInfo: OURS, ..Default::default() } },
    };
    unsafe {
        SendInput(&[button(MOUSEEVENTF_LEFTDOWN), button(MOUSEEVENTF_LEFTUP)], size_of::<INPUT>() as i32);
    }
}

/// A drag started over the switcher at `start`: picture it, close it, carry on dragging.
fn switcher_drag(start: POINT) {
    mouse_hook(None);
    let Some((hwnd, mode)) = STATE.with(|s| s.borrow().as_ref().map(|o| (o.hwnd, o.mode))) else { return };
    unsafe {
        let switcher = GetForegroundWindow();
        // Windows older than 10 (2004) can't leave the snip screen out of captures: hide it meanwhile.
        let mut affinity = 0;
        let hide = GetWindowDisplayAffinity(hwnd, &mut affinity).is_err() || affinity != WDA_EXCLUDEFROMCAPTURE.0;
        if hide {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        let shot = capture::grab(true);
        let mut windows = visible_windows();
        // The switcher's own window is see-through and covers the whole screen: not one to pick.
        if let Some(r) = pickable(switcher) {
            windows.retain(|w| *w != r);
        }
        // Esc closes the switcher without switching, so the snip screen comes back to the front.
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_ESCAPE, dwFlags: flags, dwExtraInfo: OURS, ..Default::default() } },
        };
        SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], size_of::<INPUT>() as i32);
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        if let Some(mut o) = shot.and_then(|shot| build(shot, windows, cursor, mode)) {
            o.hwnd = hwnd;
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, o.shot.x, o.shot.y, o.shot.img.w, o.shot.img.h, SWP_NOACTIVATE);
            STATE.with(|s| *s.borrow_mut() = Some(o));
            let _ = InvalidateRect(hwnd, None, false);
        }
        if hide {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        force_foreground(hwnd);
        STATE.with(|s| {
            if let Some(o) = s.borrow_mut().as_mut() {
                let (x, y) = (o.shot.x, o.shot.y);
                o.on_down((start.x - x, start.y - y));
                o.on_move((cursor.x - x, cursor.y - y));
            }
        });
    }
}

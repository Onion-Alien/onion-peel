//! A 32-bit top-down bitmap with its own memory DC: GDI can draw on it and we can touch
//! its pixels directly, with no copying between the two.

use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::*;

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub struct Dib {
    pub dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub w: i32,
    pub h: i32,
}

/// Header of an uncompressed DIB; a negative `h` means the top row comes first.
pub fn header(w: i32, h: i32, bits: u16) -> BITMAPINFOHEADER {
    BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: h,
        biPlanes: 1,
        biBitCount: bits,
        biCompression: BI_RGB.0,
        ..Default::default()
    }
}

impl Dib {
    pub fn new(w: i32, h: i32) -> Option<Dib> {
        unsafe {
            let dc = CreateCompatibleDC(HDC::default());
            let info = BITMAPINFO { bmiHeader: header(w, -h, 32), ..Default::default() };
            let mut bits = std::ptr::null_mut();
            match CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(bmp) if !bits.is_null() => Some(Dib { dc, old: SelectObject(dc, bmp), bmp, bits: bits.cast(), w, h }),
                _ => {
                    let _ = DeleteDC(dc);
                    None
                }
            }
        }
    }

    /// All pixels as BGRA bytes, top row first.
    pub fn pixels(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.bits, (self.w * self.h * 4) as usize) }
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h * 4) as usize) }
    }

    /// `n` pixels starting at (x, y).
    pub fn row(&self, x: i32, y: i32, n: i32) -> &[u8] {
        let start = ((y * self.w + x) * 4) as usize;
        &self.pixels()[start..start + n as usize * 4]
    }

    pub fn rect(&self) -> RECT {
        RECT { left: 0, top: 0, right: self.w, bottom: self.h }
    }

    /// A filled rounded box with a 1-pixel edge. Needs DC_BRUSH and DC_PEN selected.
    pub fn round_box(&self, r: RECT, fill: COLORREF, edge: COLORREF, radius: i32) {
        unsafe {
            SetDCBrushColor(self.dc, fill);
            SetDCPenColor(self.dc, edge);
            let _ = RoundRect(self.dc, r.left, r.top, r.right, r.bottom, radius * 2, radius * 2);
        }
    }

    /// Text in the selected font and colour; returns its height (with DT_CALCRECT, without drawing).
    pub fn text(&self, r: RECT, s: &str, flags: DRAW_TEXT_FORMAT) -> i32 {
        let mut r = r;
        unsafe { DrawTextW(self.dc, &mut wide(s), &mut r, flags | DT_NOPREFIX) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp);
            let _ = DeleteDC(self.dc);
        }
    }
}

//! Put part of the picture on the clipboard as a plain 24-bit CF_DIB (every app can paste it).

use std::time::Duration;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_DIB;

use crate::dib::{self, Dib};

/// Bytes per row of a 24-bit DIB (rows are padded to 4 bytes).
fn stride(w: i32) -> usize {
    (w as usize * 3 + 3) & !3
}

/// `r` of `img` as 24-bit rows, bottom row first (the layout every app understands).
pub fn pack(img: &Dib, r: RECT, out: &mut [u8]) {
    let w = r.right - r.left;
    for (row, y) in out.chunks_exact_mut(stride(w)).zip((r.top..r.bottom).rev()) {
        for (d, s) in row.chunks_exact_mut(3).zip(img.row(r.left, y, w).chunks_exact(4)) {
            d.copy_from_slice(&s[..3]);
        }
    }
}

pub fn copy(owner: HWND, img: &Dib, r: RECT) {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return;
    }
    let (head, body) = (size_of::<BITMAPINFOHEADER>(), stride(w) * h as usize);
    unsafe {
        let Ok(mem) = GlobalAlloc(GHND, head + body) else { return };
        let p = GlobalLock(mem) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(mem);
            return;
        }
        p.cast::<BITMAPINFOHEADER>().write_unaligned(BITMAPINFOHEADER { biSizeImage: body as u32, ..dib::header(w, h, 24) });
        pack(img, r, std::slice::from_raw_parts_mut(p.add(head), body));
        let _ = GlobalUnlock(mem);

        // Another app may hold the clipboard for a moment: retry briefly.
        let opened = (0..20).any(|i| {
            if i > 0 {
                std::thread::sleep(Duration::from_millis(25));
            }
            OpenClipboard(owner).is_ok()
        });
        let placed = opened && {
            let _ = EmptyClipboard();
            let ok = SetClipboardData(CF_DIB.0 as u32, HANDLE(mem.0)).is_ok();
            let _ = CloseClipboard();
            ok
        };
        if !placed {
            // The clipboard only takes ownership of the memory when SetClipboardData works.
            let _ = GlobalFree(mem);
        }
    }
}

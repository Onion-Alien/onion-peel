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
        for (d, s) in row.as_chunks_mut::<3>().0.iter_mut().zip(img.row(r.left, y, w).as_chunks::<4>().0.iter()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_padded_to_4_bytes() {
        assert_eq!([1, 2, 3, 4, 5, 100].map(stride), [4, 8, 12, 12, 16, 300]);
    }

    #[test]
    fn pack_is_bottom_up_bgr() {
        // A 4x3 picture where each pixel's bytes say where it is: B = x, G = y, R = 7, A = 255.
        let mut img = Dib::new(4, 3).unwrap();
        for (i, px) in img.pixels_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px.copy_from_slice(&[(i % 4) as u8, (i / 4) as u8, 7, 255]);
        }
        // The 3x2 box at (1, 1): rows y = 2 then y = 1, 3 bytes per pixel, padded to 12.
        let mut out = vec![0xAA; stride(3) * 2];
        pack(&img, RECT { left: 1, top: 1, right: 4, bottom: 3 }, &mut out);
        let want = |y: u8| [1, y, 7, 2, y, 7, 3, y, 7, 0xAA, 0xAA, 0xAA];
        assert_eq!(&out[..12], &want(2));
        assert_eq!(&out[12..], &want(1));
    }

    #[test]
    fn header_is_plain_rgb() {
        let h = dib::header(5, -3, 24);
        assert_eq!((h.biSize, h.biWidth, h.biHeight, h.biPlanes, h.biBitCount), (40, 5, -3, 1, 24));
        assert_eq!(h.biCompression, 0); // BI_RGB
    }
}

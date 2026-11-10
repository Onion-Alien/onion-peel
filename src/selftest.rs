//! `onion-peel --selftest <dir>`, only in builds made with `--features selftest`.
//!
//! Times a real capture (DXGI, then GDI for comparison), checks the clipboard packing, then
//! renders the snip screen in several states to BMP files. Opens no window and never touches
//! the clipboard. The renders use a fixed test pattern instead of the screen, so two builds
//! can be compared pixel for pixel; only capture.bmp holds what was really on screen.

use std::fmt::Write as _;
use std::time::Instant;
use windows::Win32::Foundation::{POINT, RECT};

use crate::capture;
use crate::clip;
use crate::dialog::{Dialog, Hit, Kind, Page};
use crate::overlay::{self, Mode, Overlay};

fn save_bmp(path: &str, w: i32, h: i32, bgra: &[u8]) {
    let size = 54 + bgra.len() as u32;
    let mut f = Vec::with_capacity(size as usize);
    f.extend_from_slice(b"BM");
    for v in [size, 0, 54, 40, w as u32, (-h) as u32] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f.extend_from_slice(&[1, 0, 32, 0]); // planes, bits per pixel
    f.extend_from_slice(&[0; 24]);
    f.extend_from_slice(bgra);
    std::fs::write(path, f).unwrap();
}

pub fn run(dir: &str) {
    let mut log = String::new();
    let t = Instant::now();
    let mut shot = capture::grab(true).expect("capture");
    writeln!(log, "capture: {} ms, virtual screen {}x{} at {},{}", t.elapsed().as_millis(), shot.img.w, shot.img.h, shot.x, shot.y).unwrap();
    for (m, how) in shot.monitors.iter().zip(&shot.how) {
        writeln!(log, "  monitor {:?} via {how}", (m.left + shot.x, m.top + shot.y, m.right - m.left, m.bottom - m.top)).unwrap();
    }
    let t = Instant::now();
    let again = capture::grab(true).expect("capture");
    writeln!(log, "second capture: {} ms, same size: {}", t.elapsed().as_millis(), again.img.rect() == shot.img.rect()).unwrap();
    drop(again);

    // GDI sees the same screen, so a wrong offset or row length shows up as a big difference.
    let t = Instant::now();
    let gdi = capture::grab(false).expect("capture");
    writeln!(log, "gdi capture: {} ms", t.elapsed().as_millis()).unwrap();
    for m in &shot.monitors {
        let (mut differ, mut lit, mut total, mut n) = (0, 0, 0, 0);
        for y in m.top..m.bottom {
            let w = m.right - m.left;
            for (a, b) in shot.img.row(m.left, y, w).as_chunks::<4>().0.iter().zip(gdi.img.row(m.left, y, w).as_chunks::<4>().0.iter()) {
                let most = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0);
                differ += (most > 8) as u64;
                total += most as u64;
                lit += (a[..3] != [0, 0, 0]) as u64;
                n += 1;
            }
        }
        let pct = |v: u64| 100.0 * v as f64 / n as f64;
        writeln!(log, "  monitor at {},{}: dxgi vs gdi {:.2}% differ, mean {:.2} levels, {:.1}% not black", m.left, m.top, pct(differ), total as f64 / n as f64, pct(lit)).unwrap();
    }
    drop(gdi);
    save_bmp(&format!("{dir}/capture.bmp"), shot.img.w, shot.img.h, shot.img.pixels());

    // Clipboard packing: a 100x50 box, bottom row first, 3 bytes per pixel.
    let m0 = shot.monitors[0];
    let r = RECT { left: m0.left + 10, top: m0.top + 20, right: m0.left + 110, bottom: m0.top + 70 };
    let mut packed = vec![0u8; 300 * 50];
    clip::pack(&shot.img, r, &mut packed);
    let pack_ok = packed.as_chunks::<300>().0.iter().enumerate().all(|(i, row)| {
        let src = shot.img.row(r.left, r.bottom - 1 - i as i32, 100);
        row.as_chunks::<3>().0.iter().zip(src.as_chunks::<4>().0.iter()).all(|(d, s)| d == &s[..3])
    });
    writeln!(log, "pack ok: {pack_ok}").unwrap();

    let t = Instant::now();
    let found = overlay::visible_windows();
    writeln!(log, "windows found: {} in {} ms", found.len(), t.elapsed().as_millis()).unwrap();

    // Fixed picture and windows from here on (screen coordinates for the windows).
    let w = shot.img.w as usize;
    for (i, p) in shot.img.pixels_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        p.copy_from_slice(&[x as u8, y as u8, ((x + y) >> 3) as u8, 255]);
    }
    let (sx, sy) = (m0.left + shot.x, m0.top + shot.y);
    let windows = vec![
        RECT { left: sx + 200, top: sy + 150, right: sx + 1100, bottom: sy + 750 },
        RECT { left: sx + 600, top: sy + 400, right: sx + 1500, bottom: sy + 900 },
    ];
    let first_window = (windows[0].left - shot.x + 40, windows[0].top - shot.y + 40);
    let centre = POINT { x: (m0.left + m0.right) / 2 + shot.x, y: (m0.top + m0.bottom) / 2 + shot.y };

    let t = Instant::now();
    let mut o = Overlay::new(shot, windows, centre, Mode::Rect).expect("overlay");
    writeln!(log, "overlay build: {} ms", t.elapsed().as_millis()).unwrap();
    let (mw, mh) = (m0.right - m0.left, m0.bottom - m0.top);
    let at = |fx: f32, fy: f32| (m0.left + (mw as f32 * fx) as i32, m0.top + (mh as f32 * fy) as i32);
    let b1 = o.buttons[1];
    let states = [
        ("01-idle", Mode::Rect, None, at(0.5, 0.5)),
        ("02-drag", Mode::Rect, Some(at(0.3, 0.3)), at(0.6, 0.55)),
        ("03-drag-bottom-edge", Mode::Rect, Some(at(0.1, 0.8)), (m0.left + mw / 3, m0.top + mh - 1)),
        ("04-drag-tiny", Mode::Rect, Some(at(0.8, 0.2)), (at(0.8, 0.2).0 + 3, at(0.8, 0.2).1 + 3)),
        ("05-window", Mode::Window, None, first_window),
        ("06-screen", Mode::Screen, None, at(0.5, 0.5)),
        ("07-bar-hover", Mode::Rect, None, ((b1.left + b1.right) / 2, (b1.top + b1.bottom) / 2)),
        ("08-drag-right-edge", Mode::Rect, Some(at(0.7, 0.4)), (m0.left + mw - 1, at(0.7, 0.6).1)),
    ];
    for (name, mode, drag, cur) in states {
        (o.mode, o.drag, o.cur) = (mode, drag, cur);
        let t = Instant::now();
        o.paint(o.back.rect());
        let ms = t.elapsed().as_millis();
        let hl = o.highlight().map(|r| (r.left, r.top, r.right - r.left, r.bottom - r.top));
        writeln!(log, "{name}: full paint {ms} ms, highlight {hl:?}").unwrap();
        save_bmp(&format!("{dir}/{name}.bmp"), o.back.w, o.back.h, o.back.pixels());
    }
    let bar = o.bar;
    writeln!(log, "bar: {:?}", (bar.left, bar.top, bar.right - bar.left, bar.bottom - bar.top)).unwrap();
    dialogs(dir, &mut log);
    std::fs::write(format!("{dir}/selftest.txt"), log).unwrap();
}

/// The install window: every page, dark and light, at 100% and 150%.
fn dialogs(dir: &str, log: &mut String) {
    let busy = || {
        let mut p = Page::install_as(false, true);
        p.busy = Some("Installing\u{2026}");
        p
    };
    let failed = || {
        let why = "Couldn't copy Onion Peel to %LOCALAPPDATA%\\Programs\\Onion Peel: The process cannot access the file \
                   because it is being used by another process. (os error 32)";
        Page::finished(Kind::Failed, "Couldn't install Onion Peel", why)
    };
    type Case<'a> = (&'a str, &'a dyn Fn() -> Page, Option<Hit>, Option<Hit>);
    let pages: [Case; 10] = [
        ("install", &|| Page::install_as(false, true), None, None),
        ("install-hover", &|| Page::install_as(false, true), Some(Hit::Button(0)), None),
        ("install-unticked-hover", &|| Page::install_as(false, false), Some(Hit::Check), None),
        ("install-press-cancel", &|| Page::install_as(false, true), Some(Hit::Button(1)), Some(Hit::Button(1))),
        ("installing", &busy, None, None),
        ("update", &|| Page::install_as(true, false), None, None),
        ("installed", &|| Page::installed(false), None, None),
        ("uninstall", &Page::uninstall, Some(Hit::Button(1)), None),
        ("uninstalled", &Page::uninstalled, None, None),
        ("failed", &failed, None, None),
    ];
    for dark in [true, false] {
        for dpi in [96, 144] {
            for (name, page, hover, pressed) in &pages {
                let mut d = Dialog::new(page(), dark, dpi).expect("dialog");
                (d.hover, d.pressed) = (*hover, *pressed);
                d.paint();
                let theme = if dark { "dark" } else { "light" };
                let file = format!("dialog-{theme}-{}-{name}", dpi * 100 / 96);
                let b = &d.lay.buttons;
                writeln!(log, "{file}: {}x{}, buttons {:?}", d.back.w, d.back.h, b.iter().map(|r| (r.left, r.top, r.right - r.left)).collect::<Vec<_>>()).unwrap();
                save_bmp(&format!("{dir}/{file}.bmp"), d.back.w, d.back.h, d.back.pixels());
            }
        }
    }
}

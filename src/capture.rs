//! Grab every monitor exactly as it looks right now.
//!
//! DXGI Desktop Duplication first (the path OBS and Discord use: it sees borderless and most
//! fullscreen games, where GDI only gets black), GDI BitBlt as the fallback per monitor.

use std::sync::mpsc::{channel, Receiver};

use windows::core::Interface;
use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::dib::Dib;
use crate::geom::offset;

/// The whole virtual screen, frozen.
pub struct Shot {
    /// Screen position of the picture's top-left pixel.
    pub x: i32,
    pub y: i32,
    pub img: Dib,
    /// Monitor rectangles, in picture coordinates.
    pub monitors: Vec<RECT>,
    /// How each monitor was captured (for the self-test log).
    pub how: Vec<String>,
}

/// One monitor's picture from DXGI, tightly packed BGRA.
struct Frame {
    w: i32,
    h: i32,
    px: Vec<u8>,
    /// Set when the desktop came as 16-bit floats: the white level it was converted with.
    white: Option<f32>,
}

/// Where the virtual screen (every monitor together) is right now: x, y, width, height.
fn virtual_screen() -> (i32, i32, i32, i32) {
    let metric = |i| unsafe { GetSystemMetrics(i) };
    (metric(SM_XVIRTUALSCREEN), metric(SM_YVIRTUALSCREEN), metric(SM_CXVIRTUALSCREEN).max(1), metric(SM_CYVIRTUALSCREEN).max(1))
}

/// One monitor through Desktop Duplication, on a thread of its own. The picture is handed over
/// the moment it's read: closing the Direct3D device afterwards takes some 10 ms, and the snip
/// screen doesn't wait for that.
fn duplicate_on_thread(a: u32, o: u32) -> Receiver<Result<Frame, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let dup = Duplication::open(a, o);
        let _ = tx.send(dup.as_ref().map_err(String::clone).and_then(Duplication::frame));
    });
    rx
}

/// `dxgi: false` uses GDI for every monitor (the self-test compares the two).
pub fn grab(dxgi: bool) -> Option<Shot> {
    let (x, y, w, h) = virtual_screen();
    let img = Dib::new(w, h)?;
    let mut shot = Shot { x, y, img, monitors: vec![], how: vec![] };

    // All monitors at once, so the freeze takes as long as the slowest one, not the sum.
    let jobs: Vec<_> = outputs().into_iter().map(|(a, o, r)| (r, dxgi.then(|| duplicate_on_thread(a, o)))).collect();
    for (r, job) in jobs {
        let r = offset(r, -x, -y);
        let how = match job.map(|j| j.recv().unwrap_or_else(|_| Err("panicked".into()))) {
            Some(Ok(frame)) => {
                shot.paste(r, &frame);
                frame.white.map_or("dxgi".into(), |w| format!("dxgi, 16-bit, white {w:.2}"))
            }
            Some(Err(why)) => {
                shot.gdi(r);
                format!("gdi ({why})")
            }
            None => {
                shot.gdi(r);
                "gdi".into()
            }
        };
        shot.monitors.push(r);
        shot.how.push(how);
    }
    if shot.monitors.is_empty() {
        // No DXGI at all (very unusual): one GDI grab of everything.
        let all = shot.img.rect();
        shot.gdi(all);
        shot.monitors.push(all);
        shot.how.push("gdi".into());
    }
    Some(shot)
}

impl Shot {
    /// The picture stretched to the screen as it is now, when the monitors changed size since it
    /// was taken (a game in exclusive fullscreen can run at its own resolution). Also returns
    /// each monitor's rectangle in the stretched picture and in this one.
    pub fn fit_to_screen(&self) -> Option<(Shot, Vec<(RECT, RECT)>)> {
        let (x, y, w, h) = virtual_screen();
        // The quick check first, so a normal snip opens no slower.
        if (x, y, w, h) == (self.x, self.y, self.img.w, self.img.h) {
            return None;
        }
        let now: Vec<RECT> = outputs().into_iter().map(|(_, _, r)| offset(r, -x, -y)).collect();
        if now.len() != self.monitors.len() {
            return None;
        }
        let img = Dib::new(w, h)?;
        unsafe {
            SetStretchBltMode(img.dc, HALFTONE);
            let _ = SetBrushOrgEx(img.dc, 0, 0, None);
            for (n, o) in now.iter().zip(&self.monitors) {
                let _ = StretchBlt(
                    img.dc, n.left, n.top, n.right - n.left, n.bottom - n.top,
                    self.img.dc, o.left, o.top, o.right - o.left, o.bottom - o.top,
                    SRCCOPY,
                );
            }
        }
        let pairs = now.iter().copied().zip(self.monitors.iter().copied()).collect();
        Some((Shot { x, y, img, monitors: now, how: self.how.clone() }, pairs))
    }

    /// Copy a monitor's frame to `r`, clipped to both.
    fn paste(&mut self, r: RECT, f: &Frame) {
        let (x0, x1) = (r.left.max(0), (r.left + f.w).min(r.right).min(self.img.w));
        let (y0, y1) = (r.top.max(0), (r.top + f.h).min(r.bottom).min(self.img.h));
        if x1 <= x0 {
            return;
        }
        let (n, w) = ((x1 - x0) as usize * 4, self.img.w);
        let dst = self.img.pixels_mut();
        for y in y0..y1 {
            let s = (((y - r.top) * f.w + x0 - r.left) * 4) as usize;
            let d = ((y * w + x0) * 4) as usize;
            dst[d..d + n].copy_from_slice(&f.px[s..s + n]);
        }
    }

    /// The fallback: copy `r` straight off the screen.
    fn gdi(&self, r: RECT) {
        unsafe {
            let screen = GetDC(HWND::default());
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            let _ = BitBlt(self.img.dc, r.left, r.top, w, h, screen, r.left + self.x, r.top + self.y, SRCCOPY | CAPTUREBLT);
            ReleaseDC(HWND::default(), screen);
        }
    }
}

/// (adapter, output, desktop rectangle) of every monitor on the desktop.
fn outputs() -> Vec<(u32, u32, RECT)> {
    let mut v = vec![];
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return v };
        for a in 0.. {
            let Ok(adapter) = factory.EnumAdapters1(a) else { break };
            for o in 0.. {
                let Ok(output) = adapter.EnumOutputs(o) else { break };
                match output.GetDesc() {
                    Ok(d) if d.AttachedToDesktop.as_bool() => v.push((a, o, d.DesktopCoordinates)),
                    _ => {}
                }
            }
        }
    }
    v
}

/// Name the step that failed, keeping Windows' reason.
fn step<T>(r: windows::core::Result<T>, name: &str) -> Result<T, String> {
    r.map_err(|e| format!("{name}: {e}"))
}

/// One monitor's Desktop Duplication session. COM objects can't cross threads, so each
/// thread opens its own adapter and output by index.
struct Duplication {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dup: IDXGIOutputDuplication,
    /// What a 16-bit float desktop's white is (1.0 for an 8-bit desktop).
    white: f32,
}

impl Duplication {
    fn open(a: u32, o: u32) -> Result<Duplication, String> {
        unsafe {
            let factory = step(CreateDXGIFactory1::<IDXGIFactory1>(), "factory")?;
            let adapter = step(factory.EnumAdapters1(a), "adapter")?;
            let output = step(adapter.EnumOutputs(o), "output")?;
            let (mut device, mut ctx) = (None, None);
            let made = D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            );
            step(made, "device")?;
            let (device, ctx): (ID3D11Device, ID3D11DeviceContext) = (device.ok_or("no device")?, ctx.ok_or("no context")?);

            // Take the desktop in its own format. HDR and auto-colour-managed desktops are 16-bit
            // floats, and Windows' own conversion of those to 8-bit comes out far too dark.
            let formats = [DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_B8G8R8A8_UNORM];
            let dup = match output.cast::<IDXGIOutput5>().and_then(|o| o.DuplicateOutput1(&device, 0, &formats)) {
                Ok(dup) => dup,
                Err(_) => step(output.cast::<IDXGIOutput1>().and_then(|o| o.DuplicateOutput(&device)), "duplicate")?,
            };
            let desc = dup.GetDesc();
            if desc.Rotation != DXGI_MODE_ROTATION_IDENTITY && desc.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
                return Err("rotated".into()); // GDI already returns it the right way up
            }
            let white = match desc.ModeDesc.Format {
                DXGI_FORMAT_R16G16B16A16_FLOAT => sdr_white(&step(output.GetDesc(), "output name")?.DeviceName),
                _ => 1.0,
            };
            Ok(Duplication { device, ctx, dup, white })
        }
    }

    /// The monitor as it looks now.
    fn frame(&self) -> Result<Frame, String> {
        // The first frame after DuplicateOutput can be an empty surface (LastPresentTime 0).
        // Only a frame Windows actually presented counts. A monitor showing a game presents
        // every refresh, so this is quick there; a monitor where nothing moves gets GDI instead.
        for _ in 0..4 {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res = None;
            unsafe {
                match self.dup.AcquireNextFrame(35, &mut info, &mut res) {
                    Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                    other => step(other, "acquire")?,
                }
                let staged = res.filter(|_| info.LastPresentTime != 0).and_then(|res| stage(&self.device, &self.ctx, &res));
                let _ = self.dup.ReleaseFrame();
                if let Some(tex) = staged {
                    return read(&self.ctx, &tex, self.white);
                }
            }
        }
        Err("no new frame".into())
    }
}

/// Copy the frame into a texture the CPU can read.
unsafe fn stage(device: &ID3D11Device, ctx: &ID3D11DeviceContext, res: &IDXGIResource) -> Option<ID3D11Texture2D> {
    let tex: ID3D11Texture2D = res.cast().ok()?;
    let mut d = D3D11_TEXTURE2D_DESC::default();
    tex.GetDesc(&mut d);
    let d = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        MipLevels: 1,
        ArraySize: 1,
        ..d
    };
    let mut staging = None;
    device.CreateTexture2D(&d, None, Some(&mut staging)).ok()?;
    let staging = staging?;
    ctx.CopyResource(&staging, &tex);
    Some(staging)
}

/// Read the staged frame back as 8-bit BGRA; a 16-bit frame is converted with `white`.
unsafe fn read(ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D, white: f32) -> Result<Frame, String> {
    let mut d = D3D11_TEXTURE2D_DESC::default();
    tex.GetDesc(&mut d);
    let fp16 = d.Format == DXGI_FORMAT_R16G16B16A16_FLOAT;
    let mut map = D3D11_MAPPED_SUBRESOURCE::default();
    step(ctx.Map(tex, 0, D3D11_MAP_READ, 0, Some(&mut map)), "map")?;
    let (w, h, pitch) = (d.Width as usize, d.Height as usize, map.RowPitch as usize);
    let row = w * if fp16 { 8 } else { 4 };
    if w == 0 || h == 0 || pitch < row || map.pData.is_null() {
        ctx.Unmap(tex, 0);
        return Err("empty frame".into());
    }
    let src = std::slice::from_raw_parts(map.pData as *const u8, pitch * (h - 1) + row);
    let px = unpack(src, pitch, w, h, fp16.then(|| srgb_table(white)).as_deref());
    ctx.Unmap(tex, 0);
    // A fully black frame usually means duplication didn't really see the screen.
    if px.as_chunks::<4>().0.iter().all(|p| p[..3] == [0, 0, 0]) {
        return Err("all black".into());
    }
    Ok(Frame { w: w as i32, h: h as i32, px, white: fp16.then_some(white) })
}

/// A mapped frame (rows `pitch` bytes apart) as tightly packed BGRA. With `lut`, the frame is
/// 16-bit float RGBA and each half goes through the table; without, it is BGRA already.
fn unpack(src: &[u8], pitch: usize, w: usize, h: usize, lut: Option<&[u8]>) -> Vec<u8> {
    let mut px = vec![0; w * h * 4];
    for (out, line) in px.chunks_exact_mut(w * 4).zip(src.chunks(pitch)) {
        match lut {
            Some(lut) => {
                for (d, p) in out.as_chunks_mut::<4>().0.iter_mut().zip(line.as_chunks::<8>().0) {
                    let c = |i: usize| lut[u16::from_le_bytes([p[i], p[i + 1]]) as usize];
                    *d = [c(4), c(2), c(0), 255]; // RGBA halves to BGRA bytes
                }
            }
            None => out.copy_from_slice(&line[..w * 4]),
        }
    }
    px
}

/// Every 16-bit float (by bit pattern) of linear scRGB light to an 8-bit sRGB value, with
/// `white` as full white. Brighter-than-white HDR light is clipped.
fn srgb_table(white: f32) -> Vec<u8> {
    (0..=u16::MAX)
        .map(|h| {
            let v = (half(h) / white).clamp(0.0, 1.0);
            let s = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0 + 0.5) as u8
        })
        .collect()
}

fn half(h: u16) -> f32 {
    let (exp, frac) = ((h >> 10) & 0x1f, (h & 0x3ff) as f32 / 1024.0);
    let v = match exp {
        0 => frac * 2f32.powi(-14),
        31 => f32::INFINITY,
        e => (1.0 + frac) * 2f32.powi(e as i32 - 15),
    };
    if h & 0x8000 != 0 { -v } else { v }
}

/// The monitor's SDR white in scRGB units (1.0 = 80 nits), from its GDI device name.
fn sdr_white(device: &[u16; 32]) -> f32 {
    let name = |s: &[u16]| s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())].to_vec();
    unsafe {
        let (mut np, mut nm) = (0, 0);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS {
            return 1.0;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None) != ERROR_SUCCESS {
            return 1.0;
        }
        for p in &paths[..np as usize] {
            let header = |kind, size: usize, adapter, id| DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: kind, size: size as u32, adapterId: adapter, id };
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: header(DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(), p.sourceInfo.adapterId, p.sourceInfo.id),
                ..Default::default()
            };
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 || name(&source.viewGdiDeviceName) != name(device) {
                continue;
            }
            let mut level = DISPLAYCONFIG_SDR_WHITE_LEVEL {
                header: header(DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(), p.targetInfo.adapterId, p.targetInfo.id),
                ..Default::default()
            };
            if DisplayConfigGetDeviceInfo(&mut level.header) == 0 && level.SDRWhiteLevel > 0 {
                return level.SDRWhiteLevel as f32 / 1000.0;
            }
        }
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats() {
        assert_eq!(half(0x0000), 0.0);
        assert_eq!(half(0x3C00), 1.0);
        assert_eq!(half(0x3800), 0.5);
        assert_eq!(half(0x4000), 2.0);
        assert_eq!(half(0xBC00), -1.0);
        assert_eq!(half(0x0001), 2f32.powi(-24)); // the smallest one
        assert_eq!(half(0x7C00), f32::INFINITY);
    }

    #[test]
    fn srgb_table_runs_black_to_white_and_clips() {
        let t = srgb_table(1.0);
        assert_eq!(t.len(), 65536);
        assert_eq!((t[0x0000], t[0x3C00]), (0, 255));
        assert_eq!(t[0x3800], 188); // linear 0.5 is sRGB 188
        assert_eq!((t[0x4000], t[0x7C00]), (255, 255)); // brighter than white
        assert_eq!((t[0xBC00], t[0xFC00]), (0, 0)); // below black
        // With a brighter SDR white, the old white is no longer full white.
        let t2 = srgb_table(2.0);
        assert_eq!((t2[0x4000], t2[0x3C00]), (255, 188));
    }

    #[test]
    fn unpack_drops_row_padding() {
        // 2x2 BGRA with 12-byte rows; the last row has no padding after it.
        let src = [1, 2, 3, 4, 5, 6, 7, 8, 0xEE, 0xEE, 0xEE, 0xEE, 9, 10, 11, 12, 13, 14, 15, 16];
        assert_eq!(unpack(&src, 12, 2, 2, None), (1..=16).collect::<Vec<u8>>());
    }

    #[test]
    fn unpack_turns_float_rgba_into_bgra_bytes() {
        // One pixel: R = 1.0, G = 0.5, B = 0.0, alpha anything.
        let src = [0x00, 0x3C, 0x00, 0x38, 0x00, 0x00, 0x34, 0x12];
        assert_eq!(unpack(&src, 8, 1, 1, Some(&srgb_table(1.0))), [0, 188, 255, 255]);
    }

    #[test]
    fn paste_clips_a_frame_to_its_monitor_and_the_picture() {
        let img = Dib::new(4, 3).unwrap();
        let mut shot = Shot { x: 0, y: 0, img, monitors: vec![], how: vec![] };
        // A 3x2 frame for a monitor at (2, 1) that is 2 wide: the third column and anything
        // below the picture must be left out.
        let frame = Frame { w: 3, h: 2, px: (0..24).map(|i| 100 + i).collect(), white: None };
        shot.paste(RECT { left: 2, top: 1, right: 4, bottom: 3 }, &frame);
        let px = shot.img.pixels();
        assert!(px[..16].iter().all(|&b| b == 0)); // row 0 untouched
        assert_eq!(&px[16..32], &[0, 0, 0, 0, 0, 0, 0, 0, 100, 101, 102, 103, 104, 105, 106, 107]);
        assert_eq!(&px[32..48], &[0, 0, 0, 0, 0, 0, 0, 0, 112, 113, 114, 115, 116, 117, 118, 119]);
        // A monitor left of the picture's edge is clipped too, not wrapped.
        shot.paste(RECT { left: -2, top: 0, right: 1, bottom: 1 }, &frame);
        assert_eq!(&shot.img.pixels()[..8], &[108, 109, 110, 111, 0, 0, 0, 0]);
    }
}

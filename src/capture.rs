//! Grab every monitor exactly as it looks right now.
//!
//! DXGI Desktop Duplication first (the path OBS and Discord use: it sees borderless and most
//! fullscreen games, where GDI only gets black), GDI BitBlt as the fallback per monitor.

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

/// `dxgi: false` uses GDI for every monitor (the self-test compares the two).
pub fn grab(dxgi: bool) -> Option<Shot> {
    let metric = |i| unsafe { GetSystemMetrics(i) };
    let (x, y) = (metric(SM_XVIRTUALSCREEN), metric(SM_YVIRTUALSCREEN));
    let img = Dib::new(metric(SM_CXVIRTUALSCREEN).max(1), metric(SM_CYVIRTUALSCREEN).max(1))?;
    let mut shot = Shot { x, y, img, monitors: vec![], how: vec![] };

    // All monitors at once, so the freeze takes as long as the slowest one, not the sum.
    let jobs: Vec<_> = outputs()
        .into_iter()
        .map(|(a, o, r)| (r, dxgi.then(|| std::thread::spawn(move || duplicate(a, o)))))
        .collect();
    for (r, job) in jobs {
        let r = RECT { left: r.left - x, top: r.top - y, right: r.right - x, bottom: r.bottom - y };
        let how = match job.map(|j| j.join().unwrap_or_else(|_| Err("panicked".into()))) {
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
        let metric = |i| unsafe { GetSystemMetrics(i) };
        let (x, y) = (metric(SM_XVIRTUALSCREEN), metric(SM_YVIRTUALSCREEN));
        let (w, h) = (metric(SM_CXVIRTUALSCREEN).max(1), metric(SM_CYVIRTUALSCREEN).max(1));
        // The quick check first, so a normal snip opens no slower.
        if (x, y, w, h) == (self.x, self.y, self.img.w, self.img.h) {
            return None;
        }
        let now: Vec<RECT> = outputs()
            .into_iter()
            .map(|(_, _, r)| RECT { left: r.left - x, top: r.top - y, right: r.right - x, bottom: r.bottom - y })
            .collect();
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

/// One monitor through Desktop Duplication. COM objects can't cross threads, so each
/// thread opens its own adapter and output by index.
fn duplicate(a: u32, o: u32) -> Result<Frame, String> {
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

        // The first frame after DuplicateOutput can be an empty surface (LastPresentTime 0).
        // Only a frame Windows actually presented counts. A monitor showing a game presents
        // every refresh, so this is quick there; a monitor where nothing moves gets GDI instead.
        for _ in 0..4 {
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res = None;
            match dup.AcquireNextFrame(35, &mut info, &mut res) {
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
                other => step(other, "acquire")?,
            }
            let staged = res.filter(|_| info.LastPresentTime != 0).and_then(|res| stage(&device, &ctx, &res));
            let _ = dup.ReleaseFrame();
            if let Some(tex) = staged {
                return read(&ctx, &tex, white);
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
    let src = std::slice::from_raw_parts(map.pData as *const u8, pitch * (h - 1) + row);
    let mut px = Vec::with_capacity(w * h * 4);
    if fp16 {
        let lut = srgb_table(white);
        for line in src.chunks(pitch) {
            for p in line[..row].as_chunks::<8>().0.iter() {
                let c = |i: usize| lut[u16::from_le_bytes([p[i], p[i + 1]]) as usize];
                px.extend_from_slice(&[c(4), c(2), c(0), 255]); // RGBA halves to BGRA bytes
            }
        }
    } else {
        for line in src.chunks(pitch) {
            px.extend_from_slice(&line[..row]);
        }
    }
    ctx.Unmap(tex, 0);
    // A fully black frame usually means duplication didn't really see the screen.
    if px.as_chunks::<4>().0.iter().all(|p| p[..3] == [0, 0, 0]) {
        return Err("all black".into());
    }
    Ok(Frame { w: w as i32, h: h as i32, px, white: fp16.then_some(white) })
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

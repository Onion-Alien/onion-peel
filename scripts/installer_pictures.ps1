<# Pictures of the installer's pages, for checking and for the README.
   Builds the /DPREVIEW installer (it installs nothing), runs it on a hidden Windows desktop
   (nothing reaches the screen), saves each page as a PNG, clicks Install, saves the last
   page, then closes it.
     powershell -File scripts\installer_pictures.ps1      (after build-release.ps1)
   Pictures land in target\installer\pictures. #>
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
          "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 is missing: winget install JRSoftware.InnoSetup' }
$out = Join-Path $root 'target\installer\pictures'
New-Item -ItemType Directory -Force $out | Out-Null

if (-not ('OnionSetupShots' -as [type])) {
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
public static class OnionSetupShots {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct StartupInfo {
        public int cb; public string reserved; public string desktop; public string title;
        public int x, y, xSize, ySize, xChars, yChars, fill, flags;
        public short show, reserved2; public IntPtr reserved3, stdIn, stdOut, stdErr;
    }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr process, thread; public int pid, tid; }
    [StructLayout(LayoutKind.Sequential)] struct Rect { public int l, t, r, b; }
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr CreateDesktop(string name, IntPtr device, IntPtr mode, int flags, uint access, IntPtr sa);
    [DllImport("user32.dll")] static extern bool SetThreadDesktop(IntPtr d);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
    [DllImport("user32.dll")] static extern bool EnumDesktopWindows(IntPtr d, EnumProc f, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr h, EnumProc f, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out Rect r);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int a, out Rect r, int size);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool CreateProcess(string app, string cmd, IntPtr pa, IntPtr ta, bool inherit, uint flags,
        IntPtr env, string dir, ref StartupInfo si, out ProcessInfo pi);
    public static string Log = "";

    static string Text(IntPtr h) { var s = new StringBuilder(256); GetWindowText(h, s, 256); return s.ToString(); }
    static string Cls(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
    static IntPtr Wizard(IntPtr desk) {
        IntPtr found = IntPtr.Zero;
        EnumDesktopWindows(desk, (h, l) => { if (Cls(h) == "TWizardForm" && IsWindowVisible(h)) found = h; return true; }, IntPtr.Zero);
        return found;
    }
    static IntPtr Button(IntPtr wiz, string text) {
        IntPtr found = IntPtr.Zero;
        EnumChildWindows(wiz, (h, l) => { if (IsWindowVisible(h) && Text(h).Replace("&", "") == text) found = h; return true; }, IntPtr.Zero);
        return found;
    }
    static void Snap(IntPtr h, string file) {
        // the whole window, then cut to what's seen (without the invisible resize borders)
        Rect r, seen; GetWindowRect(h, out r);
        if (DwmGetWindowAttribute(h, 9, out seen, 16) != 0) seen = r; // DWMWA_EXTENDED_FRAME_BOUNDS
        var crop = new Rectangle(seen.l - r.l, seen.t - r.t, seen.r - seen.l, seen.b - seen.t);
        using (var bmp = new Bitmap(r.r - r.l, r.b - r.t, PixelFormat.Format32bppArgb)) {
            using (var g = Graphics.FromImage(bmp)) { IntPtr dc = g.GetHdc(); PrintWindow(h, dc, 2); g.ReleaseHdc(dc); }
            using (var cut = bmp.Clone(crop, PixelFormat.Format32bppArgb)) cut.Save(file, ImageFormat.Png);
        }
        Log += file + " " + crop.Width + "x" + crop.Height + "\n";
    }
    static IntPtr WaitFor(Func<IntPtr> f, int ms) {
        for (int i = 0; i < ms / 100; i++) { IntPtr h = f(); if (h != IntPtr.Zero) return h; Thread.Sleep(100); }
        return IntPtr.Zero;
    }
    // Runs the setup on the hidden desktop: the first page, then Install, then the last page.
    // Returns the setup processes' ids, for the caller to close.
    public static List<uint> Run(string setup, string prefix) {
        var pids = new List<uint>();
        var t = new Thread(() => {
            SetThreadDpiAwarenessContext(new IntPtr(-4)); // per-monitor: real pixel sizes
            IntPtr desk = CreateDesktop("onion_peel_setup", IntPtr.Zero, IntPtr.Zero, 0, 0x10000000, IntPtr.Zero);
            if (desk == IntPtr.Zero || !SetThreadDesktop(desk)) { Log += "no desktop\n"; return; }
            var si = new StartupInfo(); si.cb = Marshal.SizeOf(si); si.desktop = "WinSta0" + (char)92 + "onion_peel_setup";
            ProcessInfo pi;
            if (!CreateProcess(null, "\"" + setup + "\"", IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero, null, ref si, out pi)) { Log += "no process\n"; return; }
            pids.Add((uint)pi.pid);
            IntPtr wiz = WaitFor(() => Wizard(desk), 20000);
            if (wiz == IntPtr.Zero) { Log += "no wizard\n"; return; }
            uint wp; GetWindowThreadProcessId(wiz, out wp); pids.Add(wp);
            IntPtr install = WaitFor(() => Button(wiz, "Install"), 5000);
            Thread.Sleep(800);
            Snap(wiz, prefix + "-1-install.png");
            if (install == IntPtr.Zero) { Log += "no Install button\n"; return; }
            SendMessage(install, 0x00F5, IntPtr.Zero, IntPtr.Zero); // BM_CLICK
            IntPtr finish = WaitFor(() => Button(wiz, "Finish"), 20000);
            Thread.Sleep(800);
            Snap(wiz, prefix + "-2-finished.png");
            if (finish == IntPtr.Zero) Log += "no Finish button\n";
        });
        t.Start(); t.Join();
        return pids;
    }
}
"@
}

$dir = Join-Path $root 'target\installer\preview'
& $iscc /Q /DPREVIEW "/O$dir" (Join-Path $root 'installer\OnionPeel.iss')
if ($LASTEXITCODE) { throw 'ISCC failed' }
$pids = [OnionSetupShots]::Run((Join-Path $dir 'OnionPeelSetup.exe'), (Join-Path $out 'setup'))
foreach ($p in $pids) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
[OnionSetupShots]::Log

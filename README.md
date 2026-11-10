# Onion Peel

Peel a piece off your screen. Press **Win+Shift+S**, pick an area, and it's on the
clipboard. That's all it does.

- No window, no tray icon, no history, no files saved.
- No network: it never connects to the internet.
- Freezes the screen first, so menus and tooltips stay put while you pick.
- Works over fullscreen games and on every monitor, HDR included.

## Install

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/install-dark.png">
  <img src="docs/install-light.png" width="440" alt="The Install Onion Peel window: a short description, a 'Start Onion Peel when Windows starts' checkbox, and Install and Cancel buttons">
</picture>

Download `onion-peel.exe` from the [latest release](https://github.com/Onion-Alien/onion-peel/releases/latest)
and run it. A small window asks whether Onion Peel should start with Windows; press
**Install**. No admin rights needed.

The exe isn't code-signed yet, so Windows SmartScreen may warn the first time: choose
**More info**, then **Run anyway**. Each release lists the exe's SHA-256, so you can check
the download with `Get-FileHash onion-peel.exe`, or build it yourself (below).

To uninstall, search the Start menu for **Uninstall Onion Peel**, or use
Settings > Apps > Installed apps. Running a newer `onion-peel.exe` updates it.

## Use

| Key or action | What it does |
|---|---|
| Win+Shift+S | Freeze the screen and open the snip bar |
| Drag | Snip a rectangle (Rectangle mode) |
| Click | Snip the window or monitor under the mouse (Window / Full screen mode) |
| 1, 2, 3 | Rectangle, Window, Full screen |
| Alt+Tab | Bring another window to the front; the picture is retaken with it |
| Right-click | Drop a drag in progress, otherwise close |
| Esc | Close without snipping |

The snip goes to the clipboard only; paste it anywhere.

## Limits

- Windows drops a game in exclusive fullscreen to the desktop when any window opens over
  it, Snipping Tool's too. So over such a game Win+Shift+S only takes the picture, and the
  game keeps running; the snip screen opens with that picture once you Alt+Tab out or the
  game closes. Pressing it again before then takes a newer picture. Borderless fullscreen
  games get the snip screen right away.
- On HDR screens, light brighter than SDR white is clipped.
- The snip screen is left out of screen recordings and shares (OBS, Discord).

## Build

Rust (stable) on Windows:

```
cargo build --release
```

The exe is `target/release/onion-peel.exe`. See [CONTRIBUTING.md](CONTRIBUTING.md)
for the self-test and the privacy checks.

## License

[GPL-3.0](LICENSE).

# Security and privacy

## Network

None. Onion Peel makes no network connections of any kind: no telemetry, no usage
counts, no update checks.

## What it reads

The screen, only while you snip (DXGI Desktop Duplication, GDI as a fallback), and the
list of visible windows (for Window mode). It watches the keyboard with a low-level
hook for Win+Shift+S, and, while the snip screen is open, for Esc and 1, 2, 3; every
other key passes straight through and nothing is recorded.

## What it writes

The program writes one thing: the snip, to the clipboard, when you take one. Nothing is
ever saved to disk.

Installing is done by `OnionPeelSetup.exe`, a standard [Inno Setup](https://jrsoftware.org/isinfo.php)
installer built from [installer/OnionPeel.iss](installer/OnionPeel.iss). It writes:

| Where | What |
|---|---|
| `%LOCALAPPDATA%\Programs\Onion Peel` | The program and its uninstaller |
| Start menu | "Onion Peel" and "Uninstall Onion Peel" |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{62F5E021-5387-42F1-BB03-740C0ECFC44B}_is1` | The Installed apps entry |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `OnionPeel` | Only if it starts with Windows |

Uninstalling removes all of these.

## Reporting a problem

Open an issue on GitHub. For anything that shouldn't be public, use GitHub's
private vulnerability reporting on this repository.

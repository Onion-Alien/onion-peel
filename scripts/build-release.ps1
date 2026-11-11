# Release build: the exe with no local paths inside, then the installer around it
# (target\installer\OnionPeelSetup.exe, the file that's published).
# Rust keeps the source path of every panic message, and the paths of downloaded crates
# contain this PC's user name; here they become neutral names. Then the exe and the
# installer are checked for the user name and home folder.
#   powershell -File scripts/build-release.ps1 [extra cargo args]
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME '.cargo' }
# One flag per entry (separated by 0x1f), so paths with spaces stay whole.
$env:CARGO_ENCODED_RUSTFLAGS = @(
    "--remap-path-prefix=$cargoHome=cargo",
    "--remap-path-prefix=$root=onion-peel"
) -join [char]0x1f
Push-Location $root
try {
    cargo build --release @args
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
} finally { Pop-Location }

$exe = Join-Path $root 'target\release\onion-peel.exe'
$bytes = [IO.File]::ReadAllBytes($exe)
$text = [Text.Encoding]::ASCII.GetString($bytes) + [Text.Encoding]::Unicode.GetString($bytes)
$leaks = @($env:USERNAME, $HOME, $cargoHome, $root) | Where-Object { $_ -and $text.IndexOf($_, [StringComparison]::OrdinalIgnoreCase) -ge 0 }
if ($leaks) { Write-Error "onion-peel.exe still contains: $($leaks -join ', ')" }

# The installer (installer\OnionPeel.iss): its pictures are drawn first
python (Join-Path $root 'scripts\make_installer_art.py')
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
          "$env:ProgramFiles\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { Write-Error 'Inno Setup 6 is missing: winget install JRSoftware.InnoSetup' }
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
& $iscc /Q "/DAppVersion=$version" (Join-Path $root 'installer\OnionPeel.iss')
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$setup = Join-Path $root 'target\installer\OnionPeelSetup.exe'
$bytes = [IO.File]::ReadAllBytes($setup)
$text = [Text.Encoding]::ASCII.GetString($bytes) + [Text.Encoding]::Unicode.GetString($bytes)
$leaks = @($env:USERNAME, $HOME, $root) | Where-Object { $_ -and $text.IndexOf($_, [StringComparison]::OrdinalIgnoreCase) -ge 0 }
if ($leaks) { Write-Error "OnionPeelSetup.exe still contains: $($leaks -join ', ')" }
$hash = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLower()
"$setup`nversion $version`nSHA-256 $hash`nno local paths inside"

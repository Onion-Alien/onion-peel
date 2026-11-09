# Release build with no local paths inside the exe. Rust keeps the source path of every
# panic message, and the paths of downloaded crates contain this PC's user name; here
# they become neutral names. Then the exe is checked for the user name and home folder.
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
$hash = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLower()
"$exe`nSHA-256 $hash`nno local paths inside"

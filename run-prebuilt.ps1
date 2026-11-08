# Boots the prebuilt RustOS image in QEMU. No Rust needed - only QEMU.
$qemu = (Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue).Source
if (-not $qemu) { $qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe" }
if (-not (Test-Path $qemu)) { Write-Host "QEMU not found. Install it from https://www.qemu.org/download/ and re-run."; exit 1 }
& $qemu -drive "format=raw,file=$PSScriptRoot\prebuilt\rustos.img"

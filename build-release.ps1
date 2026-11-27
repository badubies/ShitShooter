# Builds the game to send to friends: dist\shitshooter.exe plus dist\assets
#
# Compared to a plain `cargo build --release`, the C runtime is linked into the
# exe, so it runs on PCs that do not have the Visual C++ Redistributable.
# It builds in its own folder, so it works while the game is running.
$ErrorActionPreference = 'Stop'
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release --target-dir target\dist
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
New-Item -ItemType Directory -Force dist | Out-Null
try {
    Copy-Item target\dist\release\shitshooter.exe dist\shitshooter.exe -Force
} catch {
    Write-Host "dist\shitshooter.exe is in use, so it was not replaced. Close the game and run this again."
    exit 1
}
# The game loads models, textures and sounds from assets\ next to the exe.
if (Test-Path dist\assets) { Remove-Item -Recurse -Force dist\assets }
if (Test-Path assets) { Copy-Item -Recurse assets dist\assets }
# Maps made in Blender, which the game reads from maps\ next to the exe. Maps
# already in dist\maps are kept; ours are added or replaced.
New-Item -ItemType Directory -Force dist\maps | Out-Null
if (Test-Path maps) { Copy-Item maps\*.json dist\maps -Force }
Write-Host "Built dist\shitshooter.exe, dist\assets and dist\maps (send all three; only a host needs maps)"

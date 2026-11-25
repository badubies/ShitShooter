#!/bin/sh
# Makes ShitShooter.app, the launcher on a Mac, from a built launcher exe:
#   launcher/macos/make-app.sh <launcher exe> <folder to put ShitShooter.app in>
# Run from the repo root. The release workflow zips the result for new players.
# A .app opens without a Terminal window, which a bare exe always gets.
set -eu
exe=$1
out=$2
version=$(cargo metadata --no-deps --format-version 1 |
    python3 -c 'import json, sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "shitshooter-launcher"))')

app="$out/ShitShooter.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$exe" "$app/Contents/MacOS/ShitShooterLauncher"
cp launcher/macos/Info.plist "$app/Contents/Info.plist"
plutil -replace CFBundleShortVersionString -string "$version" "$app/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$version" "$app/Contents/Info.plist"

# The Finder and Dock icon, from the game's icon.ico (its largest picture is 256 px).
work=$(mktemp -d)
sips -s format png icon.ico --out "$work/icon.png" >/dev/null
mkdir "$work/AppIcon.iconset"
for size in 16 32 128 256; do
    sips -z $size $size "$work/icon.png" --out "$work/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
done
for size in 16 32 128; do
    double=$((size * 2))
    sips -z $double $double "$work/icon.png" --out "$work/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$work/AppIcon.iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -r "$work"

# Signs the bundle ad hoc, which Apple Silicon requires. The launcher's
# self-update later swaps the exe inside, which is signed by itself.
codesign --force --sign - "$app"
codesign --verify --strict --verbose "$app"
echo "Made $app ($version)"

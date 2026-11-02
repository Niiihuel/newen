#!/usr/bin/env bash
# A release's bundle: Newen with the CEF it was built against (docs/UPDATES.md). Usage:
#   scripts/package.sh linux|windows|macos <cargo release dir> <out dir>
# CEF is looked for under $CEF_PATH, where cef-dll-sys downloaded it during the build.
# Writes <out>/newen-x86_64-linux.tar.gz, <out>/newen/ (Windows, zipped by the workflow) or
# <out>/Newen.app (macOS, signed and zipped by the workflow).
set -euo pipefail
platform=$1 release=$2 out=$3
root=$(cd "$(dirname "$0")/.." && pwd)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
cef_root=${CEF_PATH:?CEF_PATH names where the build put CEF}
command -v cygpath >/dev/null && cef_root=$(cygpath -u "$cef_root")
mkdir -p "$out"

# The folder holding CEF's runtime files: the one with icudtl.dat (Linux, Windows) or the
# framework (macOS).
if [ "$platform" = macos ]; then
  framework=$(find "$cef_root" -maxdepth 4 -type d -name 'Chromium Embedded Framework.framework' -print -quit)
  [ -n "$framework" ] || { echo "no CEF framework under $cef_root" >&2; exit 1; }
else
  cef=$(dirname "$(find "$cef_root" -maxdepth 4 -name icudtl.dat -print -quit)")
  [ -f "$cef/icudtl.dat" ] || { echo "no CEF under $cef_root" >&2; exit 1; }
fi

# CEF's runtime files, without its headers, wrapper sources, import libraries and the
# sandbox's executables Newen does not use.
runtime() {
  local into=$1
  mkdir -p "$into"
  for file in "$cef"/*; do
    case $(basename "$file") in
      cmake | include | libcef_dll | tests | CMakeLists.txt | archive.json | README.txt | \
        *.lib | *.pdb | bootstrap*.exe | chrome-sandbox | cef_sandbox.*) ;;
      *) cp -R "$file" "$into/" ;;
    esac
  done
}

case $platform in
linux)
  bundle=$out/newen
  rm -rf "$bundle"
  runtime "$bundle"
  cp "$release/newen" "$bundle/newen"
  # CEF's Linux library carries its debug sections (over a gigabyte).
  strip --strip-debug "$bundle/libcef.so"
  # libcef.so must resolve beside the program, through its $ORIGIN rpath.
  if ldd "$bundle/newen" | grep 'not found'; then
    echo "newen's libraries are not all found beside it" >&2
    exit 1
  fi
  (cd "$bundle" && tar -czf "../newen-x86_64-linux.tar.gz" -- *)
  ;;
windows)
  bundle=$out/newen
  rm -rf "$bundle"
  runtime "$bundle"
  cp "$release/newen.exe" "$bundle/newen.exe"
  ;;
macos)
  app=$out/Newen.app
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
  cp "$release/newen" "$app/Contents/MacOS/newen"
  cp -R "$framework" "$app/Contents/Frameworks/"
  plist() { # <file> <executable> <identifier> <helper: 0|1> <name>
    cat >"$1" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>$2</string>
  <key>CFBundleIdentifier</key><string>$3</string>
  <key>CFBundleName</key><string>$5</string>
  <key>CFBundleDisplayName</key><string>$5</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleIconFile</key><string>newen</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><$([ "$4" = 1 ] && echo true || echo false)/>
  <key>LSEnvironment</key><dict><key>MallocNanoZone</key><string>0</string></dict>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSCameraUsageDescription</key><string>A page in Newen's browser asks for the camera.</string>
  <key>NSMicrophoneUsageDescription</key><string>A page in Newen's browser asks for the microphone.</string>
</dict>
</plist>
PLIST
  }
  plist "$app/Contents/Info.plist" newen dev.newen.newen 0 Newen
  # The helper apps Chromium runs its processes as, named as CEF expects them: after the
  # program, `newen` (inferred from CEF's defaults: no helper path is set).
  for kind in "" " (GPU)" " (Renderer)" " (Plugin)" " (Alerts)"; do
    name="newen Helper$kind"
    helper="$app/Contents/Frameworks/$name.app"
    mkdir -p "$helper/Contents/MacOS"
    cp "$release/newen-helper" "$helper/Contents/MacOS/$name"
    id=$(echo "$kind" | tr -d ' ()' | tr '[:upper:]' '[:lower:]')
    plist "$helper/Contents/Info.plist" "$name" "dev.newen.newen.helper${id:+.$id}" 1 "$name"
  done
  # Newen's icon, from its PNGs.
  iconset=$(mktemp -d)/newen.iconset
  mkdir -p "$iconset"
  icons=$root/apps/newen/assets/icons
  for size in 16 32 128 256 512; do
    cp "$icons/quill-$size.png" "$iconset/icon_${size}x${size}.png"
    if [ "$size" -lt 512 ]; then
      cp "$icons/quill-$((size * 2)).png" "$iconset/icon_${size}x${size}@2x.png"
    fi
  done
  iconutil -c icns "$iconset" -o "$app/Contents/Resources/newen.icns"
  ;;
*)
  echo "usage: $0 linux|windows|macos <release dir> <out dir>" >&2
  exit 2
  ;;
esac
echo "Newen $version for $platform in $out"

#!/usr/bin/env bash
# Install quill's icon into the person's icon theme and a launcher entry, so a Wayland desktop
# (which names windows by their app id, "quill") shows quill's icon. Usage:
#   scripts/install-desktop.sh [path/to/quill]   (default: target/release/quill, else debug)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
bin=${1:-$root/target/release/newen}
[ -x "$bin" ] || bin=$root/target/debug/newen
data=${XDG_DATA_HOME:-$HOME/.local/share}
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$root/apps/newen/assets/icons/quill-$size.png" "$data/icons/hicolor/${size}x${size}/apps/quill.png"
done
install -Dm644 "$root/apps/newen/assets/icons/quill.svg" "$data/icons/hicolor/scalable/apps/quill.svg"
install -Dm644 /dev/stdin "$data/applications/quill.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=quill
Comment=Agents at work on your code
Icon=quill
Exec="$bin" %U
StartupWMClass=quill
Categories=Development;
DESKTOP
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
echo "Installed quill's icon and launcher under $data"

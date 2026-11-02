#!/usr/bin/env bash
# Install Newen's icon into the person's icon theme and a launcher entry, so a Wayland desktop
# (which names windows by their app id, "newen") shows Newen's icon. Usage:
#   scripts/install-desktop.sh [path/to/newen]
# Without a path: the installed program ($XDG_DATA_HOME/newen/app/current/newen), else this
# checkout's build; a launcher of quill's made by hand (a wrapper, say) keeps what it started.
# quill's launcher and icons go.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
apps=$data/applications
bin=${1:-$data/newen/app/current/newen}
[ -n "${1:-}" ] || [ -x "$bin" ] || bin=$root/target/release/newen
[ -n "${1:-}" ] || [ -x "$bin" ] || bin=$root/target/debug/newen
exec_line="Exec=\"$bin\" %U"
if [ -z "${1:-}" ] && [ -f "$apps/quill.desktop" ]; then
  old=$(grep -m1 '^Exec=' "$apps/quill.desktop" || true)
  case $old in "" | *"$data/quill/app/"* | *"$root/target/"*) ;; *) exec_line=$old ;; esac
fi
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$root/apps/newen/assets/icons/quill-$size.png" "$data/icons/hicolor/${size}x${size}/apps/newen.png"
done
install -Dm644 "$root/apps/newen/assets/icons/quill.svg" "$data/icons/hicolor/scalable/apps/newen.svg"
install -Dm644 /dev/stdin "$apps/newen.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Newen
Comment=Agents at work on your code
Icon=newen
$exec_line
StartupWMClass=newen
Categories=Development;
DESKTOP
rm -f "$apps/quill.desktop" "$data"/icons/hicolor/*/apps/quill.png "$data"/icons/hicolor/*/apps/quill.svg
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
echo "Installed Newen's icon and launcher under $data"

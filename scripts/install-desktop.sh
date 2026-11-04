#!/usr/bin/env bash
# Install Newen's icon into the person's icon theme and a launcher entry, so a Wayland desktop
# (which names windows by their app id, "newen") shows Newen's icon. Usage:
#   scripts/install-desktop.sh [path/to/newen]
# Without a path: the installed program ($XDG_DATA_HOME/newen/app/current/newen, or where
# quill 0.1.9 placed it before Newen's first start moved the data), else this checkout's build.
# A launcher made by hand (a wrapper, say; Newen's, else quill's) keeps what it started, and so
# does one of quill's installation while it has no Newen. quill's launcher and icons go.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
apps=$data/applications
installed=("$data/newen/app/current/newen" "$data/quill/app/current/newen")
bin=${1:-}
if [ -z "$bin" ]; then
  for candidate in "${installed[@]}" "$root/target/release/newen" "$root/target/debug/newen"; do
    if [ -x "$candidate" ]; then bin=$candidate && break; fi
  done
fi
exec_line="Exec=\"$bin\" %U"
old=
if [ -z "${1:-}" ]; then
  # Newen's launcher first: a hand-made Exec carried over from quill's on an earlier run is there.
  for launcher in "$apps/newen.desktop" "$apps/quill.desktop"; do
    if [ -f "$launcher" ]; then old=$(grep -m1 '^Exec=' "$launcher" || true); fi
    if [ -n "$old" ]; then break; fi
  done
fi
case $old in
  "" | *"$root/target/"* | *"$data/newen/app/"*) ;;
  *"$data/quill/app/"*)
    case $bin in "${installed[0]}" | "${installed[1]}") ;; *) exec_line=$old ;; esac ;;
  *) exec_line=$old ;;
esac
if [ "$exec_line" = "Exec=\"$bin\" %U" ] && [ ! -x "$bin" ]; then
  echo "No Newen to start${bin:+ at $bin}: build it, install it, or give its path" >&2
  exit 1
fi
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "$root/apps/newen/assets/icons/newen-$size.png" "$data/icons/hicolor/${size}x${size}/apps/newen.png"
done
install -Dm644 "$root/apps/newen/assets/icons/newen.svg" "$data/icons/hicolor/scalable/apps/newen.svg"
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
# newen-ide, when it is beside Newen (as Newen looks for it): its icon, Newen's on a hanada horizon,
# and a launcher, so a Wayland desktop shows it by its app id, "newen-ide".
ide=$(dirname "$bin")/newen-ide
if [ -x "$ide" ]; then
  for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "$root/apps/newen-ide/assets/icons/newen-ide-$size.png" "$data/icons/hicolor/${size}x${size}/apps/newen-ide.png"
  done
  install -Dm644 "$root/apps/newen-ide/assets/icons/newen-ide.svg" "$data/icons/hicolor/scalable/apps/newen-ide.svg"
  install -Dm644 /dev/stdin "$apps/newen-ide.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Newen IDE
Comment=Newen's code editor
Icon=newen-ide
Exec="$ide" %F
StartupWMClass=newen-ide
Categories=Development;TextEditor;
DESKTOP
  echo "Installed newen-ide's icon and launcher too"
fi
rm -f "$apps/quill.desktop" "$data"/icons/hicolor/*/apps/quill.png "$data"/icons/hicolor/*/apps/quill.svg
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
echo "Installed Newen's icon and launcher under $data"

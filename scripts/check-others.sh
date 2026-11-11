#!/usr/bin/env sh
# Type-check the workspace for Windows and macOS from Linux, as CI's check.yml does on those
# systems, without their toolchains or their CEF: cef-dll-sys builds nothing (the `dox`
# feature) and stand-in C tools make the files the build scripts expect. A check only: it
# links no program. Once: rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin
set -eu
cd "$(dirname "$0")/.."
tools=$(mktemp -d)
trap 'rm -rf "$tools"' EXIT
# A compiler that makes the object it is asked for, empty.
cat > "$tools/cc" <<'CC'
#!/bin/sh
out="" prev=""
for a in "$@"; do
  case "$a" in
    -Fo*) out="${a#-Fo}" ;;
    /Fo*) out="${a#/Fo}" ;;
    -E) echo gcc; exit 0 ;;
  esac
  [ "$prev" = "-o" ] && out="$a"
  prev="$a"
done
[ -n "$out" ] && : > "$out"
exit 0
CC
# An archiver that makes the archive it is asked for, empty.
cat > "$tools/ar" <<'AR'
#!/bin/sh
for a in "$@"; do
  case "$a" in
    -out:*|/OUT:*|/out:*) : > "${a#*:}"; exit 0 ;;
    *.a|*.lib) : > "$a"; exit 0 ;;
  esac
done
exit 0
AR
chmod +x "$tools/cc" "$tools/ar"
for target in x86_64-pc-windows-msvc aarch64-apple-darwin; do
  name=$(echo "$target" | tr - _)
  echo "== $target"
  env "CC_$name=$tools/cc" "CXX_$name=$tools/cc" "AR_$name=$tools/ar" \
    cargo check --workspace --features newen-browser/dox --target "$target"
done

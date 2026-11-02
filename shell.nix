{ pkgs ? import <nixpkgs> { } }:
let
  # CEF as cef-rs expects to find it (`CEF_PATH`): the Release and Resources files side by
  # side, and an archive.json naming the version. The cef crate's version follows it.
  cef = pkgs.runCommand "newen-cef-${pkgs.cef-binary.version}" { } ''
    mkdir $out
    ln -s ${pkgs.cef-binary}/Release/* ${pkgs.cef-binary}/Resources/* $out/
    for name in CMakeLists.txt cmake include libcef_dll CREDITS.html; do
      ln -s ${pkgs.cef-binary}/$name $out/$name
    done
    echo '{"type":"minimal","name":"cef_binary_${pkgs.cef-binary.version}+nixpkgs","sha1":""}' > $out/archive.json
  '';
in
pkgs.mkShell {
  # cmake and ninja build CEF's C++ wrapper for cef-rs.
  nativeBuildInputs = with pkgs; [ stdenv.cc pkg-config cmake ninja ];
  buildInputs = with pkgs; [ wayland libxkbcommon libgbm ];
  CEF_PATH = "${cef}";
  # winit and wgpu dlopen these at runtime; libcef.so is linked from CEF_PATH, which comes
  # last: its own libvulkan.so.1 does not know where NixOS keeps the drivers, and with it first
  # wgpu finds no Vulkan and falls back to GL, where Chromium's GPU frames cannot be imported.
  # stdenv.cc.cc.lib is libstdc++.so.6, which Cisco's OpenH264 library (the clips of What's
  # new, downloaded from Cisco at run time) needs when it is loaded; without it, the poster.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (with pkgs; [ wayland libxkbcommon vulkan-loader libGL libx11 libxcursor libxi libxrandr libgbm stdenv.cc.cc.lib ]) + ":${cef}";
}

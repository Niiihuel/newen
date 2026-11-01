fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // A release keeps CEF's libcef.so beside the program (docs/UPDATES.md).
    if os == "linux" {
        println!("cargo::rustc-link-arg-bin=quill=-Wl,-rpath,$ORIGIN");
    }
    // libcef.dll is loaded at its first call, once native.rs has said where it is: beside the
    // program or in the installation's versions\{version}.
    if os == "windows" && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bin=quill=/DELAYLOAD:libcef.dll");
        println!("cargo::rustc-link-arg-bin=quill=delayimp.lib");
    }
    // shortcut: a native Windows build only (the build-dependency is the host's), cross-compiling
    // from Linux leaves the exe without its icon; check CARGO_CFG_TARGET_OS if that is needed.
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icons/quill.ico")
        // The systems it supports, as CEF's own programs declare them.
        .set_manifest(include_str!("assets/quill.exe.manifest"))
        .compile()
        .expect("quill's icon goes into the Windows executable");
}

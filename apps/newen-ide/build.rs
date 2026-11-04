fn main() {
    // newen-ide's icon in its Windows executable, as Newen's (apps/newen/build.rs).
    // shortcut: a native Windows build only (the build-dependency is the host's), cross-compiling
    // from Linux leaves the exe without its icon; check CARGO_CFG_TARGET_OS if that is needed.
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/icons/newen-ide.ico")
        .compile()
        .expect("newen-ide's icon goes into the Windows executable");
}

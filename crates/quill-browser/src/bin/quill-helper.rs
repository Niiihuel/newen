//! The program of quill.app's helper apps on macOS (`quill Helper.app`, `… (GPU)`, `… (Renderer)`…),
//! which Chromium runs its own processes as. Elsewhere Chromium runs quill itself again
//! (`native::subprocess`) and this program is not shipped.

fn main() {
    #[cfg(target_os = "macos")]
    std::process::exit(quill_browser::native::helper());
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("quill-helper is macOS's; Chromium runs quill itself here");
        std::process::exit(1);
    }
}

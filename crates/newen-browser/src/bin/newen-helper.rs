//! The program of Newen.app's helper apps on macOS (`newen Helper.app`, `… (GPU)`, `… (Renderer)`…),
//! which Chromium runs its own processes as. Elsewhere Chromium runs Newen itself again
//! (`native::subprocess`) and this program is not shipped.

fn main() {
    #[cfg(target_os = "macos")]
    std::process::exit(newen_browser::native::helper());
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("newen-helper is macOS's; Chromium runs Newen itself here");
        std::process::exit(1);
    }
}

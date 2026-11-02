//! Where CEF's resources are found at run time unless `QUILL_CEF_DIR` says: the CEF the
//! crate was built against (`CEF_PATH`, which shell.nix sets).
fn main() {
    println!("cargo::rerun-if-env-changed=CEF_PATH");
    let dir = std::env::var("CEF_PATH").unwrap_or_default();
    println!("cargo::rustc-env=QUILL_CEF_DIR={dir}");
}

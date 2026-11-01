//! What quill's apps share so they look the same: the tokens, the icons and the faces.

pub mod code;
pub mod customize;
pub mod find;
pub mod icons;
pub mod look;
pub mod packs;
pub mod pieces;
pub mod search;
pub mod settings;
pub mod sidebar;
pub mod tree;

/// The faces of the interface and of code: `system_sans` takes the desktop's sans-serif (else
/// limn's bundled Ubuntu) and the desktop's monospace; `code` names a monospace face that `fc-match`
/// resolves (an unknown name falls back to the desktop's). A face fontconfig cannot find keeps
/// its bundled one. `fc-match` blocks, so this must not run on the UI thread.
pub fn fonts(system_sans: bool, code: &str) -> limn::Fonts {
    use limn::FontFamily::*;
    let face = |pattern: &str| -> Option<limn::Font> {
        let out = quill_files::command("fc-match")
            .args(["-f", "%{file}", pattern])
            .output()
            .ok()?;
        let path = String::from_utf8(out.stdout).ok()?;
        limn::Font::new(std::fs::read(path.trim()).ok()?).ok()
    };
    let code = code.trim();
    let mut wanted = vec![
        (
            Monospace,
            if code.is_empty() {
                "monospace".into()
            } else {
                format!("{code},monospace")
            },
        ),
        (
            MonospaceBold,
            if code.is_empty() {
                "monospace:bold".into()
            } else {
                format!("{code},monospace:bold")
            },
        ),
    ];
    if system_sans {
        wanted.extend([
            (Sans, "sans-serif".into()),
            (SansMedium, "sans-serif:bold".into()),
            (SansBold, "sans-serif:bold".into()),
        ]);
    }
    let mut fonts = limn::Fonts::bundled();
    for (family, pattern) in wanted {
        if let Some(font) = face(&pattern) {
            fonts = fonts.face(family, font);
        }
    }
    fonts
}

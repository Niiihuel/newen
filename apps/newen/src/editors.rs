//! The editors on this computer, found as Desktop finds them: by the
//! app the desktop has for each one's URL scheme. On Linux that is a `.desktop` file declaring
//! `x-scheme-handler/<scheme>`; its icon is the one the desktop shows for it.

use std::path::{Path, PathBuf};

/// The editors Desktop knows, by scheme, and their names.
const KNOWN: [(&str, &str); 4] = [
    ("vscode", "VS Code"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("zed", "Zed"),
];

/// One editor found: what Open in shows, and the command its `.desktop` file runs for a link,
/// `%u` standing for it.
pub struct Found {
    pub editor: newen_ui::Editor,
    pub command: Vec<String>,
}

/// The editors installed, with their icons read from the desktop's own.
pub fn find() -> Vec<Found> {
    let dirs = data_dirs();
    found(&dirs)
        .into_iter()
        .map(|entry| Found {
            editor: newen_ui::Editor {
                picture: entry.icon.and_then(|path| picture(&path, entry.name)),
                key: entry.scheme.to_owned(),
                name: entry.name.to_owned(),
            },
            command: entry.command,
        })
        .collect()
}

/// The command a `.desktop` file's `Exec` runs for `link`: with `%u` and `%U` the link, `%%`
/// a percent, and the other field codes left out.
pub fn command(exec: &[String], link: &str) -> Vec<String> {
    exec.iter()
        .filter_map(|word| match word.as_str() {
            "%u" | "%U" => Some(link.to_owned()),
            w if w.len() == 2 && w.starts_with('%') && w != "%%" => None,
            w => Some(w.replace("%%", "%")),
        })
        .collect()
}

/// `Exec`'s words, as the desktop entry format splits them: at spaces, outside double quotes.
fn words(exec: &str) -> Vec<String> {
    let (mut words, mut word, mut quoted) = (vec![], String::new(), false);
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => word.extend(chars.next()),
            ' ' if !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// A known editor as a `.desktop` file declares it.
#[derive(Debug, PartialEq)]
struct Entry {
    scheme: &'static str,
    name: &'static str,
    icon: Option<PathBuf>,
    command: Vec<String>,
}

/// Where the desktop keeps applications and icons: `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`.
fn data_dirs() -> Vec<PathBuf> {
    let home = newen_files::data_dir();
    let rest = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(rest.split(':').filter(|d| !d.is_empty()).map(PathBuf::from))
        .collect()
}

/// Each known editor some `.desktop` file under `dirs` handles the scheme of, the first in the
/// desktop's order, with its icon's file when one is found and its command.
fn found(dirs: &[PathBuf]) -> Vec<Entry> {
    let entries: Vec<(String, Option<String>, Vec<String>)> = dirs
        .iter()
        .flat_map(|dir| desktop_files(&dir.join("applications")))
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .map(|text| {
            let key = |name: &str| {
                text.lines()
                    .skip_while(|l| l.trim() != "[Desktop Entry]")
                    .take_while(|l| !l.starts_with('[') || l.trim() == "[Desktop Entry]")
                    .find_map(|l| l.strip_prefix(name)?.trim_start().strip_prefix('='))
                    .map(|v| v.trim().to_owned())
            };
            (
                key("MimeType").unwrap_or_default(),
                key("Icon"),
                key("Exec").map(|e| words(&e)).unwrap_or_default(),
            )
        })
        .collect();
    KNOWN
        .iter()
        .filter_map(|&(scheme, name)| {
            let handler = format!("x-scheme-handler/{scheme}");
            let (_, icon, command) = entries
                .iter()
                .find(|(mime, ..)| mime.split(';').any(|m| m == handler))?;
            Some(Entry {
                scheme,
                name,
                icon: icon.as_deref().and_then(|i| icon_file(i, dirs)),
                command: command.clone(),
            })
        })
        .collect()
}

/// The `.desktop` files in `dir` and its folders, as the desktop reads them.
fn desktop_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut files = vec![];
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(desktop_files(&path));
        } else if path.extension().is_some_and(|e| e == "desktop") {
            files.push(path);
        }
    }
    files.sort();
    files
}

/// The PNG of the icon `name`: itself when it is a path, else the largest the hicolor theme
/// has, else the one in pixmaps. A picture shows in limn's menus; a theme's SVG does not yet.
fn icon_file(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let png = format!("{name}.png");
    dirs.iter().find_map(|dir| {
        ["256x256", "128x128", "512x512", "96x96", "64x64", "48x48"]
            .iter()
            .map(|size| dir.join("icons/hicolor").join(size).join("apps").join(&png))
            .chain(std::iter::once(dir.join("pixmaps").join(&png)))
            .find(|p| p.is_file())
    })
}

/// The PNG at `path` as limn takes a picture: its bytes, their digest and `name` to read out.
fn picture(path: &Path, name: &str) -> Option<limn::RasterImage> {
    use base64::Engine;
    use sha2::Digest;
    let bytes = std::fs::read(path).ok()?;
    if !bytes.starts_with(b"\x89PNG") {
        return None;
    }
    let digest: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    limn::RasterImage::new(
        digest,
        limn::RasterImageFormat::Png,
        u32::try_from(bytes.len()).ok()?,
        data.into(),
        name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_editor_is_found_by_the_scheme_its_desktop_file_handles_with_its_icon() {
        let root = std::env::temp_dir().join(format!("quill-editors-{}", std::process::id()));
        let (home, system) = (root.join("home"), root.join("system"));
        let write = |path: PathBuf, text: &[u8]| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            system.join("applications/code-url-handler.desktop"),
            b"[Desktop Entry]\nName=Visual Studio Code\nIcon=vscode\nExec=code --open-url %U\nMimeType=x-scheme-handler/vscode;\nNoDisplay=true\n",
        );
        // A key of another group does not count.
        write(
            system.join("applications/other.desktop"),
            b"[Desktop Entry]\nName=Other\n[Desktop Action x]\nMimeType=x-scheme-handler/zed;\n",
        );
        write(
            home.join("applications/sub/cursor.desktop"),
            b"[Desktop Entry]\nIcon=/nowhere/cursor.png\nMimeType=text/plain;x-scheme-handler/cursor;\n",
        );
        write(
            system.join("icons/hicolor/128x128/apps/vscode.png"),
            b"\x89PNG\r\n\x1a\nrest",
        );
        let found = found(&[home, system.clone()]);
        assert_eq!(
            found,
            [
                Entry {
                    scheme: "vscode",
                    name: "VS Code",
                    icon: Some(system.join("icons/hicolor/128x128/apps/vscode.png")),
                    command: vec!["code".into(), "--open-url".into(), "%U".into()],
                },
                Entry {
                    scheme: "cursor",
                    name: "Cursor",
                    icon: None,
                    command: vec![],
                },
            ]
        );
        assert_eq!(
            command(&found[0].command, "vscode://file/a.rs:3"),
            ["code", "--open-url", "vscode://file/a.rs:3"]
        );
        assert_eq!(
            words(r#""/opt/My Editor/bin" --x %f 100%%"#),
            ["/opt/My Editor/bin", "--x", "%f", "100%%"]
        );
        let image = picture(found[0].icon.as_ref().unwrap(), "VS Code").unwrap();
        assert_eq!(image.alt(), "VS Code");
        let _ = std::fs::remove_dir_all(root);
    }
}

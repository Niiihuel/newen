//! The icons of files and folders, from the pack chosen in Settings: Newen's own line drawings,
//! the Material set limn embeds, or a VS Code icon theme found on this computer, as an
//! extension installs it (its `package.json` names the theme's JSON).

use crate::{icons, look};
use limn::{Color, Icon};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const NEWEN: &str = "newen";
/// [`NEWEN`] as quill-ide, the IDE's name before, saved it.
pub const LEGACY_NEWEN: &str = "quill";
pub const MATERIAL: &str = "material";
/// A theme's key: this, then the path of its JSON.
pub const THEME: &str = "theme:";

pub enum Pack {
    Newen,
    Material,
    Theme(Box<Theme>),
}

/// A VS Code icon theme, its SVGs read: the icon of each file name, extension, language and
/// folder name it knows, and its defaults.
#[derive(Debug, Default)]
pub struct Theme {
    file: Option<&'static str>,
    folder: Option<&'static str>,
    folder_open: Option<&'static str>,
    names: HashMap<String, &'static str>,
    extensions: HashMap<String, &'static str>,
    languages: HashMap<String, &'static str>,
    folders: HashMap<String, &'static str>,
    folders_open: HashMap<String, &'static str>,
}

/// A pack Settings offers: its key and its name.
#[derive(Clone, Debug, PartialEq)]
pub struct Offered {
    pub key: String,
    pub label: String,
}

impl Pack {
    /// The icon of `path`, and the ink to draw it in: the text's for Newen's drawings, white
    /// for artwork in its own colors, which limn's tint then leaves as they are.
    pub fn icon(&self, path: &Path, dir: bool, open: bool) -> (Icon, Color) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let (icon, colored) = match self {
            Pack::Newen => {
                let drawing = match (dir, open) {
                    (true, true) => icons::FOLDER_OPEN,
                    (true, false) => icons::FOLDER,
                    _ => icons::FILE,
                };
                (icons::of(drawing), false)
            }
            Pack::Material if dir => (limn::folder_icon(&name, open), true),
            Pack::Material => (limn::file_icon(&name), true),
            Pack::Theme(theme) => match theme.svg(&name, dir, open) {
                Some(svg) => (Icon::Custom(svg), true),
                // A theme without a default falls back on Newen's drawings.
                None => return Pack::Newen.icon(path, dir, open),
            },
        };
        (icon, if colored { Color::WHITE } else { look::muted() })
    }
}

impl Theme {
    /// As VS Code resolves it: the whole name, then the longest extension, then the language
    /// the extension is in, then the default.
    fn svg(&self, name: &str, dir: bool, open: bool) -> Option<&'static str> {
        if dir {
            let (named, default) = if open {
                (&self.folders_open, self.folder_open.or(self.folder))
            } else {
                (&self.folders, self.folder)
            };
            return named.get(name).copied().or(default);
        }
        if let Some(svg) = self.names.get(name) {
            return Some(svg);
        }
        for (at, _) in name.match_indices('.') {
            let extension = &name[at + 1..];
            if let Some(svg) = self.extensions.get(extension) {
                return Some(svg);
            }
            if let Some(svg) = language(extension).and_then(|l| self.languages.get(l)) {
                return Some(svg);
            }
        }
        self.file
    }
}

/// VS Code's language of a file extension, for themes that give icons by language.
fn language(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "rs" => "rust",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "py" => "python",
        "json" => "json",
        "jsonc" => "jsonc",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "css" => "css",
        "scss" => "scss",
        "html" | "htm" => "html",
        "sh" | "bash" | "zsh" => "shellscript",
        "c" | "h" => "c",
        "cpp" | "cc" | "hpp" => "cpp",
        "go" => "go",
        "java" => "java",
        "nix" => "nix",
        "lua" => "lua",
        "rb" => "ruby",
        "sql" => "sql",
        "xml" => "xml",
        _ => return None,
    })
}

/// Read the theme at `json` and every SVG it names.
///
/// shortcut: each SVG is leaked to give limn the `&'static str` it draws, a few MB per theme
/// loaded, and once more each time one is chosen again; worth a cache by path if people
/// switch themes often.
pub fn load(json: &Path) -> Result<Theme, String> {
    let text = std::fs::read_to_string(json).map_err(|e| e.to_string())?;
    // shortcut: plain JSON only, as the generated themes are; one written by hand with
    // comments fails to load and says so.
    let theme: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let dir = json.parent().unwrap_or(Path::new("."));
    let mut svgs: HashMap<&str, &'static str> = HashMap::new();
    for (id, definition) in theme["iconDefinitions"].as_object().into_iter().flatten() {
        // Font glyphs and raster images are left out: limn draws SVG.
        let Some(file) = definition["iconPath"]
            .as_str()
            .filter(|p| p.ends_with(".svg"))
        else {
            continue;
        };
        if let Ok(svg) = std::fs::read_to_string(dir.join(file)) {
            svgs.insert(id, Box::leak(svg.into_boxed_str()));
        }
    }
    let svg = |id: &Value| id.as_str().and_then(|id| svgs.get(id)).copied();
    let table = |key: &str| -> HashMap<String, &'static str> {
        let entries = theme[key].as_object().into_iter().flatten();
        entries
            .filter_map(|(name, id)| Some((name.to_lowercase(), svg(id)?)))
            .collect()
    };
    Ok(Theme {
        file: svg(&theme["file"]),
        folder: svg(&theme["folder"]),
        folder_open: svg(&theme["folderExpanded"]),
        names: table("fileNames"),
        extensions: table("fileExtensions"),
        languages: table("languageIds"),
        folders: table("folderNames"),
        folders_open: table("folderNamesExpanded"),
    })
}

/// Where editors keep their extensions on this computer, and the folders the person added.
pub fn roots(added: &[PathBuf]) -> Vec<PathBuf> {
    let home = newen_files::home_dir();
    let installed = [".vscode", ".vscode-oss", ".cursor", ".windsurf"]
        .into_iter()
        .filter_map(|editor| Some(home.as_ref()?.join(editor).join("extensions")));
    installed.chain(added.iter().cloned()).collect()
}

/// Every icon theme in `roots`: each root may be an extension itself or a folder of them.
pub fn find(roots: &[PathBuf]) -> Vec<Offered> {
    let mut offered = vec![];
    for root in roots {
        let children = std::fs::read_dir(root).into_iter().flatten().flatten();
        let folders = std::iter::once(root.clone()).chain(children.map(|e| e.path()));
        for folder in folders {
            offered.extend(themes_of(&folder));
        }
    }
    offered.sort_by(|a, b| a.label.cmp(&b.label));
    offered.dedup_by(|a, b| a.key == b.key);
    offered
}

/// The icon themes an extension's `package.json` contributes.
fn themes_of(extension: &Path) -> Vec<Offered> {
    let Ok(text) = std::fs::read_to_string(extension.join("package.json")) else {
        return vec![];
    };
    let Ok(package) = serde_json::from_str::<Value>(&text) else {
        return vec![];
    };
    let themes = package["contributes"]["iconThemes"].as_array();
    themes
        .into_iter()
        .flatten()
        .filter_map(|theme| {
            let path = extension.join(theme["path"].as_str()?);
            let id = theme["id"].as_str().unwrap_or_default();
            // A label to translate ("%name%") is shown as its id.
            let label = theme["label"]
                .as_str()
                .filter(|l| !l.starts_with('%'))
                .unwrap_or(id);
            Some(Offered {
                key: format!("{THEME}{}", path.display()),
                label: label.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vs_code_theme_is_found_and_resolves_as_vs_code_does() {
        let root = std::env::temp_dir().join(format!("newen-ide-packs-{}", std::process::id()));
        let extension = root.join("someone.icons-1.0.0");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(extension.join("icons")).unwrap();
        let package =
            r#"{"contributes":{"iconThemes":[{"id":"pix","label":"Pix","path":"./theme.json"}]}}"#;
        std::fs::write(extension.join("package.json"), package).unwrap();
        let theme = r#"{
            "iconDefinitions": {
                "file": {"iconPath": "./icons/file.svg"},
                "rust": {"iconPath": "./icons/rust.svg"},
                "lock": {"iconPath": "./icons/lock.svg"},
                "dts": {"iconPath": "./icons/dts.svg"},
                "src": {"iconPath": "./icons/src.svg"},
                "font": {"fontCharacter": "\\E001"}
            },
            "file": "file", "folder": "src",
            "fileNames": {"Cargo.lock": "lock"},
            "fileExtensions": {"d.ts": "dts", "woff": "font"},
            "languageIds": {"rust": "rust"},
            "folderNames": {"src": "src"}
        }"#;
        std::fs::write(extension.join("theme.json"), theme).unwrap();
        for name in ["file", "rust", "lock", "dts", "src"] {
            std::fs::write(extension.join(format!("icons/{name}.svg")), name).unwrap();
        }
        let offered = find(std::slice::from_ref(&root));
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].label, "Pix");
        let json = PathBuf::from(offered[0].key.strip_prefix(THEME).unwrap());
        let theme = load(&json).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        let svg = |name: &str, dir| theme.svg(name, dir, false);
        assert_eq!(svg("cargo.lock", false), Some("lock"));
        assert_eq!(svg("index.d.ts", false), Some("dts"));
        assert_eq!(svg("main.rs", false), Some("rust"));
        assert_eq!(svg("font.woff", false), Some("file"));
        assert_eq!(svg("readme", false), Some("file"));
        assert_eq!(svg("src", true), Some("src"));
        assert_eq!(theme.svg("lib", true, true), Some("src"));
    }
}

//! What the person chose in Settings, kept in `$XDG_CONFIG_HOME/quill-ide/settings.json`, and
//! Settings' pages, drawn as quill's.

use newen_components::icons;
use newen_components::packs::{self, Offered};
use newen_components::settings::{
    choices, select, setting, setting_button, setting_switch, theme_control, SettingsPage,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const FIRST: &str = "general";
pub const ICON_FOLDER_ADD: &str = "settings:icons:add";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// "system", "light" or "dark".
    pub theme: String,
    pub font_size: u32,
    /// The editor's family, or the system's monospace when empty.
    pub editor_font: String,
    pub vim: bool,
    /// Columns of a tab and of an indent level, and whether Tab and formatting indent with
    /// spaces rather than tabs.
    pub tab_width: u32,
    pub spaces: bool,
    pub wrap: bool,
    pub minimap: bool,
    /// The file icons' pack: `packs::NEWEN`, `packs::MATERIAL`, or a theme's key.
    pub icons: String,
    /// Folders the person added to look for icon themes in, beside the editors' own.
    pub icon_folders: Vec<PathBuf>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            font_size: 13,
            editor_font: String::new(),
            vim: true,
            tab_width: 4,
            spaces: true,
            wrap: false,
            minimap: false,
            icons: packs::MATERIAL.into(),
            icon_folders: vec![],
        }
    }
}

/// Where they are kept.
pub fn path() -> PathBuf {
    let config = newen_files::config_dir().unwrap_or_else(|| ".".into());
    newen_files::named_dir(&config, "newen-ide", "quill-ide").join("settings.json")
}

/// The ones kept, or the defaults when there are none or they can't be read.
pub fn load() -> Prefs {
    read(&std::fs::read_to_string(path()).unwrap_or_default())
}

/// Prefs kept as `text`; quill-ide's own icon pack, as it saved it, is Newen's.
fn read(text: &str) -> Prefs {
    let mut prefs: Prefs = serde_json::from_str(text).unwrap_or_default();
    if prefs.icons == packs::LEGACY_NEWEN {
        prefs.icons = packs::NEWEN.into();
    }
    prefs
}

const SWITCHES: [&str; 4] = ["vim", "wrap", "minimap", "spaces"];

impl Prefs {
    /// A switch flipped, or a choice made: `name` and, for a choice, its value. False when
    /// `name` is not a setting.
    pub fn set(&mut self, name: &str, value: Option<&str>) -> bool {
        match (name, value) {
            ("vim", None) => self.vim = !self.vim,
            ("wrap", None) => self.wrap = !self.wrap,
            ("minimap", None) => self.minimap = !self.minimap,
            ("spaces", None) => self.spaces = !self.spaces,
            ("tab_width", Some(v)) => match v.parse() {
                Ok(width @ 1..=8) => self.tab_width = width,
                _ => return false,
            },
            ("theme", Some(v @ ("system" | "light" | "dark"))) => self.theme = v.into(),
            ("font_size", Some(v)) => match v.parse() {
                Ok(size @ 10..=24) => self.font_size = size,
                _ => return false,
            },
            ("icons", Some(v)) => self.icons = v.into(),
            ("editor_font", Some(v)) => self.editor_font = v.into(),
            _ => return false,
        }
        true
    }

    pub fn is_switch(name: &str) -> bool {
        SWITCHES.contains(&name)
    }
}

/// Settings' pages, with `open` the select whose menu is open, `offered` the icon themes
/// found and `fonts` the monospaced families.
pub fn pages(
    prefs: &Prefs,
    open: Option<&str>,
    offered: &[Offered],
    fonts: &[String],
) -> Vec<SettingsPage> {
    let palette = newen_components::look::palette();
    let pick = |name: &str, options: Vec<(String, String)>, current: &str| {
        select(name, &options, current, open == Some(name))
    };
    let switch = |name: &str, on: bool| setting_switch(name, on, palette);
    let sizes: Vec<(String, String)> = (11..=18)
        .map(|size| (size.to_string(), format!("{size} px")))
        .collect();
    let mut families = choices(&[("", "System monospace")]);
    families.extend(fonts.iter().map(|f| (f.clone(), f.clone())));
    let mut packs = choices(&[
        (packs::NEWEN, "Newen"),
        (packs::MATERIAL, "Material Icon Theme"),
    ]);
    packs.extend(offered.iter().map(|o| (o.key.clone(), o.label.clone())));
    let page = |key: &str, title: &str, icon, groups| SettingsPage {
        key: key.into(),
        title: title.into(),
        icon,
        groups,
    };
    vec![
        page(
            FIRST,
            "General",
            icons::SETTINGS,
            vec![(
                "Appearance".into(),
                vec![setting("Theme", "", theme_control(&prefs.theme))],
            )],
        ),
        page(
            "editor",
            "Editor",
            icons::CODE,
            vec![(
                "Editor".into(),
                vec![
                    setting(
                        "Font",
                        "Any monospaced family installed.",
                        pick("editor_font", families, &prefs.editor_font),
                    ),
                    setting(
                        "Text size",
                        "",
                        pick("font_size", sizes, &prefs.font_size.to_string()),
                    ),
                    setting(
                        "Tab width",
                        "Columns of a tab and of an indent level.",
                        pick(
                            "tab_width",
                            choices(&[("2", "2"), ("4", "4"), ("8", "8")]),
                            &prefs.tab_width.to_string(),
                        ),
                    ),
                    setting(
                        "Indent with spaces",
                        "Tab and formatting put spaces; off, a tab.",
                        switch("spaces", prefs.spaces),
                    ),
                    setting(
                        "Vim keys",
                        "Files open in Normal mode, to read; i writes.",
                        switch("vim", prefs.vim),
                    ),
                    setting(
                        "Wrap long lines",
                        "At the editor's width, instead of scrolling sideways.",
                        switch("wrap", prefs.wrap),
                    ),
                    setting(
                        "Minimap",
                        "An overview of the file beside it.",
                        switch("minimap", prefs.minimap),
                    ),
                ],
            )],
        ),
        page(
            "icons",
            "File icons",
            icons::FOLDER,
            vec![(
                "File icons".into(),
                vec![
                    setting(
                        "Icon theme",
                        "quill's drawings, the Material set, or a VS Code icon theme found \
                         among your editors' extensions.",
                        pick("icons", packs, &prefs.icons),
                    ),
                    setting(
                        "More icon themes",
                        "A folder with an icon theme's extension, or a folder of them.",
                        setting_button(ICON_FOLDER_ADD, "Add folder…"),
                    ),
                ],
            )],
        ),
    ]
}

/// The column's one group.
pub fn groups() -> Vec<(&'static str, Vec<String>)> {
    vec![(
        "Preferences",
        vec![FIRST.into(), "editor".into(), "icons".into()],
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_change_only_to_what_they_can_be_and_survive_a_trip_to_json() {
        let mut prefs = Prefs::default();
        assert!(prefs.set("vim", None) && !prefs.vim);
        assert!(prefs.set("font_size", Some("15")) && prefs.font_size == 15);
        assert!(!prefs.set("font_size", Some("200")));
        assert!(!prefs.set("theme", Some("pink")));
        assert!(prefs.set("icons", Some("theme:/x/theme.json")));
        assert!(prefs.set("tab_width", Some("2")) && prefs.tab_width == 2);
        assert!(!prefs.set("tab_width", Some("0")));
        assert!(prefs.set("spaces", None) && !prefs.spaces);
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&json).unwrap(), prefs);
        // What a later version adds is filled in from the defaults.
        let old = read(r#"{"vim": false}"#);
        assert_eq!((old.vim, old.font_size), (false, 13));
        // quill-ide's own pack is Newen's; nothing kept, the defaults.
        assert_eq!(read(r#"{"icons": "quill"}"#).icons, packs::NEWEN);
        assert_eq!(read(""), Prefs::default());
    }
}

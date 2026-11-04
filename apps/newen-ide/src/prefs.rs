//! What the person chose in Settings, kept in `$XDG_CONFIG_HOME/newen-ide/settings.json`, and
//! Settings' pages, drawn as Newen's.

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
    /// What the editor shows besides the text: line numbers, the caret's row tinted, a
    /// line at each indent level, spaces and tabs marked, and a guide after a column (0, none).
    pub line_numbers: bool,
    pub current_line: bool,
    pub indent_guides: bool,
    pub whitespace: bool,
    pub ruler: u32,
    /// The caret: "line", "block" or "underline", and whether it blinks.
    pub caret: String,
    pub caret_blink: bool,
    /// A bracket or quote typed gets its closing one.
    pub auto_pairs: bool,
    /// What a save does first: the language server formats the file, spaces at the ends of
    /// lines go, the file ends in a line break, and its line endings become "lf" or "crlf"
    /// ("keep" leaves each file's).
    pub format_on_save: bool,
    pub trim_whitespace: bool,
    pub final_newline: bool,
    pub line_ending: String,
    /// When files save themselves: "off", "delay" (a second after the last edit) or "focus"
    /// (on leaving the file or the window).
    pub autosave: String,
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
            line_numbers: true,
            current_line: true,
            indent_guides: true,
            whitespace: false,
            ruler: 0,
            caret: "line".into(),
            caret_blink: true,
            auto_pairs: true,
            format_on_save: false,
            trim_whitespace: false,
            final_newline: false,
            line_ending: "keep".into(),
            autosave: "off".into(),
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

const SWITCHES: [&str; 13] = [
    "vim",
    "wrap",
    "minimap",
    "spaces",
    "line_numbers",
    "current_line",
    "indent_guides",
    "whitespace",
    "caret_blink",
    "auto_pairs",
    "format_on_save",
    "trim_whitespace",
    "final_newline",
];

impl Prefs {
    /// A switch flipped, or a choice made: `name` and, for a choice, its value. False when
    /// `name` is not a setting.
    pub fn set(&mut self, name: &str, value: Option<&str>) -> bool {
        match (name, value) {
            ("vim", None) => self.vim = !self.vim,
            ("wrap", None) => self.wrap = !self.wrap,
            ("minimap", None) => self.minimap = !self.minimap,
            ("spaces", None) => self.spaces = !self.spaces,
            ("line_numbers", None) => self.line_numbers = !self.line_numbers,
            ("current_line", None) => self.current_line = !self.current_line,
            ("indent_guides", None) => self.indent_guides = !self.indent_guides,
            ("whitespace", None) => self.whitespace = !self.whitespace,
            ("caret_blink", None) => self.caret_blink = !self.caret_blink,
            ("auto_pairs", None) => self.auto_pairs = !self.auto_pairs,
            ("format_on_save", None) => self.format_on_save = !self.format_on_save,
            ("trim_whitespace", None) => self.trim_whitespace = !self.trim_whitespace,
            ("final_newline", None) => self.final_newline = !self.final_newline,
            ("ruler", Some(v)) => match v.parse() {
                Ok(column @ (0 | 80 | 100 | 120)) => self.ruler = column,
                _ => return false,
            },
            ("caret", Some(v @ ("line" | "block" | "underline"))) => self.caret = v.into(),
            ("line_ending", Some(v @ ("keep" | "lf" | "crlf"))) => self.line_ending = v.into(),
            ("autosave", Some(v @ ("off" | "delay" | "focus"))) => self.autosave = v.into(),
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
            vec![
                (
                    "Text".into(),
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
                    ],
                ),
                (
                    "Display".into(),
                    vec![
                        setting(
                            "Line numbers",
                            "Each line's number beside it.",
                            switch("line_numbers", prefs.line_numbers),
                        ),
                        setting(
                            "Highlight current line",
                            "A tint behind the caret's line while you write.",
                            switch("current_line", prefs.current_line),
                        ),
                        setting(
                            "Indent guides",
                            "A faint line at each indent level; the caret's block is marked.",
                            switch("indent_guides", prefs.indent_guides),
                        ),
                        setting(
                            "Show whitespace",
                            "A dot for each space and an arrow for each tab.",
                            switch("whitespace", prefs.whitespace),
                        ),
                        setting(
                            "Line length guide",
                            "A thin line after this column, to keep lines short.",
                            pick(
                                "ruler",
                                choices(&[
                                    ("0", "None"),
                                    ("80", "80"),
                                    ("100", "100"),
                                    ("120", "120"),
                                ]),
                                &prefs.ruler.to_string(),
                            ),
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
                ),
                (
                    "Cursor".into(),
                    vec![
                        setting(
                            "Cursor shape",
                            "Vim's Normal mode keeps its block.",
                            pick(
                                "caret",
                                choices(&[
                                    ("line", "Line"),
                                    ("block", "Block"),
                                    ("underline", "Underline"),
                                ]),
                                &prefs.caret,
                            ),
                        ),
                        setting(
                            "Cursor blink",
                            "Off, it stays still while you write.",
                            switch("caret_blink", prefs.caret_blink),
                        ),
                    ],
                ),
                (
                    "Typing".into(),
                    vec![
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
                            "Close brackets and quotes",
                            "Typing an opening one adds its closing one.",
                            switch("auto_pairs", prefs.auto_pairs),
                        ),
                        setting(
                            "Vim keys",
                            "Files open in Normal mode, to read; i writes.",
                            switch("vim", prefs.vim),
                        ),
                    ],
                ),
                (
                    "Saving".into(),
                    vec![
                        setting(
                            "Auto save",
                            "A second after you stop typing, or when you leave the file or \
                             the window.",
                            pick(
                                "autosave",
                                choices(&[
                                    ("off", "Off"),
                                    ("delay", "After a delay"),
                                    ("focus", "On focus change"),
                                ]),
                                &prefs.autosave,
                            ),
                        ),
                        setting(
                            "Format on save",
                            "By the file's language server, when it has one. Not on a save \
                             after a delay.",
                            switch("format_on_save", prefs.format_on_save),
                        ),
                        setting(
                            "Trim trailing whitespace",
                            "Spaces at the ends of lines go; on a save after a delay, not on \
                             the line you are writing.",
                            switch("trim_whitespace", prefs.trim_whitespace),
                        ),
                        setting(
                            "Insert final newline",
                            "The file ends in a line break.",
                            switch("final_newline", prefs.final_newline),
                        ),
                        setting(
                            "Line endings",
                            "What a save leaves: each file's own, or all the same.",
                            pick(
                                "line_ending",
                                choices(&[
                                    ("keep", "Keep each file's"),
                                    ("lf", "LF"),
                                    ("crlf", "CRLF"),
                                ]),
                                &prefs.line_ending,
                            ),
                        ),
                    ],
                ),
            ],
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
                        "Newen's drawings, the Material set, or a VS Code icon theme found \
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
        assert!(prefs.set("whitespace", None) && prefs.whitespace);
        assert!(prefs.set("ruler", Some("100")) && prefs.ruler == 100);
        assert!(!prefs.set("ruler", Some("90")));
        assert!(prefs.set("caret", Some("block")) && prefs.caret == "block");
        assert!(!prefs.set("caret", Some("beam")));
        assert!(prefs.set("autosave", Some("delay")) && prefs.autosave == "delay");
        assert!(!prefs.set("autosave", Some("always")));
        assert!(prefs.set("line_ending", Some("crlf")) && !prefs.set("line_ending", Some("cr")));
        assert!(Prefs::is_switch("format_on_save") && !Prefs::is_switch("autosave"));
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(serde_json::from_str::<Prefs>(&json).unwrap(), prefs);
        // What a later version adds is filled in from the defaults.
        let old = read(r#"{"vim": false}"#);
        assert_eq!((old.vim, old.font_size), (false, 13));
        // Settings from before these keep the editor as it was.
        assert!(old.line_numbers && old.indent_guides && !old.trim_whitespace);
        assert_eq!(
            (old.autosave.as_str(), old.line_ending.as_str()),
            ("off", "keep")
        );
        // quill-ide's own pack is Newen's; nothing kept, the defaults.
        assert_eq!(read(r#"{"icons": "quill"}"#).icons, packs::NEWEN);
        assert_eq!(read(""), Prefs::default());
    }
}

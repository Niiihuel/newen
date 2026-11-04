//! Code as Newen's apps show it: the editor in Newen's inks and rail, a Markdown file rendered
//! in a reading column, and what tells the two apart.

use crate::look;
use limn::{EditorController, Element};
use std::path::Path;

/// The inks code is drawn in, Newen's own (look's `code_*`): types, properties and functions
/// apart, as newen-ide's.
pub fn syntax_colors() -> limn::CodeSyntaxColors {
    limn::CodeSyntaxColors {
        text: look::code_text(),
        keyword: look::code_keyword(),
        string: look::code_string(),
        number: look::code_number(),
        comment: look::code_comment(),
        function: Some(look::code_function()),
        interpolation: Some(look::code_interpolation()),
        types: Some(look::code_type()),
        properties: Some(look::code_property()),
    }
}

/// A diff's tints, markers and bar in Newen's sage and clay.
pub fn diff_colors() -> limn::DiffColors {
    limn::DiffColors {
        add_row: look::diff_add_row(),
        add_word: look::diff_add_word(),
        del_row: look::diff_del_row(),
        del_word: look::diff_del_word(),
        add_bar: look::diff_added(),
        del_bar: look::diff_removed(),
    }
}

pub fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "markdown" | "mdx")
    )
}

/// The language `path` is coloured as, from its extension.
pub fn language(path: &Path) -> limn::CodeLanguage {
    limn::CodeLanguage::from_code_fence(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

/// The editor for `path`: its language, Newen's inks and Newen's rail, arrows and thumb, as
/// every other scroll surface. The host sets the rest (size, keys, what it marks).
pub fn editor(id: impl Into<String>, editor: &EditorController, path: &Path) -> limn::CodeEditor {
    limn::CodeEditor::new(id, editor.clone())
        .palette(look::palette())
        .language(language(path))
        .syntax_colors(syntax_colors())
        .scrollbar_appearance(limn::ScrollbarAppearance::default())
}

/// A diff of `path`: its language, and Newen's inks and tints, as the editor reads it. The
/// host sets the rest (size, words, height).
pub fn diff(id: impl Into<String>, lines: Vec<limn::CodeDiffLine>, path: &Path) -> limn::CodeDiff {
    limn::CodeDiff::new(id, lines)
        .palette(look::palette())
        .language(language(path))
        .colors(diff_colors())
        .syntax_colors(syntax_colors())
}

/// Where a Markdown preview's ids go: itself, its scroll, and the prefixes of its links' and
/// diagrams' actions.
pub struct PreviewIds {
    pub id: &'static str,
    pub scroll: &'static str,
    pub link: &'static str,
    pub diagram: &'static str,
}

/// A Markdown file rendered, in a reading column, as Newen renders its conversation: code in
/// its inks, diagrams drawn (`diagram_sources` shown as source), links that act.
pub fn markdown_preview(
    text: &str,
    ids: PreviewIds,
    diagram_sources: &[u64],
    [width, height]: [f32; 2],
) -> Element {
    let column = (width - 64.0).clamp(240.0, 820.0);
    let rendered = limn::Markdown::new(text, look::palette())
        .id(ids.id)
        .width(column)
        .font_size(15.0)
        .code_font_size(look::SMALL)
        .code_syntax_colors(syntax_colors())
        .link_action_prefix(ids.link)
        .diagram_action_prefix(ids.diagram)
        .diagram_sources(diagram_sources)
        .render()
        .element;
    let page = Element::row(vec![
        Element::spacer().flex(1.0),
        // In a row the column would take its widest line's width, and a quote, which fills
        // what it is given, would never wrap: it gets the reading column.
        Element::column(vec![rendered])
            .padding(24.0)
            .size(Some(column + 48.0), None),
        Element::spacer().flex(1.0),
    ]);
    Element::scroll(ids.scroll, page).size(Some(width), Some(height))
}

//! Code as quill's apps show it: the editor in quill's inks and rail, a Markdown file rendered
//! in a reading column, and what tells the two apart.

use crate::look;
use limn::{EditorController, Element};
use std::path::Path;

/// The inks code is drawn in: types, properties and functions apart, as quill-ide's.
pub fn syntax_colors() -> limn::CodeSyntaxColors {
    limn::CodeSyntaxColors {
        text: look::palette().text,
        string: look::added(),
        keyword: look::keyword(),
        function: Some(look::cursor()),
        interpolation: Some(look::removed()),
        types: Some(look::pro()),
        properties: Some(look::mention()),
        ..limn::CodeSyntaxColors::for_palette(look::palette())
    }
}

pub fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md" | "markdown" | "mdx")
    )
}

/// The editor for `path`: its language, quill's inks and quill's rail, arrows and thumb, as
/// every other scroll surface. The host sets the rest (size, keys, what it marks).
pub fn editor(id: impl Into<String>, editor: &EditorController, path: &Path) -> limn::CodeEditor {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    limn::CodeEditor::new(id, editor.clone())
        .palette(look::palette())
        .language(limn::CodeLanguage::from_code_fence(ext))
        .syntax_colors(syntax_colors())
        .scrollbar_appearance(limn::ScrollbarAppearance::default())
}

/// Where a Markdown preview's ids go: itself, its scroll, and the prefixes of its links' and
/// diagrams' actions.
pub struct PreviewIds {
    pub id: &'static str,
    pub scroll: &'static str,
    pub link: &'static str,
    pub diagram: &'static str,
}

/// A Markdown file rendered, in a reading column, as quill renders its conversation: code in
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

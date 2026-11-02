//! A search as quill's, Desktop's Ctrl+K: a field over what it found in
//! titled sections, the row Enter opens filled and marked ↵, and "Close Esc" under them.

use crate::look::{self, SMALL};
use crate::pieces::{hgap, muted, text};
use limn::{Color, Component, ComponentPalette, Element, Icon};

/// A row of what a search found: what it reports when picked, its words, and what may go
/// before and after them.
pub struct Found {
    pub id: String,
    pub label: String,
    /// Muted words after the label, as a file's folder.
    pub detail: Option<String>,
    pub icon: Option<(Icon, Color)>,
}

impl Found {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: None,
            icon: None,
        }
    }
}

/// The search over `base`: its field `field` holding `query`, then `sections` of rows, the
/// `chosen`-th of all of them the one Enter opens. `close` is reported by Esc, a click outside
/// and the dialog's own close.
pub fn dialog(
    base: Element,
    field: &str,
    close: &str,
    query: &str,
    placeholder: &str,
    sections: Vec<(String, Vec<Found>)>,
    chosen: usize,
) -> Element {
    let palette = look::palette();
    let input = limn::TextField::new(field, query, placeholder, palette)
        .render()
        .size(None, Some(36.0));
    let mut body = vec![input, Element::spacer().size(None, Some(6.0))];
    let heading = |label: String| {
        Element::row(vec![hgap(12.0), muted(label).font_size(SMALL).centered_y()])
            .size(None, Some(28.0))
    };
    let row = |found: Found, first: bool| {
        let mut items = vec![hgap(12.0)];
        if let Some((icon, ink)) = found.icon {
            items.push(
                Element::icon(icon)
                    .foreground(ink)
                    .size(Some(16.0), Some(16.0))
                    .centered_y(),
            );
        }
        items.push(text(found.label).centered_y());
        if let Some(detail) = found.detail {
            items.push(muted(detail).font_size(SMALL).centered_y());
        }
        items.push(Element::spacer().flex(1.0));
        if first {
            items.push(
                Element::icon(crate::icons::of(crate::icons::RETURN))
                    .foreground(look::muted())
                    .size(Some(14.0), Some(14.0))
                    .centered_y(),
            );
        }
        items.push(hgap(12.0));
        let row = ComponentPalette {
            hover: look::row_hover(),
            ..palette
        }
        .interactive_surface(Element::clickable(
            found.id,
            Element::row(items).gap(8.0).size(None, Some(36.0)),
        ))
        .radius(8.0);
        if first {
            row.background(look::selected())
        } else {
            row
        }
    };
    let mut at = 0;
    for (title, rows) in sections {
        body.push(heading(title));
        for found in rows {
            body.push(row(found, at == chosen));
            at += 1;
        }
    }
    let footer = Element::row(vec![
        muted("Close").font_size(SMALL).centered_y(),
        limn::Kbd::new("Esc", palette).render().centered_y(),
    ])
    .gap(6.0);
    limn::Modal::new(base, true, "", "", close, palette)
        .width(672.0)
        .body(Element::column(body).gap(2.0))
        .footer(footer)
        .initial_focus(field)
        .backdrop_dismiss(true)
        .scrim(look::scrim())
        .render()
}

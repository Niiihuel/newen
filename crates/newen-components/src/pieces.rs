//! The small pieces quill's views are built from, for every app of quill to draw the same.

use crate::look::{self, BODY, SMALL};
use limn::{Component, ComponentPalette, Element, FontFamily};

pub fn text(s: impl Into<String>) -> Element {
    Element::text(s.into())
        .font_family(FontFamily::Sans)
        .font_size(BODY)
        .foreground(look::palette().text)
}

pub fn muted(s: impl Into<String>) -> Element {
    text(s).foreground(look::muted())
}

pub fn hgap(width: f32) -> Element {
    Element::spacer().size(Some(width), None)
}

pub fn vline(height: f32) -> Element {
    Element::spacer()
        .background(look::divider())
        .size(Some(1.0), Some(height))
}

pub fn hline(width: f32, color: limn::Color) -> Element {
    Element::spacer()
        .background(color)
        .size(Some(width), Some(1.0))
}

/// An icon button as Desktop draws them: rounded 6, its icon in the text's ink,
/// `hover` under the pointer, an optional fill at rest, and a tooltip with its shortcut.
pub fn tool_surface(
    id: &str,
    icon: limn::Icon,
    label: &str,
    side: f32,
    fill: Option<limn::Color>,
    hover: limn::Color,
    shortcut: Option<&str>,
) -> Element {
    let palette = ComponentPalette {
        hover,
        selected: hover,
        ..look::palette()
    };
    // The icon is centered by hand: limn's icon button sets it a pixel to the left.
    let glyph = Element::icon(icon)
        .foreground(look::palette().text)
        .size(Some(16.0), Some(16.0))
        .centered();
    let button = palette
        .interactive_surface(Element::clickable(id, glyph).accessibility_label(label))
        .radius(6.0)
        .size(Some(side), Some(side));
    let button = match fill {
        Some(fill) => button.background(fill),
        None => button,
    };
    let tip = limn::Tooltip::new(button, id, label, look::palette());
    match shortcut {
        Some(keys) => tip.shortcut(keys),
        None => tip,
    }
    .render()
}

/// An icon button with no fill at rest.
pub fn icon_tool(
    id: &str,
    icon: limn::Icon,
    label: &str,
    side: f32,
    _palette: ComponentPalette,
) -> Element {
    tool_surface(id, icon, label, side, None, look::tool_hover(), None).centered_y()
}

/// A top-bar toggle: its icon on the active fill while what it shows is open.
pub fn toggle(
    id: &str,
    icon: limn::Icon,
    label: &str,
    on: bool,
    shortcut: Option<&str>,
) -> Element {
    let (fill, hover) = if on {
        (Some(look::active()), look::active_hover())
    } else {
        (None, look::tool_hover())
    };
    tool_surface(id, icon, label, 26.0, fill, hover, shortcut).centered_y()
}

/// A button with its keys in caps after the label, Desktop's "Revise… Esc" and
/// "Next Ctrl ⏎": the light fill, or the near-black one when
/// `primary`.
pub fn keyed_button(
    id: &str,
    label: &str,
    keys: &str,
    primary: bool,
    enabled: bool,
    palette: ComponentPalette,
) -> Element {
    let (fill, hover, ink) = if primary {
        (look::palette().text, look::primary_hover(), palette.surface)
    } else {
        (look::cancel(), look::tool_hover(), look::palette().text)
    };
    let ink = if enabled { ink } else { look::muted() };
    let mut content = vec![
        hgap(10.0),
        text(label).font_size(BODY).foreground(ink).centered_y(),
    ];
    if !keys.is_empty() {
        content.push(
            limn::Kbd::new(
                keys,
                ComponentPalette {
                    text: ink,
                    ..palette
                },
            )
            .render()
            .centered_y(),
        );
    }
    content.push(hgap(10.0));
    let row = Element::row(content).gap(8.0).size(None, Some(30.0));
    if !enabled {
        return row.background(look::cancel()).radius(8.0);
    }
    Element::clickable(id, row)
        .background(fill)
        .hover_background(hover)
        .radius(8.0)
}

/// A sidebar row `height` tall, rounded, lit under the pointer and filled while selected.
pub fn side_row(id: String, content: Vec<Element>, selected: bool, height: f32) -> Element {
    let palette = ComponentPalette {
        hover: if selected {
            look::selected_hover()
        } else {
            look::row_hover()
        },
        ..look::palette()
    };
    let row = Element::row(content).gap(6.0).size(None, Some(height));
    let row = palette
        .interactive_surface(Element::clickable(id, row))
        .radius(6.0)
        .hover_region();
    if selected {
        row.background(look::selected())
    } else {
        row
    }
}

/// The measures of quill's menus, `width` wide: Desktop's rows are 28 px with 14 px labels
/// and 13 px captions. Context menus take them too.
pub fn menu_sizing(width: f32) -> limn::SelectorSizing {
    limn::SelectorSizing {
        preferred_width: width,
        min_width: width,
        row_height: 28.0,
        detail_row_height: 44.0,
        header_height: 26.0,
        icon_size: 16.0,
        label_size: BODY,
        detail_size: SMALL,
        padding: 8.0,
        max_visible_items: 12,
        ..limn::SelectorSizing::COMPACT
    }
}

/// A menu in Desktop's style: rounded, 28 px rows, the current one marked by a blue check.
pub fn menu(
    anchor: Element,
    entries: Vec<limn::DropdownMenuEntry>,
    open: bool,
    id: &str,
    width: f32,
    placement: limn::PopoverPlacement,
) -> Element {
    let sizing = menu_sizing(width);
    limn::DropdownMenu::new(
        anchor,
        entries,
        open,
        format!("{id}:dismiss"),
        look::menu_palette(),
    )
    .sizing(sizing)
    // The current row has only Desktop's blue check, no fill (limn #127).
    .check_ink(look::handle())
    .selected_fill(false)
    .placement(placement)
    .render()
}

/// A pane's header: what it shows on the left, its tools on the right, 44 px over a
/// hairline.
pub fn pane_header(lead: Element, tools: Vec<Element>, width: f32) -> Element {
    let mut row = vec![hgap(12.0), lead, Element::spacer().flex(1.0)];
    // At their own height in the middle, a button as much as an icon.
    row.extend(tools.into_iter().map(Element::centered_y));
    row.push(hgap(4.0));
    Element::column(vec![
        Element::row(row)
            .gap(8.0)
            .size(Some(width), Some(look::PANE_HEADER)),
        hline(width, look::pane_border()),
    ])
}

/// A pane's tab, the same in Files, the browser and the terminal: its name, a muted detail
/// when it has one (a terminal's folder), and its close on a 28 px pill as wide as they are,
/// the name up to 200, so a name always reads; the one on screen on the active fill.
pub fn pane_tab(
    id: &str,
    close: &str,
    close_label: &str,
    name: &str,
    detail: Option<&str>,
    active: bool,
) -> Element {
    let mut items = vec![
        hgap(4.0),
        Element::truncated_text(name.to_owned())
            .font_family(FontFamily::Sans)
            .font_size(BODY)
            .foreground(look::palette().text)
            .max_width(200.0)
            .centered_y(),
    ];
    items.extend(detail.map(|detail| muted(detail).centered_y()));
    items.push(icon_tool(
        close,
        crate::icons::of(crate::icons::CLOSE),
        close_label,
        20.0,
        look::palette(),
    ));
    let pill = Element::row(items).gap(6.0).size(None, Some(28.0));
    let pill = Element::clickable(id, pill)
        .accessibility_label(format!("Tab {name}"))
        .radius(8.0);
    if active {
        pill.background(look::active())
    } else {
        pill.hover_background(look::tool_hover())
    }
    .centered_y()
}

/// A pane's header of tabs, the same in Files, the browser and the terminal: `tabs`, each by
/// its id, in a strip as wide as they are up to the room there is, which then scrolls
/// sideways under the wheel with its hidden edge faded and the `active` tab kept in view;
/// `after` it (a new tab's +) always in sight, and `tools`, each 28 wide, at the end.
pub fn tabs_header(
    scroll_id: &str,
    tabs: Vec<(String, Element)>,
    active: Option<usize>,
    after: Option<Element>,
    tools: Vec<Element>,
    width: f32,
) -> Element {
    // What pane_header leaves its lead: its 12 and 4 at the ends, 8 between its children,
    // the tools, and what follows the strip with its gap.
    let n = tools.len() as f32;
    let follows = if after.is_some() { 28.0 + 4.0 } else { 0.0 };
    let room = (width - 16.0 - (3.0 + n) * 8.0 - n * 28.0 - follows).max(0.0);
    let reveal = active.and_then(|at| tabs.get(at)).map(|(id, _)| id.clone());
    let tabs = tabs.into_iter().map(|(_, tab)| tab).collect();
    let strip = Element::horizontal_scroll(
        scroll_id,
        Element::row(tabs)
            .gap(4.0)
            .size(None, Some(look::PANE_HEADER)),
    )
    .horizontal_edge_fade(look::theme().background, 24.0)
    .max_width(room)
    .size(None, Some(look::PANE_HEADER));
    let strip = match reveal {
        Some(id) => strip.reveal_horizontal_item(id),
        None => strip,
    };
    let mut lead = vec![strip];
    lead.extend(after.map(Element::centered_y));
    pane_header(Element::row(lead).gap(4.0), tools, width)
}

/// A question before something that cannot be taken back, as Desktop asks it: its
/// title and words, Cancel and the soft red `confirm`, over `base` while `open`.
pub fn confirm(
    base: Element,
    open: bool,
    title: &str,
    description: &str,
    confirm: &str,
    confirm_id: &str,
    cancel_id: &str,
) -> Element {
    confirm_keeping(
        base,
        open,
        title,
        description,
        confirm,
        confirm_id,
        "Cancel",
        cancel_id,
    )
}

/// A confirmation whose other button says `cancel` rather than "Cancel".
#[allow(clippy::too_many_arguments)]
pub fn confirm_keeping(
    base: Element,
    open: bool,
    title: &str,
    description: &str,
    confirm: &str,
    confirm_id: &str,
    cancel: &str,
    cancel_id: &str,
) -> Element {
    limn::AlertDialog::new(
        base,
        open,
        title,
        description,
        confirm_id,
        cancel_id,
        // Cancel is filled with `raised`: Desktop's light gray.
        ComponentPalette {
            raised: look::cancel(),
            ..look::palette()
        },
    )
    .width(400.0)
    .confirm_label(confirm)
    .cancel_label(cancel)
    .soft_destructive(
        look::danger_fill(),
        look::danger_border(),
        look::danger_ink(),
    )
    .cancel_variant(limn::ButtonVariant::Secondary)
    .description_size(BODY)
    .description_line_height(20.0)
    .closable(false)
    // Desktop's backdrop is lighter, its title closer, its buttons 30 px with 14 px labels.
    .scrim(look::scrim())
    .title_gap(1.0)
    .button_sizing(limn::ButtonSizing {
        height: 30.0,
        font_size: BODY,
        horizontal_padding: 12.0,
        ..limn::ControlSize::Compact.button_sizing()
    })
    .render()
}

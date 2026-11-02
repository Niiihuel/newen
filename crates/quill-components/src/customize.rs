//! Customize's page as Desktop draws its parts (measured dark at 1×): a card that says what a
//! list is for, sections titled with their count, items on an icon's tile with their menu,
//! lists, Discover's cards and a tool's choices. The app says what each holds; these only lay
//! it out.

use crate::icons;
use crate::look::{self, BODY, SMALL};
use crate::pieces::{hgap, menu, muted, text};
use limn::{Component, Element, FontFamily};

/// A button of the header: filled ("Discover") or, `primary`, in the text's ink ("+ Add"),
/// 28 high, radius 6, with an optional icon before its label and chevron after it.
pub fn button(
    id: &str,
    label: &str,
    icon: Option<&'static str>,
    chevron: bool,
    primary: bool,
) -> Element {
    let (fill, ink, hover) = if primary {
        (
            look::palette().text,
            look::palette().surface,
            look::primary_hover(),
        )
    } else {
        (
            look::customize_button(),
            look::palette().text,
            look::tool_hover(),
        )
    };
    let glyph = |name: &'static str| {
        Element::icon(icons::of(name))
            .foreground(ink)
            .size(Some(14.0), Some(14.0))
            .centered_y()
    };
    let mut row = vec![hgap(4.0)];
    row.extend(icon.map(glyph));
    row.push(text(label).font_size(BODY).foreground(ink).centered_y());
    if chevron {
        row.push(glyph(icons::CARET_DOWN));
    }
    row.push(hgap(4.0));
    Element::clickable(id, Element::row(row).gap(6.0).size(None, Some(28.0)))
        .background(fill)
        .hover_background(hover)
        .radius(6.0)
}

/// A button that opens a menu of `entries` under its end, `open` while it shows.
pub fn menu_button(
    trigger: Element,
    id: &str,
    entries: Vec<limn::DropdownMenuEntry>,
    open: bool,
) -> Element {
    menu(
        trigger,
        entries,
        open,
        id,
        280.0,
        limn::PopoverPlacement::BottomEnd,
    )
}

/// The card at the page's top: what it is for, and a button to start (Desktop's "Add your
/// first …"), on the card's fill with radius 12 and 16 around.
pub fn hero(title: &str, words: &str, action: Option<Element>, width: f32) -> Element {
    let mut column = vec![
        text(title)
            .font_family(FontFamily::SansBold)
            .font_size(16.0),
        muted(words).font_size(BODY),
    ];
    if let Some(action) = action {
        column.push(Element::spacer().size(None, Some(6.0)));
        column.push(Element::row(vec![action]));
    }
    Element::column(column)
        .gap(8.0)
        .padding(16.0)
        .size(Some(width), None)
        .background(look::customize_card())
        .radius(12.0)
}

/// A section's title, 16 px, with its count in a pill, and what goes at its end (the
/// project's folder).
pub fn section(
    title: &str,
    count: Option<usize>,
    trailing: Option<Element>,
    width: f32,
) -> Element {
    let mut row = vec![text(title).font_size(16.0).centered_y()];
    if let Some(count) = count {
        row.push(
            Element::row(vec![
                hgap(6.0),
                muted(count.to_string()).font_size(11.0).centered_y(),
                hgap(6.0),
            ])
            .size(None, Some(16.0))
            .background(look::customize_count())
            .radius(8.0)
            .centered_y(),
        );
    }
    row.push(Element::spacer().flex(1.0));
    row.extend(trailing.map(|e| e.centered_y()));
    Element::row(row).gap(8.0).size(Some(width), Some(34.0))
}

/// An agent's heading over what its program lists, quill's own (Desktop has one agent): its
/// logo before its name, 18 px, and the count in a pill, over a hairline.
pub fn agent_heading(logo: &'static str, name: &str, count: Option<usize>, width: f32) -> Element {
    let mut row = vec![
        Element::icon(icons::of(logo))
            .foreground(look::palette().text)
            .size(Some(18.0), Some(18.0))
            .centered_y(),
        text(name)
            .font_family(FontFamily::SansBold)
            .font_size(18.0)
            .centered_y(),
    ];
    if let Some(count) = count {
        row.push(
            Element::row(vec![
                hgap(6.0),
                muted(count.to_string()).font_size(11.0).centered_y(),
                hgap(6.0),
            ])
            .size(None, Some(16.0))
            .background(look::customize_count())
            .radius(8.0)
            .centered_y(),
        );
    }
    Element::column(vec![
        Element::row(row).gap(8.0).size(Some(width), Some(34.0)),
        Element::spacer()
            .background(look::customize_line())
            .size(Some(width), Some(1.0)),
    ])
}

/// The agents an item is active in, at its end: each one's logo, 14 px, with its name for a
/// tooltip. `id` tells the item's marks from another's.
pub fn agent_marks(id: &str, agents: &[(&'static str, String)]) -> Element {
    let marks = agents.iter().enumerate().map(|(at, (logo, name))| {
        let mark = Element::icon(icons::of(logo))
            .foreground(look::muted())
            .size(Some(14.0), Some(14.0))
            .centered_y();
        limn::Tooltip::new(mark, format!("{id}:{at}"), name.as_str(), look::palette()).render()
    });
    Element::row(marks.collect()).gap(6.0)
}

/// How an "All agents" item stands in one agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Added to the agent's sessions.
    On,
    /// The person turned it off for this agent.
    Off,
    /// The agent has one of its own that wins.
    Replaced,
    /// The agent cannot use it.
    Refused,
}

/// Every agent's logo at an item's end, inked by how the item stands there (the text's ink
/// when on, faint when off, amber when replaced, red when refused), with the reason as its
/// tooltip. `id` tells the item's marks from another's.
pub fn agent_standing(id: &str, agents: &[(&'static str, String, Standing)]) -> Element {
    let marks = agents
        .iter()
        .enumerate()
        .map(|(at, (logo, why, standing))| {
            let ink = match standing {
                Standing::On => look::palette().text,
                Standing::Off => look::customize_edge(),
                Standing::Replaced => look::awaiting(),
                Standing::Refused => look::failed(),
            };
            let mark = Element::icon(icons::of(logo))
                .foreground(ink)
                .size(Some(14.0), Some(14.0))
                .centered_y();
            limn::Tooltip::new(mark, format!("{id}:{at}"), why.as_str(), look::palette()).render()
        });
    Element::row(marks.collect()).gap(6.0)
}

/// The folder a section is about, outlined, its icon before its name (Desktop's "work"), 28
/// high as the header's buttons.
pub fn folder_chip(id: &str, name: &str) -> Element {
    let row = Element::row(vec![
        hgap(4.0),
        Element::icon(icons::of(icons::FOLDER))
            .foreground(look::palette().text)
            .size(Some(14.0), Some(14.0))
            .centered_y(),
        text(name).font_size(SMALL).centered_y(),
        hgap(4.0),
    ])
    .gap(6.0)
    .size(None, Some(28.0));
    Element::clickable(id, row)
        .hover_background(look::tool_hover())
        .border(look::customize_edge(), 1.0)
        .radius(8.0)
}

/// One item: its icon on a 36 px tile, its name over "from {who} · {words}" on one line, what
/// goes at its end (a date, a state) and its menu, 63 high over a hairline, as Desktop's.
pub fn item(
    icon: &'static str,
    name: &str,
    words: &str,
    end: Option<Element>,
    more: Option<Element>,
    width: f32,
) -> Element {
    let picture = Element::icon(icons::of(icon))
        .foreground(look::palette().text)
        .size(Some(16.0), Some(16.0));
    item_with(picture, name, words, end, more, width)
}

/// [`item`] with its logo, 24 px on the same tile, in place of the icon. The tile keeps its
/// background, so a dark logo shows in the dark theme too.
pub fn item_logo(
    logo: &limn::RasterImage,
    name: &str,
    words: &str,
    end: Option<Element>,
    more: Option<Element>,
    width: f32,
) -> Element {
    let picture = Element::raster_image(logo.clone()).size(Some(24.0), Some(24.0));
    item_with(picture, name, words, end, more, width)
}

/// The item over `picture`, centered on its 36 px tile.
fn item_with(
    picture: Element,
    name: &str,
    words: &str,
    end: Option<Element>,
    more: Option<Element>,
    width: f32,
) -> Element {
    let tile = Element::row(vec![picture.centered().flex(1.0)])
        .size(Some(36.0), Some(36.0))
        .background(look::customize_tile())
        .radius(8.0)
        .centered_y();
    let copy = Element::column(vec![
        Element::truncated_text(name.to_owned())
            .font_family(FontFamily::Sans)
            .font_size(15.0)
            .foreground(look::palette().text),
        Element::truncated_text(words.to_owned())
            .font_family(FontFamily::Sans)
            .font_size(13.0)
            .foreground(look::muted()),
    ])
    .gap(4.0)
    .flex(1.0)
    .centered_y();
    // The copy 16 px after its tile, as Desktop's.
    let mut row = vec![Element::row(vec![tile, copy]).gap(16.0).flex(1.0)];
    row.extend(end.map(|e| e.centered_y()));
    row.extend(more.map(|e| e.centered_y()));
    Element::column(vec![
        Element::row(row).gap(12.0).size(Some(width), Some(62.0)),
        Element::spacer()
            .background(look::customize_line())
            .size(Some(width), Some(1.0)),
    ])
}

/// An item's "⋯" opening its menu, 28 px square.
pub fn more(id: &str, entries: Vec<limn::DropdownMenuEntry>, open: bool) -> Element {
    let glyph = Element::icon(icons::of(icons::DOTS_HORIZONTAL))
        .foreground(look::palette().text)
        .size(Some(16.0), Some(16.0))
        .centered();
    let trigger = Element::clickable(
        id,
        Element::row(vec![glyph.flex(1.0)]).size(Some(28.0), Some(28.0)),
    )
    .hover_background(look::tool_hover())
    .radius(6.0);
    menu_button(trigger, id, entries, open)
}

/// Something being read: a spinner beside what it waits for.
pub fn loading(id: &str, words: &str) -> Element {
    Element::row(vec![
        limn::Spinner::new(id, look::muted())
            .size(16.0)
            .render()
            .centered_y(),
        muted(words).font_size(BODY).centered_y(),
    ])
    .gap(10.0)
    .size(None, Some(48.0))
}

/// What a section says when it holds nothing: an icon's tile beside muted words, as
/// Desktop's "No plugins installed for this project yet."
pub fn empty(icon: &'static str, words: &str, width: f32) -> Element {
    let tile = Element::row(vec![Element::icon(icons::of(icon))
        .foreground(look::muted())
        .size(Some(16.0), Some(16.0))
        .centered()
        .flex(1.0)])
    .size(Some(36.0), Some(36.0))
    .background(look::customize_tile())
    .radius(8.0)
    .centered_y();
    Element::row(vec![tile, muted(words).font_size(BODY).centered_y()])
        .gap(16.0)
        .size(Some(width), Some(48.0))
}

/// The page's tabs (Skills, Connectors, Plugins) or a list's own (Your connectors, Discover):
/// each its label on the tab's fill when shown, under the pointer's when not; one that cannot
/// be used is muted and says why in its tooltip, and reports nothing.
pub fn tabs(tabs: &[(&str, &str, bool, Option<String>)]) -> Element {
    let tab = |id: &str, label: &str, on: bool, off: &Option<String>| {
        let ink = if off.is_some() {
            look::muted()
        } else {
            look::palette().text
        };
        let row = Element::row(vec![
            hgap(10.0),
            text(label).font_size(BODY).foreground(ink).centered_y(),
            hgap(10.0),
        ])
        .size(None, Some(28.0));
        if let Some(why) = off {
            return limn::Tooltip::new(row, format!("{id}:why"), why.as_str(), look::palette())
                .render();
        }
        let tab = Element::clickable(id, row).radius(8.0);
        if on {
            tab.background(look::customize_tab())
                .border(look::customize_tab_edge(), 1.0)
        } else {
            tab.hover_background(look::tool_hover())
        }
    };
    Element::row(
        tabs.iter()
            .map(|(id, label, on, off)| tab(id, label, *on, off))
            .collect(),
    )
    .gap(4.0)
}

/// The width of a list's Status column, as Desktop's.
pub const STATUS_WIDTH: f32 = 160.0;
/// The width of its Type column, which a list narrower than [`TYPED_FROM`] leaves out.
pub const TYPE_WIDTH: f32 = 120.0;
pub const TYPED_FROM: f32 = 560.0;

/// A list's column titles: the first takes what the others leave, muted and small, over a
/// hairline.
pub fn table_head(first: &str, kind: &str, status: &str, width: f32) -> Element {
    let cell = |words: &str, w: Option<f32>| {
        let cell = muted(words).font_size(SMALL).centered_y();
        match w {
            Some(w) => Element::row(vec![cell]).size(Some(w), Some(32.0)),
            None => Element::row(vec![cell]).size(None, Some(32.0)).flex(1.0),
        }
    };
    let mut row = vec![cell(first, None)];
    if width >= TYPED_FROM {
        row.push(cell(kind, Some(TYPE_WIDTH)));
    }
    row.extend([cell(status, Some(STATUS_WIDTH)), hgap(28.0)]);
    Element::column(vec![
        Element::row(row).gap(12.0).size(Some(width), Some(32.0)),
        Element::spacer()
            .background(look::customize_line())
            .size(Some(width), Some(1.0)),
    ])
}

/// One row of a list, 44 high over a hairline: `picture` (a logo or an icon, 16) on a 24 tile
/// and the name, which opens it (`open`); its type; its status; and what goes at the end (its
/// "⋯").
pub fn table_row(
    open: &str,
    picture: Element,
    name: &str,
    kind: &str,
    status: Element,
    end: Option<Element>,
    width: f32,
) -> Element {
    let tile = Element::row(vec![picture.centered().flex(1.0)])
        .size(Some(24.0), Some(24.0))
        .background(look::customize_tile())
        .radius(6.0)
        .centered_y();
    let name = Element::clickable(
        open,
        Element::row(vec![
            tile,
            Element::truncated_text(name.to_owned())
                .font_family(FontFamily::Sans)
                .font_size(BODY)
                .foreground(look::palette().text)
                .centered_y()
                .flex(1.0),
        ])
        .gap(10.0)
        .size(None, Some(44.0)),
    )
    .accessibility_label(format!("Open {name}"))
    .flex(1.0);
    let end = end.unwrap_or_else(|| hgap(28.0));
    let mut row = vec![name];
    if width >= TYPED_FROM {
        row.push(
            Element::row(vec![muted(kind).font_size(SMALL).centered_y()])
                .size(Some(TYPE_WIDTH), Some(44.0)),
        );
    }
    row.extend([
        Element::row(vec![status.centered_y()]).size(Some(STATUS_WIDTH), Some(44.0)),
        end.centered_y(),
    ]);
    Element::column(vec![
        Element::row(row).gap(12.0).size(Some(width), Some(44.0)),
        Element::spacer()
            .background(look::customize_line())
            .size(Some(width), Some(1.0)),
    ])
}

/// A status in a list: its icon, 14, beside its words, both in `ink`.
pub fn status(icon: &'static str, words: &str, ink: limn::Color) -> Element {
    Element::row(vec![
        Element::icon(icons::of(icon))
            .foreground(ink)
            .size(Some(14.0), Some(14.0))
            .centered_y(),
        Element::truncated_text(words.to_owned())
            .font_size(SMALL)
            .foreground(ink)
            .centered_y(),
    ])
    .gap(6.0)
}

/// The strip over a list when some of it needs the person, all amber: a warning, what is
/// wrong, and the action that mends it all (`action`: its id and label), on a translucent
/// amber fill inside an amber edge, the words in the amber that reads on either theme.
pub fn attention(words: &str, action: (&str, &str), width: f32) -> Element {
    let ink = look::manual();
    let amber = |a| limn::Color {
        a,
        ..look::awaiting()
    };
    let glyph = |name: &'static str| {
        Element::icon(icons::of(name))
            .foreground(ink)
            .size(Some(14.0), Some(14.0))
            .centered_y()
    };
    let (id, label) = action;
    let button = Element::clickable(
        id,
        Element::row(vec![
            hgap(4.0),
            glyph(icons::RELOAD),
            text(label).font_size(BODY).foreground(ink).centered_y(),
            hgap(4.0),
        ])
        .gap(6.0)
        .size(None, Some(28.0)),
    )
    .border(look::awaiting(), 1.0)
    .hover_background(amber(64))
    .radius(6.0);
    Element::row(vec![
        Element::icon(icons::of(icons::WARNING))
            .foreground(ink)
            .size(Some(16.0), Some(16.0))
            .centered_y(),
        text(words)
            .font_size(BODY)
            .foreground(ink)
            .centered_y()
            .flex(1.0),
        button.centered_y(),
    ])
    .gap(10.0)
    .padding(12.0)
    .size(Some(width), None)
    .background(amber(36))
    .border(look::awaiting(), 1.0)
    .radius(10.0)
}

/// A card of Discover, as Desktop's: radius `tile` (16), 16 in, 12 between its parts; a 40
/// tile with `picture`; the name in 14 medium on up to two lines, an 8 px dot after it when
/// there is an update; `meta` ("author • 12k installs") and `words` in 12 muted, two lines at
/// most; and `action`, a ghost icon button, at its top end.
#[allow(clippy::too_many_arguments)]
pub fn discover_card(
    picture: Element,
    name: &str,
    meta: &str,
    words: &str,
    update: bool,
    action: Element,
    width: f32,
) -> Element {
    let tile = Element::row(vec![picture.centered().flex(1.0)])
        .size(Some(40.0), Some(40.0))
        .background(look::customize_tile())
        .radius(10.0);
    let mut title = vec![Element::text(name.to_owned())
        .font_family(FontFamily::SansBold)
        .font_size(BODY)
        .foreground(look::palette().text)
        .max_lines(2)
        .flex(1.0)];
    if update {
        title.push(
            Element::spacer()
                .size(Some(8.0), Some(8.0))
                .background(look::handle())
                .radius(4.0),
        );
    }
    let mut copy = vec![Element::row(title).gap(6.0)];
    if !meta.is_empty() {
        copy.push(
            Element::truncated_text(meta.to_owned())
                .font_size(12.0)
                .foreground(look::muted()),
        );
    }
    let head = Element::row(vec![tile, Element::column(copy).gap(2.0).flex(1.0), action]).gap(12.0);
    let mut column = vec![head];
    if !words.is_empty() {
        column.push(
            Element::text(words.to_owned())
                .font_size(12.0)
                .foreground(look::muted())
                .max_lines(2),
        );
    }
    Element::column(column)
        .gap(12.0)
        .padding(16.0)
        .size(Some(width), Some(132.0))
        .background(look::customize_card())
        .border(look::customize_line(), 1.0)
        .hover_background(look::customize_card())
        .radius(limn::RadiusRole::Tile.px())
}

/// One fact of a detail page: its icon (12) on a 20 tile, then its words, small and muted, and
/// an (i) whose tooltip says more when there is more.
pub fn fact(id: &str, icon: &'static str, words: &str, more: Option<&str>) -> Element {
    let tile = Element::row(vec![Element::icon(icons::of(icon))
        .foreground(look::muted())
        .size(Some(12.0), Some(12.0))
        .centered()
        .flex(1.0)])
    .size(Some(20.0), Some(20.0))
    .background(look::customize_tile())
    .radius(5.0)
    .centered_y();
    let mut row = vec![tile, muted(words).font_size(SMALL).centered_y()];
    if let Some(more) = more {
        let info = Element::icon(icons::of(icons::INFO))
            .foreground(look::muted())
            .size(Some(12.0), Some(12.0))
            .centered_y();
        row.push(limn::Tooltip::new(info, format!("{id}:info"), more, look::palette()).render());
    }
    Element::row(row).gap(8.0).size(None, Some(24.0))
}

/// The three choices of when a tool may be used (Always allow, Needs approval, Blocked), each
/// an icon with its name for a tooltip, the chosen one on the tab's fill; a choice the agent
/// cannot say is muted and reports nothing, its tooltip saying why. `ids` and `chosen` are in
/// that order.
pub fn policy_control(ids: [&str; 3], chosen: Option<usize>, off: [Option<&str>; 3]) -> Element {
    let names = ["Always allow", "Needs approval", "Blocked"];
    let glyphs = [icons::CHECK_CIRCLE, icons::HAND, icons::BLOCKED];
    let cells = (0..3).map(|at| {
        let ink = if off[at].is_some() {
            look::customize_edge()
        } else {
            look::palette().text
        };
        let glyph = Element::icon(icons::of(glyphs[at]))
            .foreground(ink)
            .size(Some(14.0), Some(14.0))
            .centered();
        let cell = Element::row(vec![glyph.flex(1.0)]).size(Some(28.0), Some(24.0));
        let (cell, tip) = match off[at] {
            Some(why) => (cell, format!("{}: {why}", names[at])),
            None => {
                let cell = Element::clickable(ids[at], cell)
                    .accessibility_label(names[at])
                    .radius(6.0)
                    .hover_background(look::tool_hover());
                let cell = if chosen == Some(at) {
                    cell.background(look::customize_tab())
                        .border(look::customize_tab_edge(), 1.0)
                } else {
                    cell
                };
                (cell, names[at].to_owned())
            }
        };
        limn::Tooltip::new(cell, format!("{}:tip", ids[at]), tip, look::palette()).render()
    });
    Element::row(cells.collect())
        .gap(2.0)
        .padding(2.0)
        .background(look::customize_card())
        .radius(8.0)
}

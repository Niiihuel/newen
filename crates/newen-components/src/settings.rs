//! Settings as Newen draws them, Desktop's: a modal with a column of
//! pages under a search, and each page's settings in titled groups of rows. The app says
//! which pages there are and what each setting's control is.

use crate::look::{self, BODY, SMALL};
use crate::pieces::{hgap, hline, muted, text};
use limn::{Component, ComponentPalette, Element, FontFamily, Size};

pub const SETTINGS_OPEN: &str = "settings:open";
pub const SETTINGS_CLOSE: &str = "settings:close";
pub const SETTINGS_SEARCH: &str = "settings:search";

/// One setting: what it is called, what it does, and its control.
pub struct Setting {
    pub title: String,
    pub about: Option<String>,
    pub control: Element,
    /// A block under the row, as wide as the page (a chart): the row is its heading.
    pub below: Option<Element>,
}

pub fn setting(title: &str, about: &str, control: Element) -> Setting {
    Setting {
        title: title.to_owned(),
        about: (!about.is_empty()).then(|| about.to_owned()),
        control,
        below: None,
    }
}

impl Setting {
    /// The same row with `block` under it.
    pub fn below(mut self, block: Element) -> Self {
        self.below = Some(block);
        self
    }
}

/// A page of Settings: its key, its name, and its settings in titled groups.
pub struct SettingsPage {
    pub key: String,
    pub title: String,
    pub icon: &'static str,
    pub groups: Vec<(String, Vec<Setting>)>,
}

/// A select of Settings: its choice and a chevron at the end of a row, as Desktop's, opening
/// Newen's menu. `set:{name}` opens it and each choice reports `set:{name}={value}`.
pub fn select(name: &str, choices: &[(String, String)], current: &str, open: bool) -> Element {
    let label = choices
        .iter()
        .find(|(value, _)| value == current)
        .map_or_else(|| current.to_owned(), |(_, label)| label.clone());
    let entries = choices
        .iter()
        .map(|(value, label)| {
            limn::DropdownMenuItem::new(format!("set:{name}={value}"), label.clone())
                .selected(value == current)
                .into()
        })
        .collect();
    let chevron = Element::icon(crate::icons::of(crate::icons::CARET_DOWN))
        .foreground(look::muted())
        .size(Some(14.0), Some(14.0))
        .centered_y();
    let trigger = Element::row(vec![
        hgap(8.0),
        text(label).font_size(BODY).centered_y(),
        chevron,
        hgap(6.0),
    ])
    .gap(6.0)
    .size(None, Some(30.0));
    let trigger = ComponentPalette {
        hover: look::tool_hover(),
        ..look::palette()
    }
    .interactive_surface(Element::clickable(format!("set:{name}"), trigger))
    .radius(8.0);
    let trigger = if open {
        trigger.background(look::active())
    } else {
        trigger
    };
    crate::pieces::menu(
        trigger,
        entries,
        open,
        &format!("set:{name}"),
        260.0,
        limn::PopoverPlacement::BottomEnd,
    )
}

/// Desktop's theme setting, its three icons on a track: each reports
/// `set:theme=system|light|dark`; `current` is the one on.
pub fn theme_control(current: &str) -> Element {
    use crate::icons;
    let tabs = [
        ("system", icons::MONITOR),
        ("light", icons::SUN),
        ("dark", icons::MOON),
    ]
    .into_iter()
    .map(|(value, icon)| {
        limn::Tab::new(format!("set:theme={value}"), "", 26.0)
            .icon(icons::of(icon))
            .active(current == value)
    })
    .collect();
    limn::SegmentedControl::new(
        tabs,
        ComponentPalette {
            surface: look::settings_track(),
            selected: look::settings_chip(),
            ..look::palette()
        },
    )
    .selection_border(look::settings_chip_edge())
    .render()
}

pub fn choices(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(value, label)| ((*value).to_owned(), (*label).to_owned()))
        .collect()
}

/// Desktop's switch: white on its blue when on, on a pale gray track when off.
pub fn setting_switch(name: &str, on: bool, palette: ComponentPalette) -> Element {
    limn::Switch::new(format!("set:{name}"), on, palette)
        .variant(limn::SwitchVariant::Accent)
        .ink(look::switch_on())
        .off_ink(look::switch_off())
        .render()
}

/// A button in a settings row, as Desktop's "Manage": 28 high, filled, radius 8.
pub fn setting_button(id: &str, label: &str) -> Element {
    ComponentPalette {
        hover: look::settings_current(),
        ..look::palette()
    }
    .interactive_surface(Element::clickable(
        id,
        Element::row(vec![
            hgap(10.0),
            text(label).font_size(BODY).centered_y(),
            hgap(10.0),
        ])
        .size(None, Some(28.0)),
    ))
    .background(look::settings_button())
    .radius(8.0)
    .centered_y()
}

/// A page that is not among Settings' pages (one that went away): an empty one.
fn unreachable_page(key: &str) -> SettingsPage {
    SettingsPage {
        key: key.to_owned(),
        title: "Settings".into(),
        icon: crate::icons::SETTINGS,
        groups: vec![],
    }
}

const NAV_WIDTH: f32 = 211.0;

/// Settings over `base`, on the page `current`, or what `query` finds on every page. The
/// column lists `pages` under each of `groups`' headings, by key; a page reports
/// `settings:page:{key}` when picked. `failed` is why the last change was not kept, said
/// over the page in Desktop's words.
#[allow(clippy::too_many_arguments)]
pub fn dialog(
    base: Element,
    pages: Vec<SettingsPage>,
    groups: Vec<(&str, Vec<String>)>,
    current: &str,
    query: &str,
    failed: Option<&str>,
    window: Size,
) -> Element {
    let palette = look::palette();
    let width = limn::Modal::fit_width(window.width, 1024.0);
    let height = (window.height - 72.0).clamp(320.0, 772.0);
    // Inside the card's outline, under its 44-high title and hairline.
    let body_height = height - 45.0 - 2.0;
    let nav_width = NAV_WIDTH;
    let inner = nav_width - 32.0;
    // The column: the search, then each group's pages.
    let search = limn::SearchField::new(
        SETTINGS_SEARCH,
        query,
        "Search",
        ComponentPalette {
            surface: look::settings_field(),
            raised: look::settings_field(),
            border: look::settings_field_edge(),
            ..palette
        },
    )
    .size(limn::ControlSize::Compact)
    .width(inner)
    .render();
    let mut nav = vec![Element::spacer().size(None, Some(16.0)), search];
    let heading = |label: &str| {
        Element::column(vec![
            Element::spacer().size(None, Some(18.0)),
            muted(label.to_owned()).font_size(SMALL),
            Element::spacer().size(None, Some(8.0)),
        ])
    };
    let searching = !query.trim().is_empty();
    let link = |page: &SettingsPage| {
        let row = ComponentPalette {
            hover: look::row_hover(),
            ..palette
        }
        .interactive_surface(Element::clickable(
            format!("settings:page:{}", page.key),
            // Desktop's: the icon 9 in, its label 15 after it, at 14 px.
            Element::row(vec![
                hgap(9.0),
                Element::icon(crate::icons::of(page.icon))
                    .foreground(look::palette().text)
                    .size(Some(16.0), Some(16.0))
                    .centered_y(),
                hgap(15.0),
                Element::truncated_text(page.title.clone())
                    .font_family(FontFamily::Sans)
                    .font_size(BODY)
                    .foreground(look::palette().text)
                    .flex(1.0)
                    .centered_y(),
            ])
            .size(Some(inner - 10.0), Some(32.0)),
        ))
        .radius(8.0);
        if current == page.key && !searching {
            row.background(look::settings_current())
        } else {
            row
        }
    };
    for (label, keys) in groups {
        nav.push(heading(label));
        for page in pages.iter().filter(|p| keys.contains(&p.key)) {
            nav.push(link(page));
        }
    }
    let nav = Element::row(vec![
        Element::row(vec![hgap(16.0), Element::column(nav).gap(1.0), hgap(16.0)])
            .size(Some(nav_width), Some(body_height))
            .background(look::settings_nav()),
        Element::spacer()
            .background(look::settings_nav_edge())
            .size(Some(1.0), Some(body_height)),
    ]);
    // The page: its sections, or what the search found on every page.
    let content_width = width - 2.0 - nav_width - 1.0;
    let rows_width = content_width - 6.0 - 16.0;
    let section = |title: String, rows: Vec<Setting>| {
        // The rows keep their own 10 px inset; the title takes it too, as Desktop aligns them.
        let mut items = vec![
            Element::row(vec![hgap(10.0), text(title).font_size(15.0)]),
            Element::spacer().size(None, Some(19.0)),
        ];
        let count = rows.len();
        for (at, row) in rows.into_iter().enumerate() {
            // Desktop's at 1×: 14 px words, a description's lines 20 apart.
            let mut action = limn::ActionRow::new(row.title, palette)
                .title_size(BODY)
                .subtitle_size(BODY)
                .subtitle_line_height(20.0)
                .sizing(limn::ActionRowSizing {
                    gap: 24.0,
                    copy_gap: 6.0,
                    icon_column: 0.0,
                    min_height: 52.0,
                    min_height_with_subtitle: 68.0,
                })
                .wrap_title(true);
            if let Some(about) = row.about {
                action = action.subtitle(about);
            }
            items.push(
                action
                    .trailing(row.control)
                    .render()
                    .size(Some(rows_width), None),
            );
            if let Some(block) = row.below {
                items.push(Element::row(vec![hgap(10.0), block]));
                items.push(Element::spacer().size(None, Some(12.0)));
            }
            // The hairline runs under the copy and the control, inside the rows' inset.
            if at + 1 < count {
                items.push(Element::row(vec![
                    hgap(10.0),
                    hline(rows_width - 20.0, look::settings_row_line()),
                ]));
            }
        }
        Element::column(items)
    };
    let query = query.trim().to_lowercase();
    let mut content = vec![];
    if query.is_empty() {
        let page = pages
            .into_iter()
            .find(|p| p.key == current)
            .unwrap_or_else(|| unreachable_page(current));
        for (title, rows) in page.groups {
            content.push(section(title, rows));
        }
    } else {
        for page in pages {
            for (title, rows) in page.groups {
                let found: Vec<Setting> = rows
                    .into_iter()
                    .filter(|r| {
                        r.title.to_lowercase().contains(&query)
                            || r.about
                                .as_ref()
                                .is_some_and(|a| a.to_lowercase().contains(&query))
                    })
                    .collect();
                if !found.is_empty() {
                    content.push(section(format!("{} \u{203a} {title}", page.title), found));
                }
            }
        }
        if content.is_empty() {
            content.push(muted("No settings match your search.").font_size(15.0));
        }
    }
    // Desktop's words when a setting is not kept, over the page.
    if let Some(error) = failed {
        content.insert(
            0,
            Element::column(vec![
                text("Couldn\u{2019}t save the setting. Try again.")
                    .font_size(BODY)
                    .foreground(look::failed()),
                muted(error.to_owned()).font_size(BODY),
            ])
            .gap(4.0)
            .padding(10.0),
        );
    }
    // Each page keeps its own place, and opens at its top.
    let (inset, top) = (6.0, 18.0);
    let content = Element::scroll(
        format!("settings:content:{current}"),
        Element::row(vec![
            hgap(inset),
            Element::column(vec![
                Element::spacer().size(None, Some(top)),
                Element::column(content).gap(40.0),
                Element::spacer().size(None, Some(24.0)),
            ]),
        ]),
    )
    .size(Some(content_width), Some(body_height))
    .background(look::settings_page());
    let body = Element::row(vec![nav, content]).size(Some(width - 2.0), Some(body_height));
    limn::Modal::new(
        base,
        true,
        "Settings",
        "",
        SETTINGS_CLOSE,
        ComponentPalette {
            surface: look::settings_page(),
            border: look::settings_title_line(),
            ..palette
        },
    )
    .width(width)
    .height(height)
    .outline(look::settings_outline())
    .flush_body(true)
    .body(body)
    .render()
}

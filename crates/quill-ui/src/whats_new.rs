//! What's new (docs/UPDATES.md): a release's announcement in a card that opens on its picture,
//! before restarting into it (from the sidebar's strip) and once after; the "What's new"
//! button in the strip's place until this version's notes are opened; and the notes dialog,
//! every version quill kept, newest first. The engine downloads, matches and keeps them; this
//! shows what it reports. A clip shows its picture until limn plays video.

use crate::{updates, view, Store, Ui};
use limn::{Component, ComponentPalette, Element, FontFamily, Modal, Size, UiAction};
use quill_components::look::{self, BODY, SMALL};
use quill_components::pieces::{hgap, keyed_button, muted, text};
use quill_model::{Announcement, Event, Intent, Media, ReleaseNotes};
use std::path::PathBuf;

/// The card's close, ✕ and Escape.
pub const CARD: &str = "whats_new";
const CARD_CLOSE: &str = "whats_new:close";
/// Before the restart: Later keeps the strip; Restart and update asks first if agents work.
pub const LATER: &str = "whats_new:later";
pub const RESTART: &str = "whats_new:restart";
/// After it: Got it, and Try it when the announcement names somewhere to go.
pub const GOT_IT: &str = "whats_new:got_it";
pub const TRY_IT: &str = "whats_new:try";
/// The sidebar's "What's new", and the account menu's entry.
pub const BUTTON: &str = "whats_new:button";
pub const OPEN: &str = "whats_new:open";
/// The notes dialog's close.
pub const NOTES: &str = "whats_new:notes";

/// Keeps what the engine reports: an announcement it says to show opens now.
pub(crate) fn heard(ui: &mut Ui, event: &Event) {
    let Event::WhatsNew(whats_new) = event else {
        return;
    };
    if whats_new.announce {
        ui.announcing = whats_new.releases.first().map(|r| r.version.clone());
    }
    ui.whats_new = whats_new.clone();
}

/// The notes of the version the card announces: the downloaded one, or one installed.
fn announced(ui: &Ui) -> Option<(&ReleaseNotes, &Announcement)> {
    let version = ui.announcing.as_deref()?;
    let ready = ui.whats_new.ready.iter();
    let notes = (ready.chain(&ui.whats_new.releases)).find(|r| r.version == version)?;
    Some((notes, notes.announcement.as_ref()?))
}

/// The strip was clicked: the ready version's announcement opens first, when it has one and
/// Settings allow it; `None` restarts as before.
pub(crate) fn before_restart(ui: &mut Ui) -> Option<Vec<Intent>> {
    let version = updates::ready(ui)?.to_owned();
    let ready = ui.whats_new.ready.as_ref()?;
    if !ui.settings.whats_new || ready.version != version || ready.announcement.is_none() {
        return None;
    }
    ui.announcing = Some(version);
    Some(vec![])
}

/// The controls "Try it" may go to, by the name an announcement gives: a pane of the
/// session in view, a Settings page, or the tour. Anything else shows no button.
fn destination(ui: &Ui, to: &str) -> Option<Vec<String>> {
    let pane = match to {
        "browser" => Some((crate::Pane::Browser, view::BROWSER_TOGGLE)),
        "terminal" => Some((crate::Pane::Terminal, view::TERMINAL_TOGGLE)),
        "files" => Some((crate::Pane::Files, view::FILES_TOGGLE)),
        "diff" => Some((crate::Pane::Diff, view::DIFF_TOGGLE)),
        _ => None,
    };
    if let Some((pane, toggle)) = pane {
        ui.selected?;
        // Already open: there is nothing to toggle, it is in view.
        return Some(if ui.pane_open(pane) {
            vec![]
        } else {
            vec![toggle.to_owned()]
        });
    }
    if to == "tour" {
        return Some(vec![crate::tour::START.to_owned()]);
    }
    let page = to.strip_prefix("settings:")?;
    let pages = [
        view::SETTINGS_FIRST,
        "system",
        view::SETTINGS_ACCOUNTS,
        view::SETTINGS_SKILLS,
        view::SETTINGS_PLUGINS,
        view::SETTINGS_CONNECTORS,
    ];
    pages.contains(&page).then(|| {
        vec![
            view::SETTINGS_OPEN.to_owned(),
            format!("settings:page:{page}"),
        ]
    })
}

/// The clicks of this module, or `None` for another's.
pub(crate) fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    match id {
        CARD | LATER | GOT_IT => {
            ui.announcing = None;
            Some(vec![])
        }
        RESTART => {
            ui.announcing = None;
            Some(updates::restart(store, ui))
        }
        TRY_IT => {
            let to = announced(ui).and_then(|(_, a)| a.try_it.clone());
            ui.announcing = None;
            let clicks = to.and_then(|to| destination(ui, &to)).unwrap_or_default();
            Some(clicks.iter().flat_map(|id| click(store, ui, id)).collect())
        }
        BUTTON | OPEN => {
            ui.account_menu = false;
            ui.notes_open = true;
            Some(if ui.whats_new.unopened {
                vec![Intent::WhatsNewOpened]
            } else {
                vec![]
            })
        }
        NOTES => {
            ui.notes_open = false;
            Some(vec![])
        }
        _ => None,
    }
}

fn click(store: &Store, ui: &mut Ui, id: &str) -> Vec<Intent> {
    crate::update(store, ui, &UiAction::Clicked { id: id.to_owned() })
}

/// The pictures the card shows, for the host to read at their size off the window's thread.
pub fn media(ui: &Ui) -> Vec<PathBuf> {
    let media = announced(ui).and_then(|(_, a)| a.media.as_ref());
    media
        .map(|media| picture(media).clone())
        .into_iter()
        .collect()
}

/// The picture a medium shows: an image's own; a clip's poster.
// shortcut: a clip shows its poster, as limn has no video element yet; upgrade with limn's
// video component (limn feat/video) to play `clip` muted, looped, still under reduced motion.
fn picture(media: &Media) -> &PathBuf {
    match media {
        Media::Image { path } => path,
        Media::Video { poster, .. } => poster,
    }
}

/// The sidebar's "What's new · v0.2.0", with an accent dot, in the strip's place while no
/// version waits, until this version's notes are opened.
pub(crate) fn sidebar_button(ui: &Ui, palette: ComponentPalette) -> Option<Element> {
    let notes = ui.whats_new.releases.first()?;
    if !ui.whats_new.unopened || updates::ready(ui).is_some() {
        return None;
    }
    let dot = Element::spacer()
        .background(look::handle())
        .radius(3.0)
        .size(Some(6.0), Some(6.0))
        .centered_y();
    let row = Element::row(vec![
        hgap(5.0),
        dot,
        hgap(1.0),
        text("What's new").foreground(look::handle()).centered_y(),
        Element::spacer().flex(1.0),
        muted(format!("v{}", notes.version))
            .font_size(12.0)
            .centered_y(),
        hgap(0.0),
    ])
    .gap(10.0)
    .size(None, Some(32.0));
    let surface = ComponentPalette {
        hover: look::tool_hover(),
        ..palette
    }
    .interactive_surface(Element::clickable(BUTTON, row))
    .radius(8.0);
    Some(Element::column(vec![
        Element::spacer().size(None, Some(8.0)),
        Element::row(vec![hgap(8.0), surface.flex(1.0), hgap(8.0)]),
    ]))
}

/// The card's widest, and the words' margin inside it.
const CARD_WIDTH: f32 = 640.0;
const MARGIN: f32 = 20.0;
/// What the words and buttons take at most under the picture: a two-line title, the body,
/// the points and the buttons. Without that much room left, the picture goes.
const WORDS: f32 = 300.0;

/// The announcement card and the notes dialog over `base`.
pub(crate) fn dialogs(ui: &Ui, base: Element, window: Size, palette: ComponentPalette) -> Element {
    let base = notes_dialog(ui, base, window, palette);
    let Some((notes, announcement)) = announced(ui) else {
        return base;
    };
    let width = Modal::fit_width(window.width, CARD_WIDTH);
    let inner = width - 2.0 - 2.0 * MARGIN;
    let tall = (width - 2.0) * 9.0 / 16.0;
    let room = window.height - 48.0 - tall >= WORDS;
    let close = limn::DismissButton::new(CARD_CLOSE, palette)
        .dispatch_as(CARD)
        .target_size(28.0)
        .render();
    let surface = look::palette().surface;
    let close = Element::row(vec![close])
        // The surface at 80 %, so the picture shows through.
        .background(limn::Color { a: 204, ..surface })
        .border(look::divider(), 1.0)
        .radius(14.0);
    let mut parts = vec![];
    let mut words = vec![];
    match announcement.media.as_ref().filter(|_| room) {
        Some(media) => {
            let shown = match ui.full.get(picture(media)) {
                Some(image) => Element::raster_image(image.clone().cover())
                    .accessibility_label(announcement.title.clone()),
                // While it is read: the raised fill.
                None => Element::spacer().background(look::cancel()),
            };
            let corner = Element::column(vec![
                Element::spacer().size(None, Some(16.0)),
                Element::row(vec![Element::spacer().flex(1.0), close, hgap(16.0)]),
            ]);
            parts.push(Element::stack(vec![
                shown.size(Some(width - 2.0), Some(tall)),
                corner,
            ]));
        }
        None => words.push(Element::row(vec![Element::spacer().flex(1.0), close])),
    }
    words.push(
        text(announcement.title.clone())
            .font_family(FontFamily::SansBold)
            .font_size(24.0)
            .size(Some(inner), None),
    );
    if !announcement.body.trim().is_empty() {
        words.push(markdown(&announcement.body, inner, palette));
    }
    if !announcement.bullets.is_empty() {
        let points: Vec<String> = (announcement.bullets.iter())
            .map(|point| format!("- {point}"))
            .collect();
        words.push(markdown(&points.join("\n"), inner, palette));
    }
    // The words scroll in what the window leaves under the picture and over the buttons.
    let words_room = window.height - 48.0 - if parts.is_empty() { 0.0 } else { tall } - 64.0;
    parts.push(
        limn::ScrollArea::new(
            "whats_new:words",
            Element::column(words).gap(12.0).padding(MARGIN),
        )
        // Over the margin, so the words keep their width.
        .appearance(limn::ScrollbarAppearance::overlay())
        .render()
        .max_height(words_room.max(96.0)),
    );
    let ready = updates::ready(ui) == Some(notes.version.as_str());
    let try_it = (announcement.try_it.as_deref()).and_then(|to| destination(ui, to));
    let (buttons, primary) = if ready {
        (
            vec![
                keyed_button(LATER, "Later", "", false, true, palette),
                keyed_button(RESTART, "Restart and update", "", true, true, palette),
            ],
            RESTART,
        )
    } else if try_it.is_some() {
        (
            vec![
                keyed_button(GOT_IT, "Got it", "", false, true, palette),
                keyed_button(TRY_IT, "Try it", "", true, true, palette),
            ],
            TRY_IT,
        )
    } else {
        (
            vec![keyed_button(GOT_IT, "Got it", "", true, true, palette)],
            GOT_IT,
        )
    };
    let mut footer = vec![Element::spacer().flex(1.0)];
    footer.extend(buttons);
    Modal::new(
        base,
        true,
        announcement.title.clone(),
        announcement.body.clone(),
        CARD,
        palette,
    )
    .width(width)
    .headless(true)
    .body(Element::column(parts))
    .footer(Element::row(footer).gap(8.0))
    .scrim(look::scrim())
    .initial_focus(primary)
    .render()
}

fn markdown(text: &str, width: f32, palette: ComponentPalette) -> Element {
    limn::Markdown::new(text, palette)
        .width(width)
        .font_size(BODY)
        .render()
        .element
}

/// Every version's notes, newest first: its date and version, then New, Improved and Fixed.
fn notes_dialog(ui: &Ui, base: Element, window: Size, palette: ComponentPalette) -> Element {
    if !ui.notes_open {
        return base;
    }
    let width = Modal::fit_width(window.width, 480.0);
    let inner = Modal::body_width(width);
    let secondary = ComponentPalette {
        text: look::muted(),
        ..palette
    };
    let mut sections = vec![];
    for (at, notes) in ui.whats_new.releases.iter().enumerate() {
        if at > 0 {
            sections.push(
                Element::spacer()
                    .background(look::divider())
                    .size(None, Some(1.0)),
            );
        }
        let mut lines = vec![Element::row(vec![
            text(notes.date.clone())
                .font_family(FontFamily::SansBold)
                .font_size(15.0)
                .centered_y(),
            limn::Chip::new(format!("v{}", notes.version), palette)
                .render()
                .centered_y(),
        ])
        .gap(8.0)];
        for (group, said) in [
            ("NEW", &notes.new),
            ("IMPROVED", &notes.improved),
            ("FIXED", &notes.fixed),
        ] {
            if said.is_empty() {
                continue;
            }
            lines.push(muted(group).font_size(SMALL));
            let points: Vec<String> = said.iter().map(|point| format!("- {point}")).collect();
            lines.push(markdown(&points.join("\n"), inner, secondary));
        }
        sections.push(Element::column(lines).gap(8.0));
    }
    if sections.is_empty() {
        sections.push(muted("No release notes yet."));
    }
    Modal::new(base, true, "What's new", "", NOTES, palette)
        .width(width)
        .height((window.height * 0.85).min(720.0))
        .body(Element::column(sections).gap(16.0))
        .scrim(look::scrim())
        .render()
}

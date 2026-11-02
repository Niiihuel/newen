//! The step after the welcome: choose System, Light or Dark before anything else is shown.
//! quill's own design (inferred): Desktop has no theme step; its Settings › General has an
//! icon-only System / Light / Dark control, default System, and no onboarding step for it.
//! The cards here are miniature
//! quill windows, so the choice is seen, not read; choosing applies live, as Settings does.

use crate::Ui;
use limn::{Color, Element, Point, Size};
use quill_components::pieces::{muted, text};
use quill_components::{icons, look};
use quill_model::{Intent, Theme};

const CHOOSE: &str = "look:";
/// Continue: the step is done and does not show again.
pub const CONTINUE: &str = "look:continue";

const CARD: (f32, f32) = (196.0, 124.0);

/// Whether the step shows: after the welcome, once, while the settings are known.
pub fn looking(ui: &Ui) -> bool {
    !ui.skip_welcome && !ui.agents.is_empty() && ui.settings.welcomed && !ui.settings.looked
}

/// The clicks of the step, or `None` for another's.
pub(crate) fn clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if id == CONTINUE {
        ui.settings.looked = true;
        crate::tour::after_welcome(ui);
        return Some(vec![Intent::SaveSettings(ui.settings.clone())]);
    }
    ui.settings.theme = match id.strip_prefix(CHOOSE)? {
        "system" => Theme::System,
        "light" => Theme::Light,
        "dark" => Theme::Dark,
        _ => return None,
    };
    // The host applies `settings.theme` on its next frame; the engine keeps it.
    Some(vec![Intent::SaveSettings(ui.settings.clone())])
}

pub fn view(ui: &Ui, window: Size) -> Element {
    let centered = |e: Element| e.centered_x();
    let cards = [
        ("system", "System", Theme::System),
        ("light", "Light", Theme::Light),
        ("dark", "Dark", Theme::Dark),
    ]
    .map(|(key, label, theme)| card(key, label, ui.settings.theme == theme));
    let go = Element::clickable(
        CONTINUE,
        Element::row(vec![
            Element::spacer().flex(1.0),
            text("Continue")
                .foreground(look::theme().background)
                .centered_y(),
            Element::spacer().flex(1.0),
        ])
        .size(Some(320.0), Some(44.0)),
    )
    .background(look::palette().text)
    .hover_background(look::primary_hover())
    .radius(10.0)
    .accessibility_label("Continue");
    Element::column(vec![
        Element::spacer().flex(1.0),
        centered(
            Element::icon(icons::of(include_str!(
                "../../../apps/quill/assets/icons/quill-mark.svg"
            )))
            .foreground(look::palette().text)
            .large_icon(56.0),
        ),
        Element::spacer().size(None, Some(20.0)),
        centered(text("Choose your look").font_size(22.0)),
        Element::spacer().size(None, Some(8.0)),
        // Centered by hand: limn's text has no alignment, so each line is its own.
        centered(muted("Pick how quill looks.")),
        centered(muted("You can change it any time in Settings.")),
        Element::spacer().size(None, Some(32.0)),
        centered(Element::row(cards.into()).gap(16.0)),
        Element::spacer().size(None, Some(36.0)),
        centered(go),
        Element::spacer().flex(1.2),
    ])
    .size(Some(window.width), Some(window.height))
    .background(look::theme().background)
}

/// One choice: its miniature window over its name, ringed while chosen.
fn card(key: &str, label: &str, chosen: bool) -> Element {
    let preview = match key {
        "light" => mini(&LIGHT),
        "dark" => mini(&DARK),
        // Split down the middle: the same window, light on its left and dark on its right.
        _ => Element::row(vec![
            mini(&LIGHT).clip_to(CARD.0 / 2.0, CARD.1),
            mini(&DARK)
                .translate(Point::new(-CARD.0 / 2.0, 0.0))
                .clip_to(CARD.0 / 2.0, CARD.1),
        ]),
    }
    .size(Some(CARD.0), Some(CARD.1))
    .radius(8.0)
    .clip_children()
    .border(look::divider(), 1.0);
    let body = Element::column(vec![
        preview,
        Element::spacer().size(None, Some(10.0)),
        text(label).centered_x(),
    ])
    .padding(8.0);
    let ring = if chosen {
        (look::palette().text, 2.0)
    } else {
        (look::divider(), 1.0)
    };
    Element::clickable(format!("{CHOOSE}{key}"), body)
        .radius(12.0)
        .border(ring.0, ring.1)
        .hover_background(look::row_hover())
        .accessibility_label(format!("{label}{}", if chosen { ", chosen" } else { "" }))
}

/// A theme's colors as the miniature draws them: the same grays as the window's own.
struct Tones {
    ground: Color,
    sidebar: Color,
    line: Color,
    bubble: Color,
    ink: Color,
}

const LIGHT: Tones = Tones {
    ground: Color::rgb(0xff, 0xff, 0xff),
    sidebar: Color::rgb(0xfa, 0xfa, 0xfa),
    line: Color::rgb(0xd9, 0xd9, 0xd9),
    bubble: Color::rgb(0xea, 0xea, 0xea),
    ink: Color::rgb(0xb4, 0xb4, 0xb4),
};
const DARK: Tones = Tones {
    ground: Color::rgb(0x19, 0x19, 0x19),
    sidebar: Color::rgb(0x1f, 0x1f, 0x1f),
    line: Color::rgb(0x40, 0x40, 0x40),
    bubble: Color::rgb(0x30, 0x30, 0x30),
    ink: Color::rgb(0x6a, 0x6a, 0x6a),
};

fn bar(width: f32, height: f32, color: Color) -> Element {
    Element::spacer()
        .size(Some(width), Some(height))
        .background(color)
        .radius(height / 2.0)
}

/// A small quill window: the sidebar strip, a message and a reply, the composer.
fn mini(t: &Tones) -> Element {
    let sidebar = Element::column(vec![
        bar(22.0, 5.0, t.ink),
        Element::spacer().size(None, Some(10.0)),
        bar(28.0, 4.0, t.line),
        Element::spacer().size(None, Some(5.0)),
        bar(24.0, 4.0, t.line),
        Element::spacer().size(None, Some(5.0)),
        bar(26.0, 4.0, t.line),
    ])
    .padding(8.0)
    .size(Some(44.0), None)
    .background(t.sidebar);
    let main = Element::column(vec![
        Element::row(vec![
            Element::spacer().flex(1.0),
            Element::spacer()
                .size(Some(66.0), Some(16.0))
                .background(t.bubble)
                .radius(6.0),
        ]),
        Element::spacer().size(None, Some(10.0)),
        bar(96.0, 5.0, t.ink),
        Element::spacer().size(None, Some(5.0)),
        bar(70.0, 5.0, t.ink),
        Element::spacer().flex(1.0),
        Element::spacer()
            .size(None, Some(22.0))
            .background(t.ground)
            .border(t.line, 1.0)
            .radius(8.0),
    ])
    .padding(10.0)
    .flex(1.0);
    Element::row(vec![
        sidebar,
        Element::spacer().size(Some(1.0), None).background(t.line),
        main,
    ])
    .size(Some(CARD.0), Some(CARD.1))
    .background(t.ground)
}

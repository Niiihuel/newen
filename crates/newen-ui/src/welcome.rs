//! The first screen: Newen's mark, its beni horizon drawn left to right under the N, then a
//! title, a line about Newen and Get started. Desktop's own first screen is its mark, a
//! title, a subtitle and one wide button near the bottom; the drawn horizon is Newen's.

use crate::Ui;
use limn::{Color, Component, Element, Size};
use newen_components::pieces::{muted, text};
use newen_components::{icons, look};

/// Get started: the welcome is done and does not show again.
pub const START: &str = "welcome:start";

/// The mark's two layers in the icon's frame (`scripts/brand/generate.mjs`): the N, in the
/// text's colour, and the horizon it stands on, in beni as on the icon.
const LETTER: &str = include_str!("../../../apps/newen/assets/icons/newen-mark-letter.svg");
const HORIZON: &str = include_str!("../../../apps/newen/assets/icons/newen-mark-horizon.svg");
const BENI: Color = Color::rgb(0xc0, 0x60, 0x5c);
/// The mark's side: limn's largest icon.
const SIDE: f32 = 128.0;
/// Seconds from the first frame at which the horizon starts, and the seconds it takes to cross.
const DRAW_FROM: f64 = 0.3;
const DRAW: f64 = 1.1;
/// Seconds from the first frame at which what is under the mark comes in. Until then the
/// host paints every frame, for the horizon (`Ui::repaint_after`).
pub const REVEAL: f64 = 1.6;

pub fn view(ui: &Ui, window: Size) -> Element {
    let since = *ui.welcome_since.get_or_init(std::time::Instant::now);
    let elapsed = since.elapsed().as_secs_f64();
    // Under reduced motion it is all there at once.
    let still = ui.reduced_motion;
    let t = ((elapsed - DRAW_FROM) / DRAW).clamp(0.0, 1.0);
    let drawn = if still {
        1.0
    } else {
        (0.5 - 0.5 * (std::f64::consts::PI * t).cos()) as f32
    };
    let layer = |svg: &'static str, color: Color| {
        Element::icon(icons::of(svg))
            .foreground(color)
            .large_icon(SIDE)
    };
    // The horizon shows as far as it is drawn; the row holds its clip at the left.
    let horizon = Element::row(vec![
        layer(HORIZON, BENI).clip_to(SIDE * drawn, SIDE),
        Element::spacer().flex(1.0),
    ]);
    let mark = Element::stack(vec![horizon, layer(LETTER, look::palette().text)])
        .size(Some(SIDE), Some(SIDE))
        .accessibility_label("Newen");
    let shown = still || elapsed >= REVEAL;
    let below = shown.then(|| {
        let start = Element::clickable(
            START,
            Element::row(vec![
                Element::spacer().flex(1.0),
                text("Get started")
                    .foreground(look::theme().background)
                    .centered_y(),
                Element::spacer().flex(1.0),
            ])
            .size(Some(320.0), Some(44.0)),
        )
        .background(look::palette().text)
        .hover_background(look::primary_hover())
        .radius(10.0)
        .accessibility_label("Get started");
        Element::column(vec![
            text("Welcome to Newen").font_size(22.0).centered_x(),
            Element::spacer().size(None, Some(8.0)),
            muted("Agents at work on your code.")
                .font_size(15.0)
                .centered_x(),
            Element::spacer().size(None, Some(36.0)),
            start.centered_x(),
        ])
    });
    Element::column(vec![
        Element::spacer().flex(1.0),
        mark.centered_x(),
        Element::spacer().size(None, Some(12.0)),
        limn::Collapse::new("welcome:below", below)
            .expand()
            .render(),
        Element::spacer().flex(1.2),
    ])
    .size(Some(window.width), Some(window.height))
    .background(look::theme().background)
}

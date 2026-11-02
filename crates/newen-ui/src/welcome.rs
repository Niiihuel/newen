//! The first screen: Newen's pen writes "welcome", as the icon's pen writes its line, then
//! a line about Newen and Get started. Desktop's own first screen is its mark, a title, a
//! subtitle and one wide button near the bottom; the
//! written word is Newen's.

use crate::Ui;
use limn::{Component, Element, Size};
use newen_components::look;
use newen_components::pieces::{muted, text};

/// Get started: the welcome is done and does not show again.
pub const START: &str = "welcome:start";

/// Seconds from the word's first frame at which the pen has lifted and what is under it
/// comes in: it comes down for 0.22, writes for 2.8 and hops between its 8 runs.
const REVEAL: f64 = 4.25;

pub fn view(ui: &Ui, window: Size) -> Element {
    let marks = WELCOME
        .iter()
        // The word is drawn 24 px in, so it sits centered under the pen's room at its right.
        .map(|line| limn::Mark::Line(line.iter().map(|(x, y)| (x + 24.0, *y)).collect()))
        .collect();
    // The icon's pen, three times the working line's: 72 px from nib to tip.
    let pen = PEN
        .iter()
        .map(|line| line.iter().map(|(x, y)| (x * 3.0, y * 3.0)).collect())
        .collect();
    let word = limn::Scribble::new("welcome:word", look::palette().text)
        .size(420.0, 146.0)
        .stroke(3.2)
        .marks(marks)
        .pen(pen)
        .once()
        .duration(2.8)
        .render()
        .accessibility_label("Welcome to Newen");
    let since = *ui.welcome_since.get_or_init(std::time::Instant::now);
    let shown = since.elapsed().as_secs_f64() >= REVEAL;
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
            muted("Agents at work on your code.")
                .font_size(15.0)
                .centered_x(),
            Element::spacer().size(None, Some(36.0)),
            start.centered_x(),
        ])
    });
    Element::column(vec![
        Element::spacer().flex(1.0),
        word.centered_x(),
        Element::spacer().size(None, Some(12.0)),
        limn::Collapse::new("welcome:below", below)
            .expand()
            .render(),
        Element::spacer().flex(1.2),
    ])
    .size(Some(window.width), Some(window.height))
    .background(look::theme().background)
}

/// "welcome" in Hershey's script simplex, one stroke a run of the pen (the font's own
/// strokes, joined where one ends at the next's start), at 3.6 times its units, in the
/// word's 420 by 146 box. Hershey fonts, U.S. National Bureau of Standards, as distributed
/// with hershey-fonts (`scripts.jhf`), used under their terms.
const WELCOME: [&[(f32, f32)]; 8] = [
    &[
        (18.8, 102.0),
        (11.6, 109.2),
        (8.0, 120.0),
        (8.0, 127.2),
        (11.6, 134.4),
        (18.8, 134.4),
        (26.0, 130.8),
        (33.2, 123.6),
    ],
    &[
        (40.4, 102.0),
        (33.2, 123.6),
        (33.2, 130.8),
        (36.8, 134.4),
        (44.0, 134.4),
        (51.2, 130.8),
        (58.4, 123.6),
        (62.0, 112.8),
        (62.0, 102.0),
        (65.6, 116.4),
        (69.2, 120.0),
        (76.4, 120.0),
        (83.6, 116.4),
    ],
    &[
        (87.2, 127.2),
        (94.4, 123.6),
        (98.0, 120.0),
        (101.6, 112.8),
        (101.6, 105.6),
        (98.0, 102.0),
        (94.4, 102.0),
        (87.2, 105.6),
        (83.6, 112.8),
        (83.6, 123.6),
        (87.2, 130.8),
        (94.4, 134.4),
        (101.6, 134.4),
        (108.8, 130.8),
        (112.4, 127.2),
        (119.6, 116.4),
        (126.8, 105.6),
        (137.6, 87.6),
        (141.2, 80.4),
        (144.8, 69.6),
        (144.8, 62.4),
        (141.2, 58.8),
        (134.0, 62.4),
        (130.4, 69.6),
        (126.8, 84.0),
        (123.2, 109.2),
        (123.2, 130.8),
        (126.8, 134.4),
        (130.4, 134.4),
        (137.6, 130.8),
        (141.2, 127.2),
        (148.4, 116.4),
    ],
    &[
        (173.6, 109.2),
        (173.6, 105.6),
        (170.0, 102.0),
        (162.8, 102.0),
        (155.6, 105.6),
        (152.0, 109.2),
        (148.4, 116.4),
        (148.4, 123.6),
        (152.0, 130.8),
        (159.2, 134.4),
        (170.0, 134.4),
        (180.8, 127.2),
        (188.0, 116.4),
    ],
    &[
        (209.6, 102.0),
        (202.4, 102.0),
        (195.2, 105.6),
        (191.6, 109.2),
        (188.0, 116.4),
        (188.0, 123.6),
        (191.6, 130.8),
        (198.8, 134.4),
        (206.0, 134.4),
        (213.2, 130.8),
        (216.8, 127.2),
        (220.4, 120.0),
        (220.4, 112.8),
        (216.8, 105.6),
        (209.6, 102.0),
        (206.0, 105.6),
        (206.0, 112.8),
        (209.6, 120.0),
        (216.8, 123.6),
        (227.6, 123.6),
        (234.8, 120.0),
        (238.4, 116.4),
        (245.6, 105.6),
        (252.8, 102.0),
        (256.4, 105.6),
        (256.4, 109.2),
        (252.8, 123.6),
        (249.2, 134.4),
    ],
    &[
        (252.8, 123.6),
        (256.4, 116.4),
        (263.6, 105.6),
        (270.8, 102.0),
        (278.0, 102.0),
        (281.6, 105.6),
        (281.6, 109.2),
        (278.0, 123.6),
        (274.4, 134.4),
    ],
    &[
        (278.0, 123.6),
        (281.6, 116.4),
        (288.8, 105.6),
        (296.0, 102.0),
        (303.2, 102.0),
        (306.8, 105.6),
        (306.8, 112.8),
        (303.2, 123.6),
        (303.2, 130.8),
        (306.8, 134.4),
        (310.4, 134.4),
        (317.6, 130.8),
        (321.2, 127.2),
        (328.4, 116.4),
    ],
    &[
        (332.0, 127.2),
        (339.2, 123.6),
        (342.8, 120.0),
        (346.4, 112.8),
        (346.4, 105.6),
        (342.8, 102.0),
        (339.2, 102.0),
        (332.0, 105.6),
        (328.4, 112.8),
        (328.4, 123.6),
        (332.0, 130.8),
        (339.2, 134.4),
        (346.4, 134.4),
        (353.6, 130.8),
        (357.2, 127.2),
        (364.4, 116.4),
    ],
];

/// The icon's pen (`apps/newen/assets/icons/quill.svg`), 24 px from nib to tip, its nib at
/// (0, 0): the feather, its shaft, the ferrule and the nib.
const PEN: [&[(f32, f32)]; 4] = [
    &[
        (4.4, -5.0),
        (3.4, -6.5),
        (3.9, -8.6),
        (5.1, -11.1),
        (7.1, -13.5),
        (9.8, -15.7),
        (12.7, -17.3),
        (15.7, -18.2),
        (15.2, -15.7),
        (14.6, -13.9),
        (13.5, -13.2),
        (12.6, -11.6),
        (11.9, -9.6),
        (10.6, -9.2),
        (8.4, -8.0),
        (6.0, -6.4),
        (4.4, -5.0),
    ],
    &[(4.3, -6.0), (6.9, -9.2), (9.9, -13.2)],
    &[(2.3, -4.8), (4.3, -3.3)],
    &[
        (1.8, -4.2),
        (0.9, -1.9),
        (0.0, 0.0),
        (2.3, -0.9),
        (3.9, -3.4),
    ],
];

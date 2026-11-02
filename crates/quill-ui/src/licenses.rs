//! Settings › System › Licenses: the terms of third-party software quill uses that ask to be
//! shown, for now Cisco's OpenH264 binary, which plays the clips of What's new.

use crate::Ui;
use limn::{Component, ComponentPalette, Element, FontFamily, Modal, Size};
use quill_components::look::{self, SMALL};
use quill_components::pieces::{muted, text};

pub const OPEN: &str = "licenses:open";
const CLOSE: &str = "licenses";

/// Cisco's license of its binary, exactly as Cisco publishes it (licenses/openh264/SOURCE.txt).
pub const OPENH264: &str = include_str!("../../../licenses/openh264/BINARY_LICENSE.txt");

pub(crate) fn clicked(ui: &mut Ui, id: &str) -> Option<Vec<quill_model::Intent>> {
    match id {
        OPEN => ui.licenses_open = true,
        CLOSE => ui.licenses_open = false,
        _ => return None,
    }
    Some(vec![])
}

pub(crate) fn dialog(ui: &Ui, base: Element, window: Size, palette: ComponentPalette) -> Element {
    if !ui.licenses_open {
        return base;
    }
    let width = Modal::fit_width(window.width, 640.0);
    let inner = Modal::body_width(width);
    let body = Element::column(vec![
        text(limn_video::cisco::NOTICE).font_family(FontFamily::SansBold),
        muted(limn_video::cisco::LICENSE_URL).font_size(SMALL),
        text(OPENH264)
            .font_family(FontFamily::Monospace)
            .font_size(12.0)
            .size(Some(inner), None),
    ])
    .gap(8.0);
    Modal::new(base, true, "Licenses", "", CLOSE, palette)
        .width(width)
        .height((window.height * 0.85).min(720.0))
        .body(body)
        .scrim(look::scrim())
        .render()
}

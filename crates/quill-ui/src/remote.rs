//! Remote Control: reaching a session from another device (a phone, the web) through its agent's
//! own service (docs/remote-control.md). The row and its switch in the Environment card,
//! `/remote-control`, and the dialog with the session's link and its QR code.

use crate::{Remote, Session, Store, Ui};
use limn::{Component, Element};
use quill_components::pieces::{keyed_button, muted, text};
use quill_components::{icons, look};
use quill_model::{Event, Intent, SessionId};

pub const TOGGLE: &str = "remote:toggle";
pub const OPEN: &str = "remote:open";
/// `/remote-control`: on when it is off, and the dialog either way.
pub const COMMAND: &str = "remote:command";
const CLOSE: &str = "remote:close";
const COPY: &str = "remote:copy";
const ON: &str = "remote:on";
const OFF: &str = "remote:off";

/// Whether `session`'s agent offers it.
pub fn offered(session: &Session) -> bool {
    session.capabilities.as_ref().is_some_and(|c| c.remote)
}

/// The link the dialog shows, for the host to draw its QR code into [`Ui::qr`].
pub fn shown_url<'a>(store: &'a Store, ui: &Ui) -> Option<&'a str> {
    match &store.session(ui.remote?)?.remote {
        Remote::On(Some(url)) => Some(url),
        _ => None,
    }
}

/// The agent answered: the switch shows what it said.
pub(crate) fn heard(ui: &mut Ui, event: &Event) {
    if let Event::Agent {
        session,
        event: quill_model::AgentEvent::Remote { .. },
        ..
    } = event
    {
        if ui.remote_asked == Some(*session) {
            ui.remote_asked = None;
        }
    }
}

fn asking(ui: &Ui, session: SessionId) -> bool {
    ui.remote_asked == Some(session)
}

fn turn(ui: &mut Ui, session: &Session, on: bool) -> Vec<Intent> {
    ui.remote_asked = on.then_some(session.id);
    let name = (!session.title.is_empty()).then(|| session.title.clone());
    vec![Intent::Remote {
        session: session.id,
        on,
        name,
    }]
}

pub(crate) fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if ![TOGGLE, OPEN, COMMAND, CLOSE, COPY, ON, OFF].contains(&id) {
        return None;
    }
    let shown = if [TOGGLE, OPEN, COMMAND].contains(&id) {
        ui.selected
    } else {
        ui.remote
    };
    let session = store.session(shown?)?;
    let on = matches!(session.remote, Remote::On(_)) || asking(ui, session.id);
    Some(match id {
        TOGGLE => turn(ui, session, !on),
        OPEN => {
            ui.remote = Some(session.id);
            vec![]
        }
        COMMAND => {
            ui.remote = Some(session.id);
            if on {
                vec![]
            } else {
                turn(ui, session, true)
            }
        }
        ON => turn(ui, session, true),
        OFF => turn(ui, session, false),
        COPY => {
            if let Remote::On(Some(url)) = &session.remote {
                ui.files_copy = Some(url.clone());
            }
            vec![]
        }
        _ => {
            ui.remote = None;
            vec![]
        }
    })
}

/// The Environment card's row, when the session's agent offers it: "Remote Control" and its
/// switch; on, the words open the dialog with the link.
pub(crate) fn row(session: &Session, ui: &Ui) -> Option<Element> {
    if !offered(session) {
        return None;
    }
    let palette = look::palette();
    let on = matches!(session.remote, Remote::On(_)) || asking(ui, session.id);
    let words = Element::row(vec![
        Element::icon(icons::of(icons::PHONE))
            .foreground(palette.text)
            .size(Some(16.0), Some(16.0))
            .centered_y(),
        text("Remote Control").centered_y(),
    ])
    .gap(10.0);
    let words = if on {
        Element::clickable(OPEN, words)
    } else {
        words
    };
    let state = match &session.remote {
        _ if asking(ui, session.id) => Some(muted("Connecting\u{2026}")),
        Remote::Failed(_) => Some(muted("Failed")),
        _ => None,
    };
    let switch = limn::Switch::new(TOGGLE, on, palette)
        .variant(limn::SwitchVariant::Accent)
        .ink(look::switch_on())
        .off_ink(look::switch_off())
        .render()
        .centered_y();
    let mut parts = vec![words, Element::spacer().flex(1.0)];
    parts.extend(state.map(|s| s.font_size(look::SMALL).centered_y()));
    parts.push(switch);
    Some(Element::row(parts).gap(8.0).size(None, Some(20.0)))
}

/// The dialog: the link to open the session on another device and its QR code, or what is
/// in the way.
pub(crate) fn dialog(store: &Store, ui: &Ui, base: Element) -> Element {
    let Some(session) = ui.remote.and_then(|id| store.session(id)) else {
        return base;
    };
    let palette = look::palette();
    let center = |e: Element| {
        Element::row(vec![
            Element::spacer().flex(1.0),
            e,
            Element::spacer().flex(1.0),
        ])
    };
    let done = keyed_button(CLOSE, "Done", "", true, true, palette);
    let (body, footer) = match &session.remote {
        _ if asking(ui, session.id) => (vec![center(muted("Connecting\u{2026}"))], vec![done]),
        Remote::On(Some(url)) => {
            let mut body = vec![muted(
                "Open this link, or scan the code, on your phone or in a browser to go on with \
                 this session there. It keeps running on this computer.",
            )];
            if let Some((_, picture)) = ui.qr.as_ref().filter(|(shown, _)| shown == url) {
                body.push(center(
                    Element::raster_image(picture.clone()).size(Some(200.0), Some(200.0)),
                ));
            }
            body.push(center(muted(url.clone()).font_size(look::SMALL)));
            (
                body,
                vec![
                    keyed_button(OFF, "Turn off", "", false, true, palette),
                    keyed_button(COPY, "Copy link", "", false, true, palette),
                    done,
                ],
            )
        }
        Remote::On(None) => (
            vec![muted(
                "This session is reachable from your other devices. Turn it off and on again to \
                 see its link here.",
            )],
            vec![
                keyed_button(OFF, "Turn off", "", false, true, palette),
                done,
            ],
        ),
        Remote::Failed(error) => (
            vec![muted(format!("It could not be turned on: {error}"))],
            vec![
                keyed_button(ON, "Try again", "", false, true, palette),
                done,
            ],
        ),
        Remote::Off => (
            vec![muted(
                "Turn it on to go on with this session from your phone or a browser. While it \
                 is on, the agent's service keeps this conversation so your other devices can \
                 show it.",
            )],
            vec![keyed_button(ON, "Turn on", "", false, true, palette), done],
        ),
    };
    let footer = Element::row(
        std::iter::once(Element::spacer().flex(1.0))
            .chain(footer)
            .collect(),
    )
    .gap(8.0);
    limn::Modal::new(base, true, "Remote Control", "", CLOSE, palette)
        .width(440.0)
        .body(Element::column(body).gap(12.0))
        .footer(footer)
        .backdrop_dismiss(true)
        .scrim(look::scrim())
        .render()
}

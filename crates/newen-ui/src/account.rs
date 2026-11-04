//! The person's account with an agent as its own program keeps it: the sign-in screen and
//! the question before signing out (Settings' Accounts page holds the buttons; the sidebar's
//! foot is the GitHub identity, github.rs). The program signs in and out
//! (the engine runs it); this shows what it reports. Desktop's account menu is modeled on what it
//! shows: its avatar shows the first and
//! last word's initials with `?` as the fallback, its menu opens with the identity, and Sign
//! out is the last entry. Its sign-in screen is a web page; this one is Newen's own (inferred).

use crate::Ui;
use limn::{Component, Element, Size};
use newen_components::look;
use newen_components::pieces::{confirm, hgap, keyed_button, muted, text};
use newen_model::{AuthState, Event, Intent};
use std::collections::HashMap;

/// The account menu's entries and the session's "Sign in" button.
pub const SIGN_IN: &str = "account:sign-in";
/// An agent's "Sign in" in Settings, followed by its id.
pub const SIGN_IN_OF: &str = "account:sign-in-of:";
/// An agent's "Sign out" in Settings, followed by its id.
pub const SIGN_OUT_OF: &str = "account:sign-out-of:";
const SIGN_OUT_YES: &str = "account:sign-out:yes";
const SIGN_OUT_NO: &str = "account:sign-out:no";
/// A sign-in method's button, followed by its key.
pub const METHOD: &str = "account:method:";
pub const COPY_LINK: &str = "account:copy-link";
/// The field the sign-in page's code is pasted into, followed by the attempt's number: limn
/// keeps a field's text under its ID, so each attempt gets an empty one of its own.
pub const CODE: &str = "account:code:";
const CONTINUE: &str = "account:continue";
const CANCEL: &str = "account:cancel";
const RETRY: &str = "account:retry";

/// Where the sign-in screen is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    /// Choosing how to sign in.
    #[default]
    Choose,
    /// The program opened its page: its address once known, and whether the code went.
    Waiting {
        url: Option<String>,
        sent: bool,
    },
    Failed(String),
}

#[derive(Default)]
pub struct Account {
    /// Who each agent's program says is signed in; an agent not asked yet has no entry.
    pub states: HashMap<String, AuthState>,
    /// The sign-in screen was asked for by hand (a session's "Sign in"), though the program
    /// may still say the person is signed in.
    pub asked: bool,
    pub phase: Phase,
    /// What the code field holds, and which attempt's field it is.
    pub code: String,
    pub attempt: u32,
    /// The agent whose sign-out is being asked about.
    pub confirming: Option<String>,
    /// The agent Settings asked to sign in to, while it is not the one new sessions use.
    pub target: Option<String>,
}

/// The agent whose account the screen and the question are about: the one Settings asked
/// for, else the one new sessions use.
fn current(ui: &Ui) -> Option<&newen_model::AgentInfo> {
    let wanted = ui.account.target.as_deref().unwrap_or(&ui.agent);
    ui.agents
        .iter()
        .find(|a| a.id == wanted)
        .or(ui.agents.first())
}

pub fn state(ui: &Ui) -> Option<&AuthState> {
    ui.account.states.get(&current(ui)?.id)
}

/// Whether the sign-in screen fills the window: the program says nobody is signed in (once
/// the welcome is done), or a session asked for it.
pub fn visible(ui: &Ui) -> bool {
    !crate::welcoming(ui)
        && !crate::appearance::looking(ui)
        && (ui.account.asked || matches!(state(ui), Some(AuthState::SignedOut)))
}

/// What the engine reports: the accounts, and the sign-in's progress. The first settings
/// ask each agent's program who is signed in, so the answer is known before the first frame
/// that matters; nothing of it is stored, the program keeps the account.
pub(crate) fn heard(ui: &mut Ui, event: &Event) -> Vec<Intent> {
    match event {
        Event::Settings { agents, .. } if ui.agents.is_empty() => agents
            .iter()
            .filter(|a| !a.missing)
            .map(|a| Intent::CheckSignIn {
                agent: a.id.clone(),
            })
            .collect(),
        Event::Auth { agent, state } => {
            ui.account.states.insert(agent.clone(), state.clone());
            if matches!(state, AuthState::SignedIn { .. }) {
                ui.account.asked = false;
                ui.account.target = None;
                ui.account.phase = Phase::Choose;
                ui.account.code.clear();
            }
            vec![]
        }
        Event::SignInUrl { url, .. } => {
            ui.account.phase = Phase::Waiting {
                url: Some(url.clone()),
                sent: false,
            };
            vec![]
        }
        Event::SignInFailed { message, .. } => {
            ui.account.phase = Phase::Failed(message.clone());
            vec![]
        }
        _ => vec![],
    }
}

/// The clicks of this module, or `None` for another's.
pub(crate) fn clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if let Some(agent) = id.strip_prefix(SIGN_IN_OF) {
        ui.account.target = Some(agent.to_owned());
        ui.account.asked = true;
        ui.account.phase = Phase::Choose;
        return Some(vec![]);
    }
    if let Some(agent) = id.strip_prefix(SIGN_OUT_OF) {
        ui.account.confirming = Some(agent.to_owned());
        return Some(vec![]);
    }
    let agent = current(ui)?.id.clone();
    if let Some(method) = id.strip_prefix(METHOD) {
        ui.account.phase = Phase::Waiting {
            url: None,
            sent: false,
        };
        ui.account.code.clear();
        ui.account.attempt += 1;
        return Some(vec![Intent::SignIn {
            agent,
            method: method.to_owned(),
        }]);
    }
    Some(match id {
        SIGN_IN => {
            ui.account_menu = false;
            ui.account.asked = true;
            vec![]
        }
        SIGN_OUT_NO => {
            ui.account.confirming = None;
            vec![]
        }
        SIGN_OUT_YES => {
            let agent = ui.account.confirming.take()?;
            vec![Intent::SignOut { agent }]
        }
        COPY_LINK => {
            // The host puts it on the clipboard on its next frame.
            if let Phase::Waiting { url: Some(url), .. } = &ui.account.phase {
                ui.files_copy = Some(url.clone());
            }
            vec![]
        }
        CONTINUE => submit(ui),
        // Back to choosing; the program's login is ended, and a window opened by hand closes.
        CANCEL => {
            ui.account.phase = Phase::Choose;
            ui.account.asked = false;
            ui.account.target = None;
            vec![Intent::CancelSignIn { agent }]
        }
        RETRY => {
            ui.account.phase = Phase::Choose;
            vec![]
        }
        _ => return None,
    })
}

/// Send the pasted code, once.
pub(crate) fn submit(ui: &mut Ui) -> Vec<Intent> {
    let code = ui.account.code.trim().to_owned();
    let Some(agent) = current(ui).map(|a| a.id.clone()) else {
        return vec![];
    };
    match &mut ui.account.phase {
        Phase::Waiting { sent, .. } if !code.is_empty() && !*sent => {
            *sent = true;
            vec![Intent::SubmitSignInCode { agent, code }]
        }
        _ => vec![],
    }
}

/// The first and last words' initials in capitals, `?` for none; a lone word gives one.
pub(crate) fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let first = |w: Option<&&str>| w.and_then(|w| w.chars().next());
    match (first(words.first()), first(words.last()), words.len()) {
        (Some(a), Some(b), n) if n > 1 => format!("{}{}", a.to_uppercase(), b.to_uppercase()),
        (Some(a), ..) => a.to_uppercase().to_string(),
        _ => "?".into(),
    }
}

/// "Sign out of {agent}?", over `base`.
pub(crate) fn dialogs(ui: &Ui, base: Element) -> Element {
    let Some(agent) = ui
        .account
        .confirming
        .as_ref()
        .and_then(|id| ui.agents.iter().find(|a| &a.id == id))
    else {
        return base;
    };
    let program = ui
        .settings
        .agent(&agent.id)
        .program
        .unwrap_or_else(|| agent.id.clone());
    confirm(
        base,
        true,
        &format!("Sign out of {}?", agent.name),
        &format!("This also signs you out of {program} in your terminal."),
        "Sign out",
        SIGN_OUT_YES,
        SIGN_OUT_NO,
    )
}

/// A wide button of the screen: filled when `primary`, outlined otherwise.
pub(crate) fn wide(id: &str, label: &str, primary: bool) -> Element {
    let ink = if primary {
        look::theme().background
    } else {
        look::palette().text
    };
    let button = Element::clickable(
        id,
        Element::row(vec![
            Element::spacer().flex(1.0),
            text(label).foreground(ink).centered_y(),
            Element::spacer().flex(1.0),
        ])
        .size(Some(320.0), Some(44.0)),
    )
    .radius(10.0)
    .accessibility_label(label);
    if primary {
        button
            .background(look::palette().text)
            .hover_background(look::primary_hover())
    } else {
        button
            .border(look::composer_border(), 1.0)
            .hover_background(look::row_hover())
    }
}

/// The sign-in screen, full window like the welcome: Newen's mark, the agent's name, and the
/// steps of signing in through its program.
pub fn screen(ui: &Ui, window: Size) -> Element {
    let palette = look::palette();
    let (name, program, methods, takes_code) = current(ui)
        .map(|a| {
            let program = ui.settings.agent(&a.id).program;
            (
                a.name.clone(),
                program.unwrap_or_else(|| a.id.clone()),
                a.sign_in_methods.clone(),
                a.sign_in_code,
            )
        })
        .unwrap_or_default();
    let centered = |e: Element| e.centered_x();
    let mut column = vec![
        centered(crate::welcome::mark(56.0)),
        Element::spacer().size(None, Some(20.0)),
    ];
    match &ui.account.phase {
        Phase::Choose | Phase::Failed(_) => {
            column.push(centered(
                text(format!("Sign in to {name}"))
                    .font_size(22.0)
                    .centered_x(),
            ));
            column.push(Element::spacer().size(None, Some(8.0)));
            // Centered by hand: limn's text has no alignment, so each line is its own.
            column.push(centered(muted(format!(
                "Newen uses {program}\u{2019}s own sign-in;"
            ))));
            column.push(centered(muted("your account stays with it.")));
            column.push(Element::spacer().size(None, Some(28.0)));
            if let Phase::Failed(message) = &ui.account.phase {
                column.push(centered(text(message.clone()).foreground(look::removed())));
                column.push(Element::spacer().size(None, Some(14.0)));
                column.push(centered(wide(RETRY, "Try again", true)));
            } else {
                let buttons = methods.iter().enumerate().map(|(i, (key, label))| {
                    centered(wide(&format!("{METHOD}{key}"), label, i == 0))
                });
                column.push(Element::column(buttons.collect()).gap(10.0));
                // A screen asked for by hand, with the person still signed in, can be left.
                if !matches!(state(ui), Some(AuthState::SignedOut)) {
                    column.push(Element::spacer().size(None, Some(10.0)));
                    column.push(centered(wide(CANCEL, "Cancel", false)));
                } else {
                    // Signed out of this agent, the person can still use another (inferred).
                    for other in ui
                        .agents
                        .iter()
                        .filter(|a| Some(a.id.as_str()) != current(ui).map(|c| c.id.as_str()))
                    {
                        column.push(Element::spacer().size(None, Some(10.0)));
                        column.push(centered(wide(
                            &format!("agent:{}", other.id),
                            &format!("Use {} instead", other.name),
                            false,
                        )));
                    }
                }
            }
        }
        Phase::Waiting { url, sent } => {
            column.push(centered(
                text("Continue in your browser")
                    .font_size(22.0)
                    .centered_x(),
            ));
            column.push(Element::spacer().size(None, Some(8.0)));
            column.push(centered(muted("We opened the sign-in page.")));
            column.push(centered(muted("If it didn\u{2019}t open, copy the link.")));
            column.push(Element::spacer().size(None, Some(28.0)));
            if !takes_code {
                // The browser returns to the program by itself; nothing to paste.
                column.push(centered(muted("Waiting for the browser\u{2026}")));
                column.push(Element::spacer().size(None, Some(14.0)));
                column.push(centered(
                    Element::row(vec![
                        keyed_button(COPY_LINK, "Copy link", "", false, url.is_some(), palette),
                        keyed_button(CANCEL, "Cancel", "", false, true, palette),
                    ])
                    .gap(8.0)
                    .centered_x(),
                ));
                return finish(column, 360.0, window);
            }
            // One action leads, as on the first screen: the code's field over a 320 px
            // Continue that fills once there is a code; the ways out sit small beneath.
            column.push(centered(
                limn::TextField::new(
                    format!("{CODE}{}", ui.account.attempt),
                    ui.account.code.as_str(),
                    "Paste the code here",
                    palette,
                )
                .width(320.0)
                .render(),
            ));
            column.push(Element::spacer().size(None, Some(10.0)));
            let ready = !ui.account.code.trim().is_empty() && !sent;
            let label = if *sent {
                "Signing in\u{2026}"
            } else {
                "Continue"
            };
            column.push(centered(wide(CONTINUE, label, ready)));
            column.push(Element::spacer().size(None, Some(14.0)));
            column.push(centered(
                Element::row(vec![
                    keyed_button(COPY_LINK, "Copy link", "", false, url.is_some(), palette),
                    keyed_button(CANCEL, "Cancel", "", false, true, palette),
                ])
                .gap(8.0)
                .centered_x(),
            ));
        }
    }
    finish(column, 360.0, window)
}

/// The steps, in a column `width` wide centered in the window.
pub(crate) fn finish(column: Vec<Element>, width: f32, window: Size) -> Element {
    Element::column(vec![
        Element::spacer().flex(1.0),
        Element::row(vec![
            hgap(0.0),
            Element::column(column).size(Some(width), None),
        ])
        .centered_x(),
        Element::spacer().flex(1.2),
    ])
    .size(Some(window.width), Some(window.height))
    .background(look::theme().background)
}

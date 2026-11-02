//! GitHub as the person's identity: the state `gh` reports, the sidebar foot and its menu, the
//! sign-in with its one-time code, and the question before signing out. GitHub is not an
//! agent: the engine asks the `gh` program (`quill-engine/src/github.rs`), which keeps the
//! credentials; this shows what it reports. Desktop shows GitHub only as a connector row,
//! "Signed in as @{login}" with Disconnect and Reconnect; the foot's use of it as the
//! person's identity is quill's (inferred).

use crate::Ui;
use limn::{Component, Element};
use quill_components::pieces::{confirm, hgap, keyed_button, muted, text};
use quill_components::{icons, look};
use quill_model::{ContributionCalendar, Event, GithubState, Intent};
use std::path::PathBuf;

pub const CONNECT: &str = "github:connect";
pub const DISCONNECT: &str = "github:disconnect";
const DISCONNECT_YES: &str = "github:disconnect:yes";
const DISCONNECT_NO: &str = "github:disconnect:no";
pub const COPY_CODE: &str = "github:copy-code";
pub const CANCEL: &str = "github:cancel";
const CLOSE: &str = "github:close";
/// Asks `gh` again who is signed in.
pub const CHECK: &str = "github:check";
/// Reloads the calendar.
pub const REFRESH: &str = "github:refresh";
/// The profile's link; the host opens it on the computer.
pub const PROFILE_PREFIX: &str = "https://github.com/";

/// Where the sign-in is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    #[default]
    Idle,
    /// `gh` was started; the code once it printed it.
    Waiting {
        code: Option<String>,
    },
    Failed(String),
}

/// The contribution calendar's load.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Calendar {
    #[default]
    Idle,
    Loading,
    Ready(ContributionCalendar),
    Failed(String),
}

#[derive(Default)]
pub struct Github {
    /// `gh` was asked once, at the first settings.
    asked: bool,
    /// What `gh` says; `None` until answered.
    pub state: Option<GithubState>,
    pub phase: Phase,
    pub calendar: Calendar,
    /// Whether the question before disconnecting is up.
    pub confirming: bool,
}

/// The signed-in account: login, name and picture.
pub fn signed_in(ui: &Ui) -> Option<(&str, Option<&str>, Option<&PathBuf>)> {
    match &ui.github.state {
        Some(GithubState::SignedIn {
            login,
            name,
            avatar,
        }) => Some((login, name.as_deref(), avatar.as_ref())),
        _ => None,
    }
}

/// The login whose calendar is loaded or loading.
fn calendar_login(ui: &Ui) -> Option<&str> {
    match &ui.github.calendar {
        Calendar::Ready(calendar) => Some(&calendar.login),
        _ => None,
    }
}

/// Ask for the calendar when the Accounts page is open, somebody is signed in and the
/// calendar is not loaded or loading.
pub(crate) fn want_calendar(ui: &mut Ui) -> Vec<Intent> {
    let open = ui.settings_page.as_deref() == Some(crate::view::SETTINGS_ACCOUNTS);
    let Some((login, ..)) = signed_in(ui) else {
        return vec![];
    };
    let login = login.to_owned();
    let loaded = calendar_login(ui).is_some_and(|l| l.eq_ignore_ascii_case(&login));
    if !open || loaded || ui.github.calendar == Calendar::Loading {
        return vec![];
    }
    ui.github.calendar = Calendar::Loading;
    vec![Intent::GithubContributions { login }]
}

/// What the engine reports about GitHub; the first settings ask `gh` who is signed in.
pub(crate) fn heard(ui: &mut Ui, event: &Event) -> Vec<Intent> {
    match event {
        Event::Settings { .. } if !ui.github.asked => {
            ui.github.asked = true;
            vec![Intent::CheckGithub]
        }
        Event::Github(state) => {
            let changed = signed_in(ui).map(|(l, ..)| l.to_owned())
                != match state {
                    GithubState::SignedIn { login, .. } => Some(login.clone()),
                    _ => None,
                };
            if matches!(state, GithubState::SignedIn { .. }) {
                ui.github.phase = Phase::Idle;
            }
            ui.github.state = Some(state.clone());
            if changed {
                ui.github.calendar = Calendar::Idle;
            }
            want_calendar(ui)
        }
        Event::GithubCode { code, .. } => {
            ui.github.phase = Phase::Waiting {
                code: Some(code.clone()),
            };
            vec![]
        }
        Event::GithubFailed { message } => {
            ui.github.phase = Phase::Failed(message.clone());
            vec![]
        }
        Event::GithubContributions(result) => {
            ui.github.calendar = match result {
                Ok(calendar) => Calendar::Ready(calendar.clone()),
                Err(message) => Calendar::Failed(message.clone()),
            };
            vec![]
        }
        _ => vec![],
    }
}

/// The clicks of this module, or `None` for another's.
pub(crate) fn clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    Some(match id {
        CONNECT => {
            ui.account_menu = false;
            // Without `gh` there is nothing to start: Settings says how to install it.
            if matches!(ui.github.state, Some(GithubState::Missing(_))) {
                ui.settings_page = Some(crate::view::SETTINGS_ACCOUNTS.into());
                return Some(vec![]);
            }
            ui.github.phase = Phase::Waiting { code: None };
            vec![Intent::ConnectGithub]
        }
        DISCONNECT => {
            ui.account_menu = false;
            ui.github.confirming = true;
            vec![]
        }
        DISCONNECT_NO => {
            ui.github.confirming = false;
            vec![]
        }
        DISCONNECT_YES => {
            ui.github.confirming = false;
            let (login, ..) = signed_in(ui)?;
            vec![Intent::DisconnectGithub {
                login: login.to_owned(),
            }]
        }
        COPY_CODE => {
            if let Phase::Waiting { code: Some(code) } = &ui.github.phase {
                ui.files_copy = Some(code.clone());
            }
            vec![]
        }
        CANCEL => {
            ui.github.phase = Phase::Idle;
            vec![Intent::CancelGithub]
        }
        CLOSE => {
            ui.github.phase = Phase::Idle;
            vec![]
        }
        CHECK => vec![Intent::CheckGithub],
        REFRESH => {
            ui.github.calendar = Calendar::Idle;
            want_calendar(ui)
        }
        _ => return None,
    })
}

/// The foot's person: initials, name, and the avatar's file when GitHub is connected. Not
/// connected, it is the quill user's name.
pub(crate) fn identity(ui: &Ui) -> (String, String, Option<PathBuf>) {
    if let Some((login, name, avatar)) = signed_in(ui) {
        let shown = name.unwrap_or(login);
        return (
            crate::account::initials(shown),
            shown.to_owned(),
            avatar.cloned(),
        );
    }
    let name = ui.name.clone().filter(|n| !n.trim().is_empty());
    (
        crate::account::initials(name.as_deref().unwrap_or_default()),
        name.unwrap_or_else(|| "You".into()),
        None,
    )
}

/// The menu's first block: GitHub's name and `@login`, or why there is none; not clickable.
pub(crate) fn menu_header(ui: &Ui) -> limn::DropdownMenuEntry {
    let lines = match (&ui.github.state, signed_in(ui)) {
        (_, Some((login, name, _))) => match name {
            Some(name) => vec![text(name.to_owned()), muted(format!("@{login}"))],
            None => vec![text(format!("@{login}"))],
        },
        (Some(GithubState::Missing(_)), _) => vec![muted("GitHub CLI not installed")],
        (Some(GithubState::SignedOut), _) => vec![muted("GitHub not connected")],
        (Some(GithubState::Unknown(_)), _) => vec![muted("GitHub unavailable")],
        _ => vec![muted("GitHub")],
    };
    limn::DropdownMenuEntry::Custom(Box::new(
        Element::column(lines)
            .gap(2.0)
            .padding(8.0)
            .size(Some(244.0), None),
    ))
}

/// The menu's last entry as (id, label): Disconnect GitHub, or Connect GitHub.
pub(crate) fn menu_action(ui: &Ui) -> (&'static str, &'static str) {
    if signed_in(ui).is_some() {
        (DISCONNECT, "Disconnect GitHub")
    } else {
        (CONNECT, "Connect GitHub")
    }
}

pub(crate) fn menu_end(ui: &Ui) -> Vec<limn::DropdownMenuEntry> {
    let (id, label) = menu_action(ui);
    vec![
        limn::DropdownMenuEntry::Separator,
        limn::DropdownMenuItem::new(id, label)
            .icon(icons::of(icons::LOGOUT))
            .into(),
    ]
}

/// The sign-in's code and the question before disconnecting, over `base`.
pub(crate) fn dialogs(ui: &Ui, base: Element) -> Element {
    let palette = look::palette();
    let base = confirm(
        base,
        ui.github.confirming,
        "Disconnect GitHub?",
        "This also signs you out of gh in your terminal.",
        "Disconnect",
        DISCONNECT_YES,
        DISCONNECT_NO,
    );
    let (title, about, body, footer): (&str, String, Element, Element) = match &ui.github.phase {
        Phase::Idle => return base,
        Phase::Waiting { code } => {
            let shown = match code {
                Some(code) => Element::row(vec![
                    Element::spacer().flex(1.0),
                    text(code.clone())
                        .font_family(limn::FontFamily::MonospaceBold)
                        .font_size(30.0),
                    Element::spacer().flex(1.0),
                ]),
                None => Element::row(vec![
                    Element::spacer().flex(1.0),
                    muted("Starting\u{2026}"),
                    Element::spacer().flex(1.0),
                ]),
            };
            (
                "Connect GitHub",
                "Enter this code on the GitHub page we opened. If it did not open, \
                 go to github.com/login/device."
                    .into(),
                shown
                    .padding(12.0)
                    .background(look::chart_tint())
                    .radius(8.0),
                Element::row(vec![
                    Element::spacer().flex(1.0),
                    keyed_button(CANCEL, "Cancel", "", false, true, palette),
                    keyed_button(COPY_CODE, "Copy code", "", true, code.is_some(), palette),
                ])
                .gap(8.0),
            )
        }
        Phase::Failed(message) => (
            "Could not connect GitHub",
            message.clone(),
            hgap(0.0),
            Element::row(vec![
                Element::spacer().flex(1.0),
                keyed_button(CLOSE, "Close", "", false, true, palette),
                keyed_button(CONNECT, "Try again", "", true, true, palette),
            ])
            .gap(8.0),
        ),
    };
    limn::Modal::new(base, true, title, about, CANCEL, palette)
        .width(420.0)
        .body(body)
        .footer(footer)
        .backdrop_dismiss(false)
        .scrim(look::scrim())
        .render()
}

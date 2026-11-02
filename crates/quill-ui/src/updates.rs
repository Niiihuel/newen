//! quill's own updates in the window: Settings' Updates group, the strip over the account
//! button once a version is ready, and the dialogs (agents still working, a mandatory
//! version). The engine checks and downloads; this shows what it reports. Desktop
//! announces a ready update in the sidebar only (no toast, no menu entry:
//! desk-updates-account.md, "No se encontró ningún toast"), and so does quill.

use crate::view::ago;
use crate::{icons, Store, Ui};
use limn::{Component, ComponentPalette, Element};
use quill_components::look::{self, BODY};
use quill_components::pieces::{hgap, icon_tool, keyed_button, muted, text};
use quill_components::settings::{setting, setting_switch, Setting};
use quill_model::{Event, Intent, SessionStatus, UpdateState};

pub const UPDATE_CHECK: &str = "update:check";
pub const UPDATE_DOWNLOAD: &str = "update:download";
pub const UPDATE_RETRY: &str = "update:retry";
/// The mandatory dialog's button: nothing to wait for, it installs.
pub const UPDATE_RESTART: &str = "update:restart";
/// The Settings row's own button; it asks first, like the strip.
pub const UPDATE_RESTART_ROW: &str = "update:restart:row";
/// The sidebar strip, "Relaunch to update" in Desktop.
pub const UPDATE_STRIP: &str = "update:strip";
/// The strip's small cancel while it waits for the agents.
pub const UPDATE_WAIT_CANCEL: &str = "update:wait:cancel";
/// "Agents are still working": Cancel, Wait for agents, Update anyway
/// (Desktop's dialog reads Cancel, Wait for Claude and Update anyway).
pub const UPDATE_ASK_CANCEL: &str = "update:ask:cancel";
pub const UPDATE_ASK_WAIT: &str = "update:ask:wait";
pub const UPDATE_ASK_NOW: &str = "update:ask:now";
/// The mandatory dialog's close: it has none, so a click here does nothing.
const UPDATE_HOLD: &str = "update:hold";

/// Keeps what the engine reports. A wait or a question about a version that is no longer
/// the ready, optional one has nothing left to ask.
pub(crate) fn heard(ui: &mut Ui, event: &Event) {
    let Event::Update(state) = event else {
        return;
    };
    if !matches!(
        state,
        UpdateState::Ready {
            mandatory: false,
            ..
        }
    ) {
        ui.update_asking = false;
        ui.restart_when_idle = false;
    }
    ui.update = state.clone();
}

/// An agent is at work (or waiting for the person), so restarting would cut it off.
fn busy(store: &Store) -> bool {
    store
        .sessions
        .iter()
        .any(|s| matches!(s.status, SessionStatus::Running | SessionStatus::Awaiting))
}

/// A restart the person asked for: at once when no agent is busy, else the question first
/// (Desktop asks the same when a session is running).
pub(crate) fn restart(store: &Store, ui: &mut Ui) -> Vec<Intent> {
    if busy(store) {
        ui.update_asking = true;
        vec![]
    } else {
        vec![Intent::InstallUpdate]
    }
}

/// "Wait for agents" kept: install the first time no session is busy. Called after every
/// event the store takes, which is every status change, so it needs no timer (Desktop polls
/// every 60 s, MAIN `H6t`).
pub fn settled(store: &Store, ui: &mut Ui) -> Vec<Intent> {
    if !ui.restart_when_idle || busy(store) {
        return vec![];
    }
    ui.restart_when_idle = false;
    vec![Intent::InstallUpdate]
}

/// The clicks of this module, or `None` for another's.
pub(crate) fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    match id {
        UPDATE_CHECK | UPDATE_RETRY => Some(vec![Intent::CheckUpdates]),
        UPDATE_DOWNLOAD => Some(vec![Intent::DownloadUpdate]),
        UPDATE_RESTART => Some(vec![Intent::InstallUpdate]),
        // The strip shows the version's announcement first, when it has one.
        UPDATE_STRIP => {
            Some(crate::whats_new::before_restart(ui).unwrap_or_else(|| restart(store, ui)))
        }
        UPDATE_RESTART_ROW => Some(restart(store, ui)),
        UPDATE_ASK_CANCEL | UPDATE_WAIT_CANCEL => {
            ui.update_asking = false;
            ui.restart_when_idle = false;
            Some(vec![])
        }
        UPDATE_ASK_WAIT => {
            ui.update_asking = false;
            ui.restart_when_idle = true;
            // The agents may have finished while the question was up.
            Some(settled(store, ui))
        }
        UPDATE_ASK_NOW => {
            ui.update_asking = false;
            Some(vec![Intent::InstallUpdate])
        }
        _ => None,
    }
}

/// A version that waits for the person: optional and downloaded.
pub(crate) fn ready(ui: &Ui) -> Option<&str> {
    match &ui.update {
        UpdateState::Ready {
            version,
            mandatory: false,
        } => Some(version),
        _ => None,
    }
}

/// Desktop's "strip" over the account button: 32 px
/// high, an 8 px radius, a resting fill. Checking and downloading show nothing here; Settings
/// has them. Waiting for the agents it reads as such, with a cancel.
pub(crate) fn sidebar_strip(ui: &Ui, palette: ComponentPalette) -> Option<Element> {
    let version = ready(ui)?;
    let waiting = ui.restart_when_idle;
    let label = if waiting {
        "Restarting when agents finish"
    } else {
        "Relaunch to update"
    };
    let mut row = vec![
        hgap(0.0),
        // Set as the sidebar's own rows ("New", "Search"): a 16 px icon in the text's color,
        // the label in the body face, what follows muted at 12.
        Element::icon(icons::of(icons::RELOAD))
            .foreground(look::palette().text)
            .size(Some(16.0), Some(16.0))
            .centered_y(),
        text(label).centered_y(),
        Element::spacer().flex(1.0),
    ];
    let strip = if waiting {
        row.push(icon_tool(
            UPDATE_WAIT_CANCEL,
            icons::of(icons::CLOSE),
            "Stop waiting",
            22.0,
            palette,
        ));
        row.push(hgap(3.0));
        Element::row(row)
            .gap(10.0)
            .size(None, Some(32.0))
            .background(look::row_hover())
            .radius(8.0)
    } else {
        row.push(muted(format!("v{version}")).font_size(12.0).centered_y());
        row.push(hgap(0.0));
        let row = Element::row(row).gap(10.0).size(None, Some(32.0));
        let surface = ComponentPalette {
            hover: look::tool_hover(),
            ..palette
        }
        .interactive_surface(Element::clickable(UPDATE_STRIP, row))
        .background(look::row_hover())
        .radius(8.0);
        // Desktop's tooltip: `{detail} · v{version}`.
        limn::Tooltip::new(
            surface,
            UPDATE_STRIP,
            format!("Relaunch to update \u{b7} v{version}"),
            look::palette(),
        )
        .render()
    };
    // The same insets as the account button's row; the divider is above, the account's own
    // padding below.
    Some(Element::column(vec![
        Element::spacer().size(None, Some(8.0)),
        Element::row(vec![hgap(8.0), strip.flex(1.0), hgap(8.0)]),
    ]))
}

/// The sidebar toggle's dot while the sidebar is hidden and a version waits: Desktop's
/// small dot (there only for a mandatory restart, here for
/// any ready version, as the spec asks), over the toggle's top right.
pub(crate) fn toggle_dot(ui: &Ui, toggle: Element) -> Element {
    if ui.sidebar.open || ready(ui).is_none() {
        return toggle;
    }
    let dot = Element::spacer()
        .background(look::handle())
        .radius(3.0)
        .size(Some(6.0), Some(6.0));
    let placed = Element::column(vec![
        Element::spacer().size(None, Some(3.0)),
        Element::row(vec![Element::spacer().flex(1.0), dot, hgap(3.0)]),
    ])
    .size(Some(26.0), Some(26.0));
    Element::stack(vec![toggle, placed])
        .size(Some(26.0), Some(26.0))
        .centered_y()
}

/// The version and what it says about itself, for the state line.
fn state_line(state: &UpdateState) -> String {
    match state {
        UpdateState::Idle => "quill has not checked for updates yet.".into(),
        UpdateState::Checking => "Checking for updates\u{2026}".into(),
        UpdateState::UpToDate { checked_at } => {
            format!("quill is up to date \u{b7} Checked {}", ago(*checked_at))
        }
        UpdateState::Available { version, .. } => format!("Version {version} is available"),
        UpdateState::Downloading {
            version,
            done,
            total,
            ..
        } => {
            let percent = if *total == 0 { 0 } else { done * 100 / total };
            format!("Downloading {version} \u{b7} {percent}%")
        }
        UpdateState::Ready { version, .. } => format!("Version {version} is ready to install"),
        UpdateState::Installed { version, .. } => format!("Installing {version}\u{2026}"),
        UpdateState::Failed { reason } => reason.clone(),
        UpdateState::FromSource => "Built from source \u{2014} update with git pull".into(),
        UpdateState::NotInstalled => "Run quill --install to keep it up to date".into(),
        UpdateState::NeedsAccess => {
            "Updates need access to the private repository (set QUILL_UPDATE_TOKEN or sign in with gh)"
                .into()
        }
    }
}

/// Settings' Updates group: the state and its buttons, the release notes when there are any,
/// and the switch that downloads on its own.
pub(crate) fn settings(ui: &Ui, auto_update: bool, palette: ComponentPalette) -> Vec<Setting> {
    let mut buttons = vec![];
    match &ui.update {
        UpdateState::Ready { .. } => buttons.push(keyed_button(
            UPDATE_RESTART_ROW,
            "Restart to update",
            "",
            true,
            true,
            palette,
        )),
        UpdateState::Available {
            mandatory: false, ..
        } => buttons.push(keyed_button(
            UPDATE_DOWNLOAD,
            "Download",
            "",
            false,
            true,
            palette,
        )),
        UpdateState::Failed { .. } => buttons.push(keyed_button(
            UPDATE_RETRY,
            "Retry",
            "",
            false,
            true,
            palette,
        )),
        _ => {}
    }
    if !matches!(
        ui.update,
        UpdateState::Checking | UpdateState::FromSource | UpdateState::NotInstalled
    ) {
        buttons.push(keyed_button(
            UPDATE_CHECK,
            "Check for updates",
            "",
            false,
            true,
            palette,
        ));
    }
    // The version row is the "quill" group's, above.
    let mut out = vec![setting(
        "Updates",
        &state_line(&ui.update),
        Element::row(buttons).gap(8.0),
    )];
    if let UpdateState::Available { notes, .. } = &ui.update {
        if !notes.trim().is_empty() {
            out.push(setting(
                "Release notes",
                "",
                limn::Markdown::new(notes, palette)
                    .width(460.0)
                    .font_size(BODY)
                    .render()
                    .element,
            ));
        }
    }
    out.push(setting(
        "Install updates automatically",
        "Download new versions in the background. Installing waits for you.",
        setting_switch("auto_update", auto_update, palette),
    ));
    out.push(setting(
        "Show what's new",
        "Show a release's announcement before restarting into it and once after. The release notes stay in the account menu.",
        setting_switch("whats_new", ui.settings.whats_new, palette),
    ));
    out
}

/// The question a restart with agents at work raises, and the dialog a mandatory version
/// puts up, over `base`.
pub(crate) fn dialogs(ui: &Ui, base: Element, palette: ComponentPalette) -> Element {
    // Desktop asks "Claude is still working"; quill names no vendor, so
    // "agents". Wait is the primary button here (Desktop's is Update anyway; the spec's choice).
    let base = limn::Modal::new(
        base,
        ui.update_asking,
        "Agents are still working",
        "Updating now stops them. You can wait and quill restarts as soon as they finish.",
        UPDATE_ASK_CANCEL,
        palette,
    )
    .width(440.0)
    .scrim(look::scrim())
    .footer(
        Element::row(vec![
            Element::spacer().flex(1.0),
            keyed_button(UPDATE_ASK_CANCEL, "Cancel", "", false, true, palette),
            keyed_button(UPDATE_ASK_NOW, "Update anyway", "", false, true, palette),
            keyed_button(UPDATE_ASK_WAIT, "Wait for agents", "", true, true, palette),
        ])
        .gap(8.0),
    )
    .render();
    let version = match &ui.update {
        UpdateState::Available {
            version,
            mandatory: true,
            ..
        }
        | UpdateState::Downloading {
            version,
            mandatory: true,
            ..
        }
        | UpdateState::Ready {
            version,
            mandatory: true,
        } => version,
        _ => return base,
    };
    let (description, body, footer): (String, Element, Option<Element>) = match &ui.update {
        UpdateState::Downloading { done, total, .. } => {
            let fraction = if *total == 0 {
                0.0
            } else {
                *done as f32 / *total as f32
            };
            (
                "quill needs this version before it opens. It is downloading now.".to_owned(),
                limn::Progress::new(fraction, palette).render(),
                None,
            )
        }
        _ => (
            "Restart quill to finish updating. It opens in the new version.".to_owned(),
            hgap(0.0),
            Some(Element::row(vec![
                Element::spacer().flex(1.0),
                keyed_button(UPDATE_RESTART, "Restart to update", "", true, true, palette),
            ])),
        ),
    };
    let modal = limn::Modal::new(
        base,
        true,
        format!("quill {version} is required"),
        description,
        UPDATE_HOLD,
        palette,
    )
    .width(440.0)
    .body(Element::column(vec![body, muted(state_line(&ui.update)).font_size(BODY)]).gap(8.0))
    .closable(false)
    .backdrop_dismiss(false)
    .scrim(look::scrim());
    match footer {
        Some(footer) => modal.footer(footer).render(),
        None => modal.render(),
    }
}

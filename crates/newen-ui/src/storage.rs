//! Settings › Storage, and `/storage`: what Newen keeps on this computer by kind, as Desktop's
//! Storage lists it (docs/ui.md §2.11e): the total with Refresh and Open folder, then each kind
//! with its size, its Details, and what of it can go. Nothing goes without a question.
use crate::{view, Session, Store, Ui};
use limn::{Component, Element};
use newen_components::pieces::{confirm, keyed_button, muted, text};
use newen_components::settings::{setting, Setting, SettingsPage};
use newen_components::{icons, look};
use newen_model::{Event, Intent, SessionId, StorageClear, STORAGE_IDLE_DAYS as IDLE_DAYS};
use std::collections::HashMap;
use std::path::PathBuf;

/// The page's key, and what `/storage` reports.
pub const PAGE: &str = "storage";
pub const OPEN: &str = "storage:open";
const REFRESH: &str = "storage:refresh";
const DETAILS: &str = "storage:details:";
const CLEAR: &str = "storage:clear:";
const CONFIRM: &str = "storage:confirm";
const CANCEL: &str = "storage:cancel";
/// The most rows a kind's Details lists.
const LISTED: usize = 12;
/// A kind's size column, and its controls: the size, Details and what can go.
const SIZE: f32 = 76.0;
const CONTROLS: f32 = 376.0;

/// A kind of what Newen keeps, as the page names it in its ids.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    Sessions,
    History,
    Worktrees,
    App,
    Caches,
}

impl Part {
    const ALL: [Part; 5] = [
        Part::Sessions,
        Part::History,
        Part::Worktrees,
        Part::App,
        Part::Caches,
    ];

    fn key(self) -> &'static str {
        match self {
            Part::Sessions => "sessions",
            Part::History => "history",
            Part::Worktrees => "worktrees",
            Part::App => "app",
            Part::Caches => "caches",
        }
    }

    fn of(key: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.key() == key)
    }
}

/// What the page is doing: the kinds whose Details are open, the one asked about, the one
/// being freed, and the sessions still to be deleted before it measures again.
#[derive(Default)]
pub struct View {
    details: Vec<Part>,
    confirming: Option<Part>,
    working: Option<Part>,
    measuring: bool,
    deleting: Vec<SessionId>,
    folders_after: Vec<(PathBuf, bool)>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// No message for 30 days, not pinned and not at work; archived or not. One whose messages
/// carry no time is never inactive: nothing says how old it is.
pub fn inactive(session: &Session, now: u64) -> bool {
    inactive_since(session, session.last_message_at(), now)
}

fn inactive_since(session: &Session, last: Option<u64>, now: u64) -> bool {
    !session.working()
        && !session.pinned
        && last.is_some_and(|at| now.saturating_sub(at) >= IDLE_DAYS * 86_400)
}

/// The sessions' folders, each with whether a session that is not inactive works in it.
fn folders(store: &Store, leaving: &[SessionId]) -> Vec<(PathBuf, bool)> {
    let now = now();
    let mut folders: Vec<(PathBuf, bool)> = vec![];
    for session in store.sessions.iter().filter(|s| !leaving.contains(&s.id)) {
        let active = !inactive(session, now);
        match folders.iter_mut().find(|(f, _)| *f == session.cwd) {
            Some((_, any)) => *any |= active,
            None => folders.push((session.cwd.clone(), active)),
        }
    }
    folders
}

/// A measure, unless one is under way.
fn measure(store: &Store, ui: &mut Ui) -> Vec<Intent> {
    if std::mem::replace(&mut ui.storage_view.measuring, true) {
        return vec![];
    }
    vec![Intent::MeasureStorage {
        folders: folders(store, &[]),
    }]
}

/// What a click on the page does; `None` for one not its own.
pub(crate) fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if id == OPEN || id == format!("settings:page:{PAGE}") {
        ui.account_menu = false;
        ui.settings_page = Some(PAGE.into());
        ui.settings_query.clear();
        return Some(measure(store, ui));
    }
    if id == REFRESH {
        return Some(measure(store, ui));
    }
    if let Some(part) = id.strip_prefix(DETAILS).and_then(Part::of) {
        let open = &mut ui.storage_view.details;
        match open.iter().position(|p| *p == part) {
            Some(at) => {
                open.remove(at);
            }
            None => open.push(part),
        }
        return Some(vec![]);
    }
    if let Some(part) = id.strip_prefix(CLEAR).and_then(Part::of) {
        ui.storage_view.confirming = Some(part);
        return Some(vec![]);
    }
    if id == CANCEL {
        ui.storage_view.confirming = None;
        return Some(vec![]);
    }
    if id != CONFIRM {
        return None;
    }
    let part = ui.storage_view.confirming.take()?;
    let view = &mut ui.storage_view;
    let clear = |clear| {
        vec![Intent::ClearStorage {
            clear,
            folders: folders(store, &[]),
        }]
    };
    let intents = match part {
        Part::Sessions => {
            let now = now();
            let gone: Vec<_> = (store.sessions.iter())
                .filter(|s| inactive(s, now))
                .map(|s| s.id)
                .collect();
            // Measured again once the last one is deleted.
            view.folders_after = folders(store, &gone);
            view.deleting = gone.clone();
            gone.into_iter()
                .map(|session| Intent::Delete { session })
                .collect()
        }
        Part::History => clear(StorageClear::History),
        Part::Worktrees => {
            let unused = (ui.storage.iter())
                .flat_map(|r| &r.worktrees)
                .filter(|w| w.unused)
                .map(|w| w.path.clone())
                .collect();
            clear(StorageClear::Worktrees(unused))
        }
        Part::Caches => clear(StorageClear::Caches),
        Part::App => vec![],
    };
    if !intents.is_empty() {
        ui.storage_view.working = Some(part);
    }
    Some(intents)
}

/// What the engine said: a new measure, or one of the sessions being freed gone.
pub(crate) fn heard(ui: &mut Ui, event: &Event) -> Vec<Intent> {
    match event {
        Event::Storage(report) => {
            // Logos cleared are asked for again when shown.
            if ui.storage_view.working == Some(Part::Caches) {
                ui.logos.clear();
                ui.logo_asked.clear();
            }
            ui.storage = Some(report.clone());
            ui.storage_view.measuring = false;
            ui.storage_view.working = None;
            vec![]
        }
        Event::Deleted { session } if ui.storage_view.deleting.contains(session) => {
            let view = &mut ui.storage_view;
            view.deleting.retain(|s| s != session);
            if !view.deleting.is_empty() {
                return vec![];
            }
            view.measuring = true;
            vec![Intent::MeasureStorage {
                folders: std::mem::take(&mut view.folders_after),
            }]
        }
        _ => vec![],
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn days(n: u64) -> String {
    match n {
        0 => "today".into(),
        1 => "yesterday".into(),
        n => format!("{n} days ago"),
    }
}

/// One line of a kind's Details: its name, a word on it, and its size.
fn detail(name: String, note: String, bytes: u64) -> Element {
    Element::row(vec![
        Element::column(vec![
            text(name).font_size(look::SMALL),
            muted(note).font_size(look::SMALL),
        ])
        .gap(2.0)
        .flex(1.0),
        muted(view::human_bytes(bytes))
            .font_size(look::SMALL)
            .centered_y(),
    ])
    .gap(12.0)
}

/// A kind's row: its name and what it holds, then its size, Details, and what can go.
fn row(ui: &Ui, part: Part, title: &str, about: String, bytes: u64, free: Option<&str>) -> Setting {
    let palette = look::palette();
    // The size in a column of its own at the start, and Details at the end, at the page's right
    // edge as Refresh and Open folder are: every kind's line up, what can go just before Details.
    // Under it, a meter of its share of what Newen keeps, in the same column: they line up.
    let total = ui.storage.as_ref().map_or(0, |report| report.total);
    let meter = limn::ComponentPalette {
        text: look::muted(),
        selected: look::divider(),
        ..palette
    };
    let size = Element::column(vec![
        Element::row(vec![
            Element::spacer().flex(1.0),
            muted(view::human_bytes(bytes)),
        ]),
        limn::Progress::new(bytes as f32 / total.max(1) as f32, meter)
            .id(format!("storage:share:{}", part.key()))
            .flat()
            .width(SIZE)
            .height(3.0)
            .render()
            .accessibility_label(format!("{title}: share of what Newen keeps")),
    ])
    .gap(4.0)
    .size(Some(SIZE), None);
    let mut controls = vec![size.centered_y(), Element::spacer().flex(1.0)];
    if ui.storage_view.working == Some(part) {
        controls.push(
            limn::Spinner::new(format!("storage:working:{}", part.key()), look::muted())
                .size(14.0)
                .render()
                .centered_y(),
        );
    } else if let Some(label) = free {
        let id = format!("{CLEAR}{}", part.key());
        controls.push(keyed_button(&id, label, "", false, true, palette).centered_y());
    }
    let open = ui.storage_view.details.contains(&part);
    controls.push(
        keyed_button(
            &format!("{DETAILS}{}", part.key()),
            if open { "Hide details" } else { "Details" },
            "",
            false,
            true,
            palette,
        )
        .centered_y(),
    );
    setting(
        title,
        &about,
        Element::row(controls).gap(8.0).size(Some(CONTROLS), None),
    )
}

/// The page: the total, then each kind.
pub(crate) fn page(store: &Store, ui: &Ui) -> SettingsPage {
    let palette = look::palette();
    let refresh = match ui.storage_view.measuring {
        true => limn::Spinner::new("storage:refreshing", look::muted())
            .size(14.0)
            .render()
            .centered_y(),
        false => keyed_button(REFRESH, "Refresh", "", false, true, palette),
    };
    let buttons = Element::row(vec![
        refresh,
        keyed_button(
            view::SETTINGS_STORAGE_OPEN,
            "Open folder",
            "",
            false,
            true,
            palette,
        ),
    ])
    .gap(8.0);
    let Some(report) = &ui.storage else {
        let waiting = setting(
            "Measuring what Newen keeps\u{2026}",
            "It walks Newen's folder and your sessions' repositories.",
            limn::Spinner::new("storage:measuring", look::muted())
                .size(14.0)
                .render(),
        );
        return SettingsPage {
            key: PAGE.into(),
            title: "Storage".into(),
            icon: icons::HARD_DRIVE,
            groups: vec![("On this computer".into(), vec![waiting])],
        };
    };
    let mut total = setting(
        &format!("Newen uses {}", view::human_bytes(report.total)),
        &format!(
            "In {}. {}",
            report.root.display(),
            if ui.storage_view.measuring {
                "Measuring again\u{2026}"
            } else {
                "Measured when this page opened."
            }
        ),
        buttons,
    );
    if !report.failed.is_empty() {
        let lines = (report.failed.iter())
            .map(|why| muted(format!("Not freed: {why}")).font_size(look::SMALL))
            .collect();
        total = total.below(Element::column(lines).gap(4.0));
    }
    let shows = |part: Part| ui.storage_view.details.contains(&part);
    let mut kinds = vec![];

    // Sessions: the inactive ones first, then the largest; each looked at once a frame.
    let now = now();
    let sizes: HashMap<SessionId, u64> = report.by_session.iter().copied().collect();
    let mut sessions: Vec<(&Session, u64, Option<u64>, bool)> = (store.sessions.iter())
        .map(|s| {
            let last = s.last_message_at();
            let size = sizes.get(&s.id).copied().unwrap_or(0);
            (s, size, last, inactive_since(s, last, now))
        })
        .collect();
    sessions.sort_by_key(|(_, size, _, idle)| (!idle, std::cmp::Reverse(*size)));
    let idle = sessions.iter().filter(|s| s.3).count();
    let mut about = format!(
        "{}: their conversations and the pictures their tools gave back.",
        plural(sessions.len(), "session", "sessions")
    );
    if idle > 0 {
        about += &format!(" {idle} inactive for {IDLE_DAYS} days.");
    }
    let mut sessions_row = row(
        ui,
        Part::Sessions,
        "Sessions",
        about,
        report.sessions,
        (idle > 0).then_some("Delete inactive"),
    );
    if shows(Part::Sessions) {
        let lines: Vec<Element> = (sessions.iter().take(LISTED))
            .map(|(s, size, last, idle)| {
                let when = last.map_or("No date".to_owned(), |at| {
                    format!("Last message {}", days(now.saturating_sub(at) / 86_400))
                });
                let note = if *idle {
                    format!("{when} · inactive")
                } else {
                    when
                };
                detail(s.title.clone(), note, *size)
            })
            .collect();
        sessions_row = sessions_row.below(listed(lines, sessions.len()));
    }
    kinds.push(sessions_row);

    let unused = report.history.iter().filter(|h| !h.used).count();
    let mut about =
        "What each folder's files were before a turn, for the changes and Rewind.".to_owned();
    if unused > 0 {
        about += &format!(
            " {} no session works in.",
            plural(unused, "folder", "folders")
        );
    }
    let bytes = report.history.iter().map(|h| h.bytes).sum();
    let mut history = row(
        ui,
        Part::History,
        "File history",
        about,
        bytes,
        (unused > 0).then_some("Remove unused"),
    );
    if shows(Part::History) {
        let lines: Vec<Element> = (report.history.iter().take(LISTED))
            .map(|h| {
                let note = if h.used {
                    "A session works in it"
                } else {
                    "No session works in it"
                };
                detail(h.name.clone(), note.into(), h.bytes)
            })
            .collect();
        history = history.below(listed(lines, report.history.len()));
    }
    kinds.push(history);

    if !report.worktrees.is_empty() {
        let unused = report.worktrees.iter().filter(|w| w.unused).count();
        let mut about = format!(
            "{} of your sessions' repositories, made by agents or by you.",
            plural(report.worktrees.len(), "worktree", "worktrees")
        );
        if unused > 0 {
            about += &format!(
                " {unused} unused: no changes, no active session in or around them, and \
                 untouched for {IDLE_DAYS} days."
            );
        }
        let bytes = report.worktrees.iter().map(|w| w.bytes).sum();
        let mut worktrees = row(
            ui,
            Part::Worktrees,
            "Worktrees",
            about,
            bytes,
            (unused > 0).then_some("Remove unused"),
        );
        if shows(Part::Worktrees) {
            let lines: Vec<Element> = (report.worktrees.iter().take(LISTED))
                .map(|w| {
                    let mut note = vec![w.branch.clone().unwrap_or_else(|| "No branch".into())];
                    note.push(if w.clean { "no changes" } else { "changes" }.into());
                    note.push(format!("changed {}", days(w.idle_days)));
                    if w.unused {
                        note.push("unused".into());
                    }
                    detail(w.path.display().to_string(), note.join(" · "), w.bytes)
                })
                .collect();
            worktrees = worktrees.below(listed(lines, report.worktrees.len()));
        }
        kinds.push(worktrees);
    }

    let simple = |part: Part, title: &str, about: &str, items: &[(String, u64)], free| {
        let bytes = items.iter().map(|(_, b)| b).sum();
        let mut kind = row(ui, part, title, about.into(), bytes, free);
        if shows(part) {
            let lines = (items.iter())
                .map(|(name, bytes)| detail(name.clone(), String::new(), *bytes))
                .collect();
            kind = kind.below(listed(lines, items.len()));
        }
        kind
    };
    if !report.app.is_empty() {
        kinds.push(simple(
            Part::App,
            "Newen",
            "Its installed versions (this one, and the one before it to go back to), and the \
             programs Newen downloaded. Nothing here to free.",
            &report.app,
            None,
        ));
    }
    kinds.push(simple(
        Part::Caches,
        "Caches and logs",
        "Logos and the browser's cache are fetched again when needed; sign-ins and logs stay.",
        &report.caches,
        (report.clearable_caches > 0).then_some("Clear"),
    ));
    SettingsPage {
        key: PAGE.into(),
        title: "Storage".into(),
        icon: icons::HARD_DRIVE,
        groups: vec![
            ("On this computer".into(), vec![total]),
            ("What it keeps".into(), kinds),
        ],
    }
}

/// A Details list, saying how many more there are past the first rows.
fn listed(mut lines: Vec<Element>, all: usize) -> Element {
    if lines.is_empty() {
        lines.push(muted("Nothing here.").font_size(look::SMALL));
    }
    if all > LISTED {
        lines.push(muted(format!("And {} more.", all - LISTED)).font_size(look::SMALL));
    }
    Element::column(lines).gap(8.0)
}

/// The question before something goes.
pub(crate) fn dialog(store: &Store, ui: &Ui, base: Element) -> Element {
    let Some(part) = ui.storage_view.confirming else {
        return base;
    };
    let report = ui.storage.as_ref();
    let (title, words, action) = match part {
        Part::Sessions => {
            let now = now();
            let n = store.sessions.iter().filter(|s| inactive(s, now)).count();
            (
                format!(
                    "Delete {}?",
                    plural(n, "inactive session", "inactive sessions")
                ),
                format!(
                    "Sessions with no message for {IDLE_DAYS} days, not pinned and not at work. \
                     Their conversations and pictures go from this computer; their folders and \
                     files stay."
                ),
                "Delete",
            )
        }
        Part::History => {
            let n = report.map_or(0, |r| r.history.iter().filter(|h| !h.used).count());
            (
                format!(
                    "Remove the file history of {}?",
                    plural(n, "folder", "folders")
                ),
                "No session works in them, so nothing shows it. The folders and their files stay."
                    .into(),
                "Remove",
            )
        }
        Part::Worktrees => {
            let unused: Vec<String> = (report.iter())
                .flat_map(|r| &r.worktrees)
                .filter(|w| w.unused)
                .map(|w| w.path.display().to_string())
                .collect();
            // Each one by its path: what git ignores in it goes with it.
            let mut named = unused
                .iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if unused.len() > 5 {
                named += &format!(" and {} more", unused.len() - 5);
            }
            (
                format!(
                    "Remove {}?",
                    plural(unused.len(), "unused worktree", "unused worktrees")
                ),
                format!(
                    "{named}. None has uncommitted changes or commits no branch or tag holds, \
                     no active session works in or around them, and none changed for \
                     {IDLE_DAYS} days. Their branches stay; the files git ignores in them, like \
                     builds or a .env, go with them."
                ),
                "Remove",
            )
        }
        Part::Caches => (
            "Clear caches?".into(),
            "Logos are fetched again when shown, and the browser's cache is cleared the next \
             time the browser starts. Your sign-ins and the logs stay."
                .into(),
            "Clear",
        ),
        Part::App => return base,
    };
    confirm(base, true, &title, &words, action, CONFIRM, CANCEL)
}

//! The "@" list over the composer, as Desktop's mention menu
//! for the trigger, `epitaxy-sessions` and `ccd-files` for the rows; spec §2). Its rows are the
//! other sessions first, then the files of the session's folder; a pick writes `@path ` for a
//! file, and `@[title] ` for a session, which the next task reads (not yet sent).

use crate::{view, Session, Store, Ui};
use limn::{Component, Element, Key, SuggestionItem, SuggestionList, UiEvent};
use newen_components::look;
use newen_components::{icons, tree};
use newen_model::{Event, Intent, SessionId};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// Most characters a query may have; a longer one keeps the list shut.
const MAX_QUERY: usize = 50;
/// Sessions listed with an empty query, and with one; files listed at most.
const SESSIONS_EMPTY: usize = 3;
const SESSIONS: usize = 8;
const FILES: usize = 50;
/// The row's id and item, as the list shows them.
type Row = (String, SuggestionItem);

/// The `@` the caret is in, as its byte index and the query after it. The caret is taken to be
/// at the end of the prompt, as the `/` palette does. The `@` must start the prompt or follow
/// whitespace, so a token that starts with `/` never matches here.
pub fn mention_query(prompt: &str) -> Option<(usize, &str)> {
    let start = prompt
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(at, c)| at + c.len_utf8());
    let query = prompt[start..].strip_prefix('@')?;
    (query.chars().count() <= MAX_QUERY).then_some((start, query))
}

/// The bytes of `label` that `query` matches, for the list to draw bold. Matched on the ASCII
/// lowercase, which keeps the byte offsets.
fn matched(label: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return vec![];
    }
    let at = label.to_ascii_lowercase().find(&query.to_ascii_lowercase());
    at.map(|at| at..at + query.len()).into_iter().collect()
}

/// The other sessions that match `query`, most recent first: not archived, not the one on
/// screen, with a title. The query matches the title, the title without spaces, or the branch.
fn session_rows(store: &Store, ui: &Ui, query: &str) -> Vec<Row> {
    let wanted = query.to_lowercase();
    let mut found: Vec<&Session> = store
        .sessions
        .iter()
        .filter(|s| !s.archived && Some(s.id) != ui.selected && !s.title.is_empty())
        .filter(|s| {
            let title = s.title.to_lowercase();
            title.contains(&wanted)
                || title
                    .split_whitespace()
                    .collect::<String>()
                    .contains(&wanted)
                || s.branch
                    .as_deref()
                    .is_some_and(|b| b.to_lowercase().contains(&wanted))
        })
        .collect();
    found.sort_by_key(|s| std::cmp::Reverse(s.touched));
    let shown = if query.is_empty() {
        SESSIONS_EMPTY
    } else {
        SESSIONS
    };
    found
        .into_iter()
        .take(shown)
        .map(|s| {
            let folder = s.cwd.file_name().unwrap_or(s.cwd.as_os_str());
            let mut detail = folder.to_string_lossy().into_owned();
            if let Some(branch) = &s.branch {
                detail = format!("{detail} · {branch}");
            }
            let id = format!("mention:session:{}", s.id);
            let item = SuggestionItem::new(id.clone(), s.title.as_str())
                .icon(icons::of(icons::CHAT))
                .detail(detail)
                .matched(matched(&s.title, query));
            (id, item)
        })
        .collect()
}

/// A file or folder row: its name, its parent folder muted at the end.
fn file_row(path: &Path, query: &str, dir: bool) -> Row {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let icon = if dir {
        icons::FOLDER
    } else {
        tree::glyph(path)
    };
    let id = format!("mention:file:{}", path.display());
    let item = SuggestionItem::new(id.clone(), name.as_str())
        .icon(icons::of(icon))
        .detail(parent)
        .matched(matched(&name, query));
    (id, item)
}

/// The files for `query`, from the session on screen. The engine's answer is kept only for the
/// query it was asked for. With an empty query the folder's top level is listed (folders, then
/// files), from the Files pane's tree when that holds the folder's listing; the disk is not read
/// here. With no pane, the engine's top level. A new session's are those of the folder it
/// starts in.
fn file_rows(ui: &Ui, query: &str) -> Vec<Row> {
    let row = |path: &PathBuf| {
        let dir = path.as_os_str().to_string_lossy().ends_with('/');
        file_row(path, query, dir)
    };
    let Some(session) = ui.selected else {
        return match &ui.mention_folder_found {
            Some((folder, asked, paths)) if *folder == ui.cwd && asked == query => {
                paths.iter().take(FILES).map(row).collect()
            }
            _ => vec![],
        };
    };
    // Without the Files pane's listing, the engine's top level (folders end in `/`).
    let found = || match &ui.mention_found {
        Some((at, asked, paths)) if *at == session && asked == query => (paths.iter())
            .take(FILES)
            .map(|path| {
                let dir = path.as_os_str().to_string_lossy().ends_with('/');
                file_row(path, query, dir)
            })
            .collect(),
        _ => vec![],
    };
    if query.is_empty() {
        let Some(pane) = ui
            .files
            .get(&session)
            .filter(|p| p.tree.listed.contains_key(&p.tree.root))
        else {
            return found();
        };
        let root = &pane.tree.root;
        return (pane.tree.listed.get(root).into_iter().flatten())
            .take(FILES)
            .map(|entry| {
                let rel = entry.path.strip_prefix(root).unwrap_or(&entry.path);
                file_row(rel, query, entry.dir)
            })
            .collect();
    }
    found()
}

/// The rows for `query`: the sessions, then the files.
fn rows(store: &Store, ui: &Ui, query: &str) -> Vec<Row> {
    let mut rows = session_rows(store, ui, query);
    rows.extend(file_rows(ui, query));
    rows
}

/// The list while it is open: the `@`'s byte index and its rows. Shut when the same `@query`
/// was dismissed, or when nothing matches. A `@` token never starts with `/`, so this and the
/// "/" palette cannot open on the same token.
pub(crate) fn open(store: &Store, ui: &Ui) -> Option<(usize, Vec<Row>)> {
    let (at, query) = mention_query(&ui.prompt)?;
    if ui.mention_dismissed.as_deref() == Some(&ui.prompt[at..]) {
        return None;
    }
    let rows = rows(store, ui, query);
    (!rows.is_empty()).then_some((at, rows))
}

/// The find the query asks of the engine, when it changed from `before` (the query the prompt
/// had).
pub(crate) fn asked(ui: &Ui, before: Option<&str>) -> Vec<Intent> {
    let Some((_, query)) = mention_query(&ui.prompt) else {
        return vec![];
    };
    // An empty query is asked once too: the engine answers with the folder's top level.
    if before == Some(query) {
        return vec![];
    }
    let Some(session) = ui.selected else {
        // A new session, in the folder it starts in.
        return match ui.cwd.as_os_str().is_empty() {
            true => vec![],
            false => vec![Intent::FindFolderFiles {
                folder: ui.cwd.clone(),
                query: query.to_owned(),
            }],
        };
    };
    vec![Intent::FindFiles {
        session,
        query: query.to_owned(),
        contents: false,
    }]
}

/// The engine's answer for the file search of the query on screen. The Files pane reads the same
/// event for its own filter; each keeps only its own query.
pub(crate) fn heard(ui: &mut Ui, event: &Event) {
    if let Event::FolderFilesFound {
        folder,
        query,
        paths,
    } = event
    {
        let current = mention_query(&ui.prompt).map(|(_, query)| query);
        if ui.selected.is_none() && *folder == ui.cwd && current == Some(query.as_str()) {
            ui.mention_folder_found = Some((folder.clone(), query.clone(), paths.clone()));
        }
        return;
    }
    let Event::FilesFound {
        session,
        query,
        contents: false,
        paths,
        ..
    } = event
    else {
        return;
    };
    let current = mention_query(&ui.prompt).map(|(_, query)| query);
    if ui.selected == Some(*session) && current == Some(query.as_str()) {
        ui.mention_found = Some((*session, query.clone(), paths.clone()));
    }
}

/// The list over `anchor` while it is open: rows as the "/" palette's, 560 wide with each
/// session's folder after its title, as Desktop's mention popup.
pub fn list(store: &Store, ui: &Ui, anchor: Element, width: f32) -> Element {
    let Some((_, rows)) = open(store, ui) else {
        return anchor;
    };
    let selected = ui.mention_selected.min(rows.len() - 1);
    let items = rows.into_iter().map(|(_, item)| item).collect();
    SuggestionList::new(
        anchor,
        "mention",
        items,
        selected,
        true,
        "mention:dismiss",
        look::suggestion_palette(),
    )
    .compact(true)
    .inline_details(true)
    .sizing(newen_components::pieces::menu_sizing(560.0))
    .max_width(560.0)
    .width(width)
    .placement(limn::PopoverPlacement::TopStart)
    .render()
}

/// A key the list takes before limn while it is open over the focused composer: the arrows move
/// (no wrap), Enter picks (Tab does not: Desktop's Code mentions have no accept-on-Tab), Escape
/// shuts it for this `@query`. `false` otherwise, so Enter sends and the rest reaches the composer.
pub fn key(store: &Store, ui: &mut Ui, focused: Option<&str>, event: &UiEvent) -> bool {
    if focused != Some(view::COMPOSER) {
        return false;
    }
    let UiEvent::Key {
        key,
        ctrl: false,
        shift,
        ..
    } = event
    else {
        return false;
    };
    let Some((at, rows)) = open(store, ui) else {
        return false;
    };
    let last = rows.len() - 1;
    let selected = ui.mention_selected.min(last);
    match key {
        Key::ArrowDown => ui.mention_selected = (selected + 1).min(last),
        Key::ArrowUp => ui.mention_selected = selected.saturating_sub(1),
        Key::Enter if !shift => pick(store, ui, &rows[selected].0),
        Key::Escape => ui.mention_dismissed = Some(ui.prompt[at..].to_owned()),
        _ => return false,
    }
    true
}

/// The list's clicks: a row picked, or a press outside it, which shuts it for this `@query`.
/// `true` when `id` is one of its.
pub fn clicked(store: &Store, ui: &mut Ui, id: &str) -> bool {
    if id == "mention:dismiss" {
        ui.mention_dismissed = mention_query(&ui.prompt).map(|(at, _)| ui.prompt[at..].to_owned());
        return true;
    }
    if id.starts_with("mention:session:") || id.starts_with("mention:file:") {
        pick(store, ui, id);
        return true;
    }
    false
}

/// The sessions a sent message names: each one whose `@[title]` is still in `text`, by where
/// that first appears.
pub(crate) fn mentions_in(
    text: &str,
    mentioned: &[(SessionId, String)],
) -> Vec<(SessionId, String)> {
    let mut named: Vec<(usize, (SessionId, String))> = mentioned
        .iter()
        .filter_map(|(session, title)| {
            let at = text.find(&format!("@[{title}]"))?;
            Some((at, (*session, title.clone())))
        })
        .collect();
    named.sort_by_key(|(at, _)| *at);
    named.into_iter().map(|(_, mention)| mention).collect()
}

/// Picks row `id` in place of the `@query`: a file as `@path `, a session as `@[title] ` and
/// recorded in `mentioned_sessions` for the next task.
fn pick(store: &Store, ui: &mut Ui, id: &str) {
    let Some((at, _)) = mention_query(&ui.prompt) else {
        return;
    };
    let head = ui.prompt[..at].to_owned();
    ui.mention_selected = 0;
    ui.mention_dismissed = None;
    if let Some(session) = id
        .strip_prefix("mention:session:")
        .and_then(|s| s.parse::<SessionId>().ok())
    {
        let Some(title) = store.session(session).map(|s| s.title.clone()) else {
            return;
        };
        ui.restore_prompt = Some(format!("{head}@[{title}] "));
        if !ui.mentioned_sessions.iter().any(|(s, _)| *s == session) {
            ui.mentioned_sessions.push((session, title));
        }
    } else if let Some(path) = id.strip_prefix("mention:file:") {
        ui.restore_prompt = Some(format!("{head}@{path} "));
    }
}

//! The Files pane, as Desktop's file tile: the session's folder as
//! a tree (`newen_components::tree`, in Desktop's look) beside the files open in it, in tabs,
//! each in quill's editor or, for Markdown, rendered. The engine lists the folder, runs its file
//! operations and reads and writes its files (`Intent::ListDir`, `FileOp`, `ReadFile`,
//! `WriteFile`).

use crate::{view, Document, Pane, Store, Ui};
use limn::UiAction;
use newen_components::find::{self, Find};
use newen_components::tree::{self, FileTree, Outcome, TreeKey};
use newen_model::{AgentEvent, Event, Intent, SessionId};
use std::path::{Path, PathBuf};

/// Desktop's "Show file tree" / "Hide file tree", in the tab strip.
pub const TREE_TOGGLE: &str = "files:tree";
/// The file's tools: read it again, give up the edits, copy it, copy its path.
pub const RELOAD: &str = "doc:reload";
pub const REVERT: &str = "doc:revert";
pub const COPY: &str = "doc:copy";
pub const COPY_PATH: &str = "doc:copy-path";
/// "Discard unsaved changes?" over a tab closed with edits not saved.
pub const DISCARD: &str = "doc:discard";
pub const KEEP: &str = "doc:keep";
/// The tree's filter, Desktop's "Filter, or ? to search in files",
/// and its "Clear filter". A line found is `found:{path}:{row}`, its path from the folder.
pub const FILTER_FIELD: &str = "tree:filter";
pub const FILTER_CLEAR: &str = "tree:filter:clear";
pub const FOUND_LINE: &str = "found:";

/// What the filter found for its words: the paths that match them, or after `?` the lines
/// that hold them.
pub struct Found {
    pub query: String,
    pub contents: bool,
    pub paths: Vec<PathBuf>,
    pub hits: Vec<newen_files::find::Hit>,
}

/// The filter's words as the engine takes them: after `?` a search in the files' contents.
pub fn filter_query(filter: &str) -> (&str, bool) {
    match filter.strip_prefix('?') {
        Some(words) => (words.trim(), true),
        None => (filter.trim(), false),
    }
}

/// A session's Files pane: its folder's tree, the files open in tabs and the one on screen.
pub struct FilesPane {
    pub tree: FileTree,
    pub tabs: Vec<Document>,
    pub active: Option<usize>,
    /// The tree beside the files, on until hidden, as Desktop's `file_tree` option.
    pub tree_open: bool,
    /// Finding in the file on screen.
    pub find: Option<Find>,
    /// A tab with edits not saved, waiting on "Discard unsaved changes?".
    pub discarding: Option<usize>,
    /// The tree's filter and what it found for the words it has now.
    pub filter: String,
    pub found: Option<Found>,
    /// A line to put the caret on once its file is read: its path from the folder and row.
    pub goto: Option<(String, usize)>,
}

impl FilesPane {
    fn new(root: PathBuf) -> (Self, Vec<tree::Job>) {
        let (tree, jobs) = FileTree::new(root);
        let pane = Self {
            tree,
            tabs: vec![],
            active: None,
            tree_open: true,
            find: None,
            discarding: None,
            filter: String::new(),
            found: None,
            goto: None,
        };
        (pane, jobs)
    }

    /// The file on screen.
    pub fn document(&self) -> Option<&Document> {
        self.tabs.get(self.active?)
    }

    /// Its path in the disk's terms.
    pub fn absolute(&self, document: &Document) -> PathBuf {
        self.tree.root.join(&document.path)
    }

    /// Close the tab at `at`; the one beside it comes on screen.
    fn close(&mut self, at: usize) {
        if at >= self.tabs.len() {
            return;
        }
        self.tabs.remove(at);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > at => Some(active - 1),
            Some(active) if active == at => Some(at.min(self.tabs.len() - 1)),
            other => other,
        };
        if self.active != Some(at) {
            self.find = None;
        }
    }
}

/// The id of the editor of the file at `path`: one each, so each keeps its caret and scroll
/// while another is on screen.
pub fn editor_id(path: &str) -> String {
    format!("doc:editor:{path}")
}

/// The engine's intents for the tree's jobs; an operation runs in the session's own folder.
fn intents(session: SessionId, jobs: Vec<tree::Job>) -> Vec<Intent> {
    jobs.into_iter()
        .map(|job| match job {
            tree::Job::List(dir) => Intent::ListDir { session, dir },
            tree::Job::File(_, op) => Intent::FileOp { session, op },
        })
        .collect()
}

/// Open or close the pane. Opening it lists again what the session's tree had listed: the
/// agent may have changed the folder since. A session with no pane gets one (`shown`).
fn toggle(ui: &mut Ui) -> Vec<Intent> {
    if ui.pane_open(Pane::Files) {
        ui.close_pane(Pane::Files);
        return vec![];
    }
    let Some(session) = ui.selected else {
        return vec![];
    };
    ui.open_pane(Pane::Files);
    match ui.files.get(&session) {
        Some(pane) => intents(session, pane.tree.relist()),
        None => vec![],
    }
}

/// The pane of the session on screen, made and listed when it is open and has none.
pub(crate) fn shown(store: &Store, ui: &mut Ui) -> Vec<Intent> {
    if !ui.pane_open(Pane::Files) {
        return vec![];
    }
    let Some(session) = ui.selected.and_then(|s| store.session(s)) else {
        return vec![];
    };
    if ui.files.contains_key(&session.id) {
        return vec![];
    }
    let (pane, jobs) = FilesPane::new(session.cwd.clone());
    ui.files.insert(session.id, pane);
    intents(session.id, jobs)
}

/// Open the file at `path`, in the folder `root` of `session`, in its tab (a new one, read from
/// the disk, when it has none), and show it in the tree, as Desktop follows the tab on screen.
fn open_in(ui: &mut Ui, session: SessionId, root: &Path, path: &str) -> Vec<Intent> {
    // A file op that ends after the person went elsewhere opens it in its own session's panes.
    ui.in_panes_of(session, |ui| ui.open_pane(Pane::Files));
    let mut out = vec![];
    let pane = ui.files.entry(session).or_insert_with(|| {
        let (pane, jobs) = FilesPane::new(root.to_path_buf());
        out.extend(intents(session, jobs));
        pane
    });
    let at = match pane.tabs.iter().position(|d| d.path == path) {
        Some(at) => at,
        None => {
            pane.tabs.push(Document {
                session,
                path: path.to_owned(),
                editor: None,
                unreadable: false,
                dirty: false,
                preview: newen_components::code::is_markdown(Path::new(path)),
                error: None,
            });
            out.push(Intent::ReadFile {
                session,
                path: path.to_owned(),
            });
            pane.tabs.len() - 1
        }
    };
    if pane.active != Some(at) {
        pane.find = None;
    }
    pane.active = Some(at);
    let shown = pane.tree.reveal(&pane.tree.root.join(path));
    out.extend(intents(session, shown.jobs));
    out
}

/// What the person edited in the file on screen, written over it.
fn save(pane: &FilesPane) -> Vec<Intent> {
    let Some(document) = pane.document().filter(|d| d.dirty) else {
        return vec![];
    };
    document
        .text()
        .map(|text| {
            vec![Intent::WriteFile {
                session: document.session,
                path: document.path.clone(),
                text,
            }]
        })
        .unwrap_or_default()
}

/// What the tree asks of the window: its jobs, the tabs moved with what moved and closed with
/// what went to the trash, the focus, the clipboard, and a file to open.
fn apply(ui: &mut Ui, session: SessionId, root: &Path, outcome: Outcome) -> Vec<Intent> {
    let mut out = intents(session, outcome.jobs);
    if let Some(pane) = ui.files.get_mut(&session) {
        for (old, new) in &outcome.moved {
            for document in &mut pane.tabs {
                let at = root.join(&document.path);
                if let Some(Some(moved)) = newen_files::moved(&at, old, Some(new)) {
                    if let Ok(path) = moved.strip_prefix(root) {
                        document.path = path.to_string_lossy().into_owned();
                    }
                }
            }
        }
        if let Some(gone) = &outcome.trashed {
            while let Some(at) =
                (pane.tabs.iter()).position(|d| root.join(&d.path).starts_with(gone))
            {
                pane.close(at);
            }
        }
    }
    if outcome.copy.is_some() {
        ui.files_copy = outcome.copy;
    }
    if outcome.focus.is_some() {
        ui.files_focus = outcome.focus;
    }
    if outcome.leave {
        ui.files_focus = Some(view::COMPOSER.into());
    }
    if let Some(path) = (outcome.open.as_deref()).and_then(|f| f.strip_prefix(root).ok()) {
        out.extend(open_in(ui, session, root, &path.to_string_lossy()));
    }
    out
}

/// What a click, an edit or a key of the pane does to it; the window's own part (focus,
/// clipboard, what the tree asks) is returned for `action` to apply.
#[derive(Default)]
struct Done {
    intents: Vec<Intent>,
    tree: Option<Outcome>,
    focus: Option<String>,
    copy: Option<String>,
    reveal: Option<String>,
}

/// A click of the pane's own: its tabs, the tree's toggle, the file's tools, the find bar,
/// "Discard unsaved changes?", and the tree's rows, menus and dialogs. `None` when `id` is none
/// of them.
fn clicked(pane: &mut FilesPane, id: &str) -> Option<Done> {
    let mut done = Done::default();
    let editor = pane.document().and_then(|d| d.editor.clone());
    if let Some(at) = id.strip_prefix("doc:tab:").and_then(|i| i.parse().ok()) {
        if at < pane.tabs.len() && pane.active != Some(at) {
            pane.active = Some(at);
            pane.find = None;
        }
    } else if let Some(at) = id.strip_prefix("doc:close:").and_then(|i| i.parse().ok()) {
        if pane.tabs.get(at).is_some_and(|d: &Document| d.dirty) {
            pane.discarding = Some(at);
        } else {
            pane.close(at);
        }
    } else if id == DISCARD {
        if let Some(at) = pane.discarding.take() {
            pane.close(at);
        }
    } else if id == KEEP {
        pane.discarding = None;
    } else if id == TREE_TOGGLE {
        pane.tree_open = !pane.tree_open;
    } else if id == FILTER_CLEAR {
        pane.filter.clear();
        pane.found = None;
        done.focus = Some(FILTER_FIELD.into());
    } else if let Some((path, row)) = id
        .strip_prefix(FOUND_LINE)
        .and_then(|rest| rest.rsplit_once(':'))
        .and_then(|(path, row)| Some((path, row.parse::<usize>().ok()?)))
    {
        // Open, or go to, the file with the caret on the line, as Desktop's previews do.
        pane.goto = Some((path.to_owned(), row));
        if let Some(document) = pane
            .tabs
            .iter()
            .find(|d| d.path == path && d.editor.is_some())
        {
            go_to(document, row);
            pane.goto = None;
            done.reveal = Some(editor_id(path));
        }
        done.focus = Some(editor_id(path));
        done.tree = Some(Outcome {
            open: Some(pane.tree.root.join(path)),
            ..Outcome::default()
        });
    } else if id == view::DOC_SAVE {
        done.intents = save(pane);
    } else if id == RELOAD || id == REVERT {
        // Given up, the edits no longer hold the disk's text off.
        let at = pane.active?;
        let document = &mut pane.tabs[at];
        document.dirty = false;
        document.error = None;
        done.intents.push(Intent::ReadFile {
            session: document.session,
            path: document.path.clone(),
        });
    } else if id == view::DOC_PREVIEW {
        let at = pane.active?;
        pane.tabs[at].preview = !pane.tabs[at].preview;
    } else if id == COPY {
        done.copy = pane.document().and_then(Document::text);
    } else if id == COPY_PATH {
        done.copy = pane.document().map(|d| d.path.clone());
    } else if let Some(asked) = find::clicked(&mut pane.find, id, editor.as_ref()) {
        done.focus = match asked {
            find::Asked::Query => Some(find::FIND_QUERY.into()),
            find::Asked::Editor => pane.document().map(|d| editor_id(&d.path)),
            find::Asked::Reveal => None,
        };
    } else if let Some(dir) = id.strip_prefix("tree:reveal-dir:") {
        done.tree = Some(pane.tree.reveal_dir(PathBuf::from(dir)));
    } else {
        let root = pane.tree.root.clone();
        let tabs = &pane.tabs;
        let busy = |at: &Path| (tabs.iter()).any(|d| d.dirty && root.join(&d.path).starts_with(at));
        done.tree = Some(pane.tree.clicked(id, busy)?);
    }
    Some(done)
}

/// What `done` asks of the window, after the pane took it.
fn finish(ui: &mut Ui, session: SessionId, root: &Path, done: Done) -> Vec<Intent> {
    let mut out = done.intents;
    if done.copy.is_some() {
        ui.files_copy = done.copy;
    }
    if done.focus.is_some() {
        ui.files_focus = done.focus;
    }
    if done.reveal.is_some() {
        ui.files_reveal = done.reveal;
    }
    if let Some(outcome) = done.tree {
        out.extend(apply(ui, session, root, outcome));
    }
    out
}

/// The pane's actions: its toggle, a file opened from anywhere (`doc:open:{path}`), and, while
/// it is open, its clicks, its editors' edits and saves, and what is typed in its fields.
/// `None` when `action` is none of them.
pub(crate) fn action(store: &Store, ui: &mut Ui, action: &UiAction) -> Option<Vec<Intent>> {
    if matches!(action, UiAction::Clicked { id } if id == view::FILES_TOGGLE) {
        return Some(toggle(ui));
    }
    if let UiAction::Clicked { id } = action {
        if let Some(path) = id.strip_prefix("doc:open:") {
            let session = ui.selected.and_then(|s| store.session(s))?;
            let (id, root) = (session.id, session.cwd.clone());
            return Some(open_in(ui, id, &root, path));
        }
        // A changed file at its line (one-based), its path from the folder or absolute.
        if let Some((line, path)) = (id.strip_prefix(view::OPEN_HERE))
            .and_then(|rest| rest.split_once(':'))
            .and_then(|(line, path)| Some((line.parse::<usize>().ok()?, path)))
        {
            let session = ui.selected.and_then(|s| store.session(s))?;
            let (id, root) = (session.id, session.cwd.clone());
            let path = Path::new(path)
                .strip_prefix(&root)
                .unwrap_or(Path::new(path));
            let path = path.to_string_lossy().into_owned();
            let out = open_in(ui, id, &root, &path);
            let row = line.saturating_sub(1);
            let pane = ui.files.get_mut(&id)?;
            match pane
                .tabs
                .iter()
                .find(|d| d.path == path && d.editor.is_some())
            {
                Some(document) => {
                    go_to(document, row);
                    ui.files_reveal = Some(editor_id(&path));
                }
                None => pane.goto = Some((path.clone(), row)),
            }
            ui.files_focus = Some(editor_id(&path));
            return Some(out);
        }
    }
    if !ui.pane_open(Pane::Files) {
        return None;
    }
    let session = ui.selected?;
    let pane = ui.files.get_mut(&session)?;
    let root = pane.tree.root.clone();
    let editor = pane.document().and_then(|d| d.editor.clone());
    let done = match action {
        UiAction::EditorChanged { id, .. } => {
            let path = id.strip_prefix("doc:editor:")?;
            let document = pane.tabs.iter_mut().find(|d| d.path == path)?;
            document.dirty = true;
            document.error = None;
            Done::default()
        }
        UiAction::EditorSaveRequested { id, .. } => {
            id.strip_prefix("doc:editor:")?;
            Done {
                intents: save(pane),
                ..Done::default()
            }
        }
        UiAction::InputChanged { id, value } if id == FILTER_FIELD => {
            pane.filter = value.clone();
            let (query, contents) = filter_query(value);
            pane.found = pane
                .found
                .take()
                .filter(|f| f.query == query && f.contents == contents);
            Done {
                intents: (!query.is_empty())
                    .then(|| Intent::FindFiles {
                        session,
                        query: query.to_owned(),
                        contents,
                    })
                    .into_iter()
                    .collect(),
                ..Done::default()
            }
        }
        UiAction::InputChanged { id, value } => {
            if !pane.tree.typed(id, value)
                && !find::typed(&mut pane.find, id, value, editor.as_ref())
            {
                return None;
            }
            Done::default()
        }
        UiAction::Submitted { id, .. } if id == tree::NAME_FIELD => clicked(pane, "name:go")?,
        UiAction::Submitted { id, .. } => {
            if !find::submitted(&mut pane.find, id, editor.as_ref()) {
                return None;
            }
            Done::default()
        }
        UiAction::Clicked { id } => clicked(pane, id)?,
        _ => return None,
    };
    Some(finish(ui, session, &root, done))
}

/// A key the pane takes before limn: on a row of the tree, the arrows and nvim-tree's letters;
/// Ctrl+F in an editor, its find bar; Escape in the bar, back to the editor. `None` otherwise.
pub fn key(ui: &mut Ui, focused: Option<&str>, event: &limn::UiEvent) -> Option<Vec<Intent>> {
    let focused = focused.filter(|_| ui.pane_open(Pane::Files))?;
    let session = ui.selected?;
    let click = match event {
        limn::UiEvent::Key {
            key: limn::Key::F,
            ctrl: true,
            shift: false,
            ..
        } if focused.starts_with("doc:editor:") => Some(find::FIND_OPEN),
        limn::UiEvent::Key {
            key: limn::Key::Escape,
            ..
        } if focused.starts_with("find:") => Some("find:dismiss"),
        _ => None,
    };
    if let Some(id) = click {
        let pane = ui.files.get_mut(&session)?;
        let root = pane.tree.root.clone();
        let done = clicked(pane, id)?;
        return Some(finish(ui, session, &root, done));
    }
    let row = Some(focused).filter(|f| tree::is_row(f))?;
    let key = match event {
        limn::UiEvent::Key {
            key, ctrl: false, ..
        } => match key {
            limn::Key::ArrowUp => TreeKey::Up,
            limn::Key::ArrowDown => TreeKey::Down,
            limn::Key::ArrowLeft => TreeKey::Left,
            limn::Key::ArrowRight => TreeKey::Right,
            limn::Key::Escape => TreeKey::Escape,
            _ => return None,
        },
        limn::UiEvent::Text(typed) => match typed.as_str() {
            "k" => TreeKey::Up,
            "j" => TreeKey::Down,
            "h" => TreeKey::Left,
            "l" => TreeKey::Right,
            "a" => TreeKey::New,
            "r" => TreeKey::Rename,
            "d" => TreeKey::Trash,
            "y" => TreeKey::Copy,
            "x" => TreeKey::Cut,
            "p" => TreeKey::Paste,
            _ => return None,
        },
        _ => return None,
    };
    let pane = ui.files.get_mut(&session)?;
    let root = pane.tree.root.clone();
    let tabs = &pane.tabs;
    let busy = |at: &Path| (tabs.iter()).any(|d| d.dirty && root.join(&d.path).starts_with(at));
    let outcome = pane.tree.key(row, key, busy)?;
    Some(apply(ui, session, &root, outcome))
}

/// A file's text read, into its tab. What the person edits wins over the disk; saved as it
/// stands, the editor keeps its caret and scroll.
/// The caret to the start of `row` of the file, which the editor brings into view.
fn go_to(document: &Document, row: usize) {
    let Some(editor) = &document.editor else {
        return;
    };
    let shared = editor.shared_buffer();
    let Ok(mut text) = shared.lock() else {
        return;
    };
    let snapshot = text.snapshot();
    let row = row.min(snapshot.line_count().saturating_sub(1));
    if let Ok(line) = snapshot.line_range(row) {
        let _ = text.set_selection(limn_editor::Selection::caret(line.start));
    }
}

fn read(document: &mut Document, text: &Option<String>) {
    // shortcut: what the person edits wins over the disk; a conflict is not shown, where
    // Desktop says "This file changed on disk since you started editing."
    if document.dirty && document.text() != *text {
        return;
    }
    if document.editor.is_some() && document.text() == *text {
        document.dirty = false;
        return;
    }
    document.unreadable = text.is_none();
    document.editor = text
        .as_deref()
        .and_then(|t| limn_editor::TextBuffer::new(t).ok())
        .map(limn::EditorController::new);
    document.dirty = false;
}

/// What the engine reported for the panes: a folder listed, an operation done, a file read or
/// not written, and the agent at work, after which the folder is listed and the files not
/// edited are read again.
pub(crate) fn heard(ui: &mut Ui, event: &Event) -> Vec<Intent> {
    let Some(pane) = ui.files.get_mut(&event.session()) else {
        return vec![];
    };
    match event {
        Event::Listed {
            dir,
            entries,
            stamp,
            ..
        } => {
            pane.tree.listed(dir.clone(), entries.clone(), *stamp);
            vec![]
        }
        Event::FileOpDone {
            session,
            op,
            result,
        } => {
            let root = pane.tree.root.clone();
            let outcome = pane.tree.done(op.clone(), result.clone());
            apply(ui, *session, &root, outcome)
        }
        Event::FileRead { path, text, .. } => {
            if let Some(document) = pane.tabs.iter_mut().find(|d| d.path == *path) {
                read(document, text);
                if let Some((_, row)) = pane.goto.take_if(|(at, _)| at == path) {
                    go_to(document, row);
                    ui.files_reveal = Some(editor_id(path));
                }
            }
            vec![]
        }
        Event::FilesFound {
            query,
            contents,
            paths,
            hits,
            ..
        } => {
            // Only for the words the filter has now: an answer to older ones is let go.
            if filter_query(&pane.filter) == (query.as_str(), *contents) {
                pane.found = Some(Found {
                    query: query.clone(),
                    contents: *contents,
                    paths: paths.clone(),
                    hits: hits.clone(),
                });
            }
            vec![]
        }
        Event::FileWriteFailed { path, error, .. } => {
            if let Some(document) = pane.tabs.iter_mut().find(|d| d.path == *path) {
                document.error = Some(error.clone());
            }
            vec![]
        }
        Event::Changes { session, .. }
        | Event::Agent {
            session,
            event: AgentEvent::ToolResult { .. },
            ..
        } => (pane.tabs.iter())
            .filter(|d| !d.dirty)
            .map(|d| Intent::ReadFile {
                session: *session,
                path: d.path.clone(),
            })
            .collect(),
        Event::Agent {
            session,
            event: AgentEvent::TurnFinished { .. },
            ..
        } => intents(*session, pane.tree.relist()),
        _ => vec![],
    }
}

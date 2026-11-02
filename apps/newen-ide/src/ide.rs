//! quill-ide's state, what the person's clicks and keys ask of it, and the window (`view`).
//! The disk is the host's: `update` returns `Job`s, and what they find comes back to `loaded`.

use crate::complete::{self, Offer};
use crate::grep::{Hit, GREP_CLOSE, GREP_FIELD, GREP_OPEN};
use crate::marks::Change;
use crate::prefs::{self, Prefs};
use crate::quick::{self, QUICK_CLOSE, QUICK_FIELD, QUICK_LINE, QUICK_OPEN};
use crate::session::Session;
use crate::structure::{self as tree_sitter, Structure};
use limn::{Component, ComponentPalette, EditorController, Element, Key, Point, Size, UiAction};
use limn_editor::{BufferSnapshot, EditKind, Selection, TextBuffer};
use newen_components::code;
use newen_components::find::{self, Find, FIND_OPEN, FIND_QUERY};
use newen_components::look::{self, SMALL};
use newen_components::packs::{self, Offered, Pack, Theme};
use newen_components::search::{self, Found};
use newen_components::settings::{self, SETTINGS_CLOSE, SETTINGS_OPEN, SETTINGS_SEARCH};
use newen_components::sidebar::Sidebar;
use newen_components::tree::{self as file_tree, FileTree, Outcome};
pub use newen_components::tree::{TreeKey, NAME_FIELD};
use newen_components::{icons, pieces::*};
pub use newen_files::{Entry, Op, Stamp};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const TREE_TOGGLE: &str = "tree:toggle";
pub const TREE_FOCUS: &str = "tree:focus";
pub const TERMINAL: &str = "terminal";
pub const TERMINAL_SCROLL: &str = "terminal:scroll";
pub const TERMINAL_TOGGLE: &str = "terminal:toggle";
const TERMINAL_NEW: &str = "terminal:new";
const FILE_ERROR_DISMISS: &str = file_tree::ERROR_DISMISS;
pub const LSP_HOVER: &str = "lsp:hover";
pub const LSP_DEFINITION: &str = "lsp:definition";
pub const LSP_REFERENCES: &str = "lsp:references";
pub const LSP_RENAME: &str = "lsp:rename";
const LSP_FORMAT: &str = "lsp:format";
const HOVER_DISMISS: &str = "hover:dismiss";
pub const SYMBOL_FIELD: &str = "symbol:field";
const SYMBOL_CLOSE: &str = "symbol:close";
const REFS_FIELD: &str = "refs:field";
const REFS_CLOSE: &str = "refs:close";
/// The bar over the editor: its height, its "···" menu and the Markdown views.
const CRUMBS: f32 = 30.0;
const MORE: &str = "editor:more";
const WRAP: &str = "editor:wrap";
const MINIMAP: &str = "editor:minimap";
pub const PREVIEW_TOGGLE: &str = "doc:toggle";
const MD_LINK: &str = "md-link:";
const MD_DIAGRAM: &str = "md-diagram:";
/// The terminal's share of the window under the editor, its type and its insets, as quill's.
const TERMINAL_HEIGHT: f32 = 0.36;
pub const TERMINAL_FONT: f32 = 12.0;
const TERMINAL_PADDING: [f32; 2] = [12.0, 8.0];
const COPY_PATH: &str = "editor:copy-path";
const COPY_RELATIVE: &str = "editor:copy-relative";
const REVEAL: &str = "editor:reveal";
const NOTICE: f32 = 40.0;
/// The line under the editor: mode, file, caret, language and branch.
const FOOTER: f32 = 26.0;
pub const PROBLEM_NEXT: &str = "problems:next";
const PROBLEMS_TOGGLE: &str = "problems:toggle";
const PANE_TERMINAL: &str = "pane:terminal";
const PANE_PROBLEMS: &str = "pane:problems";
const PANE_CLOSE: &str = "pane:close";
const LSP_RESTART: &str = "lsp:restart";
const ASK_AGENT: &str = "ide:ask";

/// What the person asked of the shells, for the host, which runs them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerminalRequest {
    New,
    Select(usize),
    Close(usize),
}

/// The shells under the editor, in the folder: whether they show, what was asked of them,
/// their tabs' names and which is on screen, and its screen as the host read it.
#[derive(Default)]
pub struct TerminalPane {
    pub open: bool,
    /// Showing the problems rather than the shells.
    pub problems: bool,
    pub requests: Vec<TerminalRequest>,
    pub tabs: Vec<String>,
    pub active: usize,
    pub screen: Option<std::sync::Arc<limn::TerminalScreen>>,
}

/// The editing keys the host takes from egui, which limn's keys do not carry (Alt), for the
/// editor on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    MoveLines { down: bool },
    DuplicateLines,
    AddCursor { below: bool },
    ToggleComment,
    DeleteLines,
}

/// What the host does on the disk, beside the window.
#[derive(Debug, PartialEq)]
pub enum Job {
    List(PathBuf),
    Read(PathBuf),
    Write(PathBuf, Save),
    /// The paths watched, with when each was last seen changed.
    Stat(Vec<(PathBuf, Stamp)>),
    SavePrefs(Prefs),
    /// Look for icon themes in these folders.
    FindPacks(Vec<PathBuf>),
    /// Read the icon theme at this JSON.
    LoadPack(PathBuf),
    /// The branch checked out in this folder's repository.
    Branch(PathBuf),
    /// The monospaced families installed.
    FindFonts,
    /// The faces of this family, or of the system's monospace when empty.
    LoadFont(String),
    /// Every file of the folder git sees, for opening by name.
    FindFiles(PathBuf),
    SaveSession(PathBuf, Session),
    /// The lines of the folder's files that hold the query, for the search numbered so.
    Grep(PathBuf, String, u64),
    /// The file as its last commit holds it.
    Committed(PathBuf),
    /// What changed in the buffer's text since that commit.
    Changes(PathBuf, std::sync::Arc<str>, Save),
    /// The buffer's folds, scopes and declarations, by tree-sitter.
    Analyze(PathBuf, Save),
    /// Make, rename or trash a file of the folder.
    File(PathBuf, Op),
    /// A language server's edits (JSON) to a file that is not open, made on the disk.
    ApplyEdits(PathBuf, String),
}

/// A family's regular and bold faces, for the editor.
pub struct Faces(pub limn::Font, pub limn::Font);

impl std::fmt::Debug for Faces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Faces")
    }
}

/// The text a save writes: the buffer as it was when the person asked.
#[derive(Debug)]
pub struct Save(pub BufferSnapshot);

impl PartialEq for Save {
    fn eq(&self, other: &Self) -> bool {
        (self.0.document_id(), self.0.revision()) == (other.0.document_id(), other.0.revision())
    }
}

/// What a `Job` found.
#[derive(Debug)]
pub enum Loaded {
    Listed(PathBuf, Vec<Entry>, Stamp),
    /// The file's text, or `None` when it is not text or could not be read.
    Read(PathBuf, Option<String>, Stamp),
    Written(PathBuf, Save, Result<Stamp, String>),
    /// Of the paths a `Stat` asked about, the ones changed since, with their new stamps.
    Changed(Vec<(PathBuf, Stamp)>),
    Packs(Vec<Offered>),
    Pack(PathBuf, Result<Theme, String>),
    PrefsSaved,
    /// The branch, or the commit when none is checked out, and the HEAD file it was read
    /// from, to read again when it changes.
    Branch(Option<String>, Option<(PathBuf, Stamp)>),
    Fonts(Vec<String>),
    Font(String, Result<Faces, String>),
    /// The folder's files, from the folder.
    Files(Vec<PathBuf>),
    SessionSaved,
    Grep(u64, Vec<Hit>),
    Committed(PathBuf, Option<String>),
    /// A file's changes since its commit, at the buffer's revision.
    Changes(PathBuf, u64, Vec<(std::ops::Range<usize>, Change)>),
    Analyzed(PathBuf, Structure),
    /// What a file operation made or moved to, from the folder, or why it did not.
    File(Op, Result<Vec<PathBuf>, String>),
    /// Edits made on the disk, or why not.
    EditsApplied(PathBuf, Result<Stamp, String>),
}

/// What the person asks a language server, at a character of a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub path: PathBuf,
    pub offset: usize,
    pub kind: AskKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AskKind {
    Completion,
    Hover,
    Definition,
    References,
    Rename(String),
    Format,
}

/// Completing the word at the caret: its file, where the word starts, what the server
/// offers (none while it is asked), the row Enter takes, and where the caret was drawn when
/// it began, for the list to open under.
#[derive(Clone, Debug)]
pub struct Completion {
    pub path: PathBuf,
    pub start: usize,
    pub offers: Vec<Offer>,
    pub chosen: usize,
    pub at: limn::Rect,
}

/// A server's diagnostic, in the characters of the revision it came for.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub range: std::ops::Range<usize>,
    /// 1 error, 2 warning, 3 information, 4 hint.
    pub severity: u32,
    pub message: String,
}

/// A file's diagnostics as the server sent them, the text they came for, and the same carried
/// to the buffer's later revision while the server has not answered for it.
#[derive(Clone, Debug)]
pub struct Diagnostics {
    pub revision: u64,
    pub text: std::sync::Arc<str>,
    pub items: Vec<Diagnostic>,
    pub carried: Option<(u64, Vec<Diagnostic>)>,
}

impl Diagnostics {
    /// Those to show on `revision`: the server's own, or those carried to it.
    pub fn at(&self, revision: u64) -> Option<&[Diagnostic]> {
        if self.revision == revision {
            return Some(&self.items);
        }
        self.carried
            .as_ref()
            .filter(|(r, _)| *r == revision)
            .map(|(_, d)| d.as_slice())
    }
}

/// `items`, found in `old`, where they are in `new`: those before what changed stay, those
/// after it move with the text, and those on it go until the server says again.
pub fn carry(items: &[Diagnostic], old: &str, new: &str) -> Vec<Diagnostic> {
    let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let end = old.len() - suffix;
    items
        .iter()
        .filter_map(|item| {
            let range = if item.range.end <= prefix {
                item.range.clone()
            } else if item.range.start >= end {
                let shift = |at: usize| at + new.len() - old.len();
                shift(item.range.start)..shift(item.range.end)
            } else {
                return None;
            };
            Some(Diagnostic {
                range,
                ..item.clone()
            })
        })
        .collect()
}

/// One error or warning of the problems list: where it is, the column in UTF-16 units as
/// servers count, and what the server said.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub path: PathBuf,
    pub row: usize,
    pub column: u32,
    pub severity: u32,
    pub message: String,
}

/// A language's server as the footer shows it: its program, whether it answers yet, and why
/// it does not run, when it does not, with what may help.
#[derive(Clone, Debug, PartialEq)]
pub struct Server {
    pub name: String,
    pub ready: bool,
    pub failure: Option<String>,
    pub hint: Option<String>,
}

/// A place a server names: a file and its row.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub row: usize,
    /// In UTF-16 units of the row, as servers count them.
    pub column: u32,
}

pub struct Buffer {
    pub path: PathBuf,
    /// `None` for a file that can't be shown as text.
    pub editor: Option<EditorController>,
    pub stamp: Stamp,
    /// Changed on the disk while it held changes of the person's, which a reload would lose.
    pub stale: bool,
    /// The person asked for the disk's text over theirs.
    pub reloading: bool,
    /// Why the last save failed.
    pub error: Option<String>,
    /// Reopened from the last session and not read yet.
    pub loading: bool,
    /// The file as its last commit holds it: not asked yet, asked, or `None` inside when it
    /// has none.
    pub committed: Committed,
    /// The rows changed since that commit, the revision they were worked out at, and whether
    /// they are being worked out again.
    pub changes: Vec<(std::ops::Range<usize>, Change)>,
    pub changes_revision: Option<u64>,
    pub marking: bool,
    /// What tree-sitter found in it, whether it is looking again, and the blocks folded.
    pub structure: Option<Structure>,
    pub analyzing: bool,
    pub folded: Vec<std::ops::Range<usize>>,
    pub view: DocView,
}

/// How a Markdown file shows: its source, rendered, or both side by side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DocView {
    #[default]
    Source,
    Preview,
    Split,
}

#[derive(Clone, Debug, Default)]
pub enum Committed {
    #[default]
    Unknown,
    Asked,
    Known(Option<std::sync::Arc<str>>),
}

impl Buffer {
    fn new(path: PathBuf, editor: Option<EditorController>, stamp: Stamp, loading: bool) -> Self {
        Self {
            path,
            editor,
            stamp,
            stale: false,
            reloading: false,
            error: None,
            loading,
            committed: Committed::Unknown,
            changes: vec![],
            changes_revision: None,
            marking: false,
            structure: None,
            analyzing: false,
            folded: vec![],
            view: DocView::Source,
        }
    }

    pub fn dirty(&self) -> bool {
        let snapshot = self.editor.as_ref().and_then(|e| e.snapshot().ok());
        snapshot.is_some_and(|s| s.is_dirty())
    }

    fn editor_id(&self) -> String {
        format!("editor:{}", self.path.display())
    }
}

pub struct Ide {
    pub root: PathBuf,
    /// The files' tree, which slides shut and peeks as quill's sidebar.
    pub sidebar: Sidebar,
    /// The folder's files, as quill's Files pane shows them.
    pub tree: FileTree,
    pub buffers: Vec<Buffer>,
    pub active: Option<usize>,
    /// A `Stat` is out; the next waits for it.
    pub polling: bool,
    /// The file with unsaved changes the person asked once to close.
    pub closing: Option<PathBuf>,
    /// The person asked once to close the window with unsaved changes.
    pub quitting: bool,
    pub prefs: Prefs,
    /// The icons' pack in use, the icon themes found, and why the chosen one did not load.
    pub pack: Pack,
    pub offered: Vec<Offered>,
    pub pack_error: Option<String>,
    /// Settings: the page open, what its search holds and the select whose menu is open.
    pub settings_page: Option<String>,
    pub settings_query: String,
    pub settings_menu: Option<String>,
    /// Where the pointer is and the control under it, which the host sets every frame, for
    /// the tree's peek.
    pub pointer: Option<Point>,
    pub hovered: Option<String>,
    pub reduced_motion: bool,
    /// The Vim mode of the file on screen, which the host reads from limn every frame.
    pub vim_mode: Option<limn::VimMode>,
    pub branch: Option<String>,
    /// The repository's HEAD file and when it last changed.
    pub head: Option<(PathBuf, Stamp)>,
    /// The monospaced families found, for Settings.
    pub fonts: Vec<String>,
    /// The faces to lay code out in from the next frame, which the host installs.
    pub fonts_due: Option<limn::Fonts>,
    /// Finding in the file on screen, while its bar is open.
    pub find: Option<Find>,
    /// Opening a file by its name: what is typed and the row Enter opens.
    pub quick: Option<(String, usize)>,
    /// The folder's files, once listed for opening by name.
    pub files: Vec<PathBuf>,
    /// What the host is to do in the tree after this frame: focus a control, and bring the
    /// caret into view.
    pub focus: Option<String>,
    pub reveal: bool,
    /// Words for the clipboard, and a tree row to scroll into view, for the host.
    pub copy: Option<String>,
    pub reveal_row: Option<String>,
    /// Finding in every file: the query, the row Enter opens, the lines found, and the
    /// number of the latest search, whose lines alone are shown.
    pub grep: Option<Grep>,
    pub grep_count: u64,
    /// A file's row, and column in UTF-16 units as servers count them, to take the caret to
    /// once it is read.
    pub pending_goto: Option<(PathBuf, usize, u32)>,
    pub terminal: TerminalPane,
    /// Language servers: what was asked and waits for the host to send, each file's
    /// diagnostics with the revision they came for, the hover shown, the references listed
    /// with the row Enter opens, the name a symbol is being renamed to, and the last news.
    pub lsp_asks: Vec<Ask>,
    pub diagnostics: HashMap<PathBuf, Diagnostics>,
    pub hover: Option<String>,
    pub references: Option<(Vec<Location>, usize)>,
    pub renaming_symbol: Option<String>,
    pub lsp_note: Option<String>,
    /// Each language's server, for the footer, and those to start again.
    pub lsp_servers: HashMap<limn_language::Language, Server>,
    pub lsp_restart: Vec<limn_language::Language>,
    /// What waits to go to quill, and how the last went.
    pub link_out: Vec<newen_link::Message>,
    pub link_note: Option<String>,
    /// The "···" menu over the editor is open; the diagrams of a preview shown as source; an
    /// address a link asks the host to open in the desktop's browser.
    pub more_open: bool,
    pub diagram_sources: Vec<u64>,
    pub open_url: Option<String>,
    /// Completing the word at the caret, and where the caret was last drawn, which the host
    /// reads from limn while no popover hides it.
    pub completion: Option<Completion>,
    pub caret: Option<limn::Rect>,
}

#[derive(Default)]
pub struct Grep {
    pub query: String,
    pub chosen: usize,
    pub hits: Vec<Hit>,
    pub searching: bool,
}

impl Ide {
    pub fn new(root: PathBuf, prefs: Prefs, session: Session) -> (Self, Vec<Job>) {
        let mut jobs = vec![
            Job::List(root.clone()),
            Job::Branch(root.clone()),
            Job::FindFonts,
            Job::FindPacks(packs::roots(&prefs.icon_folders)),
        ];
        if !prefs.editor_font.is_empty() {
            jobs.push(Job::LoadFont(prefs.editor_font.clone()));
        }
        let pack = match prefs.icons.as_str() {
            packs::QUILL => Pack::Quill,
            key => {
                jobs.extend(
                    key.strip_prefix(packs::THEME)
                        .map(|json| Job::LoadPack(json.into())),
                );
                Pack::Material
            }
        };
        // What was open last time opens again, in its order, each read as it comes.
        jobs.extend(session.expanded.iter().cloned().map(Job::List));
        jobs.extend(session.open.iter().cloned().map(Job::Read));
        let buffers: Vec<Buffer> = session
            .open
            .iter()
            .map(|path| Buffer::new(path.clone(), None, None, true))
            .collect();
        let active = session.active.filter(|a| *a < buffers.len());
        let ide = Self {
            root: root.clone(),
            sidebar: Sidebar::new(look::SIDEBAR_WIDTH),
            tree: FileTree {
                expanded: session.expanded.into_iter().collect(),
                ..FileTree::new(root.clone()).0
            },
            buffers,
            active,
            polling: false,
            closing: None,
            quitting: false,
            prefs,
            pack,
            offered: vec![],
            pack_error: None,
            settings_page: None,
            settings_query: String::new(),
            settings_menu: None,
            pointer: None,
            hovered: None,
            reduced_motion: false,
            vim_mode: None,
            branch: None,
            head: None,
            fonts: vec![],
            fonts_due: None,
            find: None,
            quick: None,
            files: vec![],
            focus: None,
            reveal: false,
            copy: None,
            reveal_row: None,
            grep: None,
            grep_count: 0,
            pending_goto: None,
            terminal: TerminalPane::default(),
            lsp_asks: vec![],
            diagnostics: HashMap::new(),
            hover: None,
            references: None,
            renaming_symbol: None,
            lsp_note: None,
            lsp_servers: HashMap::new(),
            lsp_restart: vec![],
            link_out: vec![],
            link_note: None,
            more_open: false,
            diagram_sources: vec![],
            open_url: None,
            completion: None,
            caret: None,
        };
        (ide, jobs)
    }

    /// Show `path` if it is open, else read it.
    pub fn open(&mut self, path: PathBuf) -> Vec<Job> {
        match self.buffers.iter().position(|b| b.path == path) {
            Some(i) => {
                self.active = Some(i);
                vec![]
            }
            None => vec![Job::Read(path)],
        }
    }

    /// Close a file; one with unsaved changes closes when asked a second time in a row.
    pub fn close(&mut self, i: usize) {
        let Some(buffer) = self.buffers.get(i) else {
            return;
        };
        if buffer.dirty() && self.closing.as_ref() != Some(&buffer.path) {
            self.closing = Some(buffer.path.clone());
            self.active = Some(i);
            return;
        }
        self.closing = None;
        self.buffers.remove(i);
        self.active = match self.active {
            _ if self.buffers.is_empty() => None,
            Some(a) if a > i || a == self.buffers.len() => Some(a - 1),
            active => active,
        };
    }

    /// How many open files have changes not saved.
    pub fn unsaved(&self) -> usize {
        self.buffers.iter().filter(|b| b.dirty()).count()
    }

    pub fn update(&mut self, action: &UiAction) -> Vec<Job> {
        if let UiAction::EditorSaveRequested { id, .. } = action {
            let buffer = self.buffers.iter().find(|b| b.editor_id() == *id);
            let snapshot = buffer.and_then(|b| Some((b, b.editor.as_ref()?.snapshot().ok()?)));
            return snapshot
                .map(|(b, s)| vec![Job::Write(b.path.clone(), Save(s))])
                .unwrap_or_default();
        }
        if let UiAction::InputChanged { id, value } = action {
            if id == SETTINGS_SEARCH {
                self.settings_query = value.clone();
            } else if id == QUICK_FIELD {
                self.quick = Some((value.clone(), 0));
            } else if id == GREP_FIELD {
                return self.grep_for(value);
            } else if id == SYMBOL_FIELD {
                self.renaming_symbol = Some(value.clone());
            } else if self.tree.typed(id, value) {
            } else {
                let editor = self.active_buffer().and_then(|b| b.editor.clone());
                if find::typed(&mut self.find, id, value, editor.as_ref()) {
                    self.reveal = true;
                }
            }
            return vec![];
        }
        if let UiAction::Submitted { id, .. } = action {
            if id == QUICK_FIELD {
                return self.quick_chosen();
            }
            if id == GREP_FIELD {
                let chosen = self.grep.as_ref().map_or(0, |g| g.chosen);
                return self.grep_open(chosen);
            }
            if id == NAME_FIELD {
                return self.file_clicked("name:go").unwrap_or_default();
            }
            if id == SYMBOL_FIELD {
                self.rename_symbol();
            }
            if id == REFS_FIELD {
                let chosen = self.references.as_ref().map_or(0, |(_, c)| *c);
                return self.reference_open(chosen);
            }
            let editor = self.active_buffer().and_then(|b| b.editor.clone());
            if find::submitted(&mut self.find, id, editor.as_ref()) {
                self.reveal = true;
            }
            return vec![];
        }
        let UiAction::Clicked { id } = action else {
            return vec![];
        };
        // The "···" menu shuts with whatever is chosen, in it or not.
        if id != MORE {
            self.more_open = false;
        }
        if !id.starts_with("close:") {
            self.closing = None;
        }
        if let Some(jobs) = self.settings_clicked(id) {
            return jobs;
        }
        if let Some(jobs) = self.quick_clicked(id) {
            return jobs;
        }
        if id == GREP_OPEN {
            self.grep = Some(Grep::default());
            return vec![];
        }
        if id == GREP_CLOSE {
            self.grep = None;
            return vec![];
        }
        if let Some(at) = id.strip_prefix("grep:hit:").and_then(|i| i.parse().ok()) {
            return self.grep_open(at);
        }
        if self.find_clicked(id) {
            return vec![];
        }
        if let Some(jobs) = self.context_clicked(id) {
            return jobs;
        }
        if let Some(jobs) = self.structure_clicked(id) {
            return jobs;
        }
        if let Some(jobs) = self.terminal_clicked(id) {
            return jobs;
        }
        if let Some(jobs) = self.file_clicked(id) {
            return jobs;
        }
        if let Some(jobs) = self.lsp_clicked(id) {
            return jobs;
        }
        if let Some(jobs) = self.bar_clicked(id) {
            return jobs;
        }
        if id == TREE_TOGGLE {
            self.sidebar.toggle();
        } else if id == TREE_FOCUS {
            return self.focus_tree();
        } else if let Some(i) = id.strip_prefix("buffer:").and_then(|i| i.parse().ok()) {
            if i < self.buffers.len() {
                self.active = Some(i);
            }
        } else if let Some(i) = id.strip_prefix("close:").and_then(|i| i.parse().ok()) {
            self.close(i);
        } else if let Some(i) = id.strip_prefix("reload:").and_then(|i| i.parse().ok()) {
            if let Some(buffer) = self.buffers.get_mut::<usize>(i) {
                buffer.reloading = true;
                return vec![Job::Read(buffer.path.clone())];
            }
        }
        vec![]
    }

    /// Settings' own clicks: open and close, pick a page, and change a setting, which is kept
    /// at once. `None` when `id` is none of them. A select reports its trigger, `set:{name}`,
    /// then a choice, `set:{name}={value}`.
    fn settings_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        if id == SETTINGS_OPEN {
            self.settings_page = Some(prefs::FIRST.into());
            self.settings_query.clear();
            let roots = packs::roots(&self.prefs.icon_folders);
            return Some(vec![Job::FindPacks(roots), Job::FindFonts]);
        }
        self.settings_page.as_ref()?;
        if id == SETTINGS_CLOSE {
            self.settings_page = None;
            self.settings_menu = None;
            return Some(vec![]);
        }
        if let Some(page) = id.strip_prefix("settings:page:") {
            self.settings_page = Some(page.to_owned());
            self.settings_query.clear();
            return Some(vec![]);
        }
        let rest = id.strip_prefix("set:")?;
        if let Some(menu) = rest.strip_suffix(":dismiss") {
            if self.settings_menu.as_deref() == Some(menu) {
                self.settings_menu = None;
            }
            return Some(vec![]);
        }
        let (name, value) = match rest.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None if Prefs::is_switch(rest) => (rest, None),
            // A select's trigger opens its menu, or shuts it.
            None => {
                let open = self.settings_menu.as_deref() == Some(rest);
                self.settings_menu = (!open).then(|| rest.to_owned());
                return Some(vec![]);
            }
        };
        self.settings_menu = None;
        if !self.prefs.set(name, value) {
            return Some(vec![]);
        }
        let mut jobs = vec![Job::SavePrefs(self.prefs.clone())];
        if name == "editor_font" {
            jobs.push(Job::LoadFont(self.prefs.editor_font.clone()));
        }
        if name == "icons" {
            self.pack_error = None;
            match self.prefs.icons.as_str() {
                packs::QUILL => self.pack = Pack::Quill,
                packs::MATERIAL => self.pack = Pack::Material,
                key => jobs.extend(
                    key.strip_prefix(packs::THEME)
                        .map(|j| Job::LoadPack(j.into())),
                ),
            }
        }
        Some(jobs)
    }

    /// A folder to look for icon themes in, from Settings' "Add folder…".
    pub fn add_icon_folder(&mut self, folder: PathBuf) -> Vec<Job> {
        if !self.prefs.icon_folders.contains(&folder) {
            self.prefs.icon_folders.push(folder);
        }
        vec![
            Job::SavePrefs(self.prefs.clone()),
            Job::FindPacks(packs::roots(&self.prefs.icon_folders)),
        ]
    }

    /// Opening by name: open it, close it, or a row picked. `None` when `id` is none of them.
    fn quick_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        if id == QUICK_OPEN {
            self.quick = Some((String::new(), 0));
            return Some(vec![Job::FindFiles(self.root.clone())]);
        }
        if id == QUICK_LINE {
            self.quick = Some((":".into(), 0));
            self.focus = Some(QUICK_FIELD.into());
            return Some(vec![]);
        }
        if id == QUICK_CLOSE {
            self.quick = None;
            return Some(vec![]);
        }
        if id == "quick:go" {
            return Some(self.quick_chosen());
        }
        let path = id.strip_prefix("quick:file:")?;
        self.quick = None;
        Some(self.open(path.into()))
    }

    /// The files opening by name lists now, as paths from the folder.
    pub fn quick_found(&self) -> Vec<&PathBuf> {
        let query = self.quick.as_ref().map_or("", |(q, _)| q.as_str());
        quick::best(query, &self.files)
    }

    /// Move the row Enter opens by `by`, within the rows, in whichever search is open.
    pub fn step_chosen(&mut self, by: isize) {
        let step = |chosen: &mut usize, count: usize| {
            *chosen = (*chosen as isize + by).clamp(0, count.saturating_sub(1) as isize) as usize;
        };
        let count = self.quick_found().len();
        if let Some((_, chosen)) = self.quick.as_mut() {
            step(chosen, count);
        }
        if let Some(grep) = self.grep.as_mut() {
            step(&mut grep.chosen, grep.hits.len());
        }
        if let Some((found, chosen)) = self.references.as_mut() {
            step(chosen, found.len());
        }
    }

    /// Search every file for `query`, from two letters on; what an older search finds is
    /// dropped when it comes.
    fn grep_for(&mut self, query: &str) -> Vec<Job> {
        let Some(grep) = self.grep.as_mut() else {
            return vec![];
        };
        self.grep_count += 1;
        grep.query = query.to_owned();
        grep.chosen = 0;
        grep.searching = query.chars().count() >= 2;
        if !grep.searching {
            grep.hits.clear();
            return vec![];
        }
        vec![Job::Grep(
            self.root.clone(),
            query.to_owned(),
            self.grep_count,
        )]
    }

    fn grep_open(&mut self, at: usize) -> Vec<Job> {
        let Some(hit) = self.grep.as_ref().and_then(|g| g.hits.get(at)).cloned() else {
            return vec![];
        };
        self.grep = None;
        self.goto(self.root.join(&hit.path), hit.row)
    }

    /// Open `path` with the caret at the start of `row`, now or once it is read.
    pub fn goto(&mut self, path: PathBuf, row: usize) -> Vec<Job> {
        self.goto_at(path, row, 0)
    }

    /// Open `path` with the caret at `column` (UTF-16 units) of `row`.
    pub fn goto_at(&mut self, path: PathBuf, row: usize, column: u32) -> Vec<Job> {
        let jobs = self.open(path.clone());
        self.pending_goto = Some((path, row, column));
        self.apply_goto();
        jobs
    }

    /// The pending row, if its file is read: the caret to it, in view, with the keys.
    fn apply_goto(&mut self) {
        let Some((path, row, column)) = self.pending_goto.clone() else {
            return;
        };
        let Some(buffer) = self.buffers.iter().find(|b| b.path == path && !b.loading) else {
            return;
        };
        self.pending_goto = None;
        let Some(editor) = &buffer.editor else {
            return;
        };
        let shared = editor.shared_buffer();
        if let Ok(mut text) = shared.lock() {
            let snapshot = text.snapshot();
            let row = row.min(snapshot.line_count().saturating_sub(1));
            if let Ok(line) = snapshot.line_range(row) {
                let at = limn_language::Position {
                    line: row as u32,
                    character: column,
                };
                let offset = limn_language::utf16_to_offset(&snapshot, at)
                    .map_or(line.start, |o| o.clamp(line.start, line.end));
                let _ = text.set_selection(Selection::caret(offset));
            }
        }
        self.focus = Some(buffer.editor_id());
        self.reveal = true;
    }

    fn quick_chosen(&mut self) -> Vec<Job> {
        let query = self.quick.as_ref().map_or("", |(q, _)| q.as_str());
        if query.starts_with(':') {
            let target = quick::line(query).zip(self.active_buffer().map(|b| b.path.clone()));
            self.quick = None;
            return target.map_or(vec![], |(row, path)| self.goto(path, row));
        }
        let chosen = self.quick.as_ref().map_or(0, |(_, c)| *c);
        let Some(path) = self.quick_found().get(chosen).map(|p| self.root.join(p)) else {
            return vec![];
        };
        self.quick = None;
        self.open(path)
    }

    /// The find bar's own clicks: open it, close it, its options and its arrows. False when
    /// `id` is none of them.
    fn find_clicked(&mut self, id: &str) -> bool {
        let editor = self.active_buffer().and_then(|b| b.editor.clone());
        match find::clicked(&mut self.find, id, editor.as_ref()) {
            None => return false,
            Some(find::Asked::Query) => self.focus = Some(FIND_QUERY.into()),
            Some(find::Asked::Editor) => self.focus = self.editor_id(),
            Some(find::Asked::Reveal) => self.reveal = true,
        }
        true
    }

    /// A fold control, which folds its block or unfolds it, or a sticky line, which takes the
    /// caret to its scope's first row. `None` when `id` is none of them.
    fn structure_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        if let Some(rows) = id.strip_prefix("fold:") {
            let (start, end) = rows.split_once(':')?;
            let rows = start.parse().ok()?..end.parse().ok()?;
            let folded = &mut self.buffers.get_mut(self.active?)?.folded;
            match folded.iter().position(|f| *f == rows) {
                Some(at) => {
                    folded.remove(at);
                }
                None => folded.push(rows),
            }
            return Some(vec![]);
        }
        let row = id.strip_prefix("scope:")?.parse().ok()?;
        let path = self.active_buffer()?.path.clone();
        Some(self.goto(path, row))
    }

    /// The editor's own entries after limn's: copy the file's path, or show it in the tree.
    /// `None` when `id` is none of them.
    fn context_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        let path = self.active_buffer()?.path.clone();
        match id {
            COPY_PATH => self.copy = Some(path.display().to_string()),
            COPY_RELATIVE => {
                let relative = path.strip_prefix(&self.root).unwrap_or(&path);
                self.copy = Some(relative.display().to_string());
            }
            REVEAL => return Some(self.reveal_in_tree(&path)),
            _ => return None,
        }
        Some(vec![])
    }

    /// Open the tree and its folders down to `path`, and bring its row into view.
    fn reveal_in_tree(&mut self, path: &Path) -> Vec<Job> {
        if !self.sidebar.open {
            self.sidebar.toggle();
        }
        let outcome = self.tree.reveal(path);
        self.tree_outcome(outcome)
    }

    /// The tree's clicks: its rows, their menus, the naming dialog and the trash's question.
    /// `None` when `id` is none of them.
    fn file_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        let buffers = &self.buffers;
        let busy = |at: &Path| buffers.iter().any(|b| b.path.starts_with(at) && b.dirty());
        let outcome = self.tree.clicked(id, busy)?;
        Some(self.tree_outcome(outcome))
    }

    /// What the tree asks of the editor: its jobs, the open files moved with what moved and
    /// closed with what went to the trash, the clipboard, the focus, and a file to open.
    fn tree_outcome(&mut self, outcome: Outcome) -> Vec<Job> {
        let mut jobs: Vec<Job> = (outcome.jobs.into_iter())
            .map(|job| match job {
                file_tree::Job::List(dir) => Job::List(dir),
                file_tree::Job::File(root, op) => Job::File(root, op),
            })
            .collect();
        for (old, new) in &outcome.moved {
            for buffer in &mut self.buffers {
                if let Some(Some(moved)) = newen_files::moved(&buffer.path, old, Some(new)) {
                    buffer.path = moved;
                }
            }
        }
        if let Some(gone) = outcome.trashed {
            while let Some(at) = self.buffers.iter().position(|b| b.path.starts_with(&gone)) {
                self.closing = Some(self.buffers[at].path.clone());
                self.close(at);
            }
        }
        if outcome.copy.is_some() {
            self.copy = outcome.copy;
        }
        if outcome.reveal.is_some() {
            self.reveal_row = outcome.reveal;
        }
        if outcome.focus.is_some() {
            self.focus = outcome.focus;
        }
        if outcome.leave {
            self.focus = self.editor_id();
        }
        if let Some(file) = outcome.open {
            jobs.extend(self.open(file));
        }
        jobs
    }

    fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root).unwrap_or(path).to_path_buf()
    }

    /// What was typed in the editor on screen: the word at the caret is completed once it has
    /// a letter, or right after `.` or `::`; anything else ends a completion.
    pub fn typed(&mut self) {
        let Some(buffer) = self.active_buffer() else {
            return;
        };
        let Some(snapshot) = buffer.editor.as_ref().and_then(|e| e.snapshot().ok()) else {
            return;
        };
        let caret = snapshot.selection().head;
        let from = caret.saturating_sub(64);
        let before: Vec<char> = snapshot
            .text_range(from..caret)
            .unwrap_or_default()
            .chars()
            .collect();
        let word = before
            .iter()
            .rev()
            .take_while(|c| complete::word(**c))
            .count();
        let start = caret - word;
        let lead: String = before[..before.len() - word].iter().rev().take(2).collect();
        let after_member = lead.starts_with('.') || lead == "::";
        if word == 0 && !after_member {
            self.completion = None;
            return;
        }
        let path = buffer.path.clone();
        let same = self
            .completion
            .as_ref()
            .filter(|c| c.path == path && c.start == start);
        // The same word goes on narrowing what came; while nothing has, it is asked again for
        // this text, as the server drops answers for one that changed under them.
        match same {
            Some(completion) if !completion.offers.is_empty() => return,
            Some(_) => {
                self.lsp_asks.push(Ask {
                    path,
                    offset: caret,
                    kind: AskKind::Completion,
                });
                return;
            }
            None => {}
        }
        let Some(at) = self.caret else {
            return;
        };
        self.completion = Some(Completion {
            path: path.clone(),
            start,
            offers: vec![],
            chosen: 0,
            at,
        });
        self.lsp_asks.push(Ask {
            path,
            offset: caret,
            kind: AskKind::Completion,
        });
    }

    /// What the word at the caret holds so far, while completing.
    fn completion_typed(&self) -> Option<String> {
        let completion = self.completion.as_ref()?;
        let buffer = self.active_buffer().filter(|b| b.path == completion.path)?;
        let snapshot = buffer.editor.as_ref()?.snapshot().ok()?;
        let caret = snapshot.selection().head;
        let typed = snapshot.text_range(completion.start..caret).ok()?;
        typed.chars().all(complete::word).then_some(typed)
    }

    /// The offers shown now, narrowed by what was typed; empty while none match.
    pub fn completion_shown(&self) -> Vec<(usize, &Offer)> {
        match (&self.completion, self.completion_typed()) {
            (Some(completion), Some(typed)) => complete::narrowed(&completion.offers, &typed),
            _ => vec![],
        }
    }

    /// Move the row Enter takes by `by`, within those shown.
    pub fn completion_step(&mut self, by: isize) {
        let count = self.completion_shown().len();
        if let Some(completion) = self.completion.as_mut() {
            let last = count.saturating_sub(1) as isize;
            completion.chosen = (completion.chosen as isize + by).clamp(0, last) as usize;
        }
    }

    /// Write the `row`-th offer shown (the chosen one without it) over the typed word.
    pub fn accept_completion(&mut self, row: Option<usize>) {
        let shown = self.completion_shown();
        let row = row.unwrap_or_else(|| self.completion.as_ref().map_or(0, |c| c.chosen));
        let Some((_, offer)) = shown.get(row).map(|(at, o)| (*at, (*o).clone())) else {
            self.completion = None;
            return;
        };
        let Some(completion) = self.completion.take() else {
            return;
        };
        let Some(editor) = self.active_buffer().and_then(|b| b.editor.clone()) else {
            return;
        };
        let caret = editor
            .snapshot()
            .map_or(completion.start, |s| s.selection().head);
        let word = Selection {
            anchor: completion.start,
            head: caret,
        };
        let _ = editor.execute(limn_editor::EditorCommand::SetSelection(word));
        let command = if offer.snippet {
            limn_editor::EditorCommand::InsertSnippet(offer.text)
        } else {
            limn_editor::EditorCommand::InsertText(offer.text)
        };
        let _ = editor.execute(command);
        self.reveal = true;
    }

    /// The bar over the editor: its path and symbols, its menu, the Markdown views and the
    /// preview's links. `None` when `id` is none of them.
    fn bar_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        if let Some(dir) = id.strip_prefix("tree:reveal-dir:") {
            if !self.sidebar.open {
                self.sidebar.toggle();
            }
            let outcome = self.tree.reveal_dir(PathBuf::from(dir));
            return Some(self.tree_outcome(outcome));
        }
        let mut save = false;
        match id {
            MORE => self.more_open = !self.more_open,
            PROBLEM_NEXT => self.next_problem(),
            ASK_AGENT => self.ask_agent(),
            LSP_RESTART => {
                if let Some(buffer) = self.active_buffer() {
                    let path = buffer.path.to_string_lossy();
                    self.lsp_restart
                        .push(limn_language::Language::from_path(&path));
                }
            }
            "editor:more:dismiss" => self.more_open = false,
            WRAP => {
                self.prefs.wrap = !self.prefs.wrap;
                save = true;
            }
            MINIMAP => {
                self.prefs.minimap = !self.prefs.minimap;
                save = true;
            }
            PREVIEW_TOGGLE => {
                let buffer = self.active.and_then(|a| self.buffers.get_mut(a))?;
                buffer.view = match buffer.view {
                    DocView::Source => DocView::Preview,
                    _ => DocView::Source,
                };
            }
            _ => {
                if let Some(view) = id.strip_prefix("doc:") {
                    let buffer = self.active.and_then(|a| self.buffers.get_mut(a))?;
                    buffer.view = match view {
                        "preview" => DocView::Preview,
                        "split" => DocView::Split,
                        "source" => DocView::Source,
                        _ => return None,
                    };
                } else if let Some(diagram) = id.strip_prefix(MD_DIAGRAM) {
                    let diagram = u64::from_str_radix(diagram, 16).ok()?;
                    match self.diagram_sources.iter().position(|d| *d == diagram) {
                        Some(at) => {
                            self.diagram_sources.remove(at);
                        }
                        None => self.diagram_sources.push(diagram),
                    }
                } else if let Some(target) = id.strip_prefix(MD_LINK) {
                    return Some(self.follow_link(target));
                } else {
                    return None;
                }
            }
        }
        Some(if save {
            vec![Job::SavePrefs(self.prefs.clone())]
        } else {
            vec![]
        })
    }

    /// A preview's link: a web address goes to the desktop's browser, a file of the folder
    /// opens here, and the document's own anchors are left as they are.
    fn follow_link(&mut self, target: &str) -> Vec<Job> {
        let web = ["http://", "https://", "mailto:"];
        if web.iter().any(|scheme| target.starts_with(scheme)) {
            self.open_url = Some(target.to_owned());
            return vec![];
        }
        let file = target.split('#').next().unwrap_or_default();
        let Some(dir) = self.active_buffer().and_then(|b| b.path.parent()) else {
            return vec![];
        };
        if file.is_empty() || target.contains("://") {
            return vec![];
        }
        let path = dir.join(file);
        // Only what is inside the folder opens.
        match std::fs::canonicalize(&path) {
            Ok(path) if path.starts_with(&self.root) => self.open(path),
            _ => {
                self.lsp_note = Some(format!("{file} is not in the folder"));
                vec![]
            }
        }
    }

    /// Asks of the language server about the caret, and its answers' dialogs. `None` when `id`
    /// is none of them.
    fn lsp_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        let ask = |ide: &mut Ide, kind: AskKind| {
            let buffer = ide.active_buffer();
            let at =
                buffer.and_then(|b| Some((b.path.clone(), b.editor.as_ref()?.snapshot().ok()?)));
            if let Some((path, snapshot)) = at {
                let offset = snapshot.selection().head;
                ide.lsp_asks.push(Ask { path, offset, kind });
            }
        };
        match id {
            LSP_HOVER => ask(self, AskKind::Hover),
            LSP_DEFINITION => ask(self, AskKind::Definition),
            LSP_REFERENCES => ask(self, AskKind::References),
            LSP_FORMAT => ask(self, AskKind::Format),
            LSP_RENAME => {
                self.renaming_symbol = Some(self.word_at_caret().unwrap_or_default());
                self.focus = Some(SYMBOL_FIELD.into());
            }
            "symbol:go" => self.rename_symbol(),
            SYMBOL_CLOSE => self.renaming_symbol = None,
            HOVER_DISMISS => self.hover = None,
            "complete:dismiss" => self.completion = None,
            REFS_CLOSE => self.references = None,
            _ => {
                if let Some(row) = id.strip_prefix("complete:").and_then(|r| r.parse().ok()) {
                    self.accept_completion(Some(row));
                    return Some(vec![]);
                }
                let at = id.strip_prefix("refs:at:")?.parse().ok()?;
                return Some(self.reference_open(at));
            }
        }
        Some(vec![])
    }

    /// The word the caret is in, as the name a rename starts from.
    fn word_at_caret(&self) -> Option<String> {
        let snapshot = self.active_buffer()?.editor.as_ref()?.snapshot().ok()?;
        let head = snapshot.selection().head;
        let row = snapshot.row_of(head).ok()?;
        let line = snapshot.line_range(row).ok()?;
        let text: Vec<char> = snapshot.text_range(line.clone()).ok()?.chars().collect();
        let at = head - line.start;
        let word = |c: &char| c.is_alphanumeric() || *c == '_';
        let start = text[..at.min(text.len())]
            .iter()
            .rposition(|c| !word(c))
            .map_or(0, |i| i + 1);
        let end = text[at.min(text.len())..]
            .iter()
            .position(|c| !word(c))
            .map_or(text.len(), |i| at + i);
        Some(text[start..end].iter().collect())
    }

    fn rename_symbol(&mut self) {
        let Some(name) = self.renaming_symbol.take().filter(|n| !n.trim().is_empty()) else {
            return;
        };
        let at = self
            .active_buffer()
            .and_then(|b| Some((b.path.clone(), b.editor.as_ref()?.snapshot().ok()?)));
        if let Some((path, snapshot)) = at {
            let offset = snapshot.selection().head;
            self.lsp_asks.push(Ask {
                path,
                offset,
                kind: AskKind::Rename(name.trim().to_owned()),
            });
        }
    }

    fn reference_open(&mut self, at: usize) -> Vec<Job> {
        let Some(found) = self
            .references
            .take()
            .and_then(|(f, _)| f.into_iter().nth(at))
        else {
            return vec![];
        };
        self.goto_at(found.path, found.row, found.column)
    }

    /// An editing key on the file on screen, as one undoable step; the caret stays in view.
    pub fn edit(&mut self, edit: Edit) {
        let Some(buffer) = self.active_buffer() else {
            return;
        };
        let Some(editor) = &buffer.editor else {
            return;
        };
        use limn_editor::EditorCommand::*;
        let command = match edit {
            Edit::MoveLines { down } => MoveLines { down },
            Edit::DuplicateLines => DuplicateLines,
            Edit::AddCursor { below } => AddCursorVertical {
                below,
                tab_width: self.prefs.tab_width.clamp(1, 32) as usize,
            },
            Edit::DeleteLines => DeleteLines,
            Edit::ToggleComment => {
                let path = buffer.path.to_string_lossy();
                let language = limn_language::Language::from_path(&path);
                let (prefix, suffix) = language.comment_markers_for(&path);
                ToggleLineComment {
                    prefix: prefix.into(),
                    suffix: suffix.into(),
                }
            }
        };
        // What the buffer refuses (lines not in one run, the document's edge) is left as is.
        if editor.execute(command).is_ok() {
            self.reveal = true;
        }
    }

    /// The terminal's toggle, its tabs, its new and its closes. `None` when `id` is none of
    /// them.
    fn terminal_clicked(&mut self, id: &str) -> Option<Vec<Job>> {
        let terminal = &mut self.terminal;
        // The pane's toggles show their section, or shut the pane when it is the one shown.
        if id == TERMINAL_TOGGLE || id == PROBLEMS_TOGGLE {
            let problems = id == PROBLEMS_TOGGLE;
            let shown = terminal.open && terminal.problems == problems;
            terminal.open = !shown;
            terminal.problems = problems;
            self.focus = if terminal.open && !problems {
                Some(TERMINAL.into())
            } else {
                self.editor_id()
            };
        } else if id == PANE_TERMINAL || id == PANE_PROBLEMS {
            terminal.problems = id == PANE_PROBLEMS;
            self.focus = if terminal.problems {
                self.editor_id()
            } else {
                Some(TERMINAL.into())
            };
        } else if id == PANE_CLOSE {
            terminal.open = false;
            self.focus = self.editor_id();
        } else if let Some(at) = id.strip_prefix("problem:") {
            let problem = self.problems().into_iter().nth(at.parse().ok()?)?;
            return Some(self.goto_at(problem.path, problem.row, problem.column));
        } else if id == TERMINAL_NEW {
            terminal.requests.push(TerminalRequest::New);
            self.focus = Some(TERMINAL.into());
        } else if let Some(tab) = id.strip_prefix("terminal:tab:") {
            terminal
                .requests
                .push(TerminalRequest::Select(tab.parse().ok()?));
            self.focus = Some(TERMINAL.into());
        } else if let Some(tab) = id.strip_prefix("terminal:close:") {
            terminal
                .requests
                .push(TerminalRequest::Close(tab.parse().ok()?));
        } else {
            return None;
        }
        Some(vec![])
    }

    /// The terminal's grid in cells, for the shells to be sized to, in `window`.
    pub fn terminal_grid(&self, window: Size) -> (u16, u16) {
        let width = window.width - self.sidebar.docked_width() - 2.0 * TERMINAL_PADDING[0];
        let height = terminal_height(window) - look::PANE_HEADER - 1.0 - 2.0 * TERMINAL_PADDING[1];
        let (cols, rows) = limn::TerminalView::grid_size(Size::new(width, height), TERMINAL_FONT);
        (cols as u16, rows as u16)
    }

    /// The tree takes the keys, open, on the file on screen or its first row.
    fn focus_tree(&mut self) -> Vec<Job> {
        if !self.sidebar.open {
            self.sidebar.toggle();
        }
        let mut jobs = vec![];
        let row = match self.active_buffer().map(|b| b.path.clone()) {
            Some(path) => {
                jobs = self.reveal_in_tree(&path);
                Some(format!("file:{}", path.display()))
            }
            None => self.tree.row_ids().first().map(|(id, _)| id.clone()),
        };
        self.focus = row;
        jobs
    }

    /// A key on the tree's row `focused`, as nvim's file trees take them. False when the tree
    /// does not take it.
    pub fn tree_key(&mut self, focused: &str, key: TreeKey) -> (bool, Vec<Job>) {
        let buffers = &self.buffers;
        let busy = |at: &Path| buffers.iter().any(|b| b.path.starts_with(at) && b.dirty());
        match self.tree.key(focused, key, busy) {
            Some(outcome) => (true, self.tree_outcome(outcome)),
            None => (false, vec![]),
        }
    }

    /// What is open now, to open again next time.
    pub fn session(&self) -> Session {
        let mut expanded: Vec<PathBuf> = self.tree.expanded.iter().cloned().collect();
        expanded.sort();
        Session {
            open: self.buffers.iter().map(|b| b.path.clone()).collect(),
            active: self.active,
            expanded,
        }
    }

    /// What the gutter's marks need from the disk: each open file as committed, then what
    /// changed in its text since, once per revision and one at a time.
    pub fn marks_due(&mut self) -> Vec<Job> {
        let mut jobs = vec![];
        for buffer in &mut self.buffers {
            let Some(editor) = &buffer.editor else {
                continue;
            };
            let revision = editor.snapshot().map(|s| s.revision()).ok();
            let analyzed = buffer.structure.as_ref().map(|s| s.revision);
            if !buffer.analyzing && revision.is_some() && analyzed != revision {
                if let Ok(snapshot) = editor.snapshot() {
                    buffer.analyzing = true;
                    jobs.push(Job::Analyze(buffer.path.clone(), Save(snapshot)));
                }
            }
            match &buffer.committed {
                Committed::Unknown => {
                    buffer.committed = Committed::Asked;
                    jobs.push(Job::Committed(buffer.path.clone()));
                }
                Committed::Known(Some(committed)) if !buffer.marking => {
                    let Ok(snapshot) = editor.snapshot() else {
                        continue;
                    };
                    if buffer.changes_revision != Some(snapshot.revision()) {
                        buffer.marking = true;
                        let job =
                            Job::Changes(buffer.path.clone(), committed.clone(), Save(snapshot));
                        jobs.push(job);
                    }
                }
                _ => {}
            }
        }
        jobs
    }

    /// Move what moves on its own: the tree's springs and its peek.
    pub fn tick(&mut self) {
        let now = std::time::Instant::now();
        // The pace of limn's own opening and closing slots, as quill's.
        let motion = limn::SpringMotion::settling_in(std::time::Duration::from_millis(
            u64::from(limn::tokens().motion.move_ms) * 3 / 2,
        ));
        let pointer = newen_components::sidebar::Pointer {
            at: self.pointer,
            on_toggle: self.hovered.as_deref() == Some(TREE_TOGGLE),
            toggle_zone: limn::Rect::new(0.0, 0.0, 48.0, look::TOP_BAR),
            held: self.settings_page.is_some(),
        };
        self.sidebar
            .tick(&pointer, motion, self.reduced_motion, now);
        // A completion ends once the caret leaves the word it began on.
        if self.completion.is_some() && self.completion_typed().is_none() {
            self.completion = None;
        }
        // Diagnostics of an older text follow the edits until the server answers again.
        for buffer in &self.buffers {
            let Some(snapshot) = buffer.editor.as_ref().and_then(|e| e.snapshot().ok()) else {
                continue;
            };
            let Some(found) = self.diagnostics.get_mut(&buffer.path) else {
                continue;
            };
            let revision = snapshot.revision();
            if found.at(revision).is_none() {
                let now: String = snapshot.chunks().collect();
                found.carried = Some((revision, carry(&found.items, &found.text, &now)));
            }
        }
        // Matches found in an older text are found again in this one.
        let editor = self.active_buffer().and_then(|b| b.editor.clone());
        if let (Some(find), Some(editor)) = (self.find.as_mut(), editor) {
            let revision = editor.snapshot().map(|s| s.revision()).ok();
            if find.results.as_ref().map(|r| r.revision) != revision && !find.query.is_empty() {
                find.search(&editor);
            }
        }
    }

    pub fn animating(&self) -> bool {
        self.sidebar.animating(std::time::Instant::now())
    }

    /// What to look at on the disk, once a second: the open files and the folders listed.
    pub fn poll(&mut self) -> Vec<Job> {
        if self.polling {
            return vec![];
        }
        self.polling = true;
        let mut watched: Vec<(PathBuf, Stamp)> = self
            .tree
            .stamps
            .iter()
            .map(|(p, s)| (p.clone(), *s))
            .collect();
        watched.extend(self.buffers.iter().map(|b| (b.path.clone(), b.stamp)));
        watched.extend(self.head.clone());
        vec![Job::Stat(watched)]
    }

    pub fn loaded(&mut self, loaded: Loaded) -> Vec<Job> {
        match loaded {
            Loaded::Listed(dir, entries, stamp) => self.tree.listed(dir, entries, stamp),
            Loaded::Read(path, text, stamp) => self.read(path, text, stamp),
            Loaded::Packs(offered) => self.offered = offered,
            Loaded::PrefsSaved => {}
            Loaded::Fonts(fonts) => self.fonts = fonts,
            Loaded::Files(files) => self.files = files,
            Loaded::Committed(path, text) => {
                if let Some(buffer) = self.buffers.iter_mut().find(|b| b.path == path) {
                    buffer.committed = Committed::Known(text.map(Into::into));
                    buffer.changes_revision = None;
                    buffer.changes.clear();
                }
            }
            Loaded::File(op, done) => {
                let outcome = self.tree.done(op, done);
                return self.tree_outcome(outcome);
            }
            Loaded::EditsApplied(path, done) => {
                if let Err(error) = done {
                    self.lsp_note = Some(format!("{}: {error}", name(&path)));
                }
            }
            Loaded::Analyzed(path, structure) => {
                if let Some(buffer) = self.buffers.iter_mut().find(|b| b.path == path) {
                    buffer.structure = Some(structure);
                    buffer.analyzing = false;
                }
            }
            Loaded::Changes(path, revision, changes) => {
                if let Some(buffer) = self.buffers.iter_mut().find(|b| b.path == path) {
                    buffer.changes = changes;
                    buffer.changes_revision = Some(revision);
                    buffer.marking = false;
                }
            }
            Loaded::Grep(count, hits) => {
                if let Some(grep) = self.grep.as_mut().filter(|_| count == self.grep_count) {
                    grep.hits = hits;
                    grep.searching = false;
                }
            }
            Loaded::SessionSaved => {}
            // Only the family still chosen is put in use; one that fails keeps the faces.
            Loaded::Font(family, Ok(Faces(regular, bold))) => {
                if family == self.prefs.editor_font {
                    use limn::FontFamily::{Monospace, MonospaceBold};
                    let fonts = limn::fonts()
                        .face(Monospace, regular)
                        .face(MonospaceBold, bold);
                    self.fonts_due = Some(fonts);
                }
            }
            Loaded::Font(_, Err(_)) => {}
            Loaded::Branch(branch, head) => {
                self.branch = branch;
                self.head = head;
            }
            // Only the theme still chosen is put in use.
            Loaded::Pack(json, theme) => {
                if self.prefs.icons.strip_prefix(packs::THEME) == json.to_str() {
                    match theme {
                        Ok(theme) => self.pack = Pack::Theme(Box::new(theme)),
                        Err(error) => self.pack_error = Some(error),
                    }
                }
            }
            Loaded::Written(path, Save(snapshot), result) => {
                let Some(buffer) = self.buffers.iter_mut().find(|b| b.path == path) else {
                    return vec![];
                };
                match result {
                    Ok(stamp) => {
                        let shared = buffer.editor.as_ref().map(|e| e.shared_buffer());
                        if let Some(Ok(mut text)) = shared.as_ref().map(|s| s.lock()) {
                            let _ = text.mark_saved_snapshot(&snapshot);
                        }
                        buffer.stamp = stamp;
                        buffer.stale = false;
                        buffer.error = None;
                    }
                    Err(error) => buffer.error = Some(error),
                }
            }
            Loaded::Changed(changed) => {
                self.polling = false;
                let mut jobs = vec![];
                for (path, stamp) in changed {
                    if self.head.as_ref().is_some_and(|(head, _)| *head == path) {
                        jobs.push(Job::Branch(self.root.clone()));
                        // A new commit, or another branch: the files are compared with it.
                        for buffer in &mut self.buffers {
                            buffer.committed = Committed::Unknown;
                        }
                    }
                    if self.tree.stamps.contains_key(&path) {
                        jobs.push(Job::List(path.clone()));
                    }
                    if let Some(buffer) = self.buffers.iter_mut().find(|b| b.path == path) {
                        if buffer.dirty() {
                            buffer.stale = true;
                            buffer.stamp = stamp;
                        } else {
                            jobs.push(Job::Read(path));
                        }
                    }
                }
                return jobs;
            }
        }
        vec![]
    }

    /// A file read: opened, or what the disk holds now in place of what it showed, unless that
    /// would lose the person's changes.
    fn read(&mut self, path: PathBuf, text: Option<String>, stamp: Stamp) {
        let Some(buffer) = self.buffers.iter_mut().find(|b| b.path == path) else {
            let editor = text
                .and_then(|t| TextBuffer::new(&t).ok())
                .map(EditorController::new);
            self.buffers.push(Buffer::new(path, editor, stamp, false));
            self.active = Some(self.buffers.len() - 1);
            self.apply_goto();
            return;
        };
        buffer.loading = false;
        let keep_theirs = buffer.dirty() && !buffer.reloading;
        match (&buffer.editor, text) {
            (Some(editor), Some(text)) if !keep_theirs => buffer.stale = !reload(editor, &text),
            // A file that was not text may be now.
            (None, text) => {
                buffer.editor = text
                    .and_then(|t| TextBuffer::new(&t).ok())
                    .map(EditorController::new)
            }
            _ => buffer.stale = true,
        }
        buffer.stamp = stamp;
        buffer.reloading = false;
        self.apply_goto();
    }

    /// The control a Ctrl shortcut clicks. Vim's own Ctrl keys (B, F, U, D, J, N, R) stay
    /// the editor's; Ctrl+P opens a file by its name, and Ctrl+G goes to a line.
    pub fn shortcut(&self, key: Key, shift: bool) -> Option<String> {
        let step = |by: isize| {
            let n = self.buffers.len() as isize;
            self.active
                .map(|a| format!("buffer:{}", (a as isize + by).rem_euclid(n)))
        };
        match (key, shift) {
            (Key::E, true) => Some(TREE_FOCUS.into()),
            (Key::W, false) => self.active.map(|a| format!("close:{a}")),
            (Key::PageDown, false) => step(1),
            (Key::PageUp, false) => step(-1),
            // Over Vim's Ctrl+P, a line up as `k` is.
            (Key::P, false) => Some(QUICK_OPEN.into()),
            (Key::F, true) => Some(GREP_OPEN.into()),
            (Key::G, false) => Some(QUICK_LINE.into()),
            (Key::L, false) => Some(ASK_AGENT.into()),
            (Key::J, true) => Some(TERMINAL_TOGGLE.into()),
            (Key::I, true) => Some(LSP_FORMAT.into()),
            // Not V: egui reads Ctrl+Shift+V as a paste.
            (Key::M, true) => Some(PREVIEW_TOGGLE.into()),
            // With Vim keys Ctrl+F is a page down, and `/` finds.
            (Key::F, false) if !self.prefs.vim => Some(FIND_OPEN.into()),
            _ => None,
        }
    }

    /// The editor on screen's id, for what the host reads of it.
    pub fn editor_id(&self) -> Option<String> {
        self.active_buffer().map(Buffer::editor_id)
    }

    /// Every file's errors and warnings: files in path order, each one's in the order of its
    /// lines, an open file's as they stand on its revision on screen.
    pub fn problems(&self) -> Vec<Problem> {
        let mut paths: Vec<&PathBuf> = self.diagnostics.keys().collect();
        paths.sort();
        let mut list = vec![];
        for path in paths {
            let found = &self.diagnostics[path];
            let snapshot = self
                .buffers
                .iter()
                .find(|b| &b.path == path)
                .and_then(|b| b.editor.as_ref()?.snapshot().ok());
            let current = snapshot
                .as_ref()
                .and_then(|s| Some((s.chunks().collect::<String>(), found.at(s.revision())?)));
            let (text, items) = match &current {
                Some((text, items)) => (text.as_str(), *items),
                None => (&*found.text, found.items.as_slice()),
            };
            // Offsets count characters, as the editor's do: one walk places them all.
            let mut wanted: Vec<&Diagnostic> = items.iter().filter(|d| d.severity <= 2).collect();
            wanted.sort_by_key(|d| d.range.start);
            let (mut row, mut column, mut index, mut chars) = (0, 0, 0, text.chars());
            for d in wanted {
                while index < d.range.start {
                    match chars.next() {
                        Some('\n') => (row, column) = (row + 1, 0),
                        Some(c) => column += c.len_utf16() as u32,
                        None => break,
                    }
                    index += 1;
                }
                list.push(Problem {
                    path: path.clone(),
                    row,
                    column,
                    severity: d.severity,
                    message: d.message.clone(),
                });
            }
        }
        list
    }

    /// The selection, or the file when nothing is selected, sent to quill for the person to
    /// ask an agent about.
    fn ask_agent(&mut self) {
        let Some(buffer) = self.active_buffer() else {
            return;
        };
        let Some(snapshot) = buffer.editor.as_ref().and_then(|e| e.snapshot().ok()) else {
            return;
        };
        let range = snapshot.selection().range();
        let (lines, text) = if range.is_empty() {
            (None, String::new())
        } else {
            let first = snapshot.position_of(range.start).unwrap_or_default().row;
            let end = snapshot.position_of(range.end).unwrap_or_default();
            // A selection ending where a line starts leaves that line out.
            let last = if end.column == 0 && end.row > first {
                end.row - 1
            } else {
                end.row
            };
            let text = snapshot
                .slice(range)
                .map(|s| s.to_string())
                .unwrap_or_default();
            (Some((first as u32 + 1, last as u32 + 1)), text)
        };
        self.link_out.push(newen_link::Message::Ask {
            root: self.root.clone(),
            path: buffer.path.clone(),
            lines,
            text,
        });
    }

    /// The caret to the next error or warning after it, or to the first.
    fn next_problem(&mut self) {
        let Some(buffer) = self.active_buffer() else {
            return;
        };
        let Some(editor) = &buffer.editor else {
            return;
        };
        let Ok(snapshot) = editor.snapshot() else {
            return;
        };
        let found = self
            .diagnostics
            .get(&buffer.path)
            .and_then(|d| d.at(snapshot.revision()));
        let mut starts: Vec<usize> = found
            .unwrap_or_default()
            .iter()
            .filter(|d| d.severity <= 2)
            .map(|d| d.range.start)
            .collect();
        starts.sort_unstable();
        let head = snapshot.selection().head;
        let Some(&to) = starts.iter().find(|&&at| at > head).or(starts.first()) else {
            return;
        };
        if let Ok(mut text) = editor.shared_buffer().lock() {
            let _ = text.set_selection(Selection::caret(to));
        }
        self.focus = Some(buffer.editor_id());
        self.reveal = true;
    }

    fn active_buffer(&self) -> Option<&Buffer> {
        self.active.and_then(|i| self.buffers.get(i))
    }
}

/// Put the disk's `text` in the buffer as one undoable edit of only what differs, so the caret
/// and the rest stay where they were, and the buffer reads as saved. False when the buffer
/// refused the edit.
fn reload(editor: &EditorController, text: &str) -> bool {
    let shared = editor.shared_buffer();
    let Ok(mut buffer) = shared.lock() else {
        return false;
    };
    let old: Vec<char> = buffer.snapshot().chunks().flat_map(str::chars).collect();
    let new: Vec<char> = text.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let end = old.len() - suffix;
    let replacement: String = new[prefix..new.len() - suffix].iter().collect();
    if end > prefix || !replacement.is_empty() {
        let head = buffer.selection().head;
        // After the change it moves with the text; inside it, to its start.
        let head = if head >= end {
            head + new.len() - old.len()
        } else {
            head.min(prefix)
        };
        let edit = buffer.edit(
            prefix..end,
            &replacement,
            Selection::caret(head),
            EditKind::Replace,
            0,
        );
        if edit.is_err() {
            return false;
        }
    }
    buffer.mark_saved();
    true
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub fn view(ide: &Ide, window: Size) -> Element {
    let below = (window.height - look::TOP_BAR - 1.0).max(0.0);
    let with_line = || Element::row(vec![tree(ide, below), vline(below)]);
    let mut body: Vec<Element> = ide.sidebar.docked(with_line(), below).into_iter().collect();
    let width = window.width - ide.sidebar.docked_width();
    let shell = if ide.terminal.open {
        terminal_height(window)
    } else {
        0.0
    };
    let mut column = vec![editor(ide, width, below - shell).flex(1.0)];
    if shell > 0.0 {
        column.push(terminal_pane(ide, width, shell));
    }
    body.push(Element::column(column).flex(1.0));
    let body = Element::row(body).size(Some(window.width), Some(below));
    // Shut, the tree peeks over the editor under the pointer, as quill's sidebar does.
    let body = ide
        .sidebar
        .peeking(body, with_line, Size::new(window.width, below));
    let root = Element::column(vec![
        top_bar(ide),
        hline(window.width, look::pane_border()),
        body,
    ])
    .size(Some(window.width), Some(window.height))
    .background(look::palette().surface);
    let root = match &ide.quick {
        Some((query, _)) if query.starts_with(':') => {
            let lines = ide
                .active_buffer()
                .and_then(|b| b.editor.as_ref()?.snapshot().ok());
            let lines = lines.map_or(0, |s| s.line_count());
            let row = match (quick::line(query), ide.active_buffer()) {
                (_, None) => Found::new(QUICK_CLOSE, "Open a file to go to one of its lines"),
                (Some(row), Some(_)) => {
                    let mut found = Found::new("quick:go", format!("Go to line {}", row + 1));
                    found.detail = Some(format!("of {lines}"));
                    found
                }
                (None, Some(_)) => Found::new("quick:go", format!("A line from 1 to {lines}")),
            };
            search::dialog(
                root,
                QUICK_FIELD,
                QUICK_CLOSE,
                query,
                "Open a file by its name",
                vec![("Go to line".into(), vec![row])],
                0,
            )
        }
        Some((query, chosen)) => {
            let rows = ide
                .quick_found()
                .into_iter()
                .map(|path| {
                    let full = ide.root.join(path);
                    let mut row = Found::new(format!("quick:file:{}", full.display()), name(path));
                    let dir = path.parent().map(|d| d.display().to_string());
                    row.detail = dir.filter(|d| !d.is_empty());
                    row.icon = Some(ide.pack.icon(&full, false, false));
                    row
                })
                .collect();
            let title = if query.is_empty() { "Files" } else { "Found" };
            search::dialog(
                root,
                QUICK_FIELD,
                QUICK_CLOSE,
                query,
                "Open a file by its name",
                vec![(title.into(), rows)],
                *chosen,
            )
        }
        None => root,
    };
    let root = match &ide.grep {
        Some(grep) => {
            let rows = grep
                .hits
                .iter()
                .enumerate()
                .map(|(at, hit)| {
                    let full = ide.root.join(&hit.path);
                    let mut row = Found::new(format!("grep:hit:{at}"), hit.text.clone());
                    row.detail = Some(format!("{}:{}", hit.path.display(), hit.row + 1));
                    row.icon = Some(ide.pack.icon(&full, false, false));
                    row
                })
                .collect();
            let title = match (grep.searching, grep.hits.len()) {
                (true, _) => "Finding…".to_owned(),
                (false, 0) if grep.query.chars().count() >= 2 => "Nothing found".to_owned(),
                (false, 0) => "Two letters or more".to_owned(),
                (false, n) if n >= crate::grep::LIMIT => format!("The first {n} lines"),
                (false, 1) => "1 line".to_owned(),
                (false, n) => format!("{n} lines"),
            };
            search::dialog(
                root,
                GREP_FIELD,
                GREP_CLOSE,
                &grep.query,
                "Find in every file",
                vec![(title, rows)],
                grep.chosen,
            )
        }
        None => root,
    };
    let root = file_tree::dialogs(&ide.tree, root);
    let root = match (&ide.completion, ide.completion_shown()) {
        (Some(completion), shown) if !shown.is_empty() => {
            completion_list(completion, &shown, root, window)
        }
        _ => root,
    };
    let root = match &ide.renaming_symbol {
        Some(value) => {
            let row = match value.trim() {
                "" => Found::new("symbol:go", "Type the new name"),
                new => Found::new("symbol:go", format!("Rename it {new} everywhere")),
            };
            search::dialog(
                root,
                SYMBOL_FIELD,
                SYMBOL_CLOSE,
                value,
                "The symbol's new name",
                vec![("Rename symbol".into(), vec![row])],
                0,
            )
        }
        None => root,
    };
    let root = match &ide.references {
        Some((found, chosen)) => {
            let rows = found
                .iter()
                .enumerate()
                .map(|(at, place)| {
                    let mut row = Found::new(
                        format!("refs:at:{at}"),
                        format!("{}:{}", name(&place.path), place.row + 1),
                    );
                    let dir = ide
                        .relative(&place.path)
                        .parent()
                        .map(|d| d.display().to_string());
                    row.detail = dir.filter(|d| !d.is_empty());
                    row.icon = Some(ide.pack.icon(&place.path, false, false));
                    row
                })
                .collect();
            let title = match found.len() {
                1 => "1 reference".to_owned(),
                n => format!("{n} references"),
            };
            search::dialog(
                root,
                REFS_FIELD,
                REFS_CLOSE,
                "",
                "References",
                vec![(title, rows)],
                *chosen,
            )
        }
        None => root,
    };
    match &ide.settings_page {
        Some(page) => settings::dialog(
            root,
            prefs::pages(
                &ide.prefs,
                ide.settings_menu.as_deref(),
                &ide.offered,
                &ide.fonts,
            ),
            prefs::groups(),
            page,
            &ide.settings_query,
            None,
            window,
        ),
        None => root,
    }
}

/// The tree's toggle, then the open files as quill's terminal tabs: pills, the one on screen
/// filled, with a dot while a file has changes not saved; Settings at the end.
fn top_bar(ide: &Ide) -> Element {
    let open = ide.sidebar.open;
    let mut row = vec![
        hgap(9.0),
        toggle(
            TREE_TOGGLE,
            icons::of(icons::SIDEBAR),
            if open { "Hide files" } else { "Show files" },
            open,
            Some("Ctrl+Shift+E"),
        ),
        hgap(8.0),
    ];
    if ide.buffers.is_empty() {
        row.push(muted(name(&ide.root)).centered_y());
    }
    for (i, buffer) in ide.buffers.iter().enumerate() {
        let (icon, ink) = ide.pack.icon(&buffer.path, false, false);
        let icon = Element::icon(icon)
            .foreground(ink)
            .size(Some(14.0), Some(14.0))
            .centered_y();
        let mut items = vec![hgap(6.0), icon, text(name(&buffer.path)).centered_y()];
        let mark = match buffer {
            b if b.error.is_some() => Some(look::failed()),
            b if b.stale => Some(look::awaiting()),
            b if b.dirty() => Some(look::muted()),
            _ => None,
        };
        if let Some(color) = mark {
            items.push(limn::StatusDot::new(color).size(6.0).render().centered_y());
        }
        // Two hovers, as quill's rows: the tab's, and over it the close's, a step darker than
        // whatever the tab is filled with.
        let active = ide.active == Some(i);
        let (tab_hover, close_hover) = if active {
            (look::active_hover(), look::chip_open())
        } else {
            (look::row_hover(), look::tool_hover())
        };
        items.push(
            tool_surface(
                &format!("close:{i}"),
                icons::of(icons::CLOSE),
                "Close file",
                20.0,
                None,
                close_hover,
                // Ctrl+W closes the file in front only.
                active.then_some("Ctrl+W"),
            )
            .centered_y(),
        );
        items.push(hgap(0.0));
        let pill = Element::row(items).gap(6.0).size(None, Some(28.0));
        let pill = ComponentPalette {
            hover: tab_hover,
            ..look::palette()
        }
        .interactive_surface(Element::clickable(format!("buffer:{i}"), pill))
        .radius(8.0);
        // The tab's own hover is a layer the whole tab reveals, so it stays on while the
        // pointer is over the close, a control of its own.
        let lit = Element::spacer()
            .background(tab_hover)
            .radius(8.0)
            .reveal_on_hover(format!("buffer:{i}:hover"));
        let tab = Element::stack(vec![lit, pill]).radius(8.0).hover_region();
        let tab = if active {
            tab.background(look::active())
        } else {
            tab
        };
        row.push(tab.centered_y());
    }
    let pills = Element::row(row).gap(4.0).flex(1.0).clip_children();
    let settings = tool_surface(
        SETTINGS_OPEN,
        icons::of(icons::SETTINGS),
        "Settings",
        26.0,
        None,
        look::tool_hover(),
        Some("Ctrl+,"),
    );
    Element::row(vec![pills, settings.centered_y(), hgap(8.0)])
        .gap(4.0)
        .size(None, Some(look::TOP_BAR))
}

/// The folder's files as git sees them, folders first, each folder listed when first opened,
/// with lines from each to its folder and the icons of the pack in use.
fn tree(ide: &Ide, height: f32) -> Element {
    let note = ide.pack_error.as_ref().map(|error| {
        limn::Tooltip::new(
            Element::icon(icons::of(icons::WARNING))
                .foreground(look::awaiting())
                .size(Some(14.0), Some(14.0)),
            "tree:pack-error",
            format!("The icon theme did not load: {error}"),
            look::palette(),
        )
        .render()
    });
    let shown = ide.active_buffer().map(|b| b.path.as_path());
    file_tree::view(
        &ide.tree,
        &ide.pack,
        file_tree::Look::Ide,
        shown,
        note,
        [look::SIDEBAR_WIDTH, height],
    )
    .background(look::sidebar())
}

/// A line over the editor that says what the person must decide, with its button.
fn notice(words: String, ink: limn::Color, button: Option<Element>, width: f32) -> Element {
    let mut row = vec![
        hgap(12.0),
        text(words).foreground(ink).centered_y(),
        Element::spacer().flex(1.0),
    ];
    row.extend(button.map(Element::centered_y));
    row.push(hgap(6.0));
    Element::column(vec![
        Element::row(row).gap(8.0).size(None, Some(NOTICE - 1.0)),
        hline(width, look::pane_border()),
    ])
}

fn notices(ide: &Ide, width: f32) -> Vec<Element> {
    let mut shown = vec![];
    if ide.quitting {
        let files = match ide.unsaved() {
            1 => "1 file has".to_owned(),
            n => format!("{n} files have"),
        };
        let words = format!("{files} unsaved changes: close the window again to discard them.");
        shown.push(notice(words, look::palette().text, None, width));
    }
    if let Some(error) = &ide.tree.error {
        let ok = keyed_button(FILE_ERROR_DISMISS, "OK", "", false, true, look::palette());
        shown.push(notice(error.clone(), look::failed(), Some(ok), width));
    }
    let Some((i, buffer)) = ide.active.zip(ide.active_buffer()) else {
        return shown;
    };
    if let Some(error) = &buffer.error {
        shown.push(notice(
            format!("Not saved: {error}"),
            look::failed(),
            None,
            width,
        ));
    }
    if buffer.stale {
        let reload = keyed_button(
            &format!("reload:{i}"),
            "Reload",
            "",
            false,
            true,
            look::palette(),
        );
        let words = "Changed on disk. Ctrl+S keeps yours; Reload takes the disk's.";
        shown.push(notice(
            words.into(),
            look::palette().text,
            Some(reload),
            width,
        ));
    }
    if ide.closing.as_ref() == Some(&buffer.path) {
        let words = "Unsaved changes. Close it again to discard them, or save them with Ctrl+S.";
        shown.push(notice(words.into(), look::palette().text, None, width));
    }
    shown
}

fn editor(ide: &Ide, width: f32, height: f32) -> Element {
    let note = |s: &str| {
        let fill = || Element::spacer().flex(1.0);
        Element::column(vec![fill(), muted(s).centered_x(), fill()]).flex(1.0)
    };
    let bar = ide.active_buffer().map(|b| crumb_bar(ide, b, width));
    let top = usize::from(bar.is_some());
    let mut column: Vec<Element> = bar.into_iter().collect();
    column.extend(notices(ide, width));
    let height = height - (column.len() - top) as f32 * NOTICE - top as f32 * CRUMBS - FOOTER;
    let matches = ide.find.as_ref().and_then(|f| f.results.as_ref());
    column.push(match ide.active_buffer() {
        None => note("Pick a file to read it"),
        Some(Buffer { loading: true, .. }) => note(""),
        Some(Buffer { editor: None, .. }) => note("This file can't be shown as text."),
        Some(
            buffer @ Buffer {
                path,
                editor: Some(editor),
                ..
            },
        ) => {
            let snapshot = editor.snapshot().ok();
            let (document, revision) = snapshot
                .as_ref()
                .map_or((0, 0), |s| (s.document_id(), s.revision()));
            // Drawn on the revision they were found in only; an empty lane keeps the text
            // where it is meanwhile, as openADE does.
            let (folds, scopes) = structure_controls(buffer, revision);
            // An editor per file keeps each one's caret and scroll while another is on screen.
            // With Vim keys, a file opens in Normal mode: reading, until `i`.
            let code = code::editor(buffer.editor_id(), editor, path)
                .font_size(ide.prefs.font_size as f32)
                .tab_width(ide.prefs.tab_width as usize)
                .indent_style(indent_style(&ide.prefs))
                .vim_mode(ide.prefs.vim)
                .soft_wrap(ide.prefs.wrap)
                .minimap(ide.prefs.minimap)
                .vim_mode_label(false)
                .context_actions(context_actions(ide.prefs.vim))
                .change_marks(
                    editor.snapshot().map_or(0, |s| s.document_id()),
                    change_marks(&buffer.changes),
                )
                .structure(document, revision, folds, scopes)
                .folds(buffer.folded.clone())
                // A match found is not left under the find bar.
                .reveal_top_inset(if ide.find.is_some() { 48.0 } else { 0.0 })
                .diagnostics(document, revision, diagnostics(ide, path, revision))
                .search_matches(
                    matches.map_or(0, |r| r.revision),
                    matches.map(|r| r.matches.clone()).unwrap_or_default(),
                )
                .height(height)
                .render();
            let text = || {
                snapshot
                    .as_ref()
                    .map(|s| s.chunks().collect::<String>())
                    .unwrap_or_default()
            };
            match (markdown(path), buffer.view) {
                (true, DocView::Preview) => markdown_preview(ide, &text(), width, height),
                (true, DocView::Split) => {
                    let half = ((width - 1.0) / 2.0).floor();
                    // In a row the editor would take its longest line's width: give it its half.
                    Element::row(vec![
                        code.size(Some(half), Some(height)),
                        vline(height),
                        markdown_preview(ide, &text(), width - half - 1.0, height),
                    ])
                }
                _ => code,
            }
        }
    });
    // The server's hover floats at the editor's top end, as the find bar does, until a key.
    if let Some(words) = &ide.hover {
        // As tall as its lines, up to a screenful's share; longer, it scrolls.
        let lines = words.lines().count() as f32;
        let tall = (lines * 22.0 + 24.0).clamp(48.0, 260.0);
        let text = limn::Markdown::new(words, look::menu_palette())
            .width(456.0)
            .font_size(look::BODY)
            .render()
            .element;
        let card = Element::column(vec![
            Element::scroll("hover:scroll", text).size(Some(456.0), Some(tall))
        ])
        .padding(12.0)
        .size(Some(480.0), None)
        .background(look::menu_palette().surface)
        .radius(10.0)
        .shadow(
            limn::Color::rgba(0, 0, 0, 60),
            limn::Point::new(0.0, 4.0),
            16.0,
            0.0,
        );
        let anchor = Element::spacer().size(Some(width - 12.0), Some(0.0));
        let hover = Element::popover(
            anchor,
            card,
            HOVER_DISMISS,
            limn::PopoverPlacement::BottomEnd,
        )
        .popover_non_modal();
        column.insert(top, hover);
    }
    // The find bar floats under the end of the line over the editor.
    if let Some(bar) = &ide.find {
        let anchor = Element::spacer().size(Some(width - 12.0), Some(0.0));
        column.insert(top, find::bar(bar, width, anchor));
    }
    column.push(footer(ide, width));
    Element::column(column)
}

/// The name for something new, or for a rename, in quill's search: its field, then what Enter
/// does or why it can't.
/// The offers under the word being completed, at the caret as it was drawn when it began.
fn completion_list(
    completion: &Completion,
    shown: &[(usize, &Offer)],
    base: Element,
    window: Size,
) -> Element {
    let items = shown
        .iter()
        .enumerate()
        .map(|(row, (_, offer))| {
            let item = limn::SuggestionItem::new(format!("complete:{row}"), offer.label.clone());
            if offer.detail.is_empty() {
                item
            } else {
                item.detail(offer.detail.clone())
            }
        })
        .collect();
    let at = completion.at;
    let anchor = Element::spacer().size(Some(1.0), Some(at.height));
    let list = limn::SuggestionList::new(
        anchor,
        "complete",
        items,
        completion.chosen,
        true,
        "complete:dismiss",
        look::menu_palette(),
    )
    .placement(limn::PopoverPlacement::BottomStart)
    .width(420.0)
    .animation_ms(0, 0)
    .render();
    // The anchor is placed where the caret was, over the page.
    let layer = Element::column(vec![
        Element::spacer().size(None, Some(at.y.max(0.0))),
        Element::row(vec![
            Element::spacer().size(Some(at.x.max(0.0)), None),
            list,
        ]),
    ]);
    Element::stack(vec![base, layer]).size(Some(window.width), Some(window.height))
}

fn markdown(path: &Path) -> bool {
    code::is_markdown(path)
}

/// The bar over the editor, as openADE's: the file's path, its folders leading to the tree and
/// its name copying itself, then the declarations the caret is in, each taking the caret to
/// it; at the end, a Markdown file's views, Find and the editor's menu.
fn crumb_bar(ide: &Ide, buffer: &Buffer, width: f32) -> Element {
    let palette = look::palette();
    let relative = ide.relative(&buffer.path).display().to_string();
    let root = ide.root.clone();
    let file = relative.clone();
    let mut crumbs = limn::Breadcrumb::from_path("crumbs", relative, palette)
        .dirty(buffer.dirty())
        .segment_action(move |segment| {
            Some(if segment == file {
                COPY_RELATIVE.into()
            } else {
                format!("tree:reveal-dir:{}", root.join(segment).display())
            })
        });
    let snapshot = buffer.editor.as_ref().and_then(|e| e.snapshot().ok());
    if let (Some(structure), Some(snapshot)) = (&buffer.structure, &snapshot) {
        let caret = snapshot.selection().head;
        for (name, range) in tree_sitter::symbols_at(structure, caret) {
            let row = snapshot.row_of(range.start).unwrap_or(0);
            crumbs =
                crumbs.item(limn::BreadcrumbItem::new(name.clone()).action(format!("scope:{row}")));
        }
    }
    let mut tools = vec![];
    if markdown(&buffer.path) {
        let tabs = [
            ("source", "Source", DocView::Source),
            ("preview", "Preview", DocView::Preview),
            ("split", "Split", DocView::Split),
        ]
        .into_iter()
        .map(|(key, label, view)| {
            limn::Tab::new(format!("doc:{key}"), label, 64.0).active(buffer.view == view)
        })
        .collect();
        let switch = limn::SegmentedControl::new(
            tabs,
            ComponentPalette {
                raised: look::settings_track(),
                selected: look::settings_current(),
                ..palette
            },
        );
        tools.push(switch.render().centered_y());
    }
    tools.push(
        tool_surface(
            FIND_OPEN,
            icons::of(icons::SEARCH),
            "Find",
            24.0,
            None,
            look::tool_hover(),
            Some(if ide.prefs.vim { "/" } else { "Ctrl+F" }),
        )
        .centered_y(),
    );
    let item = |id: &str, label: &str, keys: &str| {
        let item = limn::DropdownMenuItem::new(id, label);
        limn::DropdownMenuEntry::from(if keys.is_empty() {
            item
        } else {
            item.shortcut(keys)
        })
    };
    let entries = vec![
        item(LSP_DEFINITION, "Go to definition", "F12"),
        item(LSP_REFERENCES, "Find references", "Shift+F12"),
        item(LSP_RENAME, "Rename symbol", "F2"),
        item(LSP_FORMAT, "Format document", "Ctrl+Shift+I"),
        limn::DropdownMenuEntry::Separator,
        item(ASK_AGENT, "Ask agent", "Ctrl+L"),
        limn::DropdownMenuEntry::Separator,
        item(QUICK_LINE, "Go to line", "Ctrl+G"),
        limn::DropdownMenuEntry::from(
            limn::DropdownMenuItem::new(WRAP, "Wrap long lines").selected(ide.prefs.wrap),
        ),
        limn::DropdownMenuEntry::from(
            limn::DropdownMenuItem::new(MINIMAP, "Minimap").selected(ide.prefs.minimap),
        ),
        limn::DropdownMenuEntry::Separator,
        item(COPY_PATH, "Copy path", ""),
        item(COPY_RELATIVE, "Copy relative path", ""),
        item(REVEAL, "Reveal in files", ""),
    ];
    let more = tool_surface(
        MORE,
        icons::of(icons::DOTS_HORIZONTAL),
        "More",
        24.0,
        ide.more_open.then(look::active),
        look::tool_hover(),
        None,
    );
    tools.push(
        menu(
            more,
            entries,
            ide.more_open,
            MORE,
            240.0,
            limn::PopoverPlacement::BottomEnd,
        )
        .centered_y(),
    );
    let tools_width = 24.0 * 2.0 + 8.0 + if markdown(&buffer.path) { 200.0 } else { 0.0 };
    let crumbs = crumbs
        .width((width - tools_width - 32.0).max(80.0))
        .height(CRUMBS - 1.0)
        .render();
    let mut row = vec![hgap(8.0), crumbs.centered_y(), Element::spacer().flex(1.0)];
    row.extend(tools);
    row.push(hgap(6.0));
    Element::column(vec![
        Element::row(row)
            .gap(4.0)
            .size(Some(width), Some(CRUMBS - 1.0)),
        hline(width, look::pane_border()),
    ])
}

/// A Markdown file rendered, in a reading column, as quill renders its conversation: code in
/// its inks, diagrams drawn, links that open.
fn markdown_preview(ide: &Ide, text: &str, width: f32, height: f32) -> Element {
    let ids = code::PreviewIds {
        id: "preview",
        scroll: "preview:scroll",
        link: MD_LINK,
        diagram: MD_DIAGRAM,
    };
    code::markdown_preview(text, ids, &ide.diagram_sources, [width, height])
}

fn terminal_height(window: Size) -> f32 {
    ((window.height - look::TOP_BAR) * TERMINAL_HEIGHT)
        .round()
        .max(120.0)
}

/// The folder's shells under the editor, as quill's terminal pane: "Terminal" and
/// the folder while there is one, pills once there are more, then its new and its close.
fn terminal_pane(ide: &Ide, width: f32, height: f32) -> Element {
    let palette = look::palette();
    let terminal = &ide.terminal;
    let folder = name(&ide.root);
    let new = icon_tool(
        TERMINAL_NEW,
        icons::of(icons::ADD),
        "New terminal",
        28.0,
        palette,
    );
    let lead = if terminal.tabs.len() <= 1 {
        Element::row(vec![
            text(terminal.tabs.first().cloned().unwrap_or("Terminal".into())).centered_y(),
            muted(folder).centered_y(),
            hgap(4.0),
            new,
        ])
        .gap(8.0)
    } else {
        let mut pills: Vec<Element> = terminal
            .tabs
            .iter()
            .enumerate()
            .map(|(i, title)| {
                let close = icon_tool(
                    &format!("terminal:close:{i}"),
                    icons::of(icons::CLOSE),
                    "Close terminal",
                    20.0,
                    palette,
                );
                let pill = Element::row(vec![hgap(8.0), text(title.clone()).centered_y(), close])
                    .gap(6.0)
                    .size(None, Some(28.0))
                    .radius(8.0);
                let pill = if i == terminal.active {
                    pill.background(look::active())
                } else {
                    pill
                };
                Element::clickable(format!("terminal:tab:{i}"), pill).centered_y()
            })
            .collect();
        pills.push(new);
        Element::row(pills).gap(4.0)
    };
    let close = icon_tool(PANE_CLOSE, icons::of(icons::CLOSE), "Close", 28.0, palette);
    // Its sections, as openADE's bottom panel: the shells, or the problems with their count.
    let problems = ide.problems();
    let label = match problems.len() {
        0 => "Problems".to_owned(),
        n => format!("Problems {n}"),
    };
    let sections = limn::SegmentedControl::new(
        vec![
            limn::Tab::new(PANE_TERMINAL, "Terminal", 80.0).active(!terminal.problems),
            limn::Tab::new(PANE_PROBLEMS, label, 100.0).active(terminal.problems),
        ],
        ComponentPalette {
            raised: look::settings_track(),
            selected: look::settings_current(),
            ..palette
        },
    )
    .render()
    .centered_y();
    let lead = if terminal.problems {
        sections
    } else {
        Element::row(vec![sections, hgap(4.0), lead]).gap(8.0)
    };
    let header = pane_header(lead, vec![close], width);
    let below = (height - 1.0 - look::PANE_HEADER).max(0.0);
    if terminal.problems {
        return Element::column(vec![
            hline(width, look::pane_border()),
            header,
            problems_list(ide, &problems, width, below),
        ])
        .size(Some(width), Some(height));
    }
    let body = match &terminal.screen {
        // Its cursor in the palette's text: quill's is the blue of its ANSI palette.
        Some(screen) => limn::TerminalView::new(
            TERMINAL,
            screen.clone(),
            ComponentPalette {
                text: look::cursor(),
                ..palette
            },
        )
        .font_size(TERMINAL_FONT)
        .background(look::theme().background)
        // The shell's own dark ink would be lost on a dark page.
        .min_contrast(4.5)
        .focus_ring(false)
        .render()
        .padding(TERMINAL_PADDING[1]),
        None => Element::spacer(),
    };
    Element::column(vec![
        hline(width, look::pane_border()),
        header,
        body.flex(1.0),
    ])
    .size(Some(width), Some(height))
}

/// The problems under their files: each file by its icon, its name, its folder and how many
/// it has; each problem by its mark, what the server said and where, a press away.
fn problems_list(ide: &Ide, problems: &[Problem], width: f32, height: f32) -> Element {
    if problems.is_empty() {
        let says = if ide
            .lsp_servers
            .values()
            .any(|s| s.ready && s.failure.is_none())
        {
            "No problems"
        } else {
            "Errors and warnings show here once a language server runs for your files."
        };
        let says = muted(says.to_owned()).font_size(SMALL).centered();
        return Element::column(vec![says]).size(Some(width), Some(height));
    }
    let mut rows = vec![];
    let mut at = 0;
    for group in problems.chunk_by(|a, b| a.path == b.path) {
        let path = &group[0].path;
        let (icon, ink) = ide.pack.icon(path, false, false);
        let folder = path
            .strip_prefix(&ide.root)
            .ok()
            .and_then(Path::parent)
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        rows.push(
            Element::row(vec![
                hgap(8.0),
                Element::icon(icon)
                    .foreground(ink)
                    .size(Some(14.0), Some(14.0))
                    .centered_y(),
                text(name(path)).centered_y(),
                muted(folder).font_size(SMALL).centered_y(),
                muted(group.len().to_string()).font_size(SMALL).centered_y(),
            ])
            .gap(6.0)
            .size(None, Some(file_tree::ROW)),
        );
        for problem in group {
            let (mark, ink) = match problem.severity {
                1 => (icons::ERROR, look::failed()),
                _ => (icons::WARNING, look::awaiting()),
            };
            let message = problem
                .message
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            let place = format!("Ln {}, Col {}", problem.row + 1, problem.column + 1);
            let content = vec![
                hgap(28.0),
                Element::icon(icons::of(mark))
                    .foreground(ink)
                    .size(Some(13.0), Some(13.0))
                    .centered_y(),
                Element::truncated_text(message)
                    .font_family(limn::FontFamily::Sans)
                    .max_width((width - 220.0).max(80.0))
                    .centered_y(),
                Element::spacer().flex(1.0),
                muted(place).font_size(SMALL).centered_y(),
                hgap(8.0),
            ];
            rows.push(side_row(
                format!("problem:{at}"),
                content,
                false,
                file_tree::ROW,
            ));
            at += 1;
        }
    }
    Element::scroll("problems:scroll", Element::column(rows).padding(6.0))
        .size(Some(width), Some(height))
}

/// The server's diagnostics of `path` as the editor underlines them, if they came for
/// `revision`: errors in quill's failed red, warnings in its amber, the rest muted.
fn diagnostics(ide: &Ide, path: &Path, revision: u64) -> Vec<limn::CodeDiagnostic> {
    let Some(found) = ide.diagnostics.get(path).and_then(|d| d.at(revision)) else {
        return vec![];
    };
    found
        .iter()
        .map(|d| limn::CodeDiagnostic {
            range: d.range.clone(),
            color: match d.severity {
                1 => look::failed(),
                2 => look::awaiting(),
                _ => look::muted(),
            },
            message: d.message.clone(),
        })
        .collect()
}

/// The fold controls and sticky lines of `buffer`'s structure, if it was found in `revision`.
fn structure_controls(
    buffer: &Buffer,
    revision: u64,
) -> (Vec<limn::CodeFoldCandidate>, Vec<limn::CodeStickyScope>) {
    let Some(structure) = buffer.structure.as_ref().filter(|s| s.revision == revision) else {
        return (vec![], vec![]);
    };
    let folds = structure
        .folds
        .iter()
        .map(|rows| limn::CodeFoldCandidate {
            start_row: rows.start,
            end_row: rows.end,
            action_id: format!("fold:{}:{}", rows.start, rows.end),
        })
        .collect();
    let scopes = structure
        .scopes
        .iter()
        .map(|rows| limn::CodeStickyScope {
            start_row: rows.start,
            end_row: rows.end,
            action_id: format!("scope:{}", rows.start),
        })
        .collect();
    (folds, scopes)
}

/// The gutter's marks, as editors draw them: added rows in quill's added green, changed rows
/// in its cursor blue, removed lines in its removed red.
fn change_marks(changes: &[(std::ops::Range<usize>, Change)]) -> Vec<limn::CodeChangeMark> {
    changes
        .iter()
        .map(|(rows, change)| limn::CodeChangeMark {
            rows: rows.clone(),
            color: match change {
                Change::Added => look::added(),
                Change::Changed => look::cursor(),
                Change::Removed => look::removed(),
            },
        })
        .collect()
}

/// What the editor's context menu offers after cutting, copying and pasting: finding, and the
/// file's place, until the language server brings its own.
fn context_actions(vim: bool) -> Vec<limn::ContextMenuEntry> {
    use limn::{ContextMenuEntry::Item, ContextMenuItem};
    vec![
        Item(ContextMenuItem::new(LSP_DEFINITION, "Go to definition").shortcut("F12")),
        Item(ContextMenuItem::new(LSP_REFERENCES, "Find references").shortcut("Shift+F12")),
        Item(ContextMenuItem::new(LSP_HOVER, "Show hover").shortcut(if vim { "K" } else { "" })),
        Item(ContextMenuItem::new(LSP_RENAME, "Rename symbol").shortcut("F2")),
        Item(ContextMenuItem::new(LSP_FORMAT, "Format document").shortcut("Ctrl+Shift+I")),
        limn::ContextMenuEntry::Separator,
        Item(ContextMenuItem::new(ASK_AGENT, "Ask agent").shortcut("Ctrl+L")),
        limn::ContextMenuEntry::Separator,
        Item(ContextMenuItem::new(FIND_OPEN, "Find").shortcut(if vim { "/" } else { "Ctrl+F" })),
        limn::ContextMenuEntry::Separator,
        Item(ContextMenuItem::new(COPY_PATH, "Copy path")),
        Item(ContextMenuItem::new(COPY_RELATIVE, "Copy relative path")),
        Item(ContextMenuItem::new(REVEAL, "Reveal in files")),
    ]
}

/// One indent level as Settings say: spaces, or a tab, so many columns wide.
pub fn indent_style(prefs: &Prefs) -> limn_editor::IndentStyle {
    let width = prefs.tab_width.clamp(1, 32) as usize;
    if prefs.spaces {
        limn_editor::IndentStyle::Spaces(width)
    } else {
        limn_editor::IndentStyle::Tabs(width)
    }
}

/// The line under the editor, quiet as quill's pane headers: the Vim mode, the file and
/// whether it is saved, then the caret, the language, the line ending and the branch.
fn footer(ide: &Ide, width: f32) -> Element {
    let small = |s: String| muted(s).font_size(SMALL).centered_y();
    let mut lead = vec![hgap(12.0)];
    let buffer = ide.active_buffer();
    if let Some(mode) = ide.vim_mode.filter(|_| buffer.is_some() && ide.prefs.vim) {
        let ink = match mode {
            limn::VimMode::Insert => look::running(),
            limn::VimMode::Normal => look::muted(),
            _ => look::pro(),
        };
        // Said as the rest of the footer, "Normal", in the mode's ink.
        let label = mode.label().to_lowercase();
        let mut chars = label.chars();
        let label: String = chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default();
        lead.push(text(label).font_size(SMALL).foreground(ink).centered_y());
    }
    let mut tail = vec![];
    for note in [&ide.link_note, &ide.lsp_note].into_iter().flatten() {
        tail.push(small(note.clone()));
    }
    let Some(buffer) = buffer else {
        lead.push(small(name(&ide.root)));
        return footer_row(ide, lead, tail, width);
    };
    if buffer.dirty() {
        lead.push(small("Unsaved".into()));
    }
    let Some(snapshot) = buffer.editor.as_ref().and_then(|e| e.snapshot().ok()) else {
        return footer_row(ide, lead, tail, width);
    };
    // Errors and warnings, each by its mark; a press goes to the next.
    let found = ide
        .diagnostics
        .get(&buffer.path)
        .and_then(|d| d.at(snapshot.revision()));
    let count = |severity| found.map_or(0, |d| d.iter().filter(|d| d.severity == severity).count());
    let mut problems = vec![];
    for (icon, ink, n) in [
        (icons::ERROR, look::failed(), count(1)),
        (icons::WARNING, look::awaiting(), count(2)),
    ] {
        if n > 0 {
            problems.push(
                Element::icon(icons::of(icon))
                    .foreground(ink)
                    .size(Some(13.0), Some(13.0))
                    .centered_y(),
            );
            problems.push(small(n.to_string()));
        }
    }
    if !problems.is_empty() {
        tail.push(chip(
            PROBLEMS_TOGGLE,
            problems,
            "Problems",
            None,
            Some("F8 goes to the next one."),
        ));
    }
    // The file's language server: a dot of its state, and what it is in the tooltip.
    let language = limn_language::Language::from_path(&buffer.path.to_string_lossy());
    if let Some(server) = ide.lsp_servers.get(&language) {
        let (ink, says) = match (&server.failure, server.ready) {
            (Some(why), _) => (look::failed(), why.clone()),
            (None, true) => (look::running(), format!("{} is running", server.name)),
            (None, false) => (look::awaiting(), format!("{} is starting", server.name)),
        };
        let dot = limn::StatusDot::new(ink).size(6.0).render().centered_y();
        if server.failure.is_some() {
            // Down, it says so, and a press starts it again: after installing it, too.
            let label = if server.ready {
                format!("{} stopped", server.name)
            } else {
                server.name.clone()
            };
            let again = Element::icon(icons::of(icons::RELOAD))
                .foreground(look::muted())
                .size(Some(13.0), Some(13.0))
                .centered_y();
            let more = match &server.hint {
                Some(hint) => format!("{hint}. Click to start it again."),
                None => "Click to start it again.".to_owned(),
            };
            tail.push(chip(
                LSP_RESTART,
                vec![dot, small(label), again],
                &says,
                None,
                Some(&more),
            ));
        } else {
            let item = Element::row(vec![hgap(6.0), dot, small(server.name.clone()), hgap(6.0)])
                .gap(6.0)
                .size(None, Some(22.0));
            let tip = limn::Tooltip::new(item, "footer:server", says, look::palette());
            tail.push(tip.render().centered_y());
        }
    }
    let at = snapshot
        .position_of(snapshot.selection().head)
        .unwrap_or_default();
    let selected = snapshot.selection().range().len();
    let mut caret = format!("Ln {}, Col {}", at.row + 1, at.column + 1);
    if selected > 0 {
        caret.push_str(&format!(" ({selected} selected)"));
    }
    tail.push(chip(
        QUICK_LINE,
        vec![small(caret)],
        "Go to line",
        Some("Ctrl+G"),
        None,
    ));
    let ext = buffer
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    tail.push(small(language_name(ext).into()));
    let ending = match snapshot.preferred_line_ending() {
        limn_editor::LineEnding::CrLf => "CRLF",
        _ => "LF",
    };
    tail.push(small(ending.into()));
    footer_row(ide, lead, tail, width)
}

/// The footer's line: what is said of the file on the left, the rest and the branch on the
/// right, under a hairline.
fn footer_row(ide: &Ide, lead: Vec<Element>, mut tail: Vec<Element>, width: f32) -> Element {
    if let Some(branch) = &ide.branch {
        let icon = Element::icon(icons::of(icons::GIT_BRANCH))
            .foreground(look::muted())
            .size(Some(13.0), Some(13.0))
            .centered_y();
        let name = muted(branch.clone()).font_size(SMALL).centered_y();
        tail.push(Element::row(vec![icon, name]).gap(4.0));
    }
    tail.push(hgap(8.0));
    let mut row = lead;
    row.push(Element::spacer().flex(1.0));
    row.extend(tail);
    Element::column(vec![
        hline(width, look::pane_border()),
        Element::row(row).gap(8.0).size(None, Some(FOOTER - 1.0)),
    ])
}

/// A footer item that does something, as quill's composer chips: filled under
/// the pointer, with what it does, its keys and more in its tooltip.
fn chip(
    id: &str,
    content: Vec<Element>,
    tip: &str,
    keys: Option<&str>,
    more: Option<&str>,
) -> Element {
    let mut items = vec![hgap(6.0)];
    items.extend(content);
    items.push(hgap(6.0));
    let row = Element::row(items).gap(4.0).size(None, Some(22.0));
    let chip = ComponentPalette {
        hover: look::chip_hover(),
        ..look::palette()
    }
    .interactive_surface(Element::clickable(id, row))
    .radius(6.0);
    let mut tip = limn::Tooltip::new(chip, id, tip, look::palette());
    if let Some(keys) = keys {
        tip = tip.shortcut(keys);
    }
    if let Some(more) = more {
        tip = tip.description(more);
    }
    tip.render().centered_y()
}

/// The name of the language limn colors `extension` in.
fn language_name(extension: &str) -> &'static str {
    use limn::CodeLanguage::*;
    match limn::CodeLanguage::from_code_fence(extension) {
        Plain => "Plain text",
        Rust => "Rust",
        Json => "JSON",
        JavaScript => "JavaScript",
        TypeScript => "TypeScript",
        Tsx => "TSX",
        Python => "Python",
        Toml => "TOML",
        Css => "CSS",
        Html => "HTML",
        Markdown => "Markdown",
        Go => "Go",
        Java => "Java",
        Kotlin => "Kotlin",
        Swift => "Swift",
        C => "C",
        Cpp => "C++",
        CSharp => "C#",
        Php => "PHP",
        Ruby => "Ruby",
        Lua => "Lua",
        Shell => "Shell",
        Yaml => "YAML",
        Sql => "SQL",
        Diff => "Diff",
        Dart => "Dart",
        Zig => "Zig",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limn::UiTree;
    use newen_components::tree::TRASH_CONFIRM;

    fn click(ide: &mut Ide, id: &str) -> Vec<Job> {
        ide.update(&UiAction::Clicked { id: id.into() })
    }

    fn with_files() -> Ide {
        let (mut ide, jobs) = Ide::new("/r".into(), Prefs::default(), Session::default());
        assert_eq!(jobs[0], Job::List("/r".into()));
        ide.reduced_motion = true;
        let entry = |p: &str, dir| Entry {
            path: p.into(),
            dir,
        };
        ide.loaded(Loaded::Listed(
            "/r".into(),
            vec![entry("/r/src", true), entry("/r/a.rs", false)],
            None,
        ));
        ide
    }

    fn read(ide: &mut Ide, path: &str, text: &str) -> Vec<Job> {
        ide.loaded(Loaded::Read(path.into(), Some(text.into()), None))
    }

    fn type_in(buffer: &Buffer, at: usize, text: &str) {
        let shared = buffer.editor.as_ref().unwrap().shared_buffer();
        let mut buffer = shared.lock().unwrap();
        let caret = Selection::caret(at + text.chars().count());
        buffer
            .edit(at..at, text, caret, EditKind::Typing, 0)
            .unwrap();
    }

    fn text_of(buffer: &Buffer) -> String {
        let snapshot = buffer.editor.as_ref().unwrap().snapshot().unwrap();
        snapshot.chunks().collect()
    }

    fn stamp(secs: u64) -> Stamp {
        Some(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }

    #[test]
    fn a_folder_is_listed_once_and_folds() {
        let mut ide = with_files();
        assert_eq!(
            click(&mut ide, "dir:/r/src"),
            vec![Job::List("/r/src".into())]
        );
        ide.loaded(Loaded::Listed("/r/src".into(), vec![], None));
        assert!(click(&mut ide, "dir:/r/src").is_empty());
        assert!(!ide.tree.expanded.contains(Path::new("/r/src")));
        assert!(click(&mut ide, "dir:/r/src").is_empty());
    }

    #[test]
    fn files_open_once_and_close_to_their_neighbour() {
        let mut ide = with_files();
        assert_eq!(
            click(&mut ide, "file:/r/a.rs"),
            vec![Job::Read("/r/a.rs".into())]
        );
        read(&mut ide, "/r/a.rs", "fn a() {}");
        ide.loaded(Loaded::Read("/r/b.bin".into(), None, None));
        assert_eq!(ide.active, Some(1));
        assert!(click(&mut ide, "file:/r/a.rs").is_empty());
        assert_eq!(ide.active, Some(0));
        assert!(ide.buffers[1].editor.is_none());
        ide.active = Some(1);
        click(&mut ide, "close:1");
        assert_eq!(ide.active, Some(0));
        click(&mut ide, "close:0");
        assert_eq!(ide.active, None);
    }

    #[test]
    fn closing_before_the_one_on_screen_keeps_it_on_screen() {
        let mut ide = with_files();
        for p in ["/r/1", "/r/2", "/r/3"] {
            read(&mut ide, p, "");
        }
        ide.active = Some(2);
        ide.close(0);
        assert_eq!(ide.buffers[ide.active.unwrap()].path, Path::new("/r/3"));
    }

    #[test]
    fn a_file_with_unsaved_changes_closes_when_asked_twice_in_a_row() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "a");
        type_in(&ide.buffers[0], 0, "x");
        click(&mut ide, "close:0");
        assert_eq!(ide.buffers.len(), 1);
        click(&mut ide, TREE_TOGGLE);
        click(&mut ide, "close:0");
        assert_eq!(ide.buffers.len(), 1);
        click(&mut ide, "close:0");
        assert!(ide.buffers.is_empty());
    }

    #[test]
    fn a_save_writes_what_the_buffer_held_and_then_it_reads_as_saved() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "a");
        type_in(&ide.buffers[0], 1, "b");
        assert!(ide.buffers[0].dirty());
        let jobs = ide.update(&UiAction::EditorSaveRequested {
            id: "editor:/r/a.rs".into(),
            document_id: 0,
            revision: 0,
        });
        let [Job::Write(path, save)] = <[Job; 1]>::try_from(jobs).unwrap() else {
            panic!("a save writes");
        };
        assert_eq!(save.0.chunks().collect::<String>(), "ab");
        type_in(&ide.buffers[0], 2, "c");
        ide.loaded(Loaded::Written(path, save, Ok(stamp(5))));
        // What was typed after the save asked is still to save.
        assert!(ide.buffers[0].dirty());
        assert_eq!(ide.buffers[0].stamp, stamp(5));
        ide.loaded(Loaded::Written(
            "/r/a.rs".into(),
            Save(ide.buffers[0].editor.as_ref().unwrap().snapshot().unwrap()),
            Err("disk full".into()),
        ));
        assert_eq!(ide.buffers[0].error.as_deref(), Some("disk full"));
    }

    #[test]
    fn a_file_changed_on_disk_reloads_in_place_keeping_the_caret() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "abc\ndef");
        let shared = ide.buffers[0].editor.as_ref().unwrap().shared_buffer();
        shared
            .lock()
            .unwrap()
            .set_selection(Selection::caret(5))
            .unwrap();
        let [Job::Stat(watched)] = <[Job; 1]>::try_from(ide.poll()).unwrap() else {
            panic!("a poll stats");
        };
        assert!(watched.contains(&("/r/a.rs".into(), None)));
        assert!(ide.poll().is_empty(), "one at a time");
        let jobs = ide.loaded(Loaded::Changed(vec![("/r/a.rs".into(), stamp(1))]));
        assert_eq!(jobs, vec![Job::Read("/r/a.rs".into())]);
        ide.loaded(Loaded::Read(
            "/r/a.rs".into(),
            Some("aXbc\ndef".into()),
            stamp(1),
        ));
        let buffer = &ide.buffers[0];
        assert_eq!(text_of(buffer), "aXbc\ndef");
        assert!(!buffer.dirty());
        assert_eq!(shared.lock().unwrap().selection().head, 6);
        assert!(shared.lock().unwrap().can_undo());
        assert_eq!(ide.poll().len(), 1);
    }

    #[test]
    fn unsaved_changes_are_kept_over_the_disk_until_the_person_reloads() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "a");
        type_in(&ide.buffers[0], 1, "b");
        let jobs = ide.loaded(Loaded::Changed(vec![("/r/a.rs".into(), stamp(2))]));
        assert!(jobs.is_empty());
        assert!(ide.buffers[0].stale);
        assert_eq!(text_of(&ide.buffers[0]), "ab");
        assert_eq!(
            click(&mut ide, "reload:0"),
            vec![Job::Read("/r/a.rs".into())]
        );
        ide.loaded(Loaded::Read(
            "/r/a.rs".into(),
            Some("disk".into()),
            stamp(2),
        ));
        assert_eq!(text_of(&ide.buffers[0]), "disk");
        assert!(!ide.buffers[0].stale && !ide.buffers[0].dirty());
    }

    #[test]
    fn a_listed_folder_that_changes_is_listed_again() {
        let mut ide = with_files();
        let jobs = ide.loaded(Loaded::Changed(vec![("/r".into(), stamp(3))]));
        assert_eq!(jobs, vec![Job::List("/r".into())]);
    }

    #[test]
    fn settings_change_the_icons_and_are_kept_at_once() {
        let mut ide = with_files();
        assert!(
            click(&mut ide, "set:vim").is_empty(),
            "shut, Settings take no clicks"
        );
        let jobs = click(&mut ide, SETTINGS_OPEN);
        assert!(matches!(
            jobs.as_slice(),
            [Job::FindPacks(_), Job::FindFonts]
        ));
        click(&mut ide, "set:icons");
        assert_eq!(ide.settings_menu.as_deref(), Some("icons"));
        let jobs = click(&mut ide, "set:icons=theme:/t/theme.json");
        assert_eq!(ide.settings_menu, None);
        assert!(
            matches!(&jobs[..], [Job::SavePrefs(_), Job::LoadPack(p)] if p == Path::new("/t/theme.json"))
        );
        // A theme that comes late, after another was chosen, is not put in use.
        click(&mut ide, "set:icons=quill");
        ide.loaded(Loaded::Pack("/t/theme.json".into(), Ok(Theme::default())));
        assert!(matches!(ide.pack, Pack::Quill));
        let jobs = click(&mut ide, "set:vim");
        assert!(!ide.prefs.vim && matches!(&jobs[..], [Job::SavePrefs(p)] if !p.vim));
        click(&mut ide, SETTINGS_CLOSE);
        assert_eq!(ide.settings_page, None);
    }

    #[test]
    fn the_chosen_family_is_laid_out_in_and_a_late_one_is_not() {
        let mut ide = with_files();
        click(&mut ide, SETTINGS_OPEN);
        let jobs = click(&mut ide, "set:editor_font=Fira Code");
        assert!(matches!(&jobs[..], [Job::SavePrefs(_), Job::LoadFont(f)] if f == "Fira Code"));
        let bundled = limn::Fonts::bundled();
        let face = bundled
            .face_of(limn::FontFamily::Monospace)
            .unwrap()
            .clone();
        let faces = || Faces(face.clone(), face.clone());
        ide.loaded(Loaded::Font("Hack".into(), Ok(faces())));
        assert!(ide.fonts_due.is_none());
        ide.loaded(Loaded::Font("Fira Code".into(), Ok(faces())));
        let due = ide.fonts_due.take().unwrap();
        assert_eq!(
            due.face_of(limn::FontFamily::Monospace).unwrap().id(),
            face.id()
        );
    }

    #[test]
    fn find_marks_the_matches_and_takes_the_caret_from_one_to_the_next() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "let a = 1;\nlet b = a;\n");
        click(&mut ide, FIND_OPEN);
        assert_eq!(ide.focus.as_deref(), Some(FIND_QUERY));
        ide.update(&UiAction::InputChanged {
            id: FIND_QUERY.into(),
            value: "let".into(),
        });
        let selection = |ide: &Ide| {
            let editor = ide.buffers[0].editor.as_ref().unwrap();
            editor.snapshot().unwrap().selection().range()
        };
        assert_eq!(ide.find.as_ref().unwrap().count_label(), "1 of 2");
        assert_eq!(selection(&ide), 0..3);
        ide.update(&UiAction::Submitted {
            id: FIND_QUERY.into(),
            value: "let".into(),
        });
        assert_eq!(selection(&ide), 11..14);
        click(&mut ide, "find:next");
        assert_eq!(selection(&ide), 0..3, "round the end");
        click(&mut ide, "find:dismiss");
        assert!(ide.find.is_none());
        assert_eq!(ide.focus.as_deref(), Some("editor:/r/a.rs"));
    }

    #[test]
    fn a_file_opens_by_its_name() {
        let mut ide = with_files();
        assert_eq!(
            click(&mut ide, QUICK_OPEN),
            vec![Job::FindFiles("/r".into())]
        );
        ide.loaded(Loaded::Files(
            ["src/ide.rs", "src/main.rs", "README.md"]
                .map(PathBuf::from)
                .into(),
        ));
        ide.update(&UiAction::InputChanged {
            id: QUICK_FIELD.into(),
            value: "rs".into(),
        });
        ide.step_chosen(1);
        ide.step_chosen(5);
        let jobs = ide.update(&UiAction::Submitted {
            id: QUICK_FIELD.into(),
            value: "rs".into(),
        });
        assert_eq!(ide.quick, None);
        assert_eq!(jobs.len(), 1);
        assert!(matches!(&jobs[0], Job::Read(p) if p.starts_with("/r/src")));
    }

    #[test]
    fn what_was_open_opens_again_in_its_order() {
        let session = Session {
            open: vec!["/r/b.rs".into(), "/r/a.rs".into()],
            active: Some(1),
            expanded: vec!["/r/src".into()],
        };
        let (mut ide, jobs) = Ide::new("/r".into(), Prefs::default(), session.clone());
        assert!(jobs.contains(&Job::List("/r/src".into())));
        assert!(jobs.contains(&Job::Read("/r/a.rs".into())));
        assert!(ide.buffers.iter().all(|b| b.loading));
        // They come back in any order, and keep theirs.
        read(&mut ide, "/r/a.rs", "a");
        read(&mut ide, "/r/b.rs", "b");
        assert_eq!(ide.active, Some(1));
        assert!(ide.buffers.iter().all(|b| !b.loading && b.editor.is_some()));
        assert_eq!(ide.session(), session);
    }

    #[test]
    fn the_context_menu_copies_the_path_and_reveals_the_file_in_the_tree() {
        let mut ide = with_files();
        read(&mut ide, "/r/src/deep/x.rs", "x");
        click(&mut ide, COPY_RELATIVE);
        assert_eq!(ide.copy.take().as_deref(), Some("src/deep/x.rs"));
        click(&mut ide, TREE_TOGGLE);
        let jobs = click(&mut ide, REVEAL);
        assert!(ide.sidebar.open);
        assert!(ide.tree.expanded.contains(Path::new("/r/src/deep")));
        assert!(ide.tree.expanded.contains(Path::new("/r/src")));
        assert!(!ide.tree.expanded.contains(Path::new("/r")));
        assert_eq!(
            jobs,
            vec![Job::List("/r/src/deep".into()), Job::List("/r/src".into())]
        );
        assert_eq!(ide.reveal_row.as_deref(), Some("file:/r/src/deep/x.rs"));
    }

    #[test]
    fn a_line_found_in_every_file_opens_with_the_caret_on_it() {
        let mut ide = with_files();
        click(&mut ide, GREP_OPEN);
        let input = |ide: &mut Ide, value: &str| {
            ide.update(&UiAction::InputChanged {
                id: GREP_FIELD.into(),
                value: value.into(),
            })
        };
        assert!(
            input(&mut ide, "l").is_empty(),
            "one letter finds nothing yet"
        );
        assert_eq!(
            input(&mut ide, "le")[0],
            Job::Grep("/r".into(), "le".into(), 2)
        );
        input(&mut ide, "let");
        let hit = |row| Hit {
            path: "src/b.rs".into(),
            row,
            text: "let".into(),
        };
        // What the older search finds is dropped.
        ide.loaded(Loaded::Grep(2, vec![hit(0)]));
        assert!(ide.grep.as_ref().unwrap().hits.is_empty());
        ide.loaded(Loaded::Grep(3, vec![hit(0), hit(2)]));
        ide.step_chosen(1);
        let jobs = ide.update(&UiAction::Submitted {
            id: GREP_FIELD.into(),
            value: "let".into(),
        });
        assert_eq!(jobs, vec![Job::Read("/r/src/b.rs".into())]);
        assert!(ide.grep.is_none());
        read(&mut ide, "/r/src/b.rs", "a\nb\nlet c = 1;\n");
        let editor = ide.buffers[0].editor.as_ref().unwrap();
        assert_eq!(editor.snapshot().unwrap().selection().head, 4);
        assert_eq!(ide.focus.as_deref(), Some("editor:/r/src/b.rs"));
        assert!(ide.reveal);
    }

    #[test]
    fn the_gutter_compares_each_file_with_its_commit_once_a_revision() {
        let mut ide = with_files();
        // Tree-sitter's work, asked for beside, is left out here.
        let marks = |ide: &mut Ide| -> Vec<Job> {
            let jobs = ide.marks_due();
            jobs.into_iter()
                .filter(|j| !matches!(j, Job::Analyze(..)))
                .collect()
        };
        read(&mut ide, "/r/a.rs", "a\nb\n");
        assert_eq!(marks(&mut ide), vec![Job::Committed("/r/a.rs".into())]);
        assert!(marks(&mut ide).is_empty(), "asked once");
        ide.loaded(Loaded::Committed("/r/a.rs".into(), Some("a\n".into())));
        let jobs = marks(&mut ide);
        assert!(
            matches!(&jobs[..], [Job::Changes(p, c, _)] if p == Path::new("/r/a.rs") && &**c == "a\n")
        );
        assert!(marks(&mut ide).is_empty(), "one at a time");
        let revision = ide.buffers[0]
            .editor
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .revision();
        let added = vec![(1..2, Change::Added)];
        ide.loaded(Loaded::Changes("/r/a.rs".into(), revision, added.clone()));
        assert_eq!(ide.buffers[0].changes, added);
        assert!(marks(&mut ide).is_empty(), "nothing new to compare");
        type_in(&ide.buffers[0], 0, "x");
        assert_eq!(marks(&mut ide).len(), 1, "an edit is compared again");
        // A new commit: the file is read from it again.
        ide.head = Some(("/r/.git/HEAD".into(), None));
        ide.loaded(Loaded::Changed(vec![("/r/.git/HEAD".into(), stamp(9))]));
        assert!(marks(&mut ide).contains(&Job::Committed("/r/a.rs".into())));
    }

    #[test]
    fn the_tree_walks_with_nvims_keys() {
        let mut ide = with_files();
        click(&mut ide, TREE_TOGGLE);
        click(&mut ide, TREE_FOCUS);
        assert!(ide.sidebar.open);
        assert_eq!(ide.focus.as_deref(), Some("dir:/r/src"));
        let key = |ide: &mut Ide, at: &str, key| ide.tree_key(at, key);
        // Right opens the folder, and once open goes into it.
        let (_, jobs) = key(&mut ide, "dir:/r/src", TreeKey::Right);
        assert_eq!(jobs, vec![Job::List("/r/src".into())]);
        let entry = |p: &str| Entry {
            path: p.into(),
            dir: false,
        };
        ide.loaded(Loaded::Listed(
            "/r/src".into(),
            vec![entry("/r/src/m.rs")],
            None,
        ));
        key(&mut ide, "dir:/r/src", TreeKey::Right);
        assert_eq!(ide.focus.as_deref(), Some("file:/r/src/m.rs"));
        // Left from a file goes to its folder, and on the open folder shuts it.
        key(&mut ide, "file:/r/src/m.rs", TreeKey::Left);
        assert_eq!(ide.focus.as_deref(), Some("dir:/r/src"));
        key(&mut ide, "dir:/r/src", TreeKey::Left);
        assert!(!ide.tree.expanded.contains(Path::new("/r/src")));
        key(&mut ide, "dir:/r/src", TreeKey::Down);
        assert_eq!(ide.focus.as_deref(), Some("file:/r/a.rs"));
        let (_, jobs) = key(&mut ide, "file:/r/a.rs", TreeKey::Right);
        assert_eq!(jobs, vec![Job::Read("/r/a.rs".into())]);
        assert!(!key(&mut ide, "file:/r/nowhere", TreeKey::Up).0);
    }

    #[test]
    fn a_colon_and_a_number_go_to_that_line() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "a\nb\nc\n");
        click(&mut ide, QUICK_LINE);
        assert_eq!(ide.quick, Some((":".into(), 0)));
        assert_eq!(ide.focus.take().as_deref(), Some(QUICK_FIELD));
        ide.update(&UiAction::InputChanged {
            id: QUICK_FIELD.into(),
            value: ":3".into(),
        });
        let jobs = ide.update(&UiAction::Submitted {
            id: QUICK_FIELD.into(),
            value: ":3".into(),
        });
        assert!(jobs.is_empty() && ide.quick.is_none());
        let editor = ide.buffers[0].editor.as_ref().unwrap();
        assert_eq!(editor.snapshot().unwrap().selection().head, 4);
    }

    #[test]
    fn tree_sitter_is_asked_once_a_revision_and_its_blocks_fold() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "fn a() {\n    1;\n}\n");
        let analyze = |ide: &mut Ide| {
            let jobs = ide.marks_due();
            jobs.into_iter()
                .filter(|j| matches!(j, Job::Analyze(..)))
                .count()
        };
        assert_eq!(analyze(&mut ide), 1);
        assert_eq!(analyze(&mut ide), 0, "one at a time");
        let revision = ide.buffers[0]
            .editor
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .revision();
        let structure = Structure {
            revision,
            folds: std::iter::once(0..3).collect(),
            scopes: std::iter::once(0..3).collect(),
            symbols: vec![("a".into(), 0..16)],
        };
        ide.loaded(Loaded::Analyzed("/r/a.rs".into(), structure));
        assert_eq!(analyze(&mut ide), 0, "nothing new to look at");
        click(&mut ide, "fold:0:3");
        assert_eq!(
            ide.buffers[0].folded,
            std::iter::once(0..3).collect::<Vec<_>>()
        );
        click(&mut ide, "fold:0:3");
        assert!(ide.buffers[0].folded.is_empty());
        click(&mut ide, "scope:2");
        let editor = ide.buffers[0].editor.as_ref().unwrap();
        assert_eq!(editor.snapshot().unwrap().selection().head, 16);
        type_in(&ide.buffers[0], 0, "x");
        assert_eq!(analyze(&mut ide), 1, "an edit is looked at again");
    }

    #[test]
    fn the_terminal_opens_under_the_editor_and_asks_the_host_for_shells() {
        let mut ide = with_files();
        click(&mut ide, TERMINAL_TOGGLE);
        assert!(ide.terminal.open);
        assert_eq!(ide.focus.take().as_deref(), Some(TERMINAL));
        click(&mut ide, TERMINAL_NEW);
        click(&mut ide, "terminal:tab:0");
        click(&mut ide, "terminal:close:1");
        use TerminalRequest::*;
        assert_eq!(ide.terminal.requests, vec![New, Select(0), Close(1)]);
        let window = Size::new(1408.0, 844.0);
        let (cols, rows) = ide.terminal_grid(window);
        assert!(cols > 80 && rows > 10, "{cols}x{rows}");
        ide.terminal.screen = Some(std::sync::Arc::default());
        let frame = UiTree::new(view(&ide, window)).layout(window);
        let shell = frame.regions.iter().find(|r| r.id == TERMINAL).unwrap();
        assert!(shell.rect.y > 500.0, "under the editor");
        click(&mut ide, TERMINAL_TOGGLE);
        assert!(!ide.terminal.open);
    }

    #[test]
    fn the_tree_makes_renames_and_trashes_files_and_the_open_ones_follow() {
        let mut ide = with_files();
        let (_, jobs) = ide.tree_key("file:/r/a.rs", TreeKey::New);
        assert!(jobs.is_empty());
        assert_eq!(ide.focus.take().as_deref(), Some(NAME_FIELD));
        let input = |ide: &mut Ide, value: &str| {
            ide.update(&UiAction::InputChanged {
                id: NAME_FIELD.into(),
                value: value.into(),
            });
            ide.update(&UiAction::Submitted {
                id: NAME_FIELD.into(),
                value: value.into(),
            })
        };
        assert!(input(&mut ide, "a/b").is_empty());
        assert_eq!(
            ide.tree.naming.as_ref().unwrap().error.as_deref(),
            Some("A name has no slashes")
        );
        let jobs = input(&mut ide, "lib/");
        let made = Op::CreateDir {
            parent: "".into(),
            name: "lib".into(),
        };
        assert_eq!(jobs, vec![Job::File("/r".into(), made.clone())]);
        let jobs = ide.loaded(Loaded::File(made, Ok(vec!["lib".into()])));
        assert!(ide.tree.naming.is_none() && ide.tree.expanded.contains(Path::new("/r/lib")));
        assert_eq!(
            jobs,
            vec![Job::List("/r".into()), Job::List("/r/lib".into())]
        );
        // A rename moves the open file with it.
        read(&mut ide, "/r/a.rs", "a");
        click(&mut ide, "tree:rename:/r/a.rs");
        assert_eq!(ide.tree.naming.as_ref().unwrap().value, "a.rs");
        let rename = Op::Rename {
            path: "a.rs".into(),
            name: "b.rs".into(),
        };
        assert_eq!(
            input(&mut ide, "b.rs"),
            vec![Job::File("/r".into(), rename.clone())]
        );
        ide.loaded(Loaded::File(rename, Ok(vec!["b.rs".into()])));
        assert_eq!(ide.buffers[0].path, Path::new("/r/b.rs"));
        // Unsaved changes keep it out of the trash; saved, it goes and its tab closes.
        type_in(&ide.buffers[0], 0, "x");
        click(&mut ide, "tree:trash:/r/b.rs");
        assert!(ide.tree.trashing.is_none() && ide.tree.error.is_some());
        click(&mut ide, FILE_ERROR_DISMISS);
        let shared = ide.buffers[0].editor.as_ref().unwrap().shared_buffer();
        shared.lock().unwrap().mark_saved();
        click(&mut ide, "tree:trash:/r/b.rs");
        let trash = Op::Trash {
            path: "b.rs".into(),
        };
        assert_eq!(
            click(&mut ide, TRASH_CONFIRM),
            vec![Job::File("/r".into(), trash.clone())]
        );
        ide.loaded(Loaded::File(trash, Ok(vec![])));
        assert!(ide.buffers.is_empty());
    }

    #[test]
    fn editing_keys_move_duplicate_comment_and_delete_lines() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "one\ntwo\n");
        let text = |ide: &Ide| text_of(&ide.buffers[0]);
        ide.edit(Edit::MoveLines { down: true });
        assert_eq!(text(&ide), "two\none\n");
        ide.edit(Edit::DuplicateLines);
        assert_eq!(text(&ide), "two\none\none\n");
        ide.edit(Edit::ToggleComment);
        assert!(text(&ide).contains("// one"), "{}", text(&ide));
        ide.edit(Edit::DeleteLines);
        assert_eq!(text(&ide).lines().count(), 2);
        ide.edit(Edit::AddCursor { below: false });
        let editor = ide.buffers[0].editor.as_ref().unwrap();
        assert_eq!(editor.snapshot().unwrap().selections().count(), 2);
    }

    #[test]
    fn the_language_server_is_asked_at_the_caret_and_its_answers_open() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "let total = 1;\n");
        let shared = ide.buffers[0].editor.as_ref().unwrap().shared_buffer();
        shared
            .lock()
            .unwrap()
            .set_selection(Selection::caret(6))
            .unwrap();
        click(&mut ide, LSP_DEFINITION);
        let at = |kind| Ask {
            path: "/r/a.rs".into(),
            offset: 6,
            kind,
        };
        assert_eq!(ide.lsp_asks, vec![at(AskKind::Definition)]);
        ide.lsp_asks.clear();
        // Rename starts from the word under the caret.
        click(&mut ide, LSP_RENAME);
        assert_eq!(ide.renaming_symbol.as_deref(), Some("total"));
        ide.update(&UiAction::InputChanged {
            id: SYMBOL_FIELD.into(),
            value: "sum".into(),
        });
        ide.update(&UiAction::Submitted {
            id: SYMBOL_FIELD.into(),
            value: "sum".into(),
        });
        assert_eq!(ide.lsp_asks, vec![at(AskKind::Rename("sum".into()))]);
        assert!(ide.renaming_symbol.is_none());
        // References list; Enter opens the one chosen.
        let place = |row| Location {
            path: "/r/b.rs".into(),
            row,
            column: 2,
        };
        ide.references = Some((vec![place(3), place(9)], 0));
        ide.step_chosen(1);
        let jobs = ide.update(&UiAction::Submitted {
            id: REFS_FIELD.into(),
            value: String::new(),
        });
        assert_eq!(jobs, vec![Job::Read("/r/b.rs".into())]);
        assert_eq!(ide.pending_goto, Some(("/r/b.rs".into(), 9, 2)));
    }

    #[test]
    fn diagnostics_follow_the_edits_until_the_server_answers() {
        let mark = |range: std::ops::Range<usize>| Diagnostic {
            range,
            severity: 1,
            message: "x".into(),
        };
        let old = "let a = 1;\nlet b = 2;\n";
        let items = [mark(4..5), mark(15..16), mark(8..10)];
        // Typed after the first line's `=`: the first stays, the second moves, the third went.
        let new = "let a = 10;\nlet b = 2;\n";
        assert_eq!(carry(&items, old, new), vec![mark(4..5), mark(16..17)]);
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", old);
        let revision = ide.buffers[0]
            .editor
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .revision();
        let text: std::sync::Arc<str> = old.into();
        let found = Diagnostics {
            revision,
            text,
            items: items.to_vec(),
            carried: None,
        };
        ide.diagnostics.insert("/r/a.rs".into(), found);
        type_in(&ide.buffers[0], 0, "  ");
        ide.tick();
        let now = ide.buffers[0]
            .editor
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .revision();
        let shown = ide.diagnostics[Path::new("/r/a.rs")]
            .at(now)
            .unwrap()
            .to_vec();
        assert_eq!(shown, vec![mark(6..7), mark(17..18), mark(10..12)]);
    }

    #[test]
    fn the_footers_problems_take_the_caret_to_the_next_one_and_round() {
        let mark = |range: std::ops::Range<usize>, severity| Diagnostic {
            range,
            severity,
            message: "x".into(),
        };
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "let a = 1;\nlet b = 2;\nlet c = 3;\n");
        let caret = |ide: &Ide| {
            let snapshot = ide.buffers[0].editor.as_ref().unwrap().snapshot().unwrap();
            snapshot.selection().head
        };
        let revision = ide.buffers[0]
            .editor
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .revision();
        // A hint is not a problem; the two others are visited in order, then the first again.
        let items = vec![mark(26..27, 2), mark(15..16, 4), mark(4..5, 1)];
        ide.diagnostics.insert(
            "/r/a.rs".into(),
            Diagnostics {
                revision,
                text: "".into(),
                items,
                carried: None,
            },
        );
        for expected in [4, 26, 4] {
            click(&mut ide, PROBLEM_NEXT);
            assert_eq!(caret(&ide), expected);
        }
    }

    #[test]
    fn the_problems_pane_lists_each_files_problems_and_a_press_goes_to_one() {
        let mark = |range: std::ops::Range<usize>, severity| Diagnostic {
            message: format!("m{}", range.start),
            range,
            severity,
        };
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "let a = 1;\nlet b😀 = 2;\n");
        let revision = ide.buffers[0]
            .editor
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .revision();
        // Offsets count characters; `😀` is two UTF-16 units, as servers count columns.
        let items = vec![mark(19..20, 2), mark(4..5, 1)];
        ide.diagnostics.insert(
            "/r/a.rs".into(),
            Diagnostics {
                revision,
                text: "".into(),
                items,
                carried: None,
            },
        );
        // A file not open keeps the text its problems came for.
        ide.diagnostics.insert(
            "/r/b.rs".into(),
            Diagnostics {
                revision: 1,
                text: "x\n  y".into(),
                items: vec![mark(4..5, 1), mark(0..1, 3)],
                carried: None,
            },
        );
        let places: Vec<(PathBuf, usize, u32)> = ide
            .problems()
            .into_iter()
            .map(|p| (p.path, p.row, p.column))
            .collect();
        let at = |path: &str, row, column| (PathBuf::from(path), row, column);
        assert_eq!(
            places,
            [
                at("/r/a.rs", 0, 4),
                at("/r/a.rs", 1, 9),
                at("/r/b.rs", 1, 2)
            ]
        );
        click(&mut ide, PROBLEMS_TOGGLE);
        assert!(ide.terminal.open && ide.terminal.problems);
        click(&mut ide, "problem:1");
        let snapshot = ide.buffers[0].editor.as_ref().unwrap().snapshot().unwrap();
        assert_eq!(snapshot.selection().head, 19);
        // Ctrl+` shows the shells in the same pane, and shuts it when they are shown.
        click(&mut ide, TERMINAL_TOGGLE);
        assert!(ide.terminal.open && !ide.terminal.problems);
        click(&mut ide, TERMINAL_TOGGLE);
        assert!(!ide.terminal.open);
    }

    #[test]
    fn ask_agent_sends_the_lines_picked_or_the_file() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "one\ntwo\nthree\n");
        click(&mut ide, ASK_AGENT);
        let file = newen_link::Message::Ask {
            root: "/r".into(),
            path: "/r/a.rs".into(),
            lines: None,
            text: String::new(),
        };
        assert_eq!(ide.link_out, [file]);
        ide.link_out.clear();
        // "two\nthree\n" ends where line 4 starts, which is left out.
        let editor = ide.buffers[0].editor.clone().unwrap();
        editor
            .shared_buffer()
            .lock()
            .unwrap()
            .set_selection(Selection {
                anchor: 4,
                head: 14,
            })
            .unwrap();
        click(&mut ide, ASK_AGENT);
        let lines = newen_link::Message::Ask {
            root: "/r".into(),
            path: "/r/a.rs".into(),
            lines: Some((2, 3)),
            text: "two\nthree\n".into(),
        };
        assert_eq!(ide.link_out, [lines]);
    }

    #[test]
    fn the_bar_switches_a_markdown_files_views_and_its_links_open() {
        let mut ide = with_files();
        read(&mut ide, "/r/README.md", "# Hi\n");
        click(&mut ide, PREVIEW_TOGGLE);
        assert_eq!(ide.buffers[0].view, DocView::Preview);
        click(&mut ide, "doc:split");
        assert_eq!(ide.buffers[0].view, DocView::Split);
        click(&mut ide, "md-link:https://example.com/a");
        assert_eq!(
            ide.open_url.take().as_deref(),
            Some("https://example.com/a")
        );
        assert!(click(&mut ide, "md-link:#section").is_empty());
        click(&mut ide, "md-diagram:000000000000002a");
        assert_eq!(ide.diagram_sources, vec![42]);
        click(&mut ide, "md-diagram:000000000000002a");
        assert!(ide.diagram_sources.is_empty());
        click(&mut ide, MORE);
        assert!(ide.more_open);
        let jobs = click(&mut ide, WRAP);
        assert!(!ide.more_open && ide.prefs.wrap);
        assert!(matches!(&jobs[..], [Job::SavePrefs(p)] if p.wrap));
        let window = Size::new(1408.0, 844.0);
        let frame = UiTree::new(view(&ide, window)).layout(window);
        assert!(frame.regions.iter().any(|r| r.id == "doc:preview"));
    }

    #[test]
    fn the_tree_copies_cuts_and_pastes_and_the_open_files_follow_a_move() {
        let mut ide = with_files();
        ide.tree_key("file:/r/a.rs", TreeKey::Copy);
        assert_eq!(ide.tree.clipboard, Some((vec!["/r/a.rs".into()], false)));
        let (_, jobs) = ide.tree_key("dir:/r/src", TreeKey::Paste);
        let copy = Op::Paste {
            sources: vec!["a.rs".into()],
            cut: false,
            into: "src".into(),
        };
        assert_eq!(jobs, vec![Job::File("/r".into(), copy)]);
        // Cut: unsaved changes keep it; saved, the move takes the open file along.
        read(&mut ide, "/r/a.rs", "a");
        type_in(&ide.buffers[0], 0, "x");
        ide.tree_key("file:/r/a.rs", TreeKey::Cut);
        assert!(ide.tree.error.take().is_some());
        let shared = ide.buffers[0].editor.as_ref().unwrap().shared_buffer();
        shared.lock().unwrap().mark_saved();
        ide.tree_key("file:/r/a.rs", TreeKey::Cut);
        let (_, jobs) = ide.tree_key("dir:/r/src", TreeKey::Paste);
        let [Job::File(_, cut @ Op::Paste { cut: true, .. })] = <[Job; 1]>::try_from(jobs).unwrap()
        else {
            panic!("a cut pastes as a move");
        };
        ide.loaded(Loaded::File(cut, Ok(vec!["src/a.rs".into()])));
        assert_eq!(ide.buffers[0].path, Path::new("/r/src/a.rs"));
        assert!(ide.tree.clipboard.is_none());
    }

    #[test]
    fn a_word_typed_is_completed_from_the_servers_offers() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "v.");
        ide.caret = Some(limn::Rect::new(100.0, 40.0, 2.0, 20.0));
        let shared = ide.buffers[0].editor.as_ref().unwrap().shared_buffer();
        shared
            .lock()
            .unwrap()
            .set_selection(Selection::caret(2))
            .unwrap();
        // Right after `.`, the server is asked.
        ide.typed();
        let ask = Ask {
            path: "/r/a.rs".into(),
            offset: 2,
            kind: AskKind::Completion,
        };
        assert_eq!(ide.lsp_asks, vec![ask]);
        let offer = |label: &str, text: &str, snippet| Offer {
            label: label.into(),
            detail: String::new(),
            text: text.into(),
            snippet,
            filter: label.into(),
            sort: label.into(),
        };
        ide.completion.as_mut().unwrap().offers = vec![
            offer("len", "len", false),
            offer("push", "push(${1:value})", true),
        ];
        type_in(&ide.buffers[0], 2, "p");
        ide.typed();
        assert_eq!(
            ide.lsp_asks.len(),
            1,
            "with offers in, the same word is not asked again"
        );
        let shown: Vec<&str> = ide
            .completion_shown()
            .iter()
            .map(|(_, o)| o.label.as_str())
            .collect();
        assert_eq!(shown, ["push"]);
        ide.accept_completion(None);
        assert_eq!(text_of(&ide.buffers[0]), "v.push(value)");
        assert!(ide.completion.is_none());
        // A space ends a word, and with it a completion.
        type_in(&ide.buffers[0], 13, " ");
        ide.typed();
        assert!(ide.completion.is_none());
    }

    #[test]
    fn the_shut_tree_peeks_from_its_toggle_and_goes_once_the_pointer_left() {
        let mut ide = with_files();
        let on_toggle = |ide: &mut Ide| {
            ide.hovered = Some(TREE_TOGGLE.into());
            ide.pointer = Some(Point::new(20.0, 18.0));
        };
        let away = |ide: &mut Ide| {
            ide.hovered = None;
            ide.pointer = Some(Point::new(700.0, 300.0));
        };
        on_toggle(&mut ide);
        click(&mut ide, TREE_TOGGLE);
        ide.tick();
        // Hidden by a click, it waits for the pointer to leave its toggle first.
        assert!(!ide.sidebar.peek);
        away(&mut ide);
        ide.tick();
        assert!(!ide.sidebar.peek);
        on_toggle(&mut ide);
        ide.tick();
        assert!(ide.sidebar.peek);
        let window = Size::new(1408.0, 844.0);
        let frame = UiTree::new(view(&ide, window)).layout(window);
        assert!(frame.regions.iter().any(|r| r.id == "file:/r/a.rs"));
    }

    #[test]
    fn shortcuts_leave_vim_its_keys_and_cycle_the_files() {
        let mut ide = with_files();
        assert_eq!(ide.shortcut(Key::B, false), None);
        assert_eq!(ide.shortcut(Key::W, false), None);
        for p in ["/r/1", "/r/2"] {
            read(&mut ide, p, "");
        }
        assert_eq!(
            ide.shortcut(Key::PageDown, false).as_deref(),
            Some("buffer:0")
        );
        assert_eq!(
            ide.shortcut(Key::PageUp, false).as_deref(),
            Some("buffer:0")
        );
        assert_eq!(ide.shortcut(Key::W, false).as_deref(), Some("close:1"));
        assert_eq!(ide.shortcut(Key::E, true).as_deref(), Some(TREE_FOCUS));
    }

    #[test]
    fn the_window_lays_out_with_and_without_the_tree() {
        let mut ide = with_files();
        read(&mut ide, "/r/a.rs", "fn a() {}\n");
        let window = Size::new(1408.0, 844.0);
        let frame = UiTree::new(view(&ide, window)).layout(window);
        let row = frame
            .regions
            .iter()
            .find(|r| r.id == "file:/r/a.rs")
            .unwrap();
        assert!(row.rect.width > 200.0);
        click(&mut ide, TREE_TOGGLE);
        ide.tick();
        let frame = UiTree::new(view(&ide, window)).layout(window);
        assert!(!frame.regions.iter().any(|r| r.id == "file:/r/a.rs"));
        assert!(frame.regions.iter().any(|r| r.id == "close:0"));
        // A stale file offers to take the disk's text.
        ide.buffers[0].stale = true;
        let frame = UiTree::new(view(&ide, window)).layout(window);
        assert!(frame.regions.iter().any(|r| r.id == "reload:0"));
    }
}

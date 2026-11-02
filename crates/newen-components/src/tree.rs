//! A folder's file tree, as quill-ide and quill's Files pane show it: folders that open in
//! place, each row's menu (something new, rename, copy, cut, paste, its path, the trash), the
//! keys nvim's trees take, and the dialogs those ask for. It holds no files and touches no
//! disk: it says what to list and which operation to run (`Job`, run by the host beside the
//! window with `newen_files`), and what the host must do about its own (`Outcome`).

use crate::icons;
use crate::look::{self, SMALL};
use crate::packs::Pack;
use crate::pieces::*;
use crate::search::{self, Found};
use limn::{Component, ComponentPalette, Element, TreeRow};
use newen_files::{Entry, Op, Stamp};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const NAME_FIELD: &str = "name:field";
pub const NAME_CLOSE: &str = "name:close";
const NAME_GO: &str = "name:go";
pub const TRASH_CONFIRM: &str = "trash:confirm";
const TRASH_CANCEL: &str = "trash:cancel";
pub const ERROR_DISMISS: &str = "file-error:dismiss";
/// A row of the tree: denser than the sidebar's 32 px, as a reader's tree is.
pub const ROW: f32 = 26.0;
const DESKTOP_ROW: f32 = 24.0;

/// How the tree is drawn: quill-ide's, with its icon pack, or Desktop's in its file tile,
/// quieter: 24 px rows, a folder's chevron alone and a file's plain glyph. Both draw depth guides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Look {
    Ide,
    Desktop,
}

/// A name being typed for the tree: of something new in a folder (ending in `/`, a folder),
/// or of a file or folder renamed. Its words, and why the last try failed.
#[derive(Clone, Debug, PartialEq)]
pub struct Naming {
    pub target: NameFor,
    pub value: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NameFor {
    NewIn(PathBuf),
    Rename(PathBuf),
}

/// The keys the tree takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TreeKey {
    Up,
    Down,
    Left,
    Right,
    Escape,
    /// nvim-tree's `a`, `r` and `d`: something new, a rename, the trash.
    New,
    Rename,
    Trash,
    /// nvim-tree's `y`, `x` and `p`: copy, cut and paste.
    Copy,
    Cut,
    Paste,
}

/// What the host runs on the disk for the tree, beside the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// List this folder (`newen_files::list`), and hand it back with `FileTree::listed`.
    List(PathBuf),
    /// Run this operation in the folder at the root (`newen_files::run`), and hand back what
    /// it made with `FileTree::done`.
    File(PathBuf, Op),
}

/// What the host does after the tree took a click, a key or a result.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub jobs: Vec<Job>,
    /// A file to open: clicked, opened with the keys, or just made.
    pub open: Option<PathBuf>,
    /// A file or folder moved (renamed, or cut and pasted), from and to: what the host holds
    /// there goes with it.
    pub moved: Vec<(PathBuf, PathBuf)>,
    /// A file or folder gone to the trash: the host lets go of what it holds there.
    pub trashed: Option<PathBuf>,
    /// Words for the clipboard.
    pub copy: Option<String>,
    /// A control to focus, and a row to bring into view.
    pub focus: Option<String>,
    pub reveal: Option<String>,
    /// Escape on a row: the keys go back to the host.
    pub leave: bool,
}

impl Outcome {
    fn jobs(jobs: Vec<Job>) -> Self {
        Self {
            jobs,
            ..Self::default()
        }
    }
}

/// The tree of the folder at `root`: what is open, listed and being done to it.
#[derive(Debug, Default)]
pub struct FileTree {
    pub root: PathBuf,
    pub expanded: HashSet<PathBuf>,
    /// The folders listed so far, each with its entries in the order shown.
    pub listed: HashMap<PathBuf, Vec<Entry>>,
    /// When each listed folder last changed, to list it again when it does.
    pub stamps: HashMap<PathBuf, Stamp>,
    /// A name being typed, what waits to go to the trash, and why the last operation failed.
    pub naming: Option<Naming>,
    pub trashing: Option<PathBuf>,
    pub error: Option<String>,
    /// What was copied (or cut, when true) to paste in another folder.
    pub clipboard: Option<(Vec<PathBuf>, bool)>,
}

/// The id of the row of `entry`.
pub fn row_id(path: &Path, dir: bool) -> String {
    let kind = if dir { "dir" } else { "file" };
    format!("{kind}:{}", path.display())
}

/// Whether `id` names a row of the tree, which takes its keys.
pub fn is_row(id: &str) -> bool {
    id.starts_with("dir:") || id.starts_with("file:")
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl FileTree {
    /// The tree of `root`, its first level asked for.
    pub fn new(root: PathBuf) -> (Self, Vec<Job>) {
        let job = Job::List(root.clone());
        let tree = Self {
            root,
            ..Self::default()
        };
        (tree, vec![job])
    }

    /// A folder's entries, as listed, and when it last changed.
    pub fn listed(&mut self, dir: PathBuf, entries: Vec<Entry>, stamp: Stamp) {
        self.stamps.insert(dir.clone(), stamp);
        self.listed.insert(dir, entries);
    }

    /// The listed folders, to list again: when the host sees one changed, or when it asks.
    pub fn relist(&self) -> Vec<Job> {
        self.listed.keys().cloned().map(Job::List).collect()
    }

    pub fn relative(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root).unwrap_or(path).to_path_buf()
    }

    /// The rows on show, depth first, with their depths.
    pub fn rows(&self) -> Vec<(&Entry, usize)> {
        let mut rows = vec![];
        self.rows_in(&self.root, 0, &mut rows);
        rows
    }

    fn rows_in<'a>(&'a self, dir: &Path, depth: usize, rows: &mut Vec<(&'a Entry, usize)>) {
        for entry in self.listed.get(dir).into_iter().flatten() {
            rows.push((entry, depth));
            if entry.dir && self.expanded.contains(&entry.path) {
                self.rows_in(&entry.path, depth + 1, rows);
            }
        }
    }

    /// The rows as they show, by id, and whether each is an open folder.
    pub fn row_ids(&self) -> Vec<(String, bool)> {
        self.rows()
            .into_iter()
            .map(|(entry, _)| {
                let open = entry.dir && self.expanded.contains(&entry.path);
                (row_id(&entry.path, entry.dir), open)
            })
            .collect()
    }

    /// The folders above `path` opened (and listed where they are not), and its row brought
    /// into view.
    pub fn reveal(&mut self, path: &Path) -> Outcome {
        let mut jobs = vec![];
        for dir in path.ancestors().skip(1) {
            if !dir.starts_with(&self.root) || dir == self.root {
                break;
            }
            self.expanded.insert(dir.to_path_buf());
            if !self.listed.contains_key(dir) {
                jobs.push(Job::List(dir.to_path_buf()));
            }
        }
        Outcome {
            jobs,
            reveal: Some(row_id(path, false)),
            ..Outcome::default()
        }
    }

    /// The folder `dir` opened, its ancestors too, and its row brought into view.
    pub fn reveal_dir(&mut self, dir: PathBuf) -> Outcome {
        let mut outcome = self.reveal(&dir.join("_"));
        outcome.jobs.retain(|job| *job != Job::List(dir.join("_")));
        self.expanded.insert(dir.clone());
        if !self.listed.contains_key(&dir) {
            outcome.jobs.push(Job::List(dir.clone()));
        }
        outcome.reveal = Some(row_id(&dir, true));
        outcome
    }

    /// What was typed in the naming dialog; `false` when `id` is not its field.
    pub fn typed(&mut self, id: &str, value: &str) -> bool {
        if id != NAME_FIELD {
            return false;
        }
        if let Some(naming) = self.naming.as_mut() {
            naming.value = value.to_owned();
            naming.error = None;
        }
        true
    }

    /// A click of the tree's: its rows, their menus, the naming dialog and the trash's
    /// question. `busy` says whether what is at a path has changes not saved, which are not
    /// moved nor trashed under the host. `None` when `id` is none of them.
    pub fn clicked(&mut self, id: &str, busy: impl Fn(&Path) -> bool) -> Option<Outcome> {
        let path = |prefix: &str| id.strip_prefix(prefix).map(PathBuf::from);
        let unsaved = |at: &Path| {
            format!(
                "{} has changes not saved: save or close them first.",
                name(at)
            )
        };
        if let Some(dir) = path("dir:") {
            if !self.expanded.remove(&dir) {
                self.expanded.insert(dir.clone());
                if !self.listed.contains_key(&dir) {
                    return Some(Outcome::jobs(vec![Job::List(dir)]));
                }
            }
        } else if let Some(file) = path("file:") {
            return Some(Outcome {
                open: Some(file),
                ..Outcome::default()
            });
        } else if let Some(dir) = path("tree:new:") {
            self.naming = Some(Naming {
                target: NameFor::NewIn(dir),
                value: String::new(),
                error: None,
            });
            return Some(Outcome {
                focus: Some(NAME_FIELD.into()),
                ..Outcome::default()
            });
        } else if let Some(at) = path("tree:rename:") {
            self.naming = Some(Naming {
                value: name(&at),
                target: NameFor::Rename(at),
                error: None,
            });
            return Some(Outcome {
                focus: Some(NAME_FIELD.into()),
                ..Outcome::default()
            });
        } else if let Some(at) = path("tree:trash:") {
            if busy(&at) {
                self.error = Some(unsaved(&at));
            } else {
                self.trashing = Some(at);
            }
        } else if let Some(at) = path("tree:yank:") {
            self.clipboard = Some((vec![at], false));
        } else if let Some(at) = path("tree:cut:") {
            if busy(&at) {
                self.error = Some(unsaved(&at));
            } else {
                self.clipboard = Some((vec![at], true));
            }
        } else if let Some(into) = path("tree:paste:") {
            let (sources, cut) = self.clipboard.clone()?;
            let sources = sources.iter().map(|s| self.relative(s)).collect();
            let into = self.relative(&into);
            let op = Op::Paste { sources, cut, into };
            return Some(Outcome::jobs(vec![Job::File(self.root.clone(), op)]));
        } else if let Some(at) = path("tree:copy:") {
            return Some(Outcome {
                copy: Some(self.relative(&at).display().to_string()),
                ..Outcome::default()
            });
        } else if id == NAME_CLOSE {
            self.naming = None;
        } else if id == NAME_GO {
            return Some(Outcome::jobs(self.name_done()));
        } else if id == TRASH_CANCEL {
            self.trashing = None;
        } else if id == TRASH_CONFIRM {
            let at = self.trashing.take()?;
            let op = Op::Trash {
                path: self.relative(&at),
            };
            return Some(Outcome::jobs(vec![Job::File(self.root.clone(), op)]));
        } else if id == ERROR_DISMISS {
            self.error = None;
        } else {
            return None;
        }
        Some(Outcome::default())
    }

    /// The name typed, made into its operation: a trailing `/` asks for a folder.
    fn name_done(&mut self) -> Vec<Job> {
        let Some(naming) = self.naming.as_mut() else {
            return vec![];
        };
        let value = naming.value.trim();
        let op = match &naming.target {
            NameFor::NewIn(dir) => {
                let parent = dir.strip_prefix(&self.root).unwrap_or(dir).to_path_buf();
                match value.strip_suffix('/') {
                    Some(folder) => Op::CreateDir {
                        parent,
                        name: folder.into(),
                    },
                    None => Op::CreateFile {
                        parent,
                        name: value.into(),
                    },
                }
            }
            NameFor::Rename(at) => {
                if name(at) == value {
                    self.naming = None;
                    return vec![];
                }
                let path = at.strip_prefix(&self.root).unwrap_or(at).to_path_buf();
                Op::Rename {
                    path,
                    name: value.into(),
                }
            }
        };
        if let Err(error) = newen_files::valid_name(match &op {
            Op::CreateFile { name, .. } | Op::CreateDir { name, .. } | Op::Rename { name, .. } => {
                name
            }
            Op::Trash { .. } | Op::Paste { .. } => "",
        }) {
            naming.error = Some(error);
            return vec![];
        }
        vec![Job::File(self.root.clone(), op)]
    }

    /// An operation done, with what it made: the tree listed again, and the host told what
    /// moved, what went to the trash and the file to open.
    pub fn done(&mut self, op: Op, result: Result<Vec<PathBuf>, String>) -> Outcome {
        let all: Vec<PathBuf> = match result {
            Ok(made) => made.into_iter().map(|m| self.root.join(m)).collect(),
            Err(error) => {
                match self.naming.as_mut() {
                    Some(naming) => naming.error = Some(error),
                    None => self.error = Some(error),
                }
                return Outcome::default();
            }
        };
        self.naming = None;
        let made = all.first().cloned();
        let mut outcome = Outcome::default();
        let list = |path: &Path| Job::List(path.parent().unwrap_or(path).to_path_buf());
        match op {
            Op::Paste { sources, cut, into } => {
                outcome.jobs.push(Job::List(self.root.join(&into)));
                if cut {
                    self.clipboard = None;
                    for (source, moved_to) in sources.iter().zip(&all) {
                        let old = self.root.join(source);
                        outcome.jobs.push(list(&old));
                        outcome.moved.push((old, moved_to.clone()));
                    }
                }
            }
            Op::CreateFile { .. } => {
                if let Some(file) = made {
                    outcome.jobs.push(list(&file));
                    outcome.open = Some(file);
                }
            }
            Op::CreateDir { .. } => {
                if let Some(dir) = made {
                    outcome.jobs.push(list(&dir));
                    self.expanded.insert(dir.clone());
                    outcome.jobs.push(Job::List(dir));
                }
            }
            Op::Rename { path, .. } => {
                let old = self.root.join(path);
                let new = made.unwrap_or_else(|| old.clone());
                self.expanded = std::mem::take(&mut self.expanded)
                    .into_iter()
                    .map(|dir| match newen_files::moved(&dir, &old, Some(&new)) {
                        Some(Some(moved)) => moved,
                        _ => dir,
                    })
                    .collect();
                self.listed.retain(|dir, _| !dir.starts_with(&old));
                self.stamps.retain(|dir, _| !dir.starts_with(&old));
                outcome.jobs.push(list(&new));
                outcome.jobs.extend(
                    self.expanded
                        .iter()
                        .filter(|d| d.starts_with(&new))
                        .cloned()
                        .map(Job::List),
                );
                outcome.moved.push((old, new));
            }
            Op::Trash { path } => {
                let gone = self.root.join(path);
                self.expanded.retain(|dir| !dir.starts_with(&gone));
                self.listed.retain(|dir, _| !dir.starts_with(&gone));
                self.stamps.retain(|dir, _| !dir.starts_with(&gone));
                outcome.jobs.push(list(&gone));
                outcome.trashed = Some(gone);
            }
        }
        outcome
    }

    /// A key on the row `focused`, as nvim's file trees take them: up and down move, right
    /// opens a folder or the file, left shuts a folder or goes to the folder above, Escape
    /// gives the keys back. `None` when `focused` is not a row on show.
    pub fn key(
        &mut self,
        focused: &str,
        key: TreeKey,
        busy: impl Fn(&Path) -> bool,
    ) -> Option<Outcome> {
        let rows = self.row_ids();
        let at = rows.iter().position(|(id, _)| id == focused)?;
        let open = rows[at].1;
        let path = PathBuf::from(focused.split_once(':').map_or("", |(_, p)| p));
        let folder = || {
            if focused.starts_with("dir:") {
                path.clone()
            } else {
                path.parent().map_or(self.root.clone(), Path::to_path_buf)
            }
        };
        let click = |verb: &str, at: &Path| format!("tree:{verb}:{}", at.display());
        let next = match key {
            TreeKey::Down => rows.get(at + 1).map(|(id, _)| id.clone()),
            TreeKey::Up => at.checked_sub(1).map(|up| rows[up].0.clone()),
            TreeKey::Right if open => rows.get(at + 1).map(|(id, _)| id.clone()),
            TreeKey::Right => return self.clicked(focused, busy),
            TreeKey::Left if open => {
                self.expanded.remove(&path);
                None
            }
            TreeKey::Left => path
                .parent()
                .filter(|dir| *dir != self.root)
                .map(|dir| row_id(dir, true)),
            TreeKey::Escape => {
                return Some(Outcome {
                    leave: true,
                    ..Outcome::default()
                })
            }
            TreeKey::New => return self.clicked(&click("new", &folder()), busy),
            TreeKey::Rename => return self.clicked(&click("rename", &path), busy),
            TreeKey::Copy => return self.clicked(&click("yank", &path), busy),
            TreeKey::Cut => return self.clicked(&click("cut", &path), busy),
            TreeKey::Paste => return self.clicked(&click("paste", &folder()), busy),
            TreeKey::Trash => return self.clicked(&click("trash", &path), busy),
        };
        Some(Outcome {
            reveal: next.clone(),
            focus: next,
            ..Outcome::default()
        })
    }
}

/// The tree for `width` by `height`: a header (the folder's name, and `note` beside it), and
/// the rows, `selected`'s marked, each with its menu. Scrolls by itself.
pub fn view(
    tree: &FileTree,
    pack: &Pack,
    style: Look,
    selected: Option<&Path>,
    note: Option<Element>,
    [width, height]: [f32; 2],
) -> Element {
    let mut header = vec![
        hgap(8.0),
        muted(name(&tree.root)).font_size(SMALL).centered_y(),
    ];
    if let Some(note) = note {
        header.push(Element::spacer().flex(1.0));
        header.push(note.centered_y());
        header.push(hgap(8.0));
    }
    let visible = tree.rows();
    let depths: Vec<usize> = visible.iter().map(|(_, depth)| *depth).collect();
    let palette = ComponentPalette {
        hover: look::row_hover(),
        selected: look::selected(),
        ..look::palette()
    };
    let row_width = width - 12.0;
    // The muted ink at about a third: the divider's gray is lost on the sidebar's fill.
    let muted = look::muted();
    let guide = limn::Color::rgba(muted.r, muted.g, muted.b, 80);
    let header = Element::row(header).size(None, Some(ROW));
    let mut rows = vec![limn::ContextMenu::new(
        "tree:menu:root",
        header,
        vec![menu_item(
            &format!("tree:new:{}", tree.root.display()),
            "New file or folder\u{2026}",
            "a",
        )],
        look::menu_palette(),
    )
    .render()];
    for ((entry, _), guides) in visible.iter().zip(limn::guides_for_depths(&depths)) {
        let open = entry.dir && tree.expanded.contains(&entry.path);
        // Desktop's: a folder's chevron alone, `<>` for code and settings and a page for the
        // rest, over the depth guides.
        let (icon, title, height) = if style == Look::Desktop {
            let glyph = match (entry.dir, open) {
                (true, true) => icons::CARET_DOWN,
                (true, false) => icons::CARET_RIGHT,
                (false, _) => glyph(&entry.path),
            };
            let icon = Element::icon(icons::of(glyph))
                .foreground(look::muted())
                .size(Some(14.0), Some(14.0))
                .centered();
            let title = Element::truncated_text(name(&entry.path))
                .font_family(limn::FontFamily::Sans)
                .font_size(look::BODY)
                .foreground(look::palette().text)
                .centered_y();
            (icon, title, DESKTOP_ROW)
        } else {
            let (icon, ink) = pack.icon(&entry.path, entry.dir, open);
            let icon = Element::icon(icon)
                .foreground(ink)
                .size(Some(16.0), Some(16.0))
                .centered();
            (icon, text(name(&entry.path)).centered_y(), ROW)
        };
        let id = row_id(&entry.path, entry.dir);
        let row = TreeRow::new(|c| Element::clickable(id, c), icon, title, guides, palette)
            .height(height)
            .width(row_width)
            .guide_color(guide)
            .selected(selected == Some(entry.path.as_path()));
        let row = row.render().radius(6.0);
        rows.push(row_menu(tree, &entry.path, entry.dir, row));
    }
    Element::scroll("tree:scroll", Element::column(rows).padding(6.0))
        .size(Some(width), Some(height))
        .clip_children()
}

/// A file's glyph in Desktop's look: `<>` for code and settings, a page for the rest.
pub fn glyph(path: &Path) -> &'static str {
    if code_like(path) {
        icons::CODE
    } else {
        icons::FILE
    }
}

/// Whether a file reads as code or settings, which Desktop marks `<>` (inferred from the
/// person's capture: package.json and docker-compose.yml are, Dockerfile and .env are not).
fn code_like(path: &Path) -> bool {
    const CODE: [&str; 30] = [
        "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "json", "yml", "yaml", "toml", "py", "go",
        "c", "h", "cpp", "java", "kt", "swift", "rb", "php", "sh", "css", "scss", "html", "vue",
        "svelte", "sql", "lua", "nix",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| CODE.contains(&e.to_ascii_lowercase().as_str()))
}

/// `base` under the tree's dialogs, when one is open: the name being typed, or the trash's
/// question.
pub fn dialogs(tree: &FileTree, base: Element) -> Element {
    let base = match &tree.naming {
        Some(naming) => name_dialog(tree, naming, base),
        None => base,
    };
    match &tree.trashing {
        Some(at) => confirm(
            base,
            true,
            "Move to the trash?",
            &format!(
                "\u{201c}{}\u{201d} goes to the desktop's trash, where it can be put back.",
                tree.relative(at).display()
            ),
            "Move to trash",
            TRASH_CONFIRM,
            TRASH_CANCEL,
        ),
        None => base,
    }
}

fn menu_item(id: &str, label: &str, keys: &str) -> limn::ContextMenuEntry {
    let item = limn::ContextMenuItem::new(id, label);
    limn::ContextMenuEntry::Item(if keys.is_empty() {
        item
    } else {
        item.shortcut(keys)
    })
}

/// A row with its menu: something new in its folder, a rename, its path, the trash; the keys
/// nvim-tree gives them.
fn row_menu(tree: &FileTree, path: &Path, dir: bool, row: Element) -> Element {
    let folder = if dir {
        path.to_path_buf()
    } else {
        path.parent().map_or(tree.root.clone(), Path::to_path_buf)
    };
    let entries = vec![
        menu_item(
            &format!("tree:new:{}", folder.display()),
            "New file or folder\u{2026}",
            "a",
        ),
        menu_item(
            &format!("tree:rename:{}", path.display()),
            "Rename\u{2026}",
            "r",
        ),
        limn::ContextMenuEntry::Separator,
        menu_item(&format!("tree:yank:{}", path.display()), "Copy", "y"),
        menu_item(&format!("tree:cut:{}", path.display()), "Cut", "x"),
        limn::ContextMenuEntry::Item(
            limn::ContextMenuItem::new(format!("tree:paste:{}", folder.display()), "Paste")
                .shortcut("p")
                .enabled(tree.clipboard.is_some()),
        ),
        limn::ContextMenuEntry::Separator,
        menu_item(
            &format!("tree:copy:{}", path.display()),
            "Copy relative path",
            "",
        ),
        limn::ContextMenuEntry::Separator,
        menu_item(
            &format!("tree:trash:{}", path.display()),
            "Move to trash",
            "d",
        ),
    ];
    limn::ContextMenu::new(
        format!("tree:menu:{}", path.display()),
        row,
        entries,
        look::menu_palette(),
    )
    .render()
}

fn name_dialog(tree: &FileTree, naming: &Naming, base: Element) -> Element {
    let value = naming.value.trim();
    let (title, placeholder, action) = match &naming.target {
        NameFor::NewIn(dir) => {
            let at = tree.relative(dir);
            let at = if at.as_os_str().is_empty() {
                name(&tree.root)
            } else {
                at.display().to_string()
            };
            let what = if value.ends_with('/') {
                "folder"
            } else {
                "file"
            };
            (
                format!("New in {at}"),
                "A name; ending in / makes a folder",
                format!("Make the {what} {}", value.trim_end_matches('/')),
            )
        }
        NameFor::Rename(at) => (
            format!("Rename {}", tree.relative(at).display()),
            "Its new name",
            format!("Rename it {value}"),
        ),
    };
    let row = match &naming.error {
        Some(error) => Found::new(NAME_GO, error.clone()),
        None if value.is_empty() => Found::new(NAME_GO, "Type a name"),
        None => Found::new(NAME_GO, action),
    };
    search::dialog(
        base,
        NAME_FIELD,
        NAME_CLOSE,
        &naming.value,
        placeholder,
        vec![(title, vec![row])],
        0,
    )
}

//! newen-ide's window: limn paints `ide::view`; listing folders and reading files run beside it.
//! `newen-ide [folder | file]` opens the folder (a file's own, and the file), the current
//! directory without one.

// A window, not a console program: Windows opens no console beside it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod complete;
mod disk;
mod grep;
mod ide;
mod lsp;
mod marks;
mod prefs;
mod quick;
mod session;
mod structure;

use eframe::egui;
use limn::{Element, Size, UiTree};
use newen_components::look;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct App {
    tree: UiTree,
    ide: ide::Ide,
    /// What the disk found since the last frame.
    inbox: Arc<Mutex<Vec<ide::Loaded>>>,
    ctx: egui::Context,
    fonts_attached: bool,
    /// `NEWEN_THEME` pins the theme over the system's, for demos and captures.
    pinned_theme: bool,
    /// When the disk was last looked at for files and folders changed by others.
    polled: std::time::Instant,
    /// A folder of icon themes chosen in the platform's dialog, picked up on the next frame.
    picked: Arc<Mutex<Option<PathBuf>>>,
    /// What was open when it was last kept.
    kept: session::Session,
    /// How many frames a control asked to take the keys has not been there yet.
    focus_waits: u32,
    /// The folder's shells, kept while the window lives.
    shells: newen_pty::Tabs,
    /// The language servers.
    lsp: lsp::Lsp,
    /// What Newen sent since the last frame, and the folder's socket while this window
    /// listens on it.
    linked: Arc<Mutex<Vec<newen_link::Message>>>,
    _link: Option<newen_link::Listener>,
}

impl App {
    /// Code to ask about goes to Newen; with none running, it starts and is sent once Newen
    /// listens.
    fn send_to_newen(&mut self) {
        for message in std::mem::take(&mut self.ide.link_out) {
            let socket = newen_link::newen_socket();
            if newen_link::send(&socket, &message).is_ok() {
                self.ide.link_note = Some("Sent to Newen".into());
                continue;
            }
            // Beside the IDE when they were built together, else the one on PATH.
            let program = std::env::current_exe()
                .ok()
                .and_then(|exe| Some(exe.parent()?.join("newen")))
                .filter(|p| p.is_file())
                .unwrap_or_else(|| "newen".into());
            if let Err(error) = newen_files::command(&program).spawn() {
                self.ide.link_note = Some(format!("Newen did not start: {error}"));
                continue;
            }
            self.ide.link_note = Some("Starting Newen…".into());
            std::thread::spawn(move || {
                for _ in 0..100 {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    if newen_link::send(&socket, &message).is_ok() {
                        return;
                    }
                }
            });
        }
    }

    /// Start, size and read the shells while their pane shows, and do what was asked of
    /// them, as Newen does for a session's folder.
    fn sync_terminal(&mut self, ctx: &egui::Context, window: Size) {
        let (cols, rows) = self.ide.terminal_grid(window);
        let pane = &mut self.ide.terminal;
        pane.screen = None;
        if !pane.open {
            pane.requests.clear();
            return;
        }
        let wake = || {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        let had = !self.shells.is_empty();
        let root = self.ide.root.clone();
        for request in std::mem::take(&mut pane.requests) {
            match request {
                ide::TerminalRequest::New => {
                    if let Err(error) = self.shells.open(&root, cols, rows, wake()) {
                        eprintln!("newen-ide: no terminal in {}: {error}", root.display());
                    }
                }
                ide::TerminalRequest::Select(tab) => self.shells.select(tab),
                ide::TerminalRequest::Close(tab) => self.shells.close(tab),
            }
        }
        self.shells.reap();
        // A shell is started when the shells are shown, not the problems.
        if self.shells.is_empty() && !had && !pane.problems {
            if let Err(error) = self.shells.open(&root, cols, rows, wake()) {
                eprintln!("newen-ide: no terminal in {}: {error}", root.display());
            }
        }
        // The last one closed, or `exit` in it, hides the pane; opening it again starts one.
        // The problems need none.
        let Some(shell) = self.shells.active_mut() else {
            pane.tabs.clear();
            pane.open = pane.problems;
            return;
        };
        shell.resize(cols, rows);
        pane.screen = Some(shell.screen());
        pane.tabs = self
            .shells
            .list()
            .into_iter()
            .map(|tab| tab.title)
            .collect();
        pane.active = self.shells.active();
    }

    /// What is typed while the terminal has the keys goes to its shell, Ctrl keys too.
    fn typed_into_terminal(&mut self, focused: Option<&str>, event: &limn::UiEvent) -> bool {
        if focused != Some(ide::TERMINAL) {
            return false;
        }
        let bytes = match event {
            limn::UiEvent::Text(text) => text.as_bytes().to_vec(),
            limn::UiEvent::Paste(text) => text.as_bytes().to_vec(),
            limn::UiEvent::Key {
                key, shift, ctrl, ..
            } => {
                match newen_pty::key_bytes(*key, *ctrl, *shift) {
                    Some(bytes) => bytes,
                    // Plain letters come again as Text.
                    None => return true,
                }
            }
            _ => return false,
        };
        if let Some(shell) = self.shells.active_mut() {
            shell.write(&bytes);
        }
        true
    }

    fn run(&self, jobs: Vec<ide::Job>) {
        for job in jobs {
            let (inbox, ctx) = (self.inbox.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let loaded = disk::run(job);
                inbox.lock().unwrap().push(loaded);
                ctx.request_repaint();
            });
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.fonts_attached {
            limn_egui::attach_fonts(&mut self.tree, &ctx);
            self.fonts_attached = true;
        }
        for loaded in std::mem::take(&mut *self.inbox.lock().unwrap()) {
            let jobs = self.ide.loaded(loaded);
            self.run(jobs);
        }
        let jobs = self.ide.marks_due();
        self.run(jobs);
        // A file Newen shows here: opened at its place, the window brought up.
        for message in std::mem::take(&mut *self.linked.lock().unwrap()) {
            if let newen_link::Message::Open {
                path, line, column, ..
            } = message
            {
                let jobs = match line {
                    _ if path.is_dir() => vec![],
                    Some(line) => self.ide.goto_at(
                        path,
                        line.saturating_sub(1) as usize,
                        column.unwrap_or(1).saturating_sub(1),
                    ),
                    None => self.ide.open(path),
                };
                self.run(jobs);
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        let wake: Arc<dyn Fn() + Send + Sync> = {
            let ctx = ctx.clone();
            Arc::new(move || ctx.request_repaint())
        };
        let jobs = self.lsp.sync(&mut self.ide, &wake);
        self.run(jobs);
        // What an agent or another editor writes shows within a second.
        if self.polled.elapsed() >= POLL {
            self.polled = std::time::Instant::now();
            let jobs = self.ide.poll();
            self.run(jobs);
        }
        // The window closes with unsaved changes only when asked twice.
        if ctx.input(|i| i.viewport().close_requested())
            && self.ide.unsaved() > 0
            && !self.ide.quitting
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.ide.quitting = true;
        }
        if let Some(folder) = self.picked.lock().unwrap().take() {
            let jobs = self.ide.add_icon_folder(folder);
            self.run(jobs);
        }
        let dark = match self.ide.prefs.theme.as_str() {
            "light" => false,
            "dark" => true,
            _ => ctx.system_theme() != Some(egui::Theme::Light),
        };
        if !self.pinned_theme && dark != look::is_dark() {
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            look::set_dark(dark);
        }
        // A family chosen in Settings: laid out and painted in from the next frame on, when
        // the tree measures with it again.
        if let Some(fonts) = self.ide.fonts_due.take() {
            limn::set_fonts(fonts);
            limn_egui::install_fonts(&ctx);
            self.fonts_attached = false;
            ctx.request_repaint();
        }
        // Context menus read as Newen's other menus, in the theme in force.
        self.tree.set_context_menu_palette(look::menu_palette());
        // The editor's menu has more rows than Newen's: all of them show, no rail taking width.
        self.tree.set_context_menu_sizing(limn::SelectorSizing {
            max_visible_items: 16,
            ..newen_components::pieces::menu_sizing(240.0)
        });
        limn_egui::normalize_keyboard_input(&ctx);
        let area = ui.max_rect();
        let window = Size::new(area.width(), area.height());
        // The tree peeks while the pointer is on its toggle or the window's edge.
        self.ide.pointer = ctx
            .input(|i| i.pointer.hover_pos())
            .map(|p| limn::Point::new(p.x - area.min.x, p.y - area.min.y));
        self.ide.hovered = self.tree.hovered_id().map(str::to_owned);
        self.ide.vim_mode = self
            .ide
            .editor_id()
            .and_then(|id| self.tree.code_editor_vim_mode(&id));
        // A save due after a delay, or on leaving the window, and the next time to look.
        self.ide.window_focused = ctx.input(|i| i.focused);
        let saves = self.ide.tick();
        self.run(saves);
        if let Some(due) = self.ide.next_due() {
            ctx.request_repaint_after(due);
        }
        self.ide.vim_mode = self
            .ide
            .editor_id()
            .and_then(|id| self.tree.code_editor_vim_mode(&id));
        // New output keeps the terminal at its end only if the person left it there.
        let shown = self.ide.terminal.screen.clone();
        let follow = shown.is_none() || self.tree.is_scroll_at_end(ide::TERMINAL_SCROLL, 4.0);
        self.sync_terminal(&ctx, window);
        self.tree.set_root(ide::view(&self.ide, window));
        self.tree.layout(window);
        // Ctrl+` shows and hides the terminal, as in Newen; limn has no key for it.
        let mut toggled = false;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Backtick)) {
            toggled = true;
        }
        let mut actions = vec![];
        // Ctrl+, opens Settings, as Newen's; limn has no key for it.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Comma)) {
            actions.push(limn::UiAction::Clicked {
                id: newen_components::settings::SETTINGS_OPEN.into(),
            });
        }
        if toggled {
            actions.push(limn::UiAction::Clicked {
                id: ide::TERMINAL_TOGGLE.into(),
            });
        }
        // F8 goes to the next problem, as VS Code's; limn has no key for it.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F8)) {
            actions.push(limn::UiAction::Clicked {
                id: ide::PROBLEM_NEXT.into(),
            });
        }
        // Editing keys with Alt, and Ctrl+/, which limn's keys do not carry: taken from egui
        // while the editor has the keys, as VS Code binds them.
        if self.tree.focused_id().is_some()
            && self.tree.focused_id() == self.ide.editor_id().as_deref()
        {
            use egui::{Key, Modifiers};
            let (alt, ctrl, shift) = (Modifiers::ALT, Modifiers::CTRL, Modifiers::SHIFT);
            let bindings = [
                (
                    ctrl | alt,
                    Key::ArrowUp,
                    ide::Edit::AddCursor { below: false },
                ),
                (
                    ctrl | alt,
                    Key::ArrowDown,
                    ide::Edit::AddCursor { below: true },
                ),
                (shift | alt, Key::ArrowUp, ide::Edit::DuplicateLines),
                (shift | alt, Key::ArrowDown, ide::Edit::DuplicateLines),
                (alt, Key::ArrowUp, ide::Edit::MoveLines { down: false }),
                (alt, Key::ArrowDown, ide::Edit::MoveLines { down: true }),
                (ctrl, Key::Slash, ide::Edit::ToggleComment),
                (ctrl | shift, Key::K, ide::Edit::DeleteLines),
            ];
            for (modifiers, key, edit) in bindings {
                if ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                    self.ide.edit(edit);
                }
            }
        }
        // Where the caret is drawn, for a completion to open under; limn says it while no
        // popover is open.
        if let Some((_, caret)) = self.tree.editor_ime_rects() {
            self.ide.caret = Some(caret);
        }
        let focused = self.tree.focused_id().map(str::to_owned);
        for event in limn_egui::events(&ctx, area.min) {
            // While offers show, the arrows choose, Enter and Tab write one, Escape ends it.
            if !self.ide.completion_shown().is_empty() {
                match &event {
                    limn::UiEvent::Key {
                        key: limn::Key::ArrowUp,
                        ..
                    } => {
                        self.ide.completion_step(-1);
                        continue;
                    }
                    limn::UiEvent::Key {
                        key: limn::Key::ArrowDown,
                        ..
                    } => {
                        self.ide.completion_step(1);
                        continue;
                    }
                    limn::UiEvent::Key {
                        key: limn::Key::Enter | limn::Key::Tab,
                        ctrl: false,
                        shift: false,
                        ..
                    } => {
                        self.ide.accept_completion(None);
                        continue;
                    }
                    limn::UiEvent::Key {
                        key: limn::Key::Escape,
                        ..
                    } => self.ide.completion = None,
                    _ => {}
                }
            }
            if self.typed_into_terminal(focused.as_deref(), &event) {
                continue;
            }
            // The hover goes with the next key or press.
            if self.ide.hover.is_some()
                && matches!(
                    &event,
                    limn::UiEvent::Key { .. }
                        | limn::UiEvent::Text(_)
                        | limn::UiEvent::PointerDown { .. }
                )
            {
                self.ide.hover = None;
            }
            // The language server's keys on the editor, as VS Code's, and Vim's K for hover.
            let on_editor = focused.is_some() && focused == self.ide.editor_id();
            if on_editor {
                let id = match &event {
                    limn::UiEvent::Key {
                        key: limn::Key::F12,
                        shift: false,
                        ..
                    } => Some(ide::LSP_DEFINITION),
                    limn::UiEvent::Key {
                        key: limn::Key::F12,
                        shift: true,
                        ..
                    } => Some(ide::LSP_REFERENCES),
                    limn::UiEvent::Key {
                        key: limn::Key::F2, ..
                    } => Some(ide::LSP_RENAME),
                    limn::UiEvent::Text(typed)
                        if typed == "K" && self.ide.vim_mode == Some(limn::VimMode::Normal) =>
                    {
                        Some(ide::LSP_HOVER)
                    }
                    _ => None,
                };
                if let Some(id) = id {
                    actions.push(limn::UiAction::Clicked { id: id.into() });
                    continue;
                }
            }
            // Opening by name, finding in every file and references: the arrows move the row
            // Enter opens.
            if self.ide.quick.is_some() || self.ide.grep.is_some() || self.ide.references.is_some()
            {
                if let limn::UiEvent::Key {
                    key: key @ (limn::Key::ArrowUp | limn::Key::ArrowDown),
                    ..
                } = &event
                {
                    self.ide
                        .step_chosen(if *key == limn::Key::ArrowUp { -1 } else { 1 });
                    continue;
                }
            }
            // On a row of the tree, nvim's keys and the arrows walk it.
            if let Some(row) = focused
                .as_deref()
                .filter(|f| f.starts_with("dir:") || f.starts_with("file:"))
            {
                let key = match &event {
                    limn::UiEvent::Key {
                        key: limn::Key::ArrowUp,
                        ctrl: false,
                        ..
                    } => Some(ide::TreeKey::Up),
                    limn::UiEvent::Key {
                        key: limn::Key::ArrowDown,
                        ctrl: false,
                        ..
                    } => Some(ide::TreeKey::Down),
                    limn::UiEvent::Key {
                        key: limn::Key::ArrowLeft,
                        ctrl: false,
                        ..
                    } => Some(ide::TreeKey::Left),
                    limn::UiEvent::Key {
                        key: limn::Key::ArrowRight,
                        ctrl: false,
                        ..
                    } => Some(ide::TreeKey::Right),
                    limn::UiEvent::Key {
                        key: limn::Key::Escape,
                        ..
                    } => Some(ide::TreeKey::Escape),
                    limn::UiEvent::Text(typed) => match typed.as_str() {
                        "k" => Some(ide::TreeKey::Up),
                        "j" => Some(ide::TreeKey::Down),
                        "h" => Some(ide::TreeKey::Left),
                        "l" => Some(ide::TreeKey::Right),
                        "a" => Some(ide::TreeKey::New),
                        "r" => Some(ide::TreeKey::Rename),
                        "d" => Some(ide::TreeKey::Trash),
                        "y" => Some(ide::TreeKey::Copy),
                        "x" => Some(ide::TreeKey::Cut),
                        "p" => Some(ide::TreeKey::Paste),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(key) = key {
                    let (taken, jobs) = self.ide.tree_key(row, key);
                    self.run(jobs);
                    if taken {
                        continue;
                    }
                }
            }
            // In the find bar, Escape closes it, as its own close does.
            if focused.as_deref().is_some_and(|f| f.starts_with("find:"))
                && matches!(
                    &event,
                    limn::UiEvent::Key {
                        key: limn::Key::Escape,
                        ..
                    }
                )
            {
                actions.push(limn::UiAction::Clicked {
                    id: "find:dismiss".into(),
                });
                continue;
            }
            // `:` in Vim's Normal mode goes to a line, as `:42` does in Vim.
            if matches!(&event, limn::UiEvent::Text(typed) if typed == ":")
                && self.ide.vim_mode == Some(limn::VimMode::Normal)
                && focused.is_some()
                && focused == self.ide.editor_id()
            {
                actions.push(limn::UiAction::Clicked {
                    id: quick::QUICK_LINE.into(),
                });
                continue;
            }
            // `/` in Vim's Normal mode finds, as in Vim.
            let editor = self.ide.editor_id();
            if matches!(&event, limn::UiEvent::Text(typed) if typed == "/")
                && self.ide.vim_mode == Some(limn::VimMode::Normal)
                && focused.is_some()
                && focused == editor
            {
                actions.push(limn::UiAction::Clicked {
                    id: newen_components::find::FIND_OPEN.into(),
                });
                continue;
            }
            if let limn::UiEvent::Key {
                key,
                ctrl: true,
                shift,
                ..
            } = &event
            {
                let on_tree = focused
                    .as_deref()
                    .is_some_and(|f| f.starts_with("dir:") || f.starts_with("file:"));
                if *key == limn::Key::E && *shift && on_tree {
                    self.ide.focus = self.ide.editor_id();
                    continue;
                }
                if let Some(id) = self.ide.shortcut(*key, *shift) {
                    actions.push(limn::UiAction::Clicked { id });
                    continue;
                }
            }
            actions.extend(self.tree.handle_event(event));
        }
        actions.extend(limn_egui::apply_access_requests(&ctx, &mut self.tree));
        for action in actions {
            if let limn::UiAction::CopyRequested { text } = &action {
                ctx.copy_text(text.clone());
            }
            if matches!(&action, limn::UiAction::Clicked { id } if id == prefs::ICON_FOLDER_ADD) {
                // The dialog blocks until the person answers, so it runs beside the window.
                let (picked, ctx) = (self.picked.clone(), ctx.clone());
                std::thread::spawn(move || {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        *picked.lock().unwrap() = Some(folder);
                        ctx.request_repaint();
                    }
                });
            }
            // Typing in the editor on screen may begin, go on with or end a completion.
            let writing = !self.ide.prefs.vim || self.ide.vim_mode == Some(limn::VimMode::Insert);
            if let limn::UiAction::EditorChanged { id, .. } = &action {
                if writing && Some(id.as_str()) == self.ide.editor_id().as_deref() {
                    self.ide.typed();
                }
            }
            let jobs = self.ide.update(&action);
            self.send_to_newen();
            self.run(jobs);
        }
        let saves = self.ide.tick();
        self.run(saves);
        self.sync_terminal(&ctx, window);
        let changed = match (&shown, &self.ide.terminal.screen) {
            (Some(before), Some(now)) => !Arc::ptr_eq(before, now),
            (None, Some(_)) => true,
            _ => false,
        };
        self.tree.set_root(ide::view(&self.ide, window));
        let mut frame = self.tree.layout(window);
        if follow && changed {
            self.tree.scroll_to_end(ide::TERMINAL_SCROLL);
            frame = self.tree.layout(window);
        }
        // What the IDE asked of the tree: a control to take the keys, the caret in view.
        if let Some(id) = self.ide.focus.take() {
            let focused = self.tree.focus(&id);
            // A tree row comes once its folder is listed: it is asked for again a while.
            if !focused && self.focus_waits < 60 {
                self.focus_waits += 1;
                self.ide.focus = Some(id.clone());
                ctx.request_repaint();
            } else {
                self.focus_waits = 0;
            }
            if focused {
                // A field opened with words in it (`:`, a name to rename) takes what is typed
                // after them.
                let typed = match id.as_str() {
                    quick::QUICK_FIELD => self.ide.quick.as_ref().map(|(q, _)| q.clone()),
                    ide::NAME_FIELD => self.ide.tree.naming.as_ref().map(|n| n.value.clone()),
                    ide::SYMBOL_FIELD => self.ide.renaming_symbol.clone(),
                    _ => None,
                };
                if let Some(typed) = typed {
                    self.tree.set_input_value(&id, typed.clone());
                    self.tree.set_input_cursor(&id, typed.chars().count());
                }
                frame = self.tree.layout(window);
            }
        }
        if let Some(text) = self.ide.copy.take() {
            ctx.copy_text(text);
        }
        // A link of a preview to the web, in the desktop's browser.
        if let Some(url) = self.ide.open_url.take() {
            let _ = newen_files::open_in_system(url);
        }
        if let Some(row) = self.ide.reveal_row.take() {
            self.tree.reveal(&row);
            frame = self.tree.layout(window);
        }
        if std::mem::take(&mut self.ide.reveal) {
            if let Some(id) = self.ide.editor_id() {
                if self.tree.reveal_code_caret(&id) {
                    frame = self.tree.layout(window);
                }
            }
        }
        // What is open is kept as it changes, to open again next time.
        let session = self.ide.session();
        if session != self.kept && self.ide.buffers.iter().all(|b| !b.loading) {
            self.kept = session.clone();
            self.run(vec![ide::Job::SaveSession(self.ide.root.clone(), session)]);
        }
        limn_egui::paint_at(ui, &frame, area.min);
        limn_egui::set_cursor(&ctx, self.tree.cursor());
        if let Some(after) = self.tree.animation_repaint_after() {
            ctx.request_repaint_after(after);
        }
        // The tree's springs are the IDE's own, not the tree's.
        if self.ide.animating() {
            ctx.request_repaint();
        }
        ctx.request_repaint_after(POLL);
    }
}

const POLL: std::time::Duration = std::time::Duration::from_secs(1);

/// A file as given on the command line, `path[:line[:column]]`, as editors take it; a path
/// that exists as written is taken whole.
fn place(arg: &str) -> (PathBuf, Option<u32>, Option<u32>) {
    if std::path::Path::new(arg).exists() {
        return (arg.into(), None, None);
    }
    let (mut rest, mut numbers) = (arg, vec![]);
    while numbers.len() < 2 {
        match rest
            .rsplit_once(':')
            .map(|(head, n)| (head, n.parse::<u32>()))
        {
            Some((head, Ok(n))) => {
                numbers.insert(0, n);
                rest = head;
            }
            _ => break,
        }
    }
    (
        rest.into(),
        numbers.first().copied(),
        numbers.get(1).copied(),
    )
}

fn main() -> eframe::Result {
    // quill-ide's folders take newen-ide's names once, as Newen's own do (a debug build only
    // when NEWEN_MIGRATE=1: it shares the person's folders); one that cannot stays in use.
    if !cfg!(debug_assertions) || std::env::var_os("NEWEN_MIGRATE").is_some_and(|v| v == "1") {
        for base in [newen_files::data_dir(), newen_files::config_dir()]
            .into_iter()
            .flatten()
        {
            if let Err(error) = newen_files::move_legacy(&base, "newen-ide", "quill-ide") {
                eprintln!("newen-ide: quill-ide's folder stays in use: {error}");
            }
        }
    }
    // `newen-ide [folder | file[:line[:column]]] [file[:line[:column]]]`.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (given, mut line, mut column) = args.first().map_or_else(
        || {
            (
                std::env::current_dir().unwrap_or_else(|_| ".".into()),
                None,
                None,
            )
        },
        |arg| place(arg),
    );
    let given = std::fs::canonicalize(&given).unwrap_or(given);
    let (root, mut file) = match given.parent() {
        Some(parent) if given.is_file() => (parent.to_path_buf(), Some(given.clone())),
        _ => (given, None),
    };
    if let Some(arg) = args.get(1) {
        let (path, l, c) = place(arg);
        let path = root.join(path);
        file = Some(std::fs::canonicalize(&path).unwrap_or(path));
        (line, column) = (l, c);
    }
    // The folder's window, if one is open, shows the file instead of a second window.
    let open = newen_link::Message::Open {
        root: root.clone(),
        path: file.clone().unwrap_or_else(|| root.clone()),
        line,
        column,
    };
    if newen_link::send(&newen_link::ide_socket(&root), &open).is_ok() {
        return Ok(());
    }
    let title = format!(
        "newen-ide · {}",
        root.file_name().unwrap_or_default().to_string_lossy()
    );
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_app_id("newen-ide")
            .with_inner_size([1408.0, 844.0])
            .with_min_inner_size([560.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "newen-ide",
        options,
        Box::new(move |creation| {
            limn::set_fonts(newen_components::fonts(true, ""));
            limn_egui::install_fonts(&creation.egui_ctx);
            // `NEWEN_THEME=dark|light`, else the system's, else dark, as Newen.
            let dark = match newen_files::env("THEME")
                .as_deref()
                .and_then(|v| v.to_str())
            {
                Some("light") => false,
                Some("dark") => true,
                _ => creation.egui_ctx.system_theme() != Some(egui::Theme::Light),
            };
            creation.egui_ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            look::set_dark(dark);
            let kept = session::load(&root);
            let (mut ide, mut jobs) = ide::Ide::new(root, prefs::load(), kept.clone());
            jobs.extend(match (file, line) {
                (Some(f), Some(line)) => ide.goto_at(
                    f,
                    line.saturating_sub(1) as usize,
                    column.unwrap_or(1).saturating_sub(1),
                ),
                (Some(f), None) => ide.open(f),
                (None, _) => vec![],
            });
            let linked: Arc<Mutex<Vec<newen_link::Message>>> = Arc::default();
            let link = {
                let (linked, ctx) = (linked.clone(), creation.egui_ctx.clone());
                newen_link::listen(&newen_link::ide_socket(&ide.root), move |message| {
                    linked.lock().unwrap().push(message);
                    ctx.request_repaint();
                })
                .ok()
            };
            let app = App {
                tree: UiTree::new(Element::spacer()),
                ide,
                inbox: Arc::default(),
                ctx: creation.egui_ctx.clone(),
                fonts_attached: false,
                pinned_theme: newen_files::env("THEME").is_some(),
                polled: std::time::Instant::now(),
                picked: Arc::default(),
                kept,
                focus_waits: 0,
                shells: newen_pty::Tabs::default(),
                lsp: lsp::Lsp::default(),
                linked,
                _link: link,
            };
            app.run(jobs);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_on_the_command_line_takes_its_line_and_column() {
        assert_eq!(
            place("src/a.rs:12:4"),
            ("src/a.rs".into(), Some(12), Some(4))
        );
        assert_eq!(place("src/a.rs:12"), ("src/a.rs".into(), Some(12), None));
        assert_eq!(place("src/a.rs"), ("src/a.rs".into(), None, None));
        assert_eq!(place("a:b.rs:3"), ("a:b.rs".into(), Some(3), None));
    }
}

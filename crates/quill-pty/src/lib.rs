//! A shell on a pseudo-terminal in a session's folder (features/terminal.md): its output is
//! parsed into a screen of cells for limn's `TerminalView`, and keys become the bytes a
//! terminal sends. A reader thread wakes the window when new output arrives.

use limn::{Color, Key, TerminalCell, TerminalCursor, TerminalCursorShape, TerminalScreen};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The sixteen ANSI colors of Claude Desktop's light terminal theme, as
/// features/terminal.md records them.
const ANSI: [Color; 16] = [
    Color::rgb(0x1a, 0x1a, 0x1a),
    Color::rgb(0xff, 0x3a, 0x30),
    Color::rgb(0x1e, 0x9e, 0x3c),
    Color::rgb(0x98, 0x80, 0x1f),
    Color::rgb(0x00, 0x73, 0xe6),
    Color::rgb(0xcd, 0x20, 0x54),
    Color::rgb(0x8e, 0x6b, 0xd9),
    Color::rgb(0x99, 0x99, 0x99),
    Color::rgb(0x66, 0x66, 0x66),
    Color::rgb(0xff, 0x50, 0x47),
    Color::rgb(0x1e, 0x9e, 0x3c),
    Color::rgb(0x98, 0x80, 0x1f),
    Color::rgb(0x00, 0x78, 0xf0),
    Color::rgb(0xcd, 0x20, 0x54),
    Color::rgb(0x8e, 0x6b, 0xd9),
    Color::rgb(0xd6, 0xd6, 0xd6),
];
const FOREGROUND: Color = Color::rgb(0x1a, 0x1a, 0x1a);
// shortcut: Desktop keeps 10000 lines; all of them are copied on new output, so
// quill keeps fewer until the view reads vt100's rows in place.
const SCROLLBACK: usize = 1000;

pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Set by the reader when output arrived since `screen` last copied it.
    dirty: Arc<AtomicBool>,
    copied: Arc<TerminalScreen>,
}

impl Terminal {
    /// The person's login shell in `cwd`, `cols` × `rows`; `wake` runs after new output.
    pub fn spawn(
        cwd: &Path,
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::run(cwd, &[], cols, rows, wake)
    }

    /// `program` (its name, then its arguments) in `cwd`, or the login shell when it is empty.
    pub fn run(
        cwd: &Path,
        program: &[String],
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = native_pty_system()
            .openpty(size)
            .map_err(std::io::Error::other)?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(windows) {
                std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
            } else {
                "/bin/sh".into()
            }
        });
        let mut command = match program {
            [name, args @ ..] => {
                let mut command = CommandBuilder::new(name);
                command.args(args);
                command
            }
            [] => CommandBuilder::new(shell),
        };
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "quill");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(std::io::Error::other)?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(std::io::Error::other)?;
        let writer = pair.master.take_writer().map_err(std::io::Error::other)?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)));
        let output = parser.clone();
        let dirty = Arc::new(AtomicBool::new(true));
        let changed = dirty.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                output.lock().unwrap().process(&buffer[..read]);
                changed.store(true, Ordering::Release);
                wake();
            }
        });
        Ok(Self {
            master: pair.master,
            writer,
            parser,
            child,
            dirty,
            copied: Arc::default(),
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (current_rows, current_cols) = self.parser.lock().unwrap().screen().size();
        if (cols, rows) == (current_cols, current_rows) || cols == 0 || rows == 0 {
            return;
        }
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        self.parser
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(rows, cols);
        self.dirty.store(true, Ordering::Release);
    }

    /// Whether a program other than the shell holds the terminal, so closing it would stop one.
    pub fn busy(&self) -> bool {
        // shortcut: Windows has no foreground process group to ask; closing never warns there.
        #[cfg(not(unix))]
        return false;
        #[cfg(unix)]
        let shell = self.child.process_id().map(|pid| pid as i32);
        #[cfg(unix)]
        matches!((self.master.process_group_leader(), shell), (Some(leader), Some(shell)) if leader != shell)
    }

    /// Forget the lines above the screen and have the shell clear it (Ctrl+L), as a
    /// terminal's "Clear" does.
    pub fn clear(&mut self) {
        self.parser.lock().unwrap().process(b"\x1b[3J");
        self.write(&[0x0c]);
        self.dirty.store(true, Ordering::Release);
    }

    /// Whether the shell is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The screen and the lines above it as they are now, for `TerminalView`.
    pub fn screen(&mut self) -> Arc<TerminalScreen> {
        if self.dirty.swap(false, Ordering::Acquire) {
            let mut parser = self.parser.lock().unwrap();
            let mut screen = screen_of(parser.screen());
            screen.scrollback = scrollback_of(parser.screen_mut());
            self.copied = Arc::new(screen);
        }
        self.copied.clone()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn color(c: vt100::Color, fallback: Color) -> Color {
    match c {
        vt100::Color::Default => fallback,
        vt100::Color::Idx(i) if i < 16 => ANSI[i as usize],
        vt100::Color::Idx(i) if i < 232 => {
            // The xterm 6×6×6 cube.
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color::rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        vt100::Color::Idx(i) => {
            let gray = 8 + (i - 232) * 10;
            Color::rgb(gray, gray, gray)
        }
        vt100::Color::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

fn cell_of(cell: Option<&vt100::Cell>) -> TerminalCell {
    let Some(cell) = cell else {
        return TerminalCell::new(' ', FOREGROUND);
    };
    let ch = cell.contents().chars().next().unwrap_or(' ');
    let mut out = TerminalCell::new(ch, color(cell.fgcolor(), FOREGROUND));
    out.bg = match cell.bgcolor() {
        vt100::Color::Default => None,
        other => Some(color(other, FOREGROUND)),
    };
    out.bold = cell.bold();
    out.underline = cell.underline();
    out.inverse = cell.inverse();
    out
}

fn screen_of(screen: &vt100::Screen) -> TerminalScreen {
    let (rows, cols) = screen.size();
    let mut cells = Vec::with_capacity(rows as usize * cols as usize);
    for row in 0..rows {
        for col in 0..cols {
            cells.push(cell_of(screen.cell(row, col)));
        }
    }
    let (row, col) = screen.cursor_position();
    let cursor = (!screen.hide_cursor()).then_some(TerminalCursor {
        col: col as usize,
        row: row as usize,
        shape: TerminalCursorShape::Block,
    });
    TerminalScreen {
        cols: cols as usize,
        rows: rows as usize,
        cells,
        scrollback: vec![],
        cursor,
    }
}

/// The lines scrolled off the top, oldest first. vt100 shows them only by moving its view
/// back, so the view is walked back a screen at a time and put back at the bottom.
fn scrollback_of(screen: &mut vt100::Screen) -> Vec<Vec<TerminalCell>> {
    let (rows, cols) = screen.size();
    screen.set_scrollback(usize::MAX);
    let total = screen.scrollback();
    let mut lines = Vec::with_capacity(total);
    let mut offset = total;
    while offset > 0 {
        screen.set_scrollback(offset);
        for row in 0..(rows as usize).min(offset) {
            lines.push(
                (0..cols)
                    .map(|col| cell_of(screen.cell(row as u16, col)))
                    .collect(),
            );
        }
        offset = offset.saturating_sub(rows as usize);
    }
    screen.set_scrollback(0);
    lines
}

/// The terminals of one folder, as the pane's tabs.
#[derive(Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: usize,
    next: u32,
}

struct Tab {
    number: u32,
    /// The name the person gave it, in place of "Terminal {n}".
    name: Option<String>,
    terminal: Terminal,
}

/// What the pane shows of a tab.
#[derive(Clone, Debug, PartialEq)]
pub struct TabInfo {
    pub title: String,
    /// A command runs in it, so closing it asks first.
    pub busy: bool,
    /// The person named it; Desktop then drops the folder from its pill.
    pub named: bool,
}

impl Tabs {
    /// Open a new tab and make it the active one.
    pub fn open(
        &mut self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<()> {
        self.open_running(cwd, &[], None, cols, rows, wake)
    }

    /// Open a tab that runs `program` (the login shell when empty), named `name`, and make it
    /// the active one.
    pub fn open_running(
        &mut self,
        cwd: &Path,
        program: &[String],
        name: Option<String>,
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<()> {
        let terminal = Terminal::run(cwd, program, cols, rows, wake)?;
        self.next += 1;
        self.tabs.push(Tab {
            number: self.next,
            name,
            terminal,
        });
        self.active = self.tabs.len() - 1;
        Ok(())
    }

    /// Close a tab, stopping whatever runs in it.
    pub fn close(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
        }
        if index < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
    }

    /// Close every tab but `index`, which becomes the active one.
    pub fn close_others(&mut self, index: usize) {
        if index < self.tabs.len() {
            let kept = self.tabs.swap_remove(index);
            self.tabs = vec![kept];
            self.active = 0;
        }
    }

    /// Name a tab; `None` or a blank name gives it back "Terminal {n}".
    pub fn rename(&mut self, index: usize, name: Option<String>) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.name = name.filter(|n| !n.trim().is_empty());
        }
    }

    /// Move a tab to `to`, keeping the same tab active.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        if from >= self.tabs.len() || to >= self.tabs.len() || from == to {
            return;
        }
        let active = self.active;
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = if active == from {
            to
        } else if from < active && active <= to {
            active - 1
        } else if to <= active && active < from {
            active + 1
        } else {
            active
        };
    }

    /// Empty a tab's screen and the lines above it; the shell draws its prompt again.
    pub fn clear(&mut self, index: usize) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.terminal.clear();
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    /// Drop the tabs whose shell ended.
    pub fn reap(&mut self) {
        let mut index = self.tabs.len();
        while index > 0 {
            index -= 1;
            if !self.tabs[index].terminal.alive() {
                self.close(index);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn active_mut(&mut self) -> Option<&mut Terminal> {
        self.tabs.get_mut(self.active).map(|tab| &mut tab.terminal)
    }

    /// Each tab as the pane shows it: its name or "Terminal {n}" as Desktop numbers them, and whether a command runs in it.
    pub fn list(&self) -> Vec<TabInfo> {
        self.tabs
            .iter()
            .map(|tab| TabInfo {
                title: tab
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Terminal {}", tab.number)),
                busy: tab.terminal.busy(),
                named: tab.name.is_some(),
            })
            .collect()
    }
}

/// The bytes a terminal sends for a key, or `None` for keys the shell does not take.
pub fn key_bytes(key: Key, ctrl: bool, shift: bool) -> Option<Vec<u8>> {
    let letter = |k: Key| -> Option<u8> {
        let name = format!("{k:?}");
        (name.len() == 1).then(|| name.as_bytes()[0].to_ascii_lowercase())
    };
    Some(match key {
        Key::Enter => b"\r".to_vec(),
        Key::Backspace => vec![0x7f],
        Key::Tab if shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => vec![0x1b],
        Key::ArrowUp => b"\x1b[A".to_vec(),
        Key::ArrowDown => b"\x1b[B".to_vec(),
        Key::ArrowRight => b"\x1b[C".to_vec(),
        Key::ArrowLeft => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        // Ctrl+letter is the letter's control code (Ctrl+C = 0x03).
        other if ctrl => vec![letter(other)? - b'a' + 1],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_what_a_terminal_sends() {
        assert_eq!(key_bytes(Key::C, true, false), Some(vec![3]));
        assert_eq!(key_bytes(Key::Enter, false, false), Some(b"\r".to_vec()));
        assert_eq!(
            key_bytes(Key::ArrowUp, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(key_bytes(Key::A, false, false), None);
    }

    #[test]
    fn colored_output_becomes_cells() {
        let mut parser = vt100::Parser::new(2, 10, 0);
        parser.process(b"\x1b[32mok\x1b[0m done");
        let screen = screen_of(parser.screen());
        assert_eq!((screen.cols, screen.rows, screen.cells.len()), (10, 2, 20));
        assert_eq!(screen.cells[0].ch, 'o');
        assert_eq!(screen.cells[0].fg, ANSI[2]);
        assert_eq!(screen.cells[3].fg, FOREGROUND);
    }

    #[test]
    fn a_shell_runs_and_its_output_arrives() {
        let dir = std::env::temp_dir();
        let mut terminal = Terminal::spawn(&dir, 40, 5, || {}).unwrap();
        terminal.write(b"echo quill-$((40+2))\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let text: String = terminal.screen().cells.iter().map(|c| c.ch).collect();
            if text.contains("quill-42") {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no output: {text}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(terminal.alive());
    }

    #[test]
    fn lines_scrolled_off_stay_above_the_screen() {
        let mut parser = vt100::Parser::new(2, 6, 100);
        parser.process(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");
        let lines = scrollback_of(parser.screen_mut());
        let text: Vec<String> = lines
            .iter()
            .map(|l| {
                l.iter()
                    .map(|c| c.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        assert_eq!(text, ["one", "two", "three"]);
        assert_eq!(parser.screen().scrollback(), 0);
    }

    #[test]
    fn tabs_open_close_and_report_a_running_command() {
        let dir = std::env::temp_dir();
        let mut tabs = Tabs::default();
        tabs.open(&dir, 40, 5, || {}).unwrap();
        tabs.open(&dir, 40, 5, || {}).unwrap();
        assert_eq!(tabs.active(), 1);
        tabs.active_mut().unwrap().write(b"sleep 5\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !tabs.list()[1].busy {
            assert!(std::time::Instant::now() < deadline, "never busy");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let titles = |tabs: &Tabs| -> Vec<(String, bool)> {
            tabs.list().into_iter().map(|t| (t.title, t.busy)).collect()
        };
        assert_eq!(
            titles(&tabs),
            [("Terminal 1".into(), false), ("Terminal 2".into(), true)]
        );
        tabs.close(1);
        assert_eq!((tabs.active(), tabs.list().len()), (0, 1));
        tabs.open(&dir, 40, 5, || {}).unwrap();
        assert_eq!(tabs.list()[1].title, "Terminal 3");
        tabs.rename(1, Some("build".into()));
        tabs.move_tab(1, 0);
        assert_eq!(
            (
                tabs.list()[0].title.as_str(),
                tabs.list()[0].named,
                tabs.active()
            ),
            ("build", true, 0)
        );
        tabs.close_others(1);
        assert_eq!(
            (tabs.list().len(), tabs.list()[0].title.as_str()),
            (1, "Terminal 1")
        );
    }
}

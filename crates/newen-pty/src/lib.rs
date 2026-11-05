//! A shell on a pseudo-terminal in a session's folder: its output is
//! parsed into a screen of cells for limn's `TerminalView` (`view`), and keys become the bytes a
//! terminal sends. A reader thread wakes the window when new output arrives.
//!
//! An agent's tab also says how its command stands (docs/terminal.md): at the shell's prompt,
//! running, or waiting for someone to type (a secret when the program turned echo off), and
//! what the command printed, from the shell's OSC 133 marks where it gives them.

mod marks;
#[cfg(feature = "view")]
mod view;
#[cfg(feature = "view")]
pub use view::key_bytes;

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

// shortcut: Desktop keeps 10000 lines; all of them are copied on new output, so
// Newen keeps fewer until the view reads vt100's rows in place.
const SCROLLBACK: usize = 1000;

pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser<Replies>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Set by the reader when output arrived since `screen` last copied it.
    dirty: Arc<AtomicBool>,
    #[cfg(feature = "view")]
    copied: Arc<limn::TerminalScreen>,
    started: Instant,
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
        let command = match program {
            [name, args @ ..] => {
                let mut command = CommandBuilder::new(name);
                command.args(args);
                command
            }
            [] => CommandBuilder::new(login_shell()),
        };
        Self::start(cwd, command, cols, rows, wake)
    }

    /// The person's login shell for an agent's commands: bash and zsh with Newen's marks of
    /// where each command starts and ends (fish gives its own), loading the person's own
    /// configuration first.
    pub fn agent(
        cwd: &Path,
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::integrated(cwd, &login_shell(), cols, rows, wake)
    }

    /// `shell` with Newen's marks when it is bash or zsh, else as it is.
    fn integrated(
        cwd: &Path,
        shell: &str,
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let mut command = CommandBuilder::new(shell);
        match marks::prepare(shell) {
            Some(marks::Launch::Args(args)) => command.args(args),
            Some(marks::Launch::Env(vars)) => {
                for (key, value) in vars {
                    command.env(key, value);
                }
            }
            None => {}
        }
        Self::start(cwd, command, cols, rows, wake)
    }

    fn start(
        cwd: &Path,
        mut command: CommandBuilder,
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
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "newen");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(std::io::Error::other)?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(std::io::Error::other)?;
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().map_err(std::io::Error::other)?,
        ));
        let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            rows,
            cols,
            SCROLLBACK,
            Replies::default(),
        )));
        let output = parser.clone();
        let answer = writer.clone();
        let dirty = Arc::new(AtomicBool::new(true));
        let changed = dirty.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                let replies = {
                    let mut parser = output.lock().unwrap();
                    parser.process(&buffer[..read]);
                    let replies = parser.callbacks_mut();
                    replies.heard = Instant::now();
                    std::mem::take(&mut replies.answers)
                };
                if !replies.is_empty() {
                    let mut answer = answer.lock().unwrap();
                    let _ = answer.write_all(&replies);
                    let _ = answer.flush();
                }
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
            #[cfg(feature = "view")]
            copied: Arc::default(),
            started: Instant::now(),
        })
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let mut writer = self.writer.lock().unwrap();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    /// Pasted text, wrapped in the bracketed-paste marks when the program asked for them
    /// (shells and editors then take it as text, not as typed keys).
    pub fn paste(&mut self, text: &str) {
        let bracketed = self.parser.lock().unwrap().screen().bracketed_paste();
        self.write(&pasted_bytes(text, bracketed));
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

    /// How the terminal stands now: the shell at its prompt, a command running, or a command
    /// waiting for someone to type.
    pub fn state(&self) -> State {
        #[cfg(unix)]
        {
            let shell = self.child.process_id().map(|pid| pid as i32);
            let leader = self.master.process_group_leader();
            let Some(leader) = leader.filter(|leader| Some(*leader) != shell) else {
                return State::Prompt;
            };
            // A line read without echo is a password's, as iTerm2 and Ghostty tell one.
            let flags = self.master.get_termios().map(|t| t.local_flags.bits());
            if flags.is_some_and(|f| f & libc::ECHO == 0 && f & libc::ICANON != 0) {
                return State::Waiting { secret: true };
            }
            if reads_terminal(leader) {
                return State::Waiting { secret: false };
            }
            State::Running
        }
        // shortcut: Windows has no foreground process to ask; a quiet second is the prompt.
        // Its shells give no marks either, so a command ends when it goes quiet.
        #[cfg(not(unix))]
        match self.idle() > Duration::from_secs(1) {
            true => State::Prompt,
            false => State::Running,
        }
    }

    /// How long since output last arrived.
    pub fn idle(&self) -> Duration {
        self.parser.lock().unwrap().callbacks().heard.elapsed()
    }

    /// Whether the shell is ready for a command: it drew its prompt, or gave no mark of one
    /// and has been quiet a moment (what a slow start prints is not typed into).
    pub fn ready(&self) -> bool {
        let parser = self.parser.lock().unwrap();
        let replies = parser.callbacks();
        // A prompt still being drawn (a theme's blank line first, its right side) is let be.
        (replies.marks.prompts > 0 && replies.heard.elapsed() > Duration::from_millis(150))
            || (self.started.elapsed() > Duration::from_secs(1)
                && replies.heard.elapsed() > Duration::from_millis(300))
    }

    /// The screen's text as it is now, without the lines above it.
    pub fn text(&self) -> String {
        self.lines(0)
    }

    /// The screen's text and up to `above` of the lines above it.
    pub fn lines(&self, above: usize) -> String {
        let mut parser = self.parser.lock().unwrap();
        let rows = parser.screen().size().0 as usize;
        let lines = all_lines(parser.screen_mut());
        let from = lines.len().saturating_sub(rows + above);
        trimmed(&lines[from..])
    }

    /// Type `command` at the shell's prompt and press Enter; what it prints from then on is
    /// the run's.
    pub fn run_command(&mut self, command: &str) -> Run {
        let (line, started, ended, marked) = {
            let mut parser = self.parser.lock().unwrap();
            let marks = parser.callbacks().marks;
            let line = line_at(parser.screen_mut());
            (line, marks.started, marks.ended, marks.prompts > 0)
        };
        // As one paste, so a command of several lines runs whole.
        self.paste(command);
        self.write(b"\r");
        Run {
            line,
            started,
            ended,
            marked,
            at: Instant::now(),
            ran: false,
        }
    }

    /// How `run` stands: whether it ended (with its exit code when the shell marks it) and
    /// what it printed since it began.
    pub fn progress(&mut self, run: &mut Run) -> Progress {
        let state = self.state();
        if state != State::Prompt {
            run.ran = true;
        }
        let mut parser = self.parser.lock().unwrap();
        let (marks, heard) = (parser.callbacks().marks, parser.callbacks().heard);
        let idle = heard.elapsed();
        let now = line_at(parser.screen_mut());
        // Ended by its mark; without marks, by the shell having the terminal back once the
        // command had it, or before anything ran (a builtin) once the shell went quiet.
        let (done, exit, end) = if marks.ended > run.ended {
            (true, marks.exit, marks.ended_at)
        } else if !run.marked
            && state == State::Prompt
            && (run.ran
                || (run.at.elapsed() > Duration::from_millis(500)
                    && idle > Duration::from_millis(300)))
        {
            // The prompt's own line is not the command's.
            (true, None, now.saturating_sub(1))
        } else {
            (false, None, now)
        };
        let lines = all_lines(parser.screen_mut());
        // Its output begins where the shell marked it, else on the line after the one it was
        // typed on.
        let from = match marks.started > run.started {
            true => marks.started_at,
            false => run.line + 1,
        }
        .min(lines.len());
        let to = (end + 1).clamp(from, lines.len());
        let last = lines[..now.min(lines.len().saturating_sub(1)) + 1]
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .cloned()
            .unwrap_or_default();
        Progress {
            state,
            done,
            exit,
            idle,
            output: trimmed(&lines[from..to]),
            last_line: last.trim_end().to_owned(),
        }
    }
}

/// What the terminal answers a program that asks about it, gathered while output is parsed and
/// written back after. fish waits for the answer to "what are you" (DA1) before its first
/// prompt, so a terminal that never answers leaves it blank and deaf; others ask where the
/// cursor is.
struct Replies {
    answers: Vec<u8>,
    marks: Marks,
    /// When output last arrived.
    heard: Instant,
}

impl Default for Replies {
    fn default() -> Self {
        Self {
            answers: vec![],
            marks: Marks::default(),
            heard: Instant::now(),
        }
    }
}

/// The shell's OSC 133 marks so far: how many prompts it drew (A), commands it started (C)
/// and ended (D), the last one's exit code and the line it ended on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Marks {
    prompts: u32,
    started: u32,
    ended: u32,
    exit: Option<i32>,
    /// The line the last command's output began on.
    started_at: usize,
    ended_at: usize,
}

impl vt100::Callbacks for Replies {
    // shortcut: fish's marks carry no nonce, so a program that prints a mark of its own can
    // make a call return early; the next read shows how things really stand.
    fn unhandled_osc(&mut self, screen: &mut vt100::Screen, params: &[&[u8]]) {
        if params.first() != Some(&&b"133"[..]) {
            return;
        }
        match params.get(1).and_then(|p| p.first()) {
            Some(b'A') => self.marks.prompts += 1,
            Some(b'C') => {
                self.marks.started += 1;
                self.marks.started_at = line_at(screen);
            }
            Some(b'D') => {
                self.marks.ended += 1;
                self.marks.exit =
                    (params.get(2)).and_then(|code| std::str::from_utf8(code).ok()?.parse().ok());
                // At the start of a line the command's output ended on the one before; the
                // prompt is drawn where the cursor is.
                let start = screen.cursor_position().1 == 0;
                self.marks.ended_at = line_at(screen).saturating_sub(start as usize);
            }
            _ => {}
        }
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i1, c, first) {
            // A VT220 with ANSI colour, as xterm-like terminals say.
            (None, 'c', 0) => self.answers.extend_from_slice(b"\x1b[?62;22c"),
            (None, 'n', 5) => self.answers.extend_from_slice(b"\x1b[0n"),
            (None, 'n', 6) => {
                let (row, col) = screen.cursor_position();
                self.answers
                    .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            _ => {}
        }
    }
}

/// How a terminal stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// The shell has the terminal: nothing runs.
    Prompt,
    Running,
    /// The command waits for someone to type; `secret` when it does not echo (a password).
    Waiting {
        secret: bool,
    },
}

/// A command typed by [`Terminal::run_command`], followed by [`Terminal::progress`].
#[derive(Clone, Debug)]
pub struct Run {
    /// The line it was typed on, counting the lines above the screen.
    line: usize,
    /// The commands the shell had started and ended before it.
    started: u32,
    ended: u32,
    /// The shell marks its commands.
    marked: bool,
    at: Instant,
    /// The command was seen holding the terminal.
    ran: bool,
}

/// What [`Terminal::progress`] tells of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub state: State,
    pub done: bool,
    /// Its exit code, when the shell marks it.
    pub exit: Option<i32>,
    /// How long since output last arrived.
    pub idle: Duration,
    /// What it printed since it began.
    pub output: String,
    /// The last line with words on it, where a question shows.
    pub last_line: String,
}

/// The shell Newen runs in a tab: the person's.
fn login_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into())
        } else {
            "/bin/sh".into()
        }
    })
}

/// The cursor's line, counting the lines above the screen.
fn line_at(screen: &mut vt100::Screen) -> usize {
    let (row, _) = screen.cursor_position();
    screen.set_scrollback(usize::MAX);
    let above = screen.scrollback();
    screen.set_scrollback(0);
    above + row as usize
}

/// Every line, the ones above the screen first, as text. vt100 shows those only by moving its
/// view back, so the view is walked back a screen at a time and put back at the bottom.
fn all_lines(screen: &mut vt100::Screen) -> Vec<String> {
    let (rows, cols) = screen.size();
    screen.set_scrollback(usize::MAX);
    let total = screen.scrollback();
    let mut lines = Vec::with_capacity(total + rows as usize);
    let mut offset = total;
    while offset > 0 {
        screen.set_scrollback(offset);
        lines.extend(screen.rows(0, cols).take((rows as usize).min(offset)));
        offset = offset.saturating_sub(rows as usize);
    }
    screen.set_scrollback(0);
    lines.extend(screen.rows(0, cols));
    lines
}

/// `lines` as one text, without the blank lines at its end or the spaces ending each.
fn trimmed(lines: &[String]) -> String {
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |at| at + 1);
    (lines[..end].iter())
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `pid` is blocked reading a terminal: its system call is `read` or `readv` on a
/// `/dev/pts` or `/dev/tty` descriptor (`/proc/<pid>/syscall`, which needs the same user).
#[cfg(target_os = "linux")]
fn reads_terminal(pid: i32) -> bool {
    let Ok(call) = std::fs::read_to_string(format!("/proc/{pid}/syscall")) else {
        return false;
    };
    let mut fields = call.split_whitespace();
    let (Some(number), Some(fd)) = (fields.next(), fields.next()) else {
        return false;
    };
    let reads: &[&str] = if cfg!(target_arch = "x86_64") {
        &["0", "19"]
    } else if cfg!(target_arch = "aarch64") {
        &["63", "65"]
    } else {
        &[]
    };
    let Ok(fd) = u64::from_str_radix(fd.trim_start_matches("0x"), 16) else {
        return false;
    };
    reads.contains(&number)
        && std::fs::read_link(format!("/proc/{pid}/fd/{fd}"))
            .is_ok_and(|path| path.starts_with("/dev/pts") || path.starts_with("/dev/tty"))
}

/// shortcut: without `/proc` (macOS) a program reading the terminal is not seen; only one that
/// turns echo off is, and the agent's call returns once the command goes quiet.
#[cfg(all(unix, not(target_os = "linux")))]
fn reads_terminal(_pid: i32) -> bool {
    false
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
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
    /// The session whose agent runs its commands here.
    owner: Option<String>,
    /// It follows a program's output (a command left running elsewhere): typing goes nowhere.
    read_only: bool,
}

/// What the pane shows of a tab.
#[derive(Clone, Debug, PartialEq)]
pub struct TabInfo {
    pub title: String,
    /// A command runs in it, so closing it asks first.
    pub busy: bool,
    /// The person named it; Desktop then drops the folder from its pill.
    pub named: bool,
    /// An agent runs its commands in it.
    pub agent: bool,
    /// Its command waits for someone to type: `Some(true)` for a secret.
    pub waiting: Option<bool>,
    /// It shows a program's output that takes no input.
    pub read_only: bool,
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
            owner: None,
            read_only: !program.is_empty(),
        });
        self.active = self.tabs.len() - 1;
        Ok(())
    }

    /// Open the tab `owner`'s agent runs its commands in, named `name`, running `shell` (the
    /// person's login shell when `None`). The person's active tab stays as it is.
    #[allow(clippy::too_many_arguments)]
    pub fn open_agent(
        &mut self,
        cwd: &Path,
        owner: &str,
        name: String,
        shell: Option<&str>,
        cols: u16,
        rows: u16,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<()> {
        let terminal = match shell {
            Some(shell) => Terminal::integrated(cwd, shell, cols, rows, wake)?,
            None => Terminal::agent(cwd, cols, rows, wake)?,
        };
        self.next += 1;
        self.tabs.push(Tab {
            number: self.next,
            name: Some(name),
            terminal,
            owner: Some(owner.to_owned()),
            read_only: false,
        });
        if self.tabs.len() == 1 {
            self.active = 0;
        }
        Ok(())
    }

    /// The tab `owner`'s agent runs its commands in, and where it is among the tabs.
    pub fn agent_mut(&mut self, owner: &str) -> Option<(usize, &mut Terminal)> {
        (self.tabs.iter_mut().enumerate())
            .find(|(_, tab)| tab.owner.as_deref() == Some(owner))
            .map(|(at, tab)| (at, &mut tab.terminal))
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
                agent: tab.owner.is_some(),
                // Only an agent's tab is asked: it is the one someone is waited on in.
                waiting: match tab.owner.as_ref().map(|_| tab.terminal.state()) {
                    Some(State::Waiting { secret }) => Some(secret),
                    _ => None,
                },
                read_only: tab.read_only,
            })
            .collect()
    }
}

/// Every folder's tabs, shared by the engine (which runs an agent's commands in them) and the
/// window (which shows them and types into them).
#[derive(Clone, Default)]
pub struct Terminals(Arc<Shared>);

#[derive(Default)]
struct Shared {
    folders: Mutex<HashMap<PathBuf, Tabs>>,
    wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// The shell an agent's tab runs, when not the person's (tests pin one).
    shell: Option<String>,
}

impl std::fmt::Debug for Terminals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminals").finish_non_exhaustive()
    }
}

impl Terminals {
    /// Terminals whose agents' tabs run `shell`, not the person's login shell.
    pub fn with_shell(shell: impl Into<String>) -> Self {
        Self(Arc::new(Shared {
            shell: Some(shell.into()),
            ..Default::default()
        }))
    }

    /// The shell an agent's tab runs, when not the person's.
    pub fn shell(&self) -> Option<&str> {
        self.0.shell.as_deref()
    }

    /// Each folder's tabs, by the folder.
    pub fn folders(&self) -> MutexGuard<'_, HashMap<PathBuf, Tabs>> {
        self.0.folders.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Run `wake` whenever a tab prints, as the window repaints then.
    pub fn on_output(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self.0.wake.lock().unwrap() = Some(Arc::new(wake));
    }

    /// What a new tab runs when it prints.
    pub fn waker(&self) -> impl Fn() + Send + 'static {
        // Looked up when it prints: the window may say how to wake it after the tab opened.
        let shared = self.0.clone();
        move || {
            let wake = shared.wake.lock().unwrap().clone();
            if let Some(wake) = wake {
                wake();
            }
        }
    }
}

fn pasted_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.as_bytes().to_vec();
    }
    // The end mark inside the text would close the paste early.
    let text = text.replace("\x1b[201~", "");
    [b"\x1b[200~", text.as_bytes(), b"\x1b[201~"].concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "view")]
    use crate::view::{screen_of, scrollback_of, ANSI, FOREGROUND};
    #[cfg(feature = "view")]
    use limn::Key;

    #[test]
    fn it_answers_what_programs_ask_of_a_terminal() {
        let mut parser = vt100::Parser::new_with_callbacks(4, 20, 0, Replies::default());
        // fish's start: questions it needs no answer to, then DA1, which it waits on.
        parser.process(b"\x1b[?u\x1b[>0q\x1b[0cab\x1b[6n");
        assert_eq!(parser.callbacks().answers, b"\x1b[?62;22c\x1b[1;3R");
    }

    #[test]
    fn a_shell_that_asks_gets_its_prompt_up() {
        // shortcut: a pty spawn per run, unix only; fish is the shell that hangs unanswered.
        if cfg!(windows) || !Path::new("/bin/sh").exists() {
            return;
        }
        // The answer has no line end: read its 9 bytes raw, as fish does.
        let script = "stty raw -echo; printf '\\033[c'; dd bs=1 count=9 >/dev/null 2>&1; stty sane; echo ready";
        let program = ["/bin/sh", "-c", script].map(String::from);
        let terminal = Terminal::run(Path::new("/"), &program, 40, 5, || {}).unwrap();
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(5) {
            if terminal.text().contains("ready") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("the shell never got its answer");
    }

    /// Polls `terminal` until `until` holds of its run's progress, or fails after 10 s.
    fn follow(
        terminal: &mut Terminal,
        run: &mut Run,
        until: impl Fn(&Progress) -> bool,
    ) -> Progress {
        let started = Instant::now();
        loop {
            let progress = terminal.progress(run);
            if until(&progress) {
                return progress;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never: {progress:?}\n{}",
                terminal.text()
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    fn ready(terminal: &Terminal) {
        let started = Instant::now();
        while !terminal.ready() {
            assert!(started.elapsed() < Duration::from_secs(10), "never ready");
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// An agent's command in bash with Newen's marks: it runs, waits for a line and then for
    /// a secret, and ends with its exit code and what it printed.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_command_runs_waits_for_someone_and_ends_with_its_code() {
        if !Path::new("/bin/bash").exists() {
            return;
        }
        let dir = std::env::temp_dir();
        let mut terminal = Terminal::integrated(&dir, "/bin/bash", 80, 10, || {}).unwrap();
        ready(&terminal);
        let script = "sleep 0.4; read -p 'Go on? ' a; read -s -p 'Password: ' b; echo; echo got-\\$a; exit 3";
        let mut run = terminal.run_command(&format!("bash -c \"{script}\""));
        let running = follow(&mut terminal, &mut run, |p| p.state == State::Running);
        assert!(!running.done);
        let asked = follow(&mut terminal, &mut run, |p| p.state != State::Running);
        assert_eq!(asked.state, State::Waiting { secret: false });
        assert_eq!(asked.last_line, "Go on?");
        terminal.write(b"yes\r");
        let secret = follow(&mut terminal, &mut run, |p| {
            p.state == State::Waiting { secret: true }
        });
        assert_eq!(secret.last_line, "Password:");
        terminal.write(b"hunter2\r");
        let done = follow(&mut terminal, &mut run, |p| p.done);
        assert_eq!(done.exit, Some(3));
        assert!(done.output.contains("got-yes"), "{}", done.output);
        // What was typed without echo is nowhere on the screen.
        assert!(!done.output.contains("hunter2") && !terminal.text().contains("hunter2"));
        assert!(!done.output.contains("Password: hunter2"));
        // The next command starts clean.
        let mut next = terminal.run_command("echo second");
        let done = follow(&mut terminal, &mut next, |p| p.done);
        assert_eq!((done.exit, done.output.as_str()), (Some(0), "second"));
    }

    /// zsh with Newen's marks: the person's configuration loads, and each command ends with
    /// its code.
    #[cfg(unix)]
    #[test]
    fn zsh_gets_newens_marks() {
        let Some(zsh) = ["/usr/bin/zsh", "/bin/zsh"]
            .into_iter()
            .find(|z| Path::new(z).exists())
        else {
            return;
        };
        let dir = std::env::temp_dir();
        let mut terminal = Terminal::integrated(&dir, zsh, 80, 10, || {}).unwrap();
        ready(&terminal);
        let mut run = terminal.run_command("echo hi; (exit 4)");
        let done = follow(&mut terminal, &mut run, |p| p.done);
        assert_eq!((done.exit, done.output.as_str()), (Some(4), "hi"));
    }

    /// fish marks its own commands, exit code included (measured with fish 4.9).
    #[cfg(unix)]
    #[test]
    fn fish_marks_its_own_commands() {
        let Some(fish) = ["/usr/bin/fish", "/bin/fish"]
            .into_iter()
            .find(|f| Path::new(f).exists())
        else {
            return;
        };
        let dir = std::env::temp_dir();
        let program = [fish, "--no-config"].map(String::from);
        let mut terminal = Terminal::run(&dir, &program, 80, 10, || {}).unwrap();
        ready(&terminal);
        let mut run = terminal.run_command("echo one; false");
        let done = follow(&mut terminal, &mut run, |p| p.done);
        assert_eq!(done.exit, Some(1));
        assert!(done.output.contains("one"), "{}", done.output);
    }

    #[test]
    fn marks_are_counted_with_the_exit_code() {
        let mut parser = vt100::Parser::new_with_callbacks(4, 20, 0, Replies::default());
        parser.process(b"\x1b]133;A\x07$ \x1b]133;C;cmdline_url=x\x07out\r\n\x1b]133;D;7\x07");
        let marks = parser.callbacks().marks;
        assert_eq!(
            (
                marks.prompts,
                marks.started,
                marks.ended,
                marks.exit,
                marks.ended_at
            ),
            (1, 1, 1, Some(7), 0)
        );
    }

    #[test]
    fn a_paste_is_bracketed_when_the_program_asks() {
        assert_eq!(pasted_bytes("a\nb", false), b"a\nb");
        assert_eq!(pasted_bytes("a\x1b[201~b", true), b"\x1b[200~ab\x1b[201~");
    }

    #[cfg(feature = "view")]
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

    #[cfg(feature = "view")]
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
        terminal.write(b"echo newen-$((40+2))\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let text = terminal.text();
            if text.contains("newen-42") {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no output: {text}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(terminal.alive());
    }

    #[cfg(feature = "view")]
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

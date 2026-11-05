//! The terminal as limn's `TerminalView` paints it: its cells and colours, and the bytes a key
//! sends. Only the window needs it; the engine runs terminals without it.

use crate::Terminal;
use limn::{Color, Key, TerminalCell, TerminalCursor, TerminalCursorShape, TerminalScreen};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// The sixteen ANSI colors of Claude Desktop's light terminal theme, as its terminal shows
/// them.
pub(crate) const ANSI: [Color; 16] = [
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
pub(crate) const FOREGROUND: Color = Color::rgb(0x1a, 0x1a, 0x1a);

impl Terminal {
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

pub(crate) fn screen_of(screen: &vt100::Screen) -> TerminalScreen {
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
pub(crate) fn scrollback_of(screen: &mut vt100::Screen) -> Vec<Vec<TerminalCell>> {
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

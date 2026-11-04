//! A Word 97-2003 document (.doc) as Markdown, read as [MS-DOC] says: its main text from the
//! piece table, each paragraph's properties from the FKP pages (a heading by the fixed heading
//! styles, a list item, a table's cells and rows), and a field by its result.
// shortcut: no bold, italic, headers or footers: they are in the character runs and the header
// stories, read them when a person misses them.
use super::{inline, line_start, table};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

const NOT_WORD: &str = "This is not a Word document.";

pub(super) fn reads(path: &Path) -> bool {
    super::extension(path) == "doc"
}

pub(super) fn markdown(path: &Path) -> Result<String, String> {
    let mut file = cfb::open(path).map_err(|_| NOT_WORD)?;
    let mut stream = |name: &str| -> Option<Vec<u8>> {
        let mut bytes = vec![];
        file.open_stream(name).ok()?.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    };
    let word = stream("WordDocument").ok_or(NOT_WORD)?;
    let fib = Fib::read(&word)?;
    let table = stream(if fib.table1 { "1Table" } else { "0Table" })
        .ok_or("This document's text can't be found.")?;
    let pieces = (table.get(fib.clx.0..fib.clx.0 + fib.clx.1))
        .and_then(pieces)
        .ok_or("This document's text can't be found.")?;
    let text = text(&word, &pieces, fib.text);
    let mut runs = paragraph_runs(&word, &table, fib.papx);
    runs.sort_by_key(|run| run.0);
    let lists = lists(&table, fib.lists, fib.list_uses);
    Ok(write(&text, &runs, &lists))
}

/// What the File Information Block says of where things are.
struct Fib {
    /// The table stream is `1Table`, else `0Table`.
    table1: bool,
    /// The main text's length, in characters.
    text: u32,
    /// Where in the table stream the piece table (Clx) and the paragraphs' pages are: offset
    /// and length.
    clx: (usize, usize),
    papx: (usize, usize),
    /// The lists (PlfLst, their levels after it) and each use of one (PlfLfo).
    lists: (usize, usize),
    list_uses: (usize, usize),
}

impl Fib {
    fn read(word: &[u8]) -> Result<Fib, String> {
        // Word 6 and 95 wrote another format: its own mark, or a version under 97's (193).
        let old = "Documents from Word 95 or older can't be read.";
        match u16_at(word, 0) {
            Some(0xA5EC) => {}
            Some(0xA5DC) => return Err(old.into()),
            _ => return Err(NOT_WORD.into()),
        }
        if u16_at(word, 2).is_none_or(|version| version < 0xC0) {
            return Err(old.into());
        }
        let flags = u16_at(word, 0x0A).ok_or(NOT_WORD)?;
        if flags & 0x0100 != 0 {
            return Err("This document is protected with a password.".into());
        }
        let words = 34 + 2 * u16_at(word, 32).ok_or(NOT_WORD)? as usize;
        let longs = words + 2;
        let pairs = longs + 4 * u16_at(word, words).ok_or(NOT_WORD)? as usize + 2;
        let pair = |n: usize| -> Option<(usize, usize)> {
            Some((
                u32_at(word, pairs + 8 * n)? as usize,
                u32_at(word, pairs + 8 * n + 4)? as usize,
            ))
        };
        Ok(Fib {
            table1: flags & 0x0200 != 0,
            text: u32_at(word, longs + 12).ok_or(NOT_WORD)?,
            clx: pair(33).ok_or(NOT_WORD)?,
            papx: pair(13).unwrap_or((0, 0)),
            lists: pair(73).unwrap_or((0, 0)),
            list_uses: pair(74).unwrap_or((0, 0)),
        })
    }
}

/// A run of text: its first character's position, its last's, where it is in the
/// WordDocument stream, and whether it is one byte a character (Windows-1252) or two (UTF-16).
struct Piece {
    start: u32,
    end: u32,
    at: u32,
    narrow: bool,
}

/// The pieces of the Clx: formatting (Prc) first, then the piece table (Pcdt).
fn pieces(clx: &[u8]) -> Option<Vec<Piece>> {
    let mut at = 0;
    loop {
        match *clx.get(at)? {
            1 => at += 3 + u16_at(clx, at + 1)? as usize,
            2 => break,
            _ => return None,
        }
    }
    let length = u32_at(clx, at + 1)? as usize;
    let plc = clx.get(at + 5..at + 5 + length)?;
    let count = length.checked_sub(4)? / 12;
    (0..count)
        .map(|n| {
            let descriptor = 4 * (count + 1) + 8 * n;
            let fc = u32_at(plc, descriptor + 2)?;
            let narrow = fc & 0x4000_0000 != 0;
            let fc = fc & 0x3FFF_FFFF;
            Some(Piece {
                start: u32_at(plc, 4 * n)?,
                end: u32_at(plc, 4 * n + 4)?,
                at: if narrow { fc / 2 } else { fc },
                narrow,
            })
        })
        .collect()
}

/// The main text, each character with where it is in the stream (its paragraph's properties
/// are found by it).
fn text(word: &[u8], pieces: &[Piece], length: u32) -> Vec<(char, u32)> {
    let mut out = vec![];
    for piece in pieces {
        let count = piece.end.min(length).saturating_sub(piece.start) as usize;
        if piece.narrow {
            let Some(bytes) = word.get(piece.at as usize..piece.at as usize + count) else {
                continue;
            };
            // Windows-1252 has one character a byte.
            let (text, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
            out.extend(text.chars().zip(piece.at..));
        } else {
            let Some(bytes) = word.get(piece.at as usize..piece.at as usize + 2 * count) else {
                continue;
            };
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b));
            let mut at = piece.at;
            for c in char::decode_utf16(units) {
                let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
                out.push((c, at));
                at += 2 * c.len_utf16() as u32;
            }
        }
    }
    out
}

/// What a paragraph's direct formatting says.
#[derive(Clone, Copy, Default)]
struct Paragraph {
    /// Its style: 1 to 9 are the fixed "heading 1" to "heading 9".
    style: u16,
    in_table: bool,
    /// It ends a table's row.
    row_end: bool,
    /// The use of a list it is an item of, from 1 (0: none), and its level.
    list: u16,
    level: u8,
}

/// A list's levels: whether each counts (`1.`, else a bullet) and from where.
type Levels = Vec<(bool, usize)>;

/// Each use of a list (from 1) with its list's levels.
fn lists(table: &[u8], (lists, _): (usize, usize), (uses, _): (usize, usize)) -> Vec<Levels> {
    let read = || -> Option<Vec<(u32, Levels)>> {
        let count = u16_at(table, lists)? as usize;
        // Every list's levels follow the lists, in their order: nine, or one for a simple one.
        let mut level = lists + 2 + 28 * count;
        let mut out = vec![];
        for n in 0..count {
            let list = lists + 2 + 28 * n;
            let simple = table.get(list + 26)? & 1 != 0;
            let mut levels = vec![];
            for _ in 0..if simple { 1 } else { 9 } {
                let start = u32_at(table, level)? as i32;
                // 23 is a bullet, 255 nothing.
                let format = *table.get(level + 4)?;
                let properties =
                    *table.get(level + 24)? as usize + *table.get(level + 25)? as usize;
                let number = level + 28 + properties;
                level = number + 2 + 2 * u16_at(table, number)? as usize;
                levels.push((!matches!(format, 23 | 255), start.max(0) as usize));
            }
            out.push((u32_at(table, list)?, levels));
        }
        Some(out)
    };
    let lists = read().unwrap_or_default();
    let count = u32_at(table, uses).unwrap_or(0) as usize;
    (0..count.min(4096))
        .map_while(|n| u32_at(table, uses + 4 + 16 * n))
        .map(|id| {
            (lists.iter())
                .find(|(list, _)| *list == id)
                .map_or(vec![], |(_, levels)| levels.clone())
        })
        .collect()
}

/// The paragraphs' properties from the FKP pages the bin table names: (first byte, byte after
/// the last, properties), each range of the WordDocument stream.
fn paragraph_runs(
    word: &[u8],
    table: &[u8],
    (at, length): (usize, usize),
) -> Vec<(u32, u32, Paragraph)> {
    let Some(plc) = table.get(at..at + length) else {
        return vec![];
    };
    let count = length.saturating_sub(4) / 8;
    let mut runs = vec![];
    for n in 0..count {
        let Some(page) = u32_at(plc, 4 * (count + 1) + 4 * n)
            .map(|pn| (pn & 0x3F_FFFF) as usize * 512)
            .and_then(|start| word.get(start..start + 512))
        else {
            continue;
        };
        let runs_in_page = page[511] as usize;
        for run in 0..runs_in_page {
            let (Some(start), Some(end)) = (u32_at(page, 4 * run), u32_at(page, 4 * run + 4))
            else {
                break;
            };
            let offset = page
                .get(4 * (runs_in_page + 1) + 13 * run)
                .copied()
                .unwrap_or(0);
            let properties = match offset {
                0 => Some(Paragraph::default()),
                offset => properties(page, 2 * offset as usize),
            };
            if let Some(properties) = properties {
                runs.push((start, end, properties));
            }
        }
    }
    runs
}

/// A PAPX in an FKP page: its style, then its properties (sprms) one after another.
fn properties(page: &[u8], at: usize) -> Option<Paragraph> {
    let (start, length) = match *page.get(at)? {
        0 => (at + 2, 2 * *page.get(at + 1)? as usize),
        size => (at + 1, 2 * size as usize - 1),
    };
    let grpprl = page.get(start..start + length)?;
    let mut paragraph = Paragraph {
        style: u16_at(grpprl, 0)?,
        ..Paragraph::default()
    };
    let mut at = 2;
    while let Some(sprm) = u16_at(grpprl, at) {
        at += 2;
        let size = match sprm >> 13 {
            0 | 1 => 1,
            2 | 4 | 5 => 2,
            3 => 4,
            7 => 3,
            // A table's definition counts its length in two bytes; another, in one.
            _ if sprm == 0xD608 => 1 + u16_at(grpprl, at)? as usize,
            _ => match *grpprl.get(at)? {
                255 => break,
                size => 1 + size as usize,
            },
        };
        let operand = grpprl.get(at..at + size)?;
        match sprm {
            0x2416 => paragraph.in_table = operand[0] != 0,
            0x2417 | 0x244C => paragraph.row_end |= operand[0] != 0,
            0x6649 => paragraph.in_table |= u32_at(operand, 0)? > 0,
            // From 0xF800 a use says the paragraph is in no list.
            0x460B => paragraph.list = u16_at(operand, 0).filter(|n| *n < 0xF800).unwrap_or(0),
            0x260A => paragraph.level = operand[0],
            _ => {}
        }
        at += size;
    }
    Some(paragraph)
}

/// The text as Markdown: paragraphs, headings, list items and tables, a field by its result.
fn write(text: &[(char, u32)], runs: &[(u32, u32, Paragraph)], lists: &[Levels]) -> String {
    let properties = |fc: u32| -> Option<Paragraph> {
        let n = runs.partition_point(|run| run.0 <= fc).checked_sub(1)?;
        (fc < runs[n].1).then_some(runs[n].2)
    };
    // The number each level of each list is at; an item starts the levels under it again.
    let mut counters: HashMap<(u16, u8), usize> = HashMap::new();
    let mut marker = |paragraph: Paragraph| -> String {
        let at = (paragraph.list, paragraph.level);
        let Some(&(counts, start)) = (lists.get(paragraph.list as usize - 1))
            .and_then(|levels| levels.get(paragraph.level as usize))
        else {
            return "-".into();
        };
        counters.retain(|(list, level), _| *list != at.0 || *level <= at.1);
        let n = counters.entry(at).or_insert(start.saturating_sub(1));
        *n += 1;
        if counts {
            format!("{n}.")
        } else {
            "-".into()
        }
    };
    let mut out = String::new();
    let mut words = String::new();
    let (mut cells, mut rows): (Vec<String>, Vec<Vec<String>>) = (vec![], vec![]);
    // Each field open: whether its code is being read (its result comes after a separator).
    let mut fields: Vec<bool> = vec![];
    let mut previous = '\0';
    for &(c, fc) in text {
        match c {
            '\u{13}' => fields.push(true),
            '\u{14}' => {
                if let Some(code) = fields.last_mut() {
                    *code = false;
                }
            }
            '\u{15}' => {
                fields.pop();
            }
            _ if fields.iter().any(|code| *code) => {}
            // A cell's end, or a row's: the row's comes right after its last cell's when its
            // properties cannot say.
            '\u{07}' => {
                let row_end = properties(fc).map_or(previous == '\u{07}', |p| p.row_end);
                if row_end {
                    rows.push(std::mem::take(&mut cells));
                    words.clear();
                } else {
                    cells.push(cell(&std::mem::take(&mut words)));
                }
            }
            '\r' | '\u{0C}' | '\u{0E}' => {
                let paragraph = properties(fc).unwrap_or_default();
                if paragraph.in_table && c == '\r' {
                    words.push(' ');
                } else {
                    if !rows.is_empty() {
                        end_list(&mut out);
                        out += &table(&std::mem::take(&mut rows));
                    }
                    let item = (paragraph.list != 0 && !words.trim().is_empty())
                        .then(|| marker(paragraph));
                    block(&mut out, &std::mem::take(&mut words), paragraph, item);
                }
            }
            '\u{0B}' => words.push('\n'),
            '\t' => words.push(' '),
            '\u{1E}' => words.push('-'),
            // A picture, or an embedded object, in the text.
            '\u{01}' => words.push('\u{FFFC}'),
            c if (c as u32) < 0x20 => {}
            c => words.push(c),
        }
        previous = c;
    }
    if !rows.is_empty() {
        end_list(&mut out);
        out += &table(&rows);
    }
    block(&mut out, &words, Paragraph::default(), None);
    out
}

/// Markdown of a paragraph's words: escaped, its pictures marked.
fn words_of(words: &str) -> String {
    inline(words).replace('\u{FFFC}', "*\\[Image\\]*")
}

fn cell(words: &str) -> String {
    words_of(&words.replace('\n', " ")).trim().to_owned()
}

/// A paragraph, or a list item with its marker (`3.` or `-`).
fn block(out: &mut String, words: &str, paragraph: Paragraph, item: Option<String>) {
    let words = words.trim();
    if words.is_empty() {
        return;
    }
    if let Some(marker) = item {
        let words = words_of(&words.replace('\n', " "));
        let indent = "  ".repeat(paragraph.level.min(4) as usize);
        *out += &format!("{indent}{marker} {words}\n");
        return;
    }
    end_list(out);
    *out += &match paragraph.style {
        level @ 1..=3 => format!(
            "{} {}\n\n",
            "#".repeat(level as usize),
            words_of(&words.replace('\n', " "))
        ),
        4..=9 => format!("**{}**\n\n", words_of(&words.replace('\n', " "))),
        _ => {
            let lines: Vec<String> = (words.lines())
                .map(|line| line_start(&words_of(line.trim())))
                .collect();
            format!("{}\n\n", lines.join("  \n"))
        }
    };
}

fn end_list(out: &mut String) {
    if !out.is_empty() && !out.ends_with("\n\n") {
        *out += "\n";
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A .doc of these paragraphs, as Word 97 lays one out: each with its end mark and its
    /// properties (style, then sprms), the text as UTF-16 at 1024, the paragraphs' page at
    /// 2048.
    fn doc(name: &str, paragraphs: &[(&str, u16, &[u8])]) -> std::path::PathBuf {
        let text: String = paragraphs.iter().map(|p| p.0).collect();
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut word = vec![0u8; 2560];
        let put16 =
            |w: &mut Vec<u8>, at: usize, v: u16| w[at..at + 2].copy_from_slice(&v.to_le_bytes());
        let put32 =
            |w: &mut Vec<u8>, at: usize, v: u32| w[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put16(&mut word, 0, 0xA5EC);
        put16(&mut word, 2, 0x00C1);
        put16(&mut word, 0x0A, 0x0200);
        put16(&mut word, 32, 14);
        put16(&mut word, 62, 22);
        put32(&mut word, 64 + 12, units.len() as u32);
        put16(&mut word, 152, 93);
        let pairs = 154;
        put32(&mut word, pairs + 33 * 8, 0);
        put32(&mut word, pairs + 33 * 8 + 4, 21);
        put32(&mut word, pairs + 13 * 8, 32);
        put32(&mut word, pairs + 13 * 8 + 4, 12);
        for (n, unit) in units.iter().enumerate() {
            put16(&mut word, 1024 + 2 * n, *unit);
        }
        // The page: each paragraph's bytes, then where its properties are, kept from 300 on.
        let page = 2048;
        word[page + 511] = paragraphs.len() as u8;
        let mut at = 1024u32;
        put32(&mut word, page, at);
        let mut kept = 300;
        for (n, (words, style, sprms)) in paragraphs.iter().enumerate() {
            at += 2 * words.encode_utf16().count() as u32;
            put32(&mut word, page + 4 * (n + 1), at);
            let mut grpprl = style.to_le_bytes().to_vec();
            grpprl.extend_from_slice(sprms);
            if grpprl.len() % 2 == 1 {
                grpprl.push(0);
            }
            word[page + 4 * (paragraphs.len() + 1) + 13 * n] = (kept / 2) as u8;
            word[page + kept] = 0;
            word[page + kept + 1] = (grpprl.len() / 2) as u8;
            word[page + kept + 2..page + kept + 2 + grpprl.len()].copy_from_slice(&grpprl);
            kept += 2 + grpprl.len() + (grpprl.len() % 2);
        }
        // The table stream: the piece table at 0, the paragraphs' bin table at 32.
        let mut table = vec![0u8; 44];
        table[0] = 2;
        table[1..5].copy_from_slice(&16u32.to_le_bytes());
        table[9..13].copy_from_slice(&(units.len() as u32).to_le_bytes());
        table[15..19].copy_from_slice(&1024u32.to_le_bytes());
        table[32..36].copy_from_slice(&1024u32.to_le_bytes());
        table[36..40].copy_from_slice(&at.to_le_bytes());
        table[40..44].copy_from_slice(&4u32.to_le_bytes());
        let dir = std::env::temp_dir().join(format!("newen-doc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut file = cfb::create(&path).unwrap();
        file.create_stream("WordDocument")
            .unwrap()
            .write_all(&word)
            .unwrap();
        file.create_stream("1Table")
            .unwrap()
            .write_all(&table)
            .unwrap();
        file.flush().unwrap();
        path
    }

    const IN_TABLE: &[u8] = &[0x16, 0x24, 1];
    const ROW_END: &[u8] = &[0x16, 0x24, 1, 0x17, 0x24, 1];
    const LIST: &[u8] = &[0x0B, 0x46, 1, 0, 0x0A, 0x26, 1];

    /// A heading by its fixed style, a field by its result, a line break, a picture, a list
    /// item, a table's cells and rows (an empty cell too), and text that looks like a block.
    #[test]
    fn a_word_97_document_reads_as_markdown() {
        let path = doc(
            "a.doc",
            &[
                ("Informe\r", 1, &[]),
                (
                    "Ver \u{13} HYPERLINK \"x\" \u{14}la web\u{15} y_más\u{0B}- sigue\u{01}\r",
                    0,
                    &[],
                ),
                ("punto\r", 0, LIST),
                ("A\u{07}", 0, IN_TABLE),
                ("\u{07}", 0, IN_TABLE),
                ("\u{07}", 0, ROW_END),
                ("1|2\u{07}", 0, IN_TABLE),
                ("x\ry\u{07}", 0, IN_TABLE),
                ("\u{07}", 0, ROW_END),
                ("# fin\r", 0, &[]),
            ],
        );
        assert_eq!(
            markdown(&path).unwrap(),
            "# Informe\n\nVer la web y\\_más  \n\u{200B}- sigue*\\[Image\\]*\n\n  - punto\n\n\
             | A |  |\n|---|---|\n| 1\\|2 | x y |\n\n\u{200B}# fin\n\n"
        );
    }

    /// Not a Word document, a password, a broken one: each says so, and none panics.
    #[test]
    fn what_cannot_be_read_says_why() {
        let dir = std::env::temp_dir().join(format!("newen-doc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("plain.doc");
        std::fs::write(&plain, "hola").unwrap();
        assert_eq!(markdown(&plain).unwrap_err(), NOT_WORD);
        let path = doc("locked.doc", &[("x\r", 0, &[])]);
        let mut file = cfb::open_rw(&path).unwrap();
        let mut word = vec![];
        file.open_stream("WordDocument")
            .unwrap()
            .read_to_end(&mut word)
            .unwrap();
        word[0x0B] |= 1;
        file.create_stream("WordDocument")
            .unwrap()
            .write_all(&word)
            .unwrap();
        drop(file);
        assert!(markdown(&path).unwrap_err().contains("password"));
        // Every prefix of a document's streams cut short reads or fails, never panics.
        for cut in (0..2560).step_by(97) {
            let w: Vec<u8> = word[..cut].to_vec();
            let _ = Fib::read(&w).map(|fib| (fib.text, fib.clx));
            let _ = pieces(&w[..cut.min(w.len())]);
            let _ = paragraph_runs(&w, &w, (0, cut));
        }
    }
}

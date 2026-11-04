//! Spreadsheets and Word documents (.docx and .doc) as Markdown, for the Files pane to show and never to edit:
//! each sheet a table under its name, a document its paragraphs, headings, lists and tables.
//! The Markdown is limn's: headings `#` to `###`, lists by two spaces a level, pipe tables, and
//! backslash escapes only for `\ * _ ~ ` [ ] ( )`.
use std::path::Path;

mod doc;
mod sheets;
mod word;

/// Whether `markdown` turns this file into Markdown, by its extension.
pub fn converts(path: &Path) -> bool {
    word::reads(path) || doc::reads(path) || sheets::reads(path)
}

/// The file as Markdown, or why it cannot be read.
pub fn markdown(path: &Path) -> Result<String, String> {
    let markdown = if word::reads(path) {
        word::markdown(path)
    } else if doc::reads(path) {
        doc::markdown(path)
    } else {
        sheets::markdown(path)
    }?;
    Ok(match markdown.trim().is_empty() {
        true => "This file is empty.\n".into(),
        false => markdown,
    })
}

fn extension(path: &Path) -> String {
    (path.extension().and_then(|e| e.to_str()))
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Text that limn's Markdown shows as it is: the characters it reads as marks, escaped.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '~' | '`' | '[' | ']' | '(' | ')') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A line that would start a block of its own (a heading, a list item, a quote, a table) stays
/// text: a zero-width space goes before it, as limn's Markdown has no escape for these.
fn line_start(line: &str) -> String {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let block = line.starts_with(['#', '-', '>', '|'])
        || (1..=5).contains(&digits) && line[digits..].starts_with(". ");
    match block {
        true => format!("\u{200B}{line}"),
        false => line.to_owned(),
    }
}

/// A pipe table with `rows[0]` as its header, each row padded to the widest. Cells are Markdown
/// already; a pipe in one is escaped.
fn table(rows: &[Vec<String>]) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (n, row) in rows.iter().enumerate() {
        let cells: Vec<String> = (0..width)
            .map(|c| {
                let cell = row.get(c).map_or("", String::as_str);
                cell.split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .replace('|', "\\|")
            })
            .collect();
        out += &format!("| {} |\n", cells.join(" | "));
        if n == 0 {
            out += &format!("|{}\n", "---|".repeat(width));
        }
    }
    out + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A zip of these parts, in a folder of the test's own.
    pub(super) fn zipped(name: &str, parts: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("newen-office-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let stored =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (part, text) in parts {
            zip.start_file(*part, stored).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn only_spreadsheets_and_word_documents_are_converted() {
        for name in [
            "a.xlsx", "a.XLS", "a.xlsb", "a.xlsm", "a.ods", "a.docx", "a.doc",
        ] {
            assert!(converts(Path::new(name)), "{name}");
        }
        for name in ["a.md", "a.txt", "xlsx"] {
            assert!(!converts(Path::new(name)), "{name}");
        }
    }

    /// Only what limn reads as marks is escaped, and a line that would start a block does not.
    #[test]
    fn text_stays_text_in_limns_markdown() {
        assert_eq!(
            inline("a*b_[c](d)~`\\ # | < > 1."),
            "a\\*b\\_\\[c\\]\\(d\\)\\~\\`\\\\ # | < > 1."
        );
        for line in ["# no", "- no", "> no", "| no", "12. no", "-# no"] {
            assert_eq!(line_start(line), format!("\u{200B}{line}"));
        }
        for line in ["#hashtag", "1.5 kg", "123456. no", "texto"] {
            let kept = if line.starts_with('#') {
                format!("\u{200B}{line}")
            } else {
                line.into()
            };
            assert_eq!(line_start(line), kept);
        }
        assert_eq!(
            table(&[vec!["a".into(), "b|c".into()], vec!["1".into()]]),
            "| a | b\\|c |\n|---|---|\n| 1 |  |\n\n"
        );
    }
}

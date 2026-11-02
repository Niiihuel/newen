//! What tree-sitter finds in a file: its foldable blocks, the scopes whose first line stays
//! in view while their body scrolls under it, and its declarations, for the footer's path of
//! symbols.

use limn_editor::BufferSnapshot;
use limn_language::{Cancellation, Language, ParserEngine};
use std::ops::Range;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Structure {
    /// The revision it was found in; it is drawn only on that one.
    pub revision: u64,
    /// Blocks that fold, by rows, the first one kept in view.
    pub folds: Vec<Range<usize>>,
    /// Scopes whose first row stays at the top while the rest scrolls, by rows.
    pub scopes: Vec<Range<usize>>,
    /// Declarations by name and characters, a parent before what it holds.
    pub symbols: Vec<(String, Range<usize>)>,
}

/// Bounds of what is drawn, as openADE keeps them.
const MOST: usize = 65_536;

/// `snapshot`'s structure, in the language `path` says it is in; empty for one tree-sitter
/// has no grammar for.
pub fn analyze(snapshot: &BufferSnapshot, path: &Path) -> Structure {
    let language = Language::from_path(&path.to_string_lossy());
    let mut structure = Structure {
        revision: snapshot.revision(),
        ..Structure::default()
    };
    if language == Language::Plain {
        return structure;
    }
    let Ok(analysis) =
        ParserEngine::default().analyze(snapshot, language, &Cancellation::default())
    else {
        return structure;
    };
    let lines = snapshot.line_count();
    structure.folds = analysis
        .folds
        .iter()
        .take(MOST)
        .map(|fold| fold.rows.clone())
        .filter(|rows| rows.start + 1 < rows.end && rows.end <= lines)
        .collect();
    let mut starts = std::collections::BTreeSet::new();
    for symbol in analysis.symbols.iter().take(MOST) {
        if symbol.range.is_empty() || symbol.range.end > snapshot.char_count() {
            continue;
        }
        structure
            .symbols
            .push((symbol.name.clone(), symbol.range.clone()));
        let rows = snapshot
            .row_of(symbol.range.start)
            .and_then(|start| Ok(start..snapshot.row_of(symbol.range.end - 1)? + 1));
        if let Some(rows) = rows.ok().filter(|r| r.start + 1 < r.end) {
            if starts.insert(rows.start) {
                structure.scopes.push(rows);
            }
        }
    }
    // Blocks without a name, as an `if`, still keep their first line in view.
    for fold in &structure.folds {
        if starts.insert(fold.start) {
            structure.scopes.push(fold.clone());
        }
    }
    structure
        .scopes
        .sort_by_key(|rows| (rows.start, std::cmp::Reverse(rows.end)));
    structure
}

/// The declarations that hold character `at`, the outermost first.
pub fn symbols_at(
    structure: &Structure,
    at: usize,
) -> impl Iterator<Item = &(String, Range<usize>)> {
    structure
        .symbols
        .iter()
        .filter(move |(_, range)| range.contains(&at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rust_file_folds_its_blocks_and_names_where_the_caret_is() {
        let source =
            "struct A;\n\nmod m {\n    fn go() {\n        let x = 1;\n        x;\n    }\n}\n";
        let snapshot = limn_editor::TextBuffer::new(source).unwrap().snapshot();
        let structure = analyze(&snapshot, Path::new("a.rs"));
        assert!(
            structure.folds.iter().any(|f| f.start == 2),
            "{:?}",
            structure.folds
        );
        assert!(structure.scopes.iter().any(|s| s.start == 3));
        let inside = source.find("let x").unwrap();
        let names: Vec<&str> = symbols_at(&structure, inside)
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(names, ["m", "go"]);
        assert!(analyze(&snapshot, Path::new("a.txt")).folds.is_empty());
    }
}

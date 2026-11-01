//! The rows of a file changed since its last commit, as editors mark them in the gutter.

use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Added,
    Changed,
    /// Lines removed before the range's row; the range is empty.
    Removed,
}

/// What changed from `head` to `now`, by rows of `now`.
pub fn changes(head: &str, now: &str) -> Vec<(Range<usize>, Change)> {
    use similar::DiffOp::*;
    similar::TextDiff::from_lines(head, now)
        .ops()
        .iter()
        .filter_map(|op| match *op {
            Equal { .. } => None,
            Insert {
                new_index, new_len, ..
            } => Some((new_index..new_index + new_len, Change::Added)),
            Delete { new_index, .. } => Some((new_index..new_index, Change::Removed)),
            Replace {
                new_index, new_len, ..
            } => Some((new_index..new_index + new_len, Change::Changed)),
        })
        .collect()
}

/// `path` as its repository's last commit holds it; `None` when it has none, or the file is
/// new to it.
pub fn committed(path: &Path) -> Option<String> {
    let (dir, name) = (path.parent()?, path.file_name()?);
    let out = quill_files::command("git")
        .arg("-C")
        .arg(dir)
        .arg("show")
        .arg(format!("HEAD:./{}", name.to_string_lossy()))
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_changed_and_removed_rows_are_told_apart() {
        let head = "a\nb\nc\nd\n";
        let now = "a\nB\nc\nnew\n";
        assert_eq!(
            changes(head, now),
            vec![(1..2, Change::Changed), (3..4, Change::Changed)]
        );
        assert_eq!(changes("a\nb\n", "a\nx\nb\n"), vec![(1..2, Change::Added)]);
        assert_eq!(
            changes("a\nb\nc\n", "a\nc\n"),
            vec![(1..1, Change::Removed)]
        );
        assert!(changes(head, head).is_empty());
    }
}

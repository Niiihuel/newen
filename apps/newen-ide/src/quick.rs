//! Opening a file by its name, as nvim's pickers do: quill-files' `best`, and `:` to go to a
//! line of the file on screen.

use std::path::PathBuf;

pub const QUICK_OPEN: &str = "quick:open";
pub const QUICK_FIELD: &str = "quick:field";
pub const QUICK_CLOSE: &str = "quick:close";
/// Opening by name with `:` typed: going to a line of the file on screen.
pub const QUICK_LINE: &str = "quick:line";

/// The one-based line `:42` asks for, as a zero-based row.
pub fn line(query: &str) -> Option<usize> {
    query
        .strip_prefix(':')?
        .trim()
        .parse::<usize>()
        .ok()
        .map(|n| n.saturating_sub(1))
}
/// The most it lists.
pub const SHOWN: usize = 50;

pub use newen_files::find::score;

/// The files that match `query` best, at most `SHOWN`; with no query, the first ones.
pub fn best<'a>(query: &str, files: &'a [PathBuf]) -> Vec<&'a PathBuf> {
    newen_files::find::best(query, files, SHOWN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colon_and_a_number_ask_for_a_line() {
        assert_eq!(
            (line(":42"), line(": 1"), line(":"), line("42")),
            (Some(41), Some(0), None, None)
        );
    }
}

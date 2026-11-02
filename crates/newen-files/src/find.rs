//! Finding in a folder, for quill-ide's pickers and the Files pane's filter: its files as git
//! sees them, a file by the letters of its path as nvim's pickers do, and the lines that hold
//! some words as ripgrep would, case told apart only when the words have a capital.

use std::path::{Path, PathBuf};

/// The most `grep` finds; and the largest file it reads.
pub const LIMIT: usize = 200;
const LARGEST: u64 = 2 * 1024 * 1024;

/// Every file under `root` git sees, as paths from it, sorted.
///
/// shortcut: at most 100 000, all walked again each time; worth an index kept up to date
/// for larger trees.
pub fn files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| e.path().strip_prefix(root).ok().map(Path::to_path_buf))
        .take(100_000)
        .collect();
    files.sort();
    files
}

/// The folder's own entries, as an empty "@" lists them: folders first, each ending in `/`, then
/// files, by name, without `.git`.
pub fn top(root: &Path) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(root) else {
        return vec![];
    };
    let mut entries: Vec<(bool, String)> = dir
        .filter_map(Result::ok)
        .filter(|e| e.file_name() != ".git")
        .map(|e| {
            (
                !e.file_type().is_ok_and(|t| t.is_dir()),
                e.file_name().to_string_lossy().into_owned(),
            )
        })
        .collect();
    entries.sort();
    entries
        .into_iter()
        .map(|(file, name)| PathBuf::from(if file { name } else { format!("{name}/") }))
        .collect()
}

/// How well `query` matches `path`: all its letters in order, case aside, scored higher when
/// they run together, begin a word or fall in the file's name, and for shorter paths. `None`
/// when they do not all appear.
pub fn score(query: &str, path: &str) -> Option<i32> {
    let name_at = path.rfind('/').map_or(0, |at| at + 1);
    let path: Vec<char> = path.chars().collect();
    let mut score = 0;
    let mut from = 0;
    let mut last: Option<usize> = None;
    for wanted in query.chars().flat_map(char::to_lowercase) {
        let at = (from..path.len()).find(|&i| path[i].to_lowercase().eq([wanted]))?;
        score += 1;
        if last == Some(at.wrapping_sub(1)) {
            score += 5;
        }
        if at == 0 || matches!(path[at - 1], '/' | '_' | '-' | '.' | ' ') {
            score += 8;
        }
        if at >= name_at {
            score += 3;
        }
        last = Some(at);
        from = at + 1;
    }
    Some(score * 10 - path.len() as i32)
}

/// The files that match `query` best, at most `shown`; with no query, the first ones.
pub fn best<'a>(query: &str, files: &'a [PathBuf], shown: usize) -> Vec<&'a PathBuf> {
    if query.is_empty() {
        return files.iter().take(shown).collect();
    }
    let mut scored: Vec<(i32, &PathBuf)> = files
        .iter()
        .filter_map(|f| Some((score(query, &f.to_string_lossy())?, f)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().take(shown).map(|(_, f)| f).collect()
}

/// A line that holds the query: its file from the folder, its row from 0, and its words.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hit {
    pub path: PathBuf,
    pub row: usize,
    pub text: String,
}

/// The lines of `files` under `root` that hold `query`, at most `LIMIT`. Files that are
/// not text, or larger than 2 MB, are passed over.
pub fn grep(root: &Path, files: &[PathBuf], query: &str) -> Vec<Hit> {
    let exact = query.chars().any(char::is_uppercase);
    let wanted = if exact {
        query.to_owned()
    } else {
        query.to_lowercase()
    };
    let mut hits = vec![];
    for path in files {
        let full = root.join(path);
        if std::fs::metadata(&full).map_or(true, |m| m.len() > LARGEST) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&full) else {
            continue;
        };
        for (row, line) in text.lines().enumerate() {
            let holds = if exact {
                line.contains(&wanted)
            } else {
                line.to_lowercase().contains(&wanted)
            };
            if holds {
                let text = line.trim().chars().take(160).collect();
                hits.push(Hit {
                    path: path.clone(),
                    row,
                    text,
                });
                if hits.len() == LIMIT {
                    return hits;
                }
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_in_order_match_and_names_and_word_starts_win() {
        assert_eq!(score("xyz", "src/main.rs"), None);
        assert!(score("mrs", "src/main.rs").is_some());
        let files: Vec<PathBuf> = [
            "docs/engine.md",
            "crates/newen-engine/src/lib.rs",
            "apps/newen-ide/src/ide.rs",
            "crates/newen-ui/src/view.rs",
        ]
        .map(PathBuf::from)
        .into();
        let best = |query: &str| best(query, &files, 50);
        assert_eq!(best("ide")[0], &PathBuf::from("apps/newen-ide/src/ide.rs"));
        assert_eq!(
            best("view")[0],
            &PathBuf::from("crates/newen-ui/src/view.rs")
        );
        assert_eq!(best("engmd")[0], &PathBuf::from("docs/engine.md"));
        assert_eq!(best("").len(), 4);
    }

    #[test]
    fn the_top_level_lists_folders_then_files_without_git() {
        let root = std::env::temp_dir().join(format!("quill-files-top-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in [".git", "src", "docs"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("README.md"), "").unwrap();
        std::fs::write(root.join(".env"), "").unwrap();
        let top = top(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            top,
            ["docs/", "src/", ".env", "README.md"].map(PathBuf::from)
        );
    }

    #[test]
    fn lines_are_found_in_every_file_and_a_capital_tells_case_apart() {
        let root = std::env::temp_dir().join(format!("quill-files-grep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn main() {}\n    Main();\n").unwrap();
        std::fs::write(root.join("b.bin"), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(files(&root), [PathBuf::from("b.bin"), "src/a.rs".into()]);
        let all = files(&root);
        let any = grep(&root, &all, "main");
        let exact = grep(&root, &all, "Main");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(any.len(), 2);
        assert_eq!(
            exact,
            vec![Hit {
                path: "src/a.rs".into(),
                row: 1,
                text: "Main();".into()
            }]
        );
    }
}

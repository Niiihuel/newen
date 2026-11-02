//! Finding in the file on screen, for quill-ide's editor and quill's Files pane: limn's
//! FindBar over the editor, its matches marked, and the caret taken from one to the next.

use limn::{EditorController, FindOptions};
use limn_editor::{SearchOptions, SearchResults, Selection};

/// The bar's id; limn derives its controls' from it (`find:query`, `find:next`…).
pub const FIND: &str = "find";
pub const FIND_OPEN: &str = "find:open";
pub const FIND_QUERY: &str = "find:query";

#[derive(Default)]
pub struct Find {
    pub query: String,
    pub options: FindOptions,
    pub results: Option<SearchResults>,
    /// The match the caret is on.
    pub current: Option<usize>,
    pub error: Option<String>,
}

impl Find {
    /// Search `editor` again, the current match the first at or after its caret.
    pub fn search(&mut self, editor: &EditorController) {
        self.error = None;
        self.results = None;
        self.current = None;
        if self.query.is_empty() {
            return;
        }
        let options = SearchOptions {
            case_sensitive: self.options.case,
            whole_word: self.options.word,
            regex: self.options.regex,
            scope: None,
        };
        match editor.search(&self.query, &options) {
            Ok(results) => {
                let caret = editor
                    .snapshot()
                    .map(|s| s.selection().range().start)
                    .unwrap_or(0);
                let matches = &results.matches;
                self.current = matches
                    .iter()
                    .position(|m| m.start >= caret)
                    .or((!matches.is_empty()).then_some(0));
                self.results = Some(results);
            }
            Err(error) => self.error = Some(format!("{error:?}")),
        }
    }

    /// Go `by` matches on, round the ends, and select it.
    pub fn step(&mut self, editor: &EditorController, by: isize) {
        let count = self.results.as_ref().map_or(0, |r| r.matches.len()) as isize;
        if count == 0 {
            return;
        }
        let at = self
            .current
            .map_or(0, |c| (c as isize + by).rem_euclid(count));
        self.current = Some(at as usize);
        self.select(editor);
    }

    /// Select the current match in the editor.
    pub fn select(&self, editor: &EditorController) {
        let (Some(results), Some(at)) = (&self.results, self.current) else {
            return;
        };
        let found = results.matches[at].clone();
        let shared = editor.shared_buffer();
        let Ok(mut buffer) = shared.lock() else {
            return;
        };
        let _ = buffer.set_selection(Selection {
            anchor: found.start,
            head: found.end,
        });
    }

    /// "3 of 12", or nothing before a query.
    pub fn count_label(&self) -> String {
        match (&self.results, self.current) {
            (Some(results), Some(at)) => {
                let more = if results.truncated { "+" } else { "" };
                format!("{} of {}{more}", at + 1, results.matches.len())
            }
            (Some(_), None) => "No results".into(),
            _ => String::new(),
        }
    }
}

/// What the host does after the bar took a click.
#[derive(Debug, PartialEq)]
pub enum Asked {
    /// Focus the bar's field.
    Query,
    /// The bar closed: the keys go back to the editor.
    Editor,
    /// A match was selected: bring the caret into view.
    Reveal,
}

/// The bar's clicks with `editor` the file on screen: open it (with what is selected as the
/// query, else the last one), close it, its options and its arrows. `None` when `id` is none of
/// them.
pub fn clicked(
    find: &mut Option<Find>,
    id: &str,
    editor: Option<&EditorController>,
) -> Option<Asked> {
    let part = id.strip_prefix("find:")?;
    if part == "open" {
        let Some(editor) = editor else {
            return Some(Asked::Query);
        };
        let selected = editor.snapshot().ok().and_then(|s| {
            let range = s.selection().range();
            (!range.is_empty() && range.len() < 200)
                .then(|| s.text_range(range).ok())
                .flatten()
        });
        let mut bar = find.take().unwrap_or_default();
        if let Some(selected) = selected.filter(|s| !s.contains('\n')) {
            bar.query = selected;
        }
        bar.search(editor);
        *find = Some(bar);
        return Some(Asked::Query);
    }
    if part == "dismiss" {
        *find = None;
        return Some(Asked::Editor);
    }
    let (Some(bar), Some(editor)) = (find.as_mut(), editor) else {
        return Some(Asked::Reveal);
    };
    match part {
        "next" => bar.step(editor, 1),
        "previous" => bar.step(editor, -1),
        "case" | "word" | "regex" => {
            let options = &mut bar.options;
            match part {
                "case" => options.case = !options.case,
                "word" => options.word = !options.word,
                _ => options.regex = !options.regex,
            }
            bar.search(editor);
            bar.select(editor);
        }
        _ => {}
    }
    Some(Asked::Reveal)
}

/// The query typed, searched at once; `false` when `id` is not the bar's field.
pub fn typed(
    find: &mut Option<Find>,
    id: &str,
    value: &str,
    editor: Option<&EditorController>,
) -> bool {
    if id != FIND_QUERY {
        return false;
    }
    if let (Some(bar), Some(editor)) = (find.as_mut(), editor) {
        bar.query = value.to_owned();
        bar.search(editor);
        bar.select(editor);
    }
    true
}

/// Enter in the field: the next match; `false` when `id` is not the bar's field.
pub fn submitted(find: &mut Option<Find>, id: &str, editor: Option<&EditorController>) -> bool {
    if id != FIND_QUERY {
        return false;
    }
    if let (Some(bar), Some(editor)) = (find.as_mut(), editor) {
        bar.step(editor, 1);
    }
    true
}

/// The bar, `width` wide at most, floating under the end of `anchor`.
pub fn bar(find: &Find, width: f32, anchor: limn::Element) -> limn::Element {
    limn::FindBar::new(FIND, find.query.as_str(), crate::look::palette())
        .surface(crate::look::menu_palette())
        .options(find.options)
        .count_label(find.count_label())
        .has_matches(find.results.as_ref().is_some_and(|r| !r.matches.is_empty()))
        .has_current(find.current.is_some())
        .no_matches(find.results.as_ref().is_some_and(|r| r.matches.is_empty()))
        .error(find.error.as_deref())
        .width((width - 24.0).min(520.0))
        .attach(anchor)
}

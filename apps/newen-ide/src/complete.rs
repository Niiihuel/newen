//! Completion: what a language server offers at the caret, narrowed as the person goes on
//! typing, and what choosing one writes.

use serde_json::Value;

/// One offer: what the list shows, what it writes, and whether that is a snippet (`$1`).
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub label: String,
    pub detail: String,
    pub text: String,
    pub snippet: bool,
    /// What typing is matched against, and the order the server wants.
    pub filter: String,
    pub sort: String,
}

/// The most the list shows.
pub const SHOWN: usize = 50;

/// A completion answer's offers: a list, or `{ items }`.
pub fn offers(value: &Value) -> Vec<Offer> {
    let items = match value {
        Value::Array(items) => items,
        Value::Object(_) => match value["items"].as_array() {
            Some(items) => items,
            None => return vec![],
        },
        _ => return vec![],
    };
    items
        .iter()
        .filter_map(|item| {
            let label = item["label"].as_str()?.to_owned();
            let text = item["textEdit"]["newText"]
                .as_str()
                .or(item["insertText"].as_str())
                .unwrap_or(&label)
                .to_owned();
            Some(Offer {
                detail: item["detail"].as_str().unwrap_or_default().to_owned(),
                snippet: item["insertTextFormat"].as_u64() == Some(2),
                filter: item["filterText"].as_str().unwrap_or(&label).to_owned(),
                sort: item["sortText"].as_str().unwrap_or(&label).to_owned(),
                text,
                label,
            })
        })
        .collect()
}

/// The offers that match what was typed, best first: those that start with it, then those
/// that hold its letters in order, each in the server's order.
pub fn narrowed<'a>(offers: &'a [Offer], typed: &str) -> Vec<(usize, &'a Offer)> {
    let typed = typed.to_lowercase();
    let mut found: Vec<(bool, usize, &Offer)> = offers
        .iter()
        .enumerate()
        .filter_map(|(at, offer)| {
            let filter = offer.filter.to_lowercase();
            if filter.starts_with(&typed) {
                Some((true, at, offer))
            } else {
                crate::quick::score(&typed, &filter).map(|_| (false, at, offer))
            }
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.sort.cmp(&b.2.sort)));
    found
        .into_iter()
        .take(SHOWN)
        .map(|(_, at, offer)| (at, offer))
        .collect()
}

/// Whether `c` is part of a name, for where the typed word starts.
pub fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn offers_are_read_and_narrowed_by_what_is_typed() {
        let answer = json!({"isIncomplete": false, "items": [
            {"label": "len", "detail": "fn(&self) -> usize", "sortText": "b"},
            {"label": "is_empty", "sortText": "a"},
            {"label": "push", "insertText": "push(${1:value})", "insertTextFormat": 2},
            {"label": "split_off", "textEdit": {"newText": "split_off", "range": {}}}
        ]});
        let found = offers(&answer);
        assert_eq!(found.len(), 4);
        assert!(found[2].snippet && found[2].text == "push(${1:value})");
        let labels = |typed| -> Vec<&str> {
            narrowed(&found, typed)
                .into_iter()
                .map(|(_, o)| o.label.as_str())
                .collect()
        };
        assert_eq!(labels("p"), ["push", "is_empty", "split_off"]);
        assert_eq!(labels("le"), ["len"]);
        assert_eq!(labels(""), ["is_empty", "len", "push", "split_off"]);
        assert!(offers(&Value::Null).is_empty());
    }
}

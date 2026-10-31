//! The Terminals card of the right panel: the commands the agent left running in the
//! background, each with its state and "Stop"; a click shows its output in the Terminal pane,
//! in a tab that follows the file the agent writes it to.

use crate::{Item, Pane, Session, Store, TerminalRequest, Ui};
use limn::{Component, Element};
use quill_components::pieces::{muted, text};
use quill_components::{icons, look};
use quill_model::Intent;
use std::path::PathBuf;

const OPEN: &str = "terminals:open:";
const STOP: &str = "terminals:stop:";

/// Where a command left running writes its output, as the CLI's answer to its call says
/// ("Output is being written to: <path>", Claude Code's words for a background Bash).
pub(crate) fn output_file(session: &Session, call: &str) -> Option<PathBuf> {
    session.items.iter().find_map(|item| match item {
        Item::Tool {
            call: c,
            result: Some(result),
            ..
        } if c == call => {
            let (_, rest) = result.output.split_once("Output is being written to: ")?;
            Some(PathBuf::from(rest.split_whitespace().next()?))
        }
        _ => None,
    })
}

/// The card's rows, when the session ran commands in the background.
pub(crate) fn rows(session: &Session, width: f32) -> Option<Vec<Element>> {
    let commands: Vec<_> = (session.tasks.iter().rev())
        .filter(|t| t.kind == "command")
        .collect();
    if commands.is_empty() {
        return None;
    }
    let palette = look::palette();
    let rows = commands.into_iter().map(|task| {
        let state: Element = if task.running {
            let stop = palette
                .interactive_surface(Element::clickable(
                    format!("{STOP}{}", task.id),
                    text("Stop").font_size(look::SMALL).padding(4.0),
                ))
                .radius(6.0);
            Element::row(vec![
                limn::StatusDot::new(look::running())
                    .pulse(true)
                    .size(6.0)
                    .render()
                    .centered_y(),
                stop.centered_y(),
            ])
            .gap(8.0)
        } else {
            let word = match (task.failed, task.stopped) {
                (_, true) => "Stopped",
                (true, _) => "Failed",
                _ => "Done",
            };
            muted(word).font_size(look::SMALL).centered_y()
        };
        let words = Element::row(vec![
            Element::icon(icons::of(icons::TERMINAL))
                .foreground(palette.text)
                .size(Some(16.0), Some(16.0))
                .centered_y(),
            text(task.description.clone())
                .font_size(look::SMALL)
                .size(Some(width - 120.0), None)
                .centered_y(),
        ])
        .gap(10.0);
        let words = palette
            .interactive_surface(Element::clickable(format!("{OPEN}{}", task.id), words))
            .radius(6.0);
        Element::row(vec![words, Element::spacer().flex(1.0), state])
            .gap(8.0)
            .size(None, Some(24.0))
    });
    Some(rows.collect())
}

pub(crate) fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    let session = store.session(ui.selected?)?;
    if let Some(task) = id.strip_prefix(STOP) {
        return Some(vec![Intent::StopTask {
            session: session.id,
            task: task.to_owned(),
        }]);
    }
    let task = id.strip_prefix(OPEN)?;
    let task = session.tasks.iter().find(|t| t.id == task)?;
    // Without its file (another agent's words), the tasks pane says what it is.
    match task
        .call
        .as_deref()
        .and_then(|call| output_file(session, call))
    {
        Some(path) => {
            ui.terminal.requests.push(TerminalRequest::Output {
                name: task.description.clone(),
                path,
            });
            ui.open_pane(Pane::Terminal);
        }
        None => ui.open_pane(Pane::Tasks),
    }
    Some(vec![])
}

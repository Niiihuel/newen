//! The Background tasks card of the right panel: what the agent left running beside its turns,
//! each with how long it has run and a square to stop it. A command's row shows its output in
//! the Terminal pane, in a tab that follows the file the agent writes it to; a helper agent's
//! opens its pane.

use crate::{Item, Pane, Session, Store, TerminalRequest, Ui};
use limn::{Component, Element};
use newen_components::pieces::{muted, text};
use newen_components::{icons, look};
use newen_model::Intent;
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

/// How long a task has run, as Desktop's rail writes it: "14h 35m", "9m 4s", "14s".
fn ran(secs: u64) -> String {
    match secs {
        3600.. => format!("{}h {}m", secs / 3600, secs % 3600 / 60),
        _ => crate::view::elapsed(secs),
    }
}

/// The Background tasks card, as Desktop's rail has it: each task still running, with how
/// long it has run and a square to stop it, and the finished ones as one row that opens the
/// Background tasks pane. `None` while the session has none.
pub(crate) fn rows(session: &Session, width: f32) -> Option<Vec<Element>> {
    if session.tasks.is_empty() {
        return None;
    }
    let palette = look::palette();
    let icon = |svg| {
        Element::icon(icons::of(svg))
            .foreground(palette.text)
            .size(Some(16.0), Some(16.0))
            .centered_y()
    };
    // Each row is a sidebar row (32 px, 10 px in, rounded, lit under the pointer), and its
    // Stop the composer's.
    let mut rows = vec![];
    for task in session.tasks.iter().filter(|t| t.running) {
        let open = format!("{OPEN}{}", task.id);
        let row = crate::view::side_row(
            open.clone(),
            vec![
                icon(match task.kind.as_str() {
                    "agent" => icons::AGENTS,
                    "command" => icons::TERMINAL,
                    _ => icons::LIST,
                }),
                // One line that fades out at its end: a long description never runs into
                // the time; the hint has it whole.
                // In the interface face as the panel's other rows: an element's own is mono.
                Element::faded_text(task.description.clone(), 24.0)
                    .font_family(limn::FontFamily::Sans)
                    .font_size(look::BODY)
                    .foreground(palette.text)
                    .size(None, Some(18.0))
                    .flex(1.0)
                    .centered_y(),
                muted(ran(task.since.elapsed().as_secs()))
                    .font_size(look::SMALL)
                    .centered_y(),
                crate::view::stop_button(&format!("{STOP}{}", task.id)).centered_y(),
            ],
            false,
            palette,
        )
        .size(Some(width), None);
        rows.push(hint(session, task, row, &open));
    }
    let finished = session.tasks.iter().filter(|t| !t.running).count();
    if finished > 0 {
        rows.push(
            crate::view::side_row(
                crate::view::TASKS_OPEN.into(),
                vec![
                    icon(icons::CHECK_CIRCLE),
                    text(format!("{finished} finished")).centered_y(),
                ],
                false,
                palette,
            )
            .size(Some(width), None),
        );
    }
    Some(rows)
}

/// A running task's hint: its whole description, the command it runs, and what kind it is,
/// how long it has run, what it used so far and the folder it runs in.
fn hint(session: &Session, task: &crate::Task, anchor: Element, id: &str) -> Element {
    let command = task.call.as_deref().and_then(|call| {
        session.items.iter().find_map(|item| match item {
            Item::Tool { call: c, input, .. } if c == call => input["command"].as_str(),
            _ => None,
        })
    });
    let kind = match task.kind.as_str() {
        "agent" => "Agent",
        "command" => "Command",
        "workflow" => "Workflow",
        "monitor" => "Monitor",
        _ => "Task",
    };
    let mut about = vec![
        kind.to_owned(),
        format!("running for {}", ran(task.since.elapsed().as_secs())),
    ];
    if let Some(usage) = task.usage.filter(|u| u.tokens > 0) {
        about.push(crate::view::tokens(usage.tokens));
    }
    limn::Tooltip::new(anchor, id, task.description.clone(), look::palette())
        // At the window's right edge it opens to the left, to stay whole.
        .placement(limn::TooltipPlacement::Left)
        .description(command.unwrap_or_default())
        .footer(format!(
            "{} \u{b7} {}",
            about.join(" \u{b7} "),
            session.cwd.display()
        ))
        .render()
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
    if let (Some(call), "agent") = (task.call.clone(), task.kind.as_str()) {
        let click = limn::UiAction::Clicked {
            id: format!("agent:{call}"),
        };
        return Some(crate::update(store, ui, &click));
    }
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

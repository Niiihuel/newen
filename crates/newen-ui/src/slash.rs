//! The "/" palette over the composer, as Desktop's slash menu. Its rows are Newen's own commands
//! that map to what Newen already does, then the agent's commands; a pick either inserts
//! `/name ` for the agent or runs the existing action.

use crate::{view, Session, Store, Ui};
use limn::{Component, Element, Key, SuggestionItem, SuggestionList, UiAction, UiEvent};
use newen_components::look;
use newen_model::Intent;
use std::ops::Range;

/// Most characters a query may have; a longer one keeps the palette shut.
const MAX_QUERY: usize = 50;
/// The rows as the list shows them: the id a pick reports, and the item.
type Row = (String, SuggestionItem);

/// One of Newen's own commands: its name and aliases, Desktop's words for it, and the action
/// the existing button or menu item reports.
struct Own {
    name: &'static str,
    aliases: &'static [&'static str],
    description: &'static str,
    action: String,
}

/// The commands whose argument the palette completes, as their menus list it: a pick writes
/// `/name ` and the rows are the options.
const TAKES_ARGUMENT: [&str; 3] = ["model", "effort", "permissions"];

/// The last whitespace-delimited token of `text`, as its byte index and the token.
fn last_token(text: &str) -> (usize, &str) {
    let start = text
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(at, c)| at + c.len_utf8());
    (start, &text[start..])
}

/// The `/` the caret is in, as its byte index and the query after it. The caret is taken to be
/// at the end of the prompt, since limn's composer does not report it. The `/` must start the
/// prompt or follow whitespace; the query has no whitespace and at most 50 characters.
pub fn slash_query(prompt: &str) -> Option<(usize, &str)> {
    let (start, token) = last_token(prompt);
    let query = token.strip_prefix('/')?;
    (query.chars().count() <= MAX_QUERY).then_some((start, query))
}

/// The argument the prompt ends with, `/<command> <query>` for one of [`TAKES_ARGUMENT`]: the
/// `/`'s byte index, the command and the query.
fn argument(prompt: &str) -> Option<(usize, &str, &str)> {
    let (start, query) = last_token(prompt);
    let (at, word) = last_token(prompt[..start].trim_end());
    let command = word.strip_prefix('/')?;
    (TAKES_ARGUMENT.contains(&command) && query.chars().count() <= MAX_QUERY)
        .then_some((at, command, query))
}

/// Desktop's rank: 0 exact name, 1 exact alias, 2 name prefix, 3 alias prefix, 4 a
/// substring of the name; `None` drops the command. Case does not matter. The range is the
/// part of the name the query matched, for the list to draw bold.
fn rank<S: AsRef<str>>(
    query: &str,
    name: &str,
    aliases: &[S],
) -> Option<(u8, Option<Range<usize>>)> {
    if query.is_empty() {
        return Some((0, None));
    }
    let (q, n) = (query.to_ascii_lowercase(), name.to_ascii_lowercase());
    if n == q {
        return Some((0, Some(0..name.len())));
    }
    if aliases
        .iter()
        .any(|a| a.as_ref().eq_ignore_ascii_case(query))
    {
        return Some((1, None));
    }
    if n.starts_with(&q) {
        return Some((2, Some(0..q.len())));
    }
    if aliases
        .iter()
        .any(|a| a.as_ref().to_ascii_lowercase().starts_with(&q))
    {
        return Some((3, None));
    }
    n.find(&q).map(|at| (4, Some(at..at + q.len())))
}

/// Newen's own commands whose action exists now: the ones that need a session only show once
/// one is selected, and plan only when the agent offers that mode.
fn own_commands(store: &Store, session: Option<&Session>, ui: &Ui) -> Vec<Own> {
    let own = |name, aliases, description, action: &str| Own {
        name,
        aliases,
        description,
        action: action.to_owned(),
    };
    // Their picks complete the argument instead of opening the menu (`argument:`).
    let mut commands = vec![
        own(
            "model",
            &[],
            "Set the model for this session",
            "argument:model",
        ),
        own(
            "effort",
            &[],
            "Set the effort level for this session",
            "argument:effort",
        ),
        own(
            "permissions",
            &["allowed-tools"],
            "Change the permission mode",
            "argument:permissions",
        ),
    ];
    if view::offered_modes(store, session, ui)
        .iter()
        .any(|m| m == "plan")
    {
        commands.push(own("plan", &[], "Switch to plan mode", "mode:plan"));
    }
    commands.extend([
        own("mcp", &[], "Manage MCP connectors", view::CONNECTORS_MANAGE),
        own(
            "plugin",
            &["plugins", "marketplace"],
            "Manage plugins",
            view::PLUGINS_MANAGE,
        ),
        own(
            "skills",
            &[],
            "Browse and manage your skills",
            view::CUSTOMIZE_OPEN,
        ),
        own(
            "config",
            &["settings"],
            "Open settings",
            view::SETTINGS_OPEN,
        ),
        own(
            "storage",
            &[],
            "See and free what Newen keeps on this computer",
            crate::storage::OPEN,
        ),
        own(
            "usage",
            &["stats", "cost"],
            "Show plan usage and rate limits",
            view::USAGE_MENU,
        ),
    ]);
    if session.is_some_and(crate::remote::offered) {
        commands.push(own(
            "remote-control",
            &["rc"],
            "Continue this session from your phone or a browser",
            crate::remote::COMMAND,
        ));
    }
    if let Some(id) = ui.selected {
        commands.extend([
            own("tasks", &[], "Show this session's tasks", view::TASKS_OPEN),
            own(
                "diff",
                &[],
                "Show this session's changes",
                view::DIFF_TOGGLE,
            ),
            Own {
                name: "fork",
                aliases: &[],
                description: "Fork this session",
                action: format!("fork:{id}"),
            },
            Own {
                name: "rename",
                aliases: &[],
                description: "Rename this session",
                action: format!("rename:{id}"),
            },
        ]);
    }
    commands
}

/// The rows for `query`, in Desktop's order: Newen's commands, then the agent's, each kept in
/// that order among equal ranks.
fn rows(store: &Store, ui: &Ui, query: &str) -> Vec<Row> {
    let session = ui.selected.and_then(|id| store.session(id));
    let agent = session
        .and_then(|s| s.capabilities.as_ref())
        .map(|c| c.commands.as_slice())
        .unwrap_or_default();
    let own = own_commands(store, session, ui);
    let mut found: Vec<(u8, Row)> = Vec::new();
    for command in &own {
        if let Some((rank, matched)) = rank(query, command.name, command.aliases) {
            let id = format!("slash:newen:{}", command.action);
            let item = SuggestionItem::new(id.clone(), command.name)
                .description(command.description)
                .matched(matched.into_iter().collect());
            found.push((rank, (id, item)));
        }
    }
    for command in agent {
        if let Some((rank, matched)) = rank(query, &command.name, command.aliases.as_slice()) {
            // A name Newen also has goes to the agent, and says so; Desktop's rows show no
            // argument hints.
            let custom = own.iter().any(|o| o.name == command.name);
            let id = format!("slash:agent:{}", command.name);
            let detail = if custom { "Custom command" } else { "" };
            let item = SuggestionItem::new(id.clone(), command.name.as_str())
                .description(command.description.as_str())
                .detail(detail)
                .matched(matched.into_iter().collect());
            found.push((rank, (id, item)));
        }
    }
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, row)| row).collect()
}

/// The options of the argument `command` that match `query`, as its menu lists them: the
/// models, the effort levels or the permission modes. Each row's id is the id of the menu item
/// a click on it reports, so a pick does what the click does.
fn options(store: &Store, ui: &Ui, command: &str, query: &str) -> Vec<Row> {
    let session = ui.selected.and_then(|id| store.session(id));
    // The menu item's id, the value the query matches, the label, the description, and whether
    // it is the one in use.
    let options: Vec<(String, String, String, String, bool)> = match command {
        "model" => {
            let current = view::current_model(store, session, ui).map(|m| m.id.clone());
            let (latest, more) = view::split_models(view::offered_models(store, session, ui));
            latest
                .into_iter()
                .chain(more)
                .map(|m| {
                    let in_use = current.as_ref() == Some(&m.id);
                    (
                        format!("model:{}", m.id),
                        m.id.clone(),
                        m.name.clone(),
                        m.provider.clone().unwrap_or_default(),
                        in_use,
                    )
                })
                .collect()
        }
        "effort" => view::efforts(store, ui)
            .into_iter()
            .map(|e| {
                let about = if e == "medium" { "Recommended" } else { "" };
                let in_use = ui.effort.as_ref() == Some(&e);
                (
                    format!("effort:{e}"),
                    e.clone(),
                    view::effort_label(&e),
                    about.to_owned(),
                    in_use,
                )
            })
            .collect(),
        _ => {
            let current = session.map_or(ui.mode.as_str(), |s| s.mode.as_str());
            view::menu_modes(store, session, ui)
                .into_iter()
                .filter_map(|m| {
                    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
                    let words = view::mode_entry(ui, agent, &m)?;
                    let in_use = m == current;
                    Some((
                        format!("mode:{m}"),
                        m.clone(),
                        words.label.clone(),
                        words.about.clone(),
                        in_use,
                    ))
                })
                .collect()
        }
    };
    let mut found: Vec<(u8, Row)> = Vec::new();
    for (click, value, label, about, in_use) in options {
        if let Some((rank, matched)) = rank(query, &label, &[value]) {
            let id = format!("slash:option:{click}");
            let item = SuggestionItem::new(id.clone(), label)
                .description(about)
                .detail(if in_use { "current" } else { "" })
                .matched(matched.into_iter().collect());
            found.push((rank, (id, item)));
        }
    }
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, row)| row).collect()
}

/// The palette while it is open: the `/`'s byte index and its rows. In argument mode the rows
/// are the command's options, and an empty list shuts it. Shut when the `/` was dismissed, or
/// when nothing matches.
pub(crate) fn open(store: &Store, ui: &Ui) -> Option<(usize, Vec<Row>)> {
    let (at, rows) = match argument(&ui.prompt) {
        Some((at, command, query)) => (at, options(store, ui, command, query)),
        None => {
            let (at, query) = slash_query(&ui.prompt)?;
            (at, rows(store, ui, query))
        }
    };
    if ui.slash_dismissed == Some(at) {
        return None;
    }
    (!rows.is_empty()).then_some((at, rows))
}

/// The title of an argument's list: its command's name and description.
fn argument_title(store: &Store, ui: &Ui) -> Option<String> {
    let (_, command, _) = argument(&ui.prompt)?;
    let session = ui.selected.and_then(|id| store.session(id));
    own_commands(store, session, ui)
        .into_iter()
        .find(|own| own.name == command)
        .map(|own| format!("{} — {}", own.name, own.description))
}

/// The prompt without the `/` token it ends with: where a menu the palette opened returns it.
pub(crate) fn without_token(prompt: &str) -> String {
    prompt[..last_token(prompt).0].trim_end().to_owned()
}

/// The composer `anchor` with the palette above it while it is open. Rows and width are
/// Desktop's: compact, about 320 px, in the
/// composer's text size.
pub fn list(store: &Store, ui: &Ui, anchor: Element, width: f32) -> Element {
    let Some((_, rows)) = open(store, ui) else {
        return anchor;
    };
    let selected = ui.slash_selected.min(rows.len() - 1);
    let items = rows.into_iter().map(|(_, item)| item).collect();
    let mut list = SuggestionList::new(
        anchor,
        "slash",
        items,
        selected,
        true,
        "slash:dismiss",
        look::suggestion_palette(),
    )
    .compact(true)
    .sizing(newen_components::pieces::menu_sizing(320.0))
    .max_width(320.0)
    .width(width)
    .placement(limn::PopoverPlacement::TopStart);
    // An argument's list is titled by its command; until the session runs, the agent's
    // commands are not known.
    if let Some(title) = argument_title(store, ui) {
        list = list.title(title);
    } else if ui
        .selected
        .and_then(|id| store.session(id))
        .is_none_or(|s| s.capabilities.is_none())
    {
        list = list.title("More commands available once the session is running");
    }
    list.render()
}

/// A key the palette takes before limn while it is open over the focused composer: the arrows
/// move (no wrap), Enter and Tab pick, Escape shuts it for this `/`. `None` otherwise, so
/// Enter sends and the rest reaches the composer.
pub fn key(
    store: &Store,
    ui: &mut Ui,
    focused: Option<&str>,
    event: &UiEvent,
) -> Option<Vec<Intent>> {
    if focused != Some(view::COMPOSER) {
        return None;
    }
    let UiEvent::Key {
        key,
        ctrl: false,
        shift,
        ..
    } = event
    else {
        return None;
    };
    let (at, rows) = open(store, ui)?;
    let last = rows.len() - 1;
    let selected = ui.slash_selected.min(last);
    match key {
        Key::ArrowDown => ui.slash_selected = (selected + 1).min(last),
        Key::ArrowUp => ui.slash_selected = selected.saturating_sub(1),
        Key::Enter if !shift => return Some(pick(store, ui, at, &rows[selected].0)),
        Key::Tab => return Some(pick(store, ui, at, &rows[selected].0)),
        Key::Escape => ui.slash_dismissed = Some(at),
        _ => return None,
    }
    Some(vec![])
}

/// The palette's clicks: a row picked, or a press outside it, which shuts it for this `/`.
pub fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if id == "slash:dismiss" {
        ui.slash_dismissed = slash_query(&ui.prompt).map(|(at, _)| at);
        return Some(vec![]);
    }
    let row = ["slash:agent:", "slash:newen:", "slash:option:"]
        .iter()
        .any(|prefix| id.starts_with(prefix));
    let (at, _) = open(store, ui)?;
    row.then(|| pick(store, ui, at, id))
}

/// Picks row `id` for the `/` at `at`, in place of its token. An agent command, or a command
/// that takes an argument, writes `/name ` for the agent or the options to read. An option or
/// an action leaves the prompt without the token and runs what its button or menu item would.
/// A menu with nothing to choose opens over the composer, which keeps out of its way until the
/// menu closes and then gets the prompt back.
fn pick(store: &Store, ui: &mut Ui, at: usize, id: &str) -> Vec<Intent> {
    let head = ui.prompt[..at].to_owned();
    ui.slash_selected = 0;
    ui.slash_dismissed = None;
    let name = id
        .strip_prefix("slash:agent:")
        .or_else(|| id.strip_prefix("slash:newen:argument:"));
    if let Some(name) = name {
        ui.restore_prompt = Some(format!("{head}/{name} "));
        return vec![];
    }
    let Some(click) = id
        .strip_prefix("slash:option:")
        .or_else(|| id.strip_prefix("slash:newen:"))
    else {
        return vec![];
    };
    let intents = crate::update(
        store,
        ui,
        &UiAction::Clicked {
            id: click.to_owned(),
        },
    );
    if ui.open_menu.is_some() {
        ui.slash_dismissed = Some(at);
        ui.menu_from_palette = true;
    } else {
        ui.restore_prompt = Some(head.trim_end().to_owned());
    }
    intents
}

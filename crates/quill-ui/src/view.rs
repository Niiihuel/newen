//! The window: sidebar of sessions, transcript and composer (features/code-*.md, docs/ui.md §2).

use crate::annotate;
use crate::icons;
use crate::look::{self, chat, BODY, SMALL};
use crate::{
    Item, Pane, Session, Store, ToolResult, Ui, NOTICE_DONE, NOTICE_PERMISSION, NOTICE_QUESTION,
};
use limn::{
    Attachment, Avatar, AvatarGroup, Button, ButtonGroup, ButtonSize, ButtonVariant, Color,
    Component, ComponentPalette, Composer, ControlSize, Element, FontFamily, Markdown, OutputBlock,
    Size, Skeleton, Sketch, Spinner, TextShimmer,
};
use quill_components::pieces::{
    confirm, confirm_keeping, hgap, hline, icon_tool, keyed_button, menu, muted, pane_header,
    pane_tab, tabs_header, text, toggle, tool_surface, vline,
};
use quill_components::settings::{
    self, choices, setting, setting_button, setting_switch, Setting, SettingsPage,
};
use quill_model::{
    ChangeKind, FileEffect, NotifyLevel, QueuedMessage, SessionStatus, TranscriptView, WebSource,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A square image tile, 85 px: in the composer and in a message (docs/ui.md §2.6).
const IMAGE_TILE: f32 = 85.0;
/// A generated image's tile, wider than a message's: it is what the turn made.
const GENERATED_TILE: f32 = 168.0;
/// The pictures at a step row's end, as Desktop's: 24 px tall and as wide as their shape up
/// to 44, the last three of them.
const THUMB_HEIGHT: f32 = 24.0;
const THUMB_WIDTH: f32 = 44.0;
const THUMBS_SHOWN: usize = 3;
/// The card a thumbnail shows under the pointer: 320 px wide, its picture at most 240 tall.
const PREVIEW_WIDTH: f32 = 320.0;
const PREVIEW_HEIGHT: f32 = 240.0;
/// A web search source card, tall (websearch-spec (a)).
const CARD_HEIGHT: f32 = 84.0;

pub const COMPOSER: &str = "composer";
pub const NEW_SESSION: &str = "new-session";
pub const STOP: &str = "stop";
pub const REWIND_UNDO: &str = "rewind:undo";
pub const MODE_MENU: &str = "mode-menu";
pub const MODEL_MENU: &str = "model-menu";
pub const DIFF_TOGGLE: &str = "diff:toggle";
/// The Changes card's counts: they show the diff, as the top bar's button does, and are a
/// control of their own, heard as the figures they show.
pub const CHANGES_ROW: &str = "panel:changes:open";
/// "open-file:<line>:<path>": a file the agent changed, to show in the editor at that line;
/// a path from the session's folder or absolute.
/// A changed file of the diff: folded away, opened in the Files pane at a line
/// (`{line}:{path}`), its Open in menu, and a place from it (`{place}:{line}:{path}`).
pub const DIFF_FOLD: &str = "diff:fold:";
pub const OPEN_HERE: &str = "doc:open-at:";
/// A link in the conversation, `{LINK}{scope}:{offset}:{url}` as limn's Markdown writes it,
/// its right-click menu, and what the menu does with its address.
pub const LINK: &str = "link:";
/// The composer's "+": its Connectors (the session's MCP servers, each switched by
/// `{CONNECTOR_SWITCH}{on}:{name}`) and its Plugins, found and managed.
pub const CONNECTORS_BROWSE: &str = "plus:connectors:browse";
pub const CONNECTORS_MANAGE: &str = "plus:connectors:manage";
pub const CONNECTOR_SWITCH: &str = "plus:connector:";
pub const PLUGINS_BROWSE: &str = "plus:plugins:browse";
pub const PLUGINS_MANAGE: &str = "plus:plugins:manage";
pub const PLUGIN_SKILL: &str = "plus:skill:";
pub const LINK_MENU: &str = "link-menu";
pub const LINK_BROWSER: &str = "link-browser:";
pub const LINK_EXTERNAL: &str = "link-external:";
pub const LINK_COPY: &str = "link-copy:";

/// The address of a link's action, `{LINK}{scope}:{offset}:{url}`.
pub fn link_url(id: &str) -> Option<&str> {
    let mut parts = id.strip_prefix(LINK)?.splitn(3, ':');
    parts.next()?;
    parts.next()?;
    parts.next()
}
pub const OPEN_FILE_MENU: &str = "open-file-menu:";
pub const OPEN_FILE_IN: &str = "open-file-in:";
/// The top bar's "Open in" menu, of the folder on screen.
pub const OPEN_IN_MENU: &str = "open-in-menu";
/// quill's own editor, offered whatever else is installed.
pub const QUILL_IDE: &str = "quill-ide";
pub const DIFF_CLOSE: &str = "pane:diff:close";
pub const PANEL_TOGGLE: &str = "panel:toggle";
/// Asks the host for the platform folder picker; the host sets `Ui::cwd` with the answer.
pub const PICK_FOLDER: &str = "pick-folder";
pub const PLUS_MENU: &str = "plus-menu";
pub const FOLDER_MENU: &str = "folder-menu";
pub const AGENT_MENU: &str = "agent-menu";
pub const SEND: &str = "send";
pub const USAGE_MENU: &str = "usage-menu";
pub const EFFORT_MENU: &str = "effort-menu";
/// "See all →": the plan's usage page, which the host opens in the browser.
pub const USAGE_ALL: &str = "usage:all";
pub const SCROLL_END: &str = "scroll-end";
/// The prefix of a code block's copy button; the code follows it.
pub const COPY_CODE: &str = "copy-code:";
/// "+" menu actions the host carries out: pickers for @-mentions, and the slash.
pub const ATTACH_FILES: &str = "attach:files";
/// The image viewer's close control (also Escape and a click on the backdrop), and its annotate
/// control, which only a composer image has.
pub const IMAGE_CLOSE: &str = "image:close";
pub const IMAGE_ANNOTATE: &str = "image:annotate";
/// The viewer's arrows, through the pictures of a run of steps.
pub const IMAGE_PREVIOUS: &str = "image:previous";
pub const IMAGE_NEXT: &str = "image:next";
/// The prefix of the thumbnails under the viewer's picture; the index in its run follows it.
pub const IMAGE_AT: &str = "image:at:";
/// The viewer's zoom toggle; the viewer zooms itself and reports the scale.
pub const IMAGE_ZOOM: &str = "image:zoom";
pub const ATTACH_FOLDER: &str = "attach:folder";
pub const SLASH: &str = "attach:slash";
pub const TERMINAL_TOGGLE: &str = "terminal:toggle";
/// The terminal pane; while it has the focus the host sends keys to the shell.
pub const TERMINAL: &str = "terminal";
/// The terminal's scroll area, which follows new output while it is at the end.
pub const TERMINAL_SCROLL: &str = "terminal:scroll";
pub const TERMINAL_NEW: &str = "terminal:new";
/// The field a tab's pill turns into while the person renames it.
pub const TERMINAL_RENAME: &str = "terminal:rename";
pub const TERMINAL_CLOSE_CONFIRM: &str = "terminal:close-confirm";
pub const TERMINAL_CLOSE_CANCEL: &str = "terminal:close-cancel";
pub const SIDEBAR_TOGGLE: &str = "sidebar:toggle";
pub const SEARCH: &str = "search";
pub const SEARCH_FIELD: &str = "search:field";
pub const SEARCH_CLOSE: &str = "search:close";
pub const SEARCH_NEW: &str = "search:new";
/// Ctrl+O: the selected session's transcript view goes to the next one.
pub const TRANSCRIPT_VIEW_CYCLE: &str = "transcript-view:cycle";
/// The chevron beside the session's title and the menu it opens.
pub const TITLE_MENU: &str = "title-menu";
/// The field the title turns into while the person renames the session.
pub const TITLE_FIELD: &str = "title:field";
/// The branch bar's pull request, the menu that picks its kind, and its close button.
pub const PR_CREATE: &str = "pr:create";
/// The branch bar's counts, which open the diff of what is uncommitted.
pub const VIEW_CHANGES: &str = "branch-bar:changes";
pub const PR_MENU: &str = "pr-menu";
pub const BAR_CLOSE: &str = "branch-bar:close";
/// The queue bar's expand and collapse, its "Clear all", and the prefix of each row's "⋮" menu.
pub const QUEUE_TOGGLE: &str = "queue:toggle";
pub const QUEUE_CLEAR: &str = "queue:clear";
pub const QUEUE_MENU: &str = "queue:menu:";
pub const FILTER_RESET: &str = "filter:reset";
/// The reply field of a side chat's pane.
pub const SIDE_FIELD: &str = "side:field";
/// The side pane's tool that shows its session on its own.
pub const SIDE_OPEN: &str = "side:open";
/// Settings: its gear and Ctrl+,, its close, its search, the page it opens on, and the
/// data folder's "Open".
pub use quill_components::settings::{SETTINGS_CLOSE, SETTINGS_OPEN, SETTINGS_SEARCH};
pub const SETTINGS_FIRST: &str = "general";
pub const SETTINGS_STORAGE_OPEN: &str = "settings:storage:open";
/// The sidebar's Customize, and the person's button at its foot with its menu.
pub const CUSTOMIZE_OPEN: &str = "customize:open";
pub const ACCOUNT_MENU: &str = "account:menu";
/// Customize's pages: each agent's skills and plugins.
pub const SETTINGS_SKILLS: &str = "skills";
/// Settings' Accounts page: GitHub and each agent's sign-in and usage.
pub const SETTINGS_ACCOUNTS: &str = "accounts";
pub const SETTINGS_PLUGINS: &str = "plugins";
pub const SETTINGS_CONNECTORS: &str = "connectors";
/// Connectors' search of the MCP registry, and "Add custom connector"'s fields
/// (`{SRV_FIELD}name`, `target`, `env`, `headers`).
pub const SRV_SEARCH: &str = "srv-search";
pub const SRV_FIELD: &str = "srv-field:";
/// The fields of the dialogs that make or import a skill for "All agents".
pub const GSK_FIELD: &str = "gsk-field:";
/// Customize's question before removing, and the marketplace's field (`{id}{agent}`).
pub const EXT_REMOVE_YES: &str = "ext-remove:yes";
pub const EXT_REMOVE_NO: &str = "ext-remove:no";
pub const EXT_MKT_FIELD: &str = "ext-mkt-field:";
/// Customize's page search and Discover's.
pub const CZ_SEARCH: &str = "cz-search";
pub const CZ_DIR_SEARCH: &str = "cz-dir-search";
/// The session's MCP servers' dialog: closing it, and a server's actions (`{id}{name}`).
pub const MCP_DONE: &str = "mcp:done";
pub const MCP_RECONNECT: &str = "mcp:reconnect:";
pub const MCP_TURN_ON: &str = "mcp:on:";
pub const MCP_CONNECT: &str = "mcp:connect:";
pub const MCP_CLEAR: &str = "mcp:clear:";
pub const MCP_CLEAR_YES: &str = "mcp:clear-yes";
pub const MCP_CLEAR_NO: &str = "mcp:clear-no";
pub const MCP_ROW: &str = "mcp:row:";
pub const MCP_BACK: &str = "mcp:back";
/// The Files pane's buttons over the file on screen.
pub const DOC_PREVIEW: &str = "doc:preview";
pub const DOC_SAVE: &str = "doc:save";
/// "Show more" under Completed.
pub const SHOW_MORE: &str = "sidebar:show-more";
/// The effort slider, its width, and the way its thumb travels.
pub const EFFORT_SLIDER: &str = "effort:slider";
const EFFORT_WIDTH: f32 = 192.0;
/// The stops and the thumb's middle sit 10 px in, so the 20 px thumb at an end meets the
/// track's end, flush, its corners on the track's, as the person asked;
/// Desktop's dots sit 13.5 px in.
const EFFORT_INSET: f32 = 10.0;
pub const EFFORT_TRAVEL: f32 = EFFORT_WIDTH - 2.0 * EFFORT_INSET;
pub const DELETE_CONFIRM: &str = "delete:confirm";
pub const DELETE_CANCEL: &str = "delete:cancel";
const TERMINAL_FONT: f32 = 12.0;
/// The shell's text starts 12 px in and 8 px under the pane's header.
const TERMINAL_PADDING: [f32; 2] = [12.0, 8.0];

/// The last part of a path, as Desktop names a session's folder.
pub(crate) fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// How the space right of the sidebar is shared.
struct Layout {
    /// The conversation; zero while a pane is expanded over it.
    main: f32,
    /// The column of panes, laid out `column` wide and drawn `shown` wide as it slides, and
    /// the panes in it from the top.
    column: f32,
    shown: f32,
    panes: Vec<Pane>,
    /// The right panel, while no pane is open or as it gives way to them.
    panel: f32,
}

fn layout(store: &Store, ui: &Ui, window: Size) -> Layout {
    let session = ui.selected.and_then(|id| store.session(id));
    let rest = (window.width - sidebar_width(ui)).max(0.0);
    // While the column shuts it still draws the pane closed last.
    let open = if ui.panes.is_empty() {
        ui.closing.as_slice()
    } else {
        &ui.panes
    };
    let mut open = open.to_vec();
    // A stacked pane that closed stays in its place while the stack closes over it.
    if let Some((at, pane)) = ui.stack_closing.filter(|_| open.len() == 1) {
        open.insert(at.min(open.len()), pane);
    }
    let mut panes: Vec<Pane> = open
        .into_iter()
        .filter(|p| *p != Pane::Diff || diff_files(store, ui).is_some())
        // A browser belongs to a session.
        .filter(|p| *p != Pane::Browser || session.is_some())
        .collect();
    let now = Instant::now();
    // An expanded pane grows over the conversation, and shrinks back, on a spring.
    let expanded = ui.expand_shown.value(now).clamp(0.0, 1.0);
    if let Some(pane) = ui.expanded_pane.filter(|p| panes.contains(p)) {
        panes = vec![pane];
    }
    // The widest pane open sets the column; a dragged boundary sets it instead, as in
    // Desktop. It moves on the spring between the two (`Ui::tick`).
    let share = match ui.column_share.target() {
        target if target > 0.0 => ui.column_share.value(now),
        _ => ui
            .column_fraction
            .unwrap_or_else(|| panes.iter().map(|p| p.share()).fold(0.0, f32::max)),
    };
    let column = if panes.is_empty() {
        0.0
    } else {
        let at_rest = column_width(rest, share);
        (at_rest + (rest - at_rest) * expanded).round()
    };
    let shown = (column * ui.column_shown.value(now).clamp(0.0, 1.0)).round();
    let panel = if session.is_some() && rest - look::PANEL_WIDTH >= look::column() {
        (ui.panel_shown.value(now) * (1.0 - expanded)).round()
    } else {
        0.0
    };
    Layout {
        main: (rest - shown - panel).max(0.0),
        column,
        shown,
        panes,
        panel,
    }
}

/// The narrowest the conversation and the column of panes get when the boundary is dragged.
pub const MIN_CHAT: f32 = 360.0;
pub const MIN_COLUMN: f32 = 280.0;
pub const MIN_PANE: f32 = 120.0;
/// The boundaries the person drags: between the conversation and the panes, and between
/// two stacked panes. Each is 2 px, Desktop's hairline plus the blue it turns under the
/// pointer.
pub const COLUMN_HANDLE: &str = "split:column";
pub const STACK_HANDLE: &str = "split:stack";
/// The built-in browser (docs/browser.md): its toggle, the page's input surface, the
/// address bar, and the toolbar's buttons; tabs are "browser:tab:{id}" and their close
/// "browser:close:{id}".
pub const BROWSER_TOGGLE: &str = "browser:toggle";
/// The Background tasks pane: its opening from the running line, and its trash.
pub const TASKS_OPEN: &str = "tasks:open";
pub const TASKS_TOGGLE: &str = "tasks:toggle";
pub const TASKS_CLEAR: &str = "tasks:clear";
/// Before a group's key: the chevron at the end of an open group, which folds it.
pub const GROUP_FOLD: &str = "fold:";
/// The Files pane: the session's folder as a tree, and the tree's filter.
pub const FILES_TOGGLE: &str = "files:toggle";
pub use crate::files::{FILTER_CLEAR, FILTER_FIELD};
pub const BROWSER_VIEW: &str = "browser:view";
pub const BROWSER_URL: &str = "browser:url";
pub const BROWSER_NEW_TAB: &str = "browser:new";
pub const BROWSER_BACK: &str = "browser:back";
pub const BROWSER_FORWARD: &str = "browser:forward";
pub const BROWSER_RELOAD: &str = "browser:reload";
pub const BROWSER_STOP: &str = "browser:stop";
pub const BROWSER_SELECT: &str = "browser:select";
pub const BROWSER_VIEWPORT: &str = "browser:viewport";
/// Settings › Browser's "Choose…": the host asks for the Chromium to run.
pub const BROWSER_PROGRAM_CHOOSE: &str = "settings:browser:program";
/// Desktop's Viewport presets: Responsive is the pane's own size.
pub const VIEWPORTS: [(&str, Option<(u32, u32)>); 3] = [
    ("Responsive", None),
    ("Mobile", Some((375, 812))),
    ("Tablet", Some((768, 1024))),
];
pub const BROWSER_NOTICE_CLOSE: &str = "browser:notice:close";

/// The surface a session's tab is drawn on; the host presents its frames there.
pub fn browser_surface(session: quill_model::SessionId, tab: u32) -> String {
    format!("browser:{session}:{tab}")
}
/// The right panel's left edge: dragged it sets the panel's width, clicked it hides the
/// panel, as Desktop's side column does ("Click to collapse", "Drag to resize").
pub const PANEL_HANDLE: &str = "split:panel";
/// The widest the right panel gets; inferred, Desktop's column has no fixed limit but the
/// conversation's.
const MAX_PANEL: f32 = 560.0;
const HANDLE: f32 = 2.0;

/// The column's width for a share of the space right of the sidebar, within the minimums.
/// The sidebar's width with its border as it is drawn now: nothing while it is hidden, less
/// than all of it while it slides.
pub fn sidebar_width(ui: &Ui) -> f32 {
    ui.sidebar.docked_width()
}

/// The right panel's width for `wanted`: no narrower than its cards are drawn for, and
/// leaving the conversation its minimum.
pub fn panel_width(rest: f32, wanted: f32) -> f32 {
    wanted
        .min(MAX_PANEL)
        .min(rest - MIN_CHAT)
        .max(look::PANEL_WIDTH)
        .round()
}

pub fn column_width(rest: f32, fraction: f32) -> f32 {
    (rest * fraction).round().clamp(
        MIN_COLUMN.min(rest),
        (rest - MIN_CHAT).max(MIN_COLUMN.min(rest)),
    )
}

/// The heights of the panes in the column, top first, as they rest and as they are drawn
/// now: one fills it; two share it, evenly until the person drags their boundary, the lower
/// opening and closing on a spring (`Ui::stack_shown`).
fn pane_heights(layout: &Layout, ui: &Ui, window: Size) -> Vec<(f32, f32)> {
    let below = window.height - look::TOP_BAR - 1.0;
    if layout.panes.len() < 2 {
        return vec![(below, below)];
    }
    let room = below - HANDLE;
    let top = (room * ui.stack_fraction.unwrap_or(0.5))
        .round()
        .clamp(MIN_PANE, (room - MIN_PANE).max(MIN_PANE));
    let open = ui.stack_shown.value(Instant::now()).clamp(0.0, 1.0);
    let top_now = (top + (room - top) * (1.0 - open)).round();
    vec![(top, top_now), (room - top, room - top_now)]
}

/// A boundary: Desktop's hairline at rest, its blue under the pointer and while dragged.
fn divider(id: &str, across: bool, length: f32, palette: ComponentPalette) -> Element {
    let blue = ComponentPalette {
        hover: look::handle(),
        selected: look::handle(),
        ..palette
    };
    if across {
        let line = Element::row(vec![vline(length), hgap(HANDLE - 1.0)]);
        let handle = limn::ResizeHandle::vertical(id, blue)
            .thickness(HANDLE)
            .render();
        Element::overlay(line, handle).size(Some(HANDLE), Some(length))
    } else {
        let line = Element::column(vec![
            hline(length, look::pane_border()),
            Element::spacer().size(None, Some(HANDLE - 1.0)),
        ]);
        let handle = limn::ResizeHandle::horizontal(id, blue)
            .thickness(HANDLE)
            .render();
        Element::overlay(line, handle).size(Some(length), Some(HANDLE))
    }
}

/// The panes' width: the column less the boundary beside the conversation.
fn pane_width(layout: &Layout) -> f32 {
    if layout.main > 0.0 {
        layout.column - HANDLE
    } else {
        layout.column
    }
}

/// The files of the request the diff pane shows.
fn diff_files<'a>(store: &'a Store, ui: &Ui) -> Option<&'a Vec<FileEffect>> {
    let session = store.session(ui.selected?)?;
    // Without a request, the pane shows what is uncommitted, as Desktop's does.
    let Some((id, request)) = ui.diff.as_ref() else {
        return session
            .working
            .as_ref()
            .filter(|_| ui.pane_open(Pane::Diff) || ui.closing == Some(Pane::Diff));
    };
    let session = Some(session).filter(|s| s.id == *id)?;
    session.items.iter().find_map(|i| match i {
        Item::Changes {
            request: r, files, ..
        } if r == request => Some(files),
        _ => None,
    })
}

/// The columns and rows the terminal pane holds in `window`, for the host's shell.
pub fn terminal_grid(store: &Store, ui: &Ui, window: Size) -> (u16, u16) {
    let layout = layout(store, ui, window);
    let at = layout
        .panes
        .iter()
        .position(|p| *p == Pane::Terminal)
        .unwrap_or(0);
    // The height it rests at: the shell is not resized frame by frame as the stack moves.
    let height = pane_heights(&layout, ui, window)[at.min(layout.panes.len().saturating_sub(1))].0;
    let inner = Size::new(
        pane_width(&layout) - 2.0 * TERMINAL_PADDING[0],
        height - look::PANE_HEADER - 1.0 - 2.0 * TERMINAL_PADDING[1],
    );
    let (cols, rows) = limn::TerminalView::grid_size(inner, TERMINAL_FONT);
    (cols as u16, rows as u16)
}

/// `child` with `x` of space on its sides and `y` above and below it.
fn pad(child: Element, x: f32, y: f32) -> Element {
    let gap = |w: Option<f32>, h: Option<f32>| Element::spacer().size(w, h);
    Element::column(vec![
        gap(None, Some(y)),
        Element::row(vec![
            gap(Some(x), None),
            child.flex(1.0),
            gap(Some(x), None),
        ])
        .flex(1.0),
        gap(None, Some(y)),
    ])
}

pub fn view(store: &Store, ui: &Ui, window: Size) -> Element {
    if crate::welcoming(ui) {
        return crate::welcome::view(ui, window);
    }
    if crate::appearance::looking(ui) {
        return crate::appearance::view(ui, window);
    }
    if crate::account::visible(ui) {
        return crate::account::screen(ui, window);
    }
    let palette = look::palette();
    let session = ui.selected.and_then(|id| store.session(id));
    let layout = layout(store, ui, window);
    let rest = layout.main + layout.shown + layout.panel;
    let below = window.height - look::TOP_BAR - 1.0;
    let mut row = vec![];
    if layout.main > 0.0 {
        let column = look::column().min(layout.main - 64.0).max(240.0);
        let body = match session {
            Some(session) => transcript(store, session, ui, column, palette),
            None => empty(ui),
        };
        row.push(
            Element::column(vec![
                body.flex(1.0),
                composer(store, session, ui, column, palette).centered_x(),
                Element::spacer().size(None, Some(12.0)),
            ])
            .size(Some(layout.main), Some(below)),
        );
    }
    if !layout.panes.is_empty() && layout.shown > 0.0 {
        let heights = pane_heights(&layout, ui, window);
        let width = pane_width(&layout);
        let mut stack = vec![];
        for (i, (pane, (height, now))) in layout.panes.iter().zip(heights).enumerate() {
            if i > 0 {
                stack.push(divider(STACK_HANDLE, false, width, palette));
            }
            // The upper pane is drawn at the height it has now, as when its boundary is
            // dragged; the lower one at the height it rests at, cut to what has opened.
            let height = if i == 0 { now } else { height };
            let drawn = match pane {
                Pane::Diff => diff_pane(store, ui, width, height, palette),
                Pane::Terminal => terminal_pane(session, ui, width, height, palette),
                Pane::Agent => agent_pane(store, session, ui, width, height, palette),
                Pane::Plan => plan_pane(session, ui, width, height),
                Pane::Side => side_pane(store, ui, width, height, palette),
                Pane::Browser => browser_pane(session, ui, width, height, palette),
                Pane::Files => files_pane(ui, width, height, palette),
                Pane::Tasks => tasks_pane(session, ui, width, height, palette),
            };
            stack.push(if i == 0 {
                drawn
            } else {
                Element::column(vec![drawn])
                    .size(Some(width), Some(now.max(0.0)))
                    .clip_children()
            });
        }
        let mut column = vec![];
        if layout.main > 0.0 {
            column.push(divider(COLUMN_HANDLE, true, below, palette));
        }
        column.push(Element::column(stack).size(Some(width), Some(below)));
        // Laid out at its full width and cut to what has slid in, as the panel is.
        row.push(
            Element::row(vec![
                Element::row(column).size(Some(layout.column), Some(below))
            ])
            .size(Some(layout.shown), Some(below))
            .clip_children(),
        );
    }
    if let (Some(session), true) = (session, layout.panel > 0.0) {
        // Laid out at its full width and cut to what has slid in, so nothing in it rewraps.
        let width = ui.panel_width;
        let blue = ComponentPalette {
            hover: look::handle(),
            selected: look::handle(),
            ..palette
        };
        let edge = Element::row(vec![
            limn::Tooltip::new(
                limn::ResizeHandle::vertical(PANEL_HANDLE, blue)
                    .thickness(HANDLE)
                    .accessibility_label("Resize panel")
                    .render(),
                PANEL_HANDLE,
                "Click to collapse",
                look::palette(),
            )
            .description("Drag to resize")
            .placement(limn::TooltipPlacement::Left)
            .render(),
            Element::spacer().flex(1.0),
        ]);
        row.push(
            Element::row(vec![Element::stack(vec![
                right_panel(store, session, ui, width, below, palette),
                edge,
            ])
            .size(Some(width), Some(below))])
            .size(Some(layout.panel), Some(below))
            .clip_children(),
        );
    }
    // Customize takes the conversation's place, and the panes beside it.
    let content = match &ui.customize {
        Some(_) => Element::column(vec![
            top_bar(None, ui, rest, 0.0),
            hline(rest, look::theme().background),
            crate::customize::page(store, ui, rest, below),
        ]),
        None => Element::column(vec![
            top_bar(session, ui, layout.main, layout.shown + layout.panel),
            hline(rest, look::theme().background),
            Element::row(row).size(Some(rest), Some(below)),
        ]),
    }
    .size(Some(rest), Some(window.height));
    let with_line = || {
        Element::row(vec![
            sidebar(store, ui, window.height, palette),
            vline(window.height),
        ])
    };
    let mut root: Vec<Element> = ui
        .sidebar
        .docked(with_line(), window.height)
        .into_iter()
        .collect();
    root.push(content);
    let root = Element::row(root)
        .background(look::theme().background)
        .size(Some(window.width), Some(window.height));
    // The hidden sidebar peeking over the page.
    let root = ui.sidebar.peeking(root, with_line, window);
    let root = search_dialog(store, ui, root, palette);
    let root = link_menu(ui, root, window);
    // The Files pane's naming dialog, its trash's question, and a tab's edits to give up.
    let root = match ui.files_pane() {
        Some(pane) if ui.pane_open(Pane::Files) => files_dialogs(pane, root),
        _ => root,
    };
    let root = settings_dialog(store, ui, root, window, palette);
    let root = mcp_dialog(store, ui, root, window, palette);
    let root = directory_dialog(ui, root, window, palette);
    let root = marketplace_dialog(ui, root, palette);
    let root = server_dialog(ui, root, palette);
    let root = skill_dialog(ui, root, palette);
    let root = crate::remote::dialog(store, ui, root);
    let root = crate::updates::dialogs(ui, root, palette);
    let root = crate::account::dialogs(ui, root);
    let root = crate::github::dialogs(ui, root);
    let root = removing_dialog(ui, root);
    let root = image_viewer(ui, root, window, palette);
    let root = annotator(ui, root, window, palette);
    let root = crate::tour::layer(ui, root, palette);
    // Desktop's own wording for both closes and for a delete, less its vendor's servers: quill forgets only its own record.
    let tabs = &ui.terminal.tabs;
    let deleting = ui.deleting.and_then(|id| store.session(id));
    let (title, description, label) = match (ui.terminal.closing, deleting) {
        (_, Some(session)) => (
            "Delete session?",
            format!(
                "\u{201c}{}\u{201d} will be removed from quill. The agent's own transcript of it stays on this computer.",
                session_title(session)
            ),
            "Delete",
        ),
        (Some(crate::Closing::Others(keep)), None) => {
            let busy = tabs
                .iter()
                .enumerate()
                .filter(|(i, t)| *i != keep && t.busy)
                .count();
            let description = if busy == 1 {
                "A command is still running in one of the other terminals. Closing them stops it."
                    .to_string()
            } else {
                format!("Commands are still running in {busy} of the other terminals. Closing them stops those commands.")
            };
            (
                "Close other terminals?",
                description,
                "Close other terminals",
            )
        }
        (Some(crate::Closing::One(tab)), None) => (
            "Close terminal?",
            format!(
                "A command is still running in {}. Closing the terminal stops it.",
                tabs.get(tab).map_or("the terminal", |t| t.title.as_str())
            ),
            "Close terminal",
        ),
        (None, None) => ("", String::new(), ""),
    };
    let (confirm_id, cancel_id) = if deleting.is_some() {
        (DELETE_CONFIRM, DELETE_CANCEL)
    } else {
        (TERMINAL_CLOSE_CONFIRM, TERMINAL_CLOSE_CANCEL)
    };
    confirm(
        root,
        ui.terminal.closing.is_some() || deleting.is_some(),
        title,
        &description,
        label,
        confirm_id,
        cancel_id,
    )
}

/// Expand and close, the tools every pane carries.
fn pane_tools(pane: Pane, ui: &Ui, palette: ComponentPalette) -> Vec<Element> {
    let expanded = ui.expanded_pane == Some(pane);
    let key = pane.key();
    vec![
        icon_tool(
            &format!("pane:{key}:expand"),
            if expanded {
                icons::of(icons::COLLAPSE)
            } else {
                icons::of(icons::EXPAND)
            },
            if expanded { "Collapse" } else { "Expand" },
            28.0,
            palette,
        ),
        icon_tool(
            &format!("pane:{key}:close"),
            icons::of(icons::CLOSE),
            "Close",
            28.0,
            palette,
        ),
    ]
}

/// The session's shells: "Terminal" and the folder while there is one, pills once
/// there are more, "New terminal" after them.
fn terminal_pane(
    session: Option<&Session>,
    ui: &Ui,
    width: f32,
    height: f32,
    palette: ComponentPalette,
) -> Element {
    let new = icon_tool(
        TERMINAL_NEW,
        icons::of(icons::ADD),
        "New terminal",
        28.0,
        palette,
    );
    let tabs = &ui.terminal.tabs;
    let folder = folder_name(session.map_or(&ui.cwd, |s| &s.cwd));
    let tools = pane_tools(Pane::Terminal, ui, palette);
    let header = if tabs.len() <= 1 {
        let title = match tabs.first() {
            Some(tab) if tab.named => tab.title.clone(),
            _ => "Terminal".into(),
        };
        let label = Element::row(vec![
            text(title).centered_y(),
            muted(folder.clone()).centered_y(),
        ])
        .gap(8.0);
        let label = match ui.terminal.renaming {
            Some((0, ref name)) => rename_field(TERMINAL_RENAME, name, 160.0, palette),
            _ => tab_menu(0, 1, label, palette),
        };
        let lead = Element::row(vec![label, Element::spacer().size(Some(4.0), None), new]).gap(8.0);
        pane_header(lead, tools, width)
    } else {
        let pills = (tabs.iter().enumerate())
            .map(|(i, tab)| {
                let id = format!("terminal:tab:{i}");
                if let Some((at, name)) = &ui.terminal.renaming {
                    if *at == i {
                        return (id, rename_field(TERMINAL_RENAME, name, 160.0, palette));
                    }
                }
                // "Terminal 2  work ×": a named tab drops the folder.
                let pill = pane_tab(
                    &id,
                    &format!("terminal:close:{i}"),
                    "Close terminal",
                    &tab.title,
                    (!tab.named).then_some(folder.as_str()),
                    i == ui.terminal.active,
                );
                (id, tab_menu(i, tabs.len(), pill, palette))
            })
            .collect();
        let active = Some(ui.terminal.active);
        tabs_header("terminal:tabs", pills, active, Some(new), tools, width)
    };
    let body = match &ui.terminal.screen {
        Some(screen) => pad(
            // The view draws its cursor in the palette's text: Desktop's is blue.
            limn::TerminalView::new(
                TERMINAL,
                screen.clone(),
                ComponentPalette {
                    text: look::cursor(),
                    ..palette
                },
            )
            .font_size(TERMINAL_FONT)
            .background(look::theme().background)
            // The shell's default ink is near black, lost on the dark page without it.
            .min_contrast(4.5)
            // Desktop's terminal marks its focus with the cursor alone.
            .focus_ring(false)
            .render(),
            TERMINAL_PADDING[0],
            TERMINAL_PADDING[1],
        ),
        None => Element::spacer(),
    };
    Element::column(vec![header, body.flex(1.0)]).size(Some(width), Some(height))
}

/// The Files pane, as Desktop's file tile: the open files' tabs, the tree's toggle beside expand and close, and the file
/// on screen with the tree at its right. Narrower than the two side by side, the tree takes
/// the pane while it shows, as Desktop's does over its tabs.
fn files_pane(ui: &Ui, width: f32, height: f32, palette: ComponentPalette) -> Element {
    use quill_components::{packs::Pack, tree};
    let Some(pane) = ui.files_pane() else {
        return Element::spacer().size(Some(width), Some(height));
    };
    let toggle = tool_surface(
        crate::files::TREE_TOGGLE,
        icons::of(icons::LIST),
        if pane.tree_open {
            "Hide file tree"
        } else {
            "Show file tree"
        },
        28.0,
        pane.tree_open.then(look::active),
        look::tool_hover(),
        None,
    );
    let mut tools = vec![toggle];
    tools.extend(pane_tools(Pane::Files, ui, palette));
    let tabs = (pane.tabs.iter().enumerate())
        .map(|(i, d)| {
            let name = d.path.rsplit('/').next().unwrap_or_default();
            let active = pane.active == Some(i);
            let (tab, close) = (format!("doc:tab:{i}"), format!("doc:close:{i}"));
            let pill = pane_tab(&tab, &close, "Close file", name, None, active);
            (tab, pill)
        })
        .collect();
    let header = tabs_header("files:tabs", tabs, pane.active, None, tools, width);
    let below = height - look::PANE_HEADER - 1.0;
    // Desktop's tree is 240 px by default, beside a file at least 160 wide.
    let tree_width = 240.0_f32.min((width * 0.45).round());
    let beside = width >= 240.0 + 160.0;
    let shown = pane.document().map(|d| pane.absolute(d));
    let tree = |w: f32| {
        let mut column = vec![];
        if let Some(error) = &pane.tree.error {
            let ok = keyed_button(tree::ERROR_DISMISS, "OK", "", false, true, palette);
            column.push(
                Element::row(vec![
                    muted(error.clone())
                        .foreground(look::failed())
                        .flex(1.0)
                        .centered_y(),
                    ok.centered_y(),
                ])
                .gap(8.0)
                .padding(8.0),
            );
        }
        // Desktop's filter over the tree.
        let filter = limn::SearchField::new(
            FILTER_FIELD,
            &pane.filter,
            "Filter, or ? to search in files",
            palette,
        )
        .size(ControlSize::Compact)
        .width(w - 16.0)
        .clearable(true)
        .render();
        let rows = below - 44.0 - if column.is_empty() { 0.0 } else { 44.0 };
        column.push(
            Element::row(vec![filter])
                .padding(8.0)
                .size(None, Some(44.0)),
        );
        column.push(if pane.filter.is_empty() {
            tree::view(
                &pane.tree,
                &Pack::Quill,
                tree::Look::Desktop,
                shown.as_deref(),
                None,
                [w, rows.max(0.0)],
            )
        } else {
            found_view(pane, w, rows.max(0.0))
        });
        Element::column(column).size(Some(w), Some(below))
    };
    let body = match (pane.tree_open, beside) {
        (true, true) => Element::row(vec![
            file_view(pane, width - tree_width - 1.0, below, palette),
            vline(below),
            tree(tree_width),
        ]),
        (true, false) => tree(width),
        (false, _) => file_view(pane, width, below, palette),
    };
    Element::column(vec![header, body]).size(Some(width), Some(height))
}

/// What the tree's filter found, in Desktop's words: the files whose
/// paths match, their folders muted; after `?` each file that holds the words over its lines,
/// five at most; or why there is nothing.
fn found_view(pane: &crate::files::FilesPane, width: f32, height: f32) -> Element {
    use quill_components::tree;
    use quill_files::find::LIMIT;
    let (query, contents) = crate::files::filter_query(&pane.filter);
    let note = |words: String| {
        Element::truncated_text(words)
            .font_family(FontFamily::Sans)
            .font_size(SMALL)
            .foreground(look::muted())
            .max_width(width - 24.0)
            .padding(12.0)
    };
    let row = |id: String, content: Vec<Element>| {
        let mut items = vec![hgap(4.0)];
        items.extend(content);
        Element::clickable(id, Element::row(items).gap(6.0).size(None, Some(24.0)))
            .hover_background(look::row_hover())
            .radius(6.0)
            .size(Some(width - 12.0), None)
    };
    let file = |path: &std::path::Path| {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into());
        let folder = path
            .parent()
            .map_or_else(String::new, |p| p.display().to_string());
        row(
            tree::row_id(&pane.tree.root.join(path), false),
            vec![
                Element::icon(icons::of(tree::glyph(path)))
                    .foreground(look::muted())
                    .size(Some(14.0), Some(14.0))
                    .centered_y(),
                text(name).centered_y(),
                Element::truncated_text(folder)
                    .font_family(FontFamily::Sans)
                    .font_size(SMALL)
                    .foreground(look::muted())
                    .flex(1.0)
                    .centered_y(),
            ],
        )
    };
    let mut rows = vec![];
    match pane.found.as_ref() {
        _ if contents && query.is_empty() => {
            rows.push(note("Type after ? to search in files".into()))
        }
        None => rows.push(
            Element::row(vec![limn::Spinner::new("tree:finding", look::muted())
                .size(14.0)
                .render()])
            .padding(12.0),
        ),
        Some(found) if !contents => {
            if found.paths.is_empty() {
                rows.push(note("No matching files".into()));
            }
            rows.extend(found.paths.iter().map(|p| file(p)));
            if found.paths.len() >= LIMIT {
                rows.push(note(format!(
                    "Only the first {LIMIT} results are shown. Keep typing to narrow."
                )));
            }
        }
        Some(found) => {
            if found.hits.is_empty() {
                rows.push(note("No matches in file contents".into()));
            }
            let mut at = 0;
            while let Some(first) = found.hits.get(at) {
                let same = found.hits[at..]
                    .iter()
                    .take_while(|h| h.path == first.path)
                    .count();
                rows.push(file(&first.path));
                for hit in found.hits[at..at + same].iter().take(5) {
                    let id = format!(
                        "{}{}:{}",
                        crate::files::FOUND_LINE,
                        hit.path.display(),
                        hit.row
                    );
                    rows.push(row(
                        id,
                        vec![
                            hgap(16.0),
                            muted((hit.row + 1).to_string())
                                .font_size(SMALL)
                                .centered_y(),
                            Element::truncated_text(hit.text.clone())
                                .font_family(FontFamily::Sans)
                                .font_size(SMALL)
                                .foreground(look::palette().text)
                                .flex(1.0)
                                .centered_y(),
                        ],
                    ));
                }
                if same > 5 {
                    rows.push(note("More matches in this file.".into()));
                }
                at += same;
            }
            if found.hits.len() >= LIMIT {
                rows.push(note(format!(
                    "Only {LIMIT} matches are shown. Keep typing to narrow."
                )));
            }
        }
    }
    Element::scroll("tree:found", Element::column(rows).gap(2.0).padding(6.0))
        .size(Some(width), Some(height))
        .clip_children()
}

/// Desktop's Background tasks: what the agent runs beside its turns, under Running and Finished with their
/// counts, each its description over what it is, how it stands and how long it ran, then
/// what it used and, for a helper agent, "View transcript" (its pane). A running one stops;
/// the trash clears the finished. Desktop also names each agent's model, which the agent's
/// task reports do not say.
fn tasks_pane(
    session: Option<&Session>,
    ui: &Ui,
    width: f32,
    height: f32,
    palette: ComponentPalette,
) -> Element {
    let tasks: Vec<&crate::Task> = session.map_or(vec![], |s| s.tasks.iter().collect());
    let finished = tasks.iter().filter(|t| !t.running).count();
    let mut tools = vec![];
    if finished > 0 {
        tools.push(icon_tool(
            TASKS_CLEAR,
            icons::of(icons::TRASH),
            "Clear finished tasks",
            28.0,
            palette,
        ));
    }
    tools.extend(pane_tools(Pane::Tasks, ui, palette));
    let header = pane_header(text("Background tasks").centered_y(), tools, width);
    let below = height - look::PANE_HEADER - 1.0;
    if tasks.is_empty() {
        let fill = || Element::spacer().flex(1.0);
        let body = Element::column(vec![
            fill(),
            Element::icon(icons::of(icons::AGENTS))
                .foreground(look::muted())
                .large_icon(24.0)
                .size(Some(24.0), Some(24.0))
                .centered_x(),
            muted("Background tasks appear here").centered_x(),
            fill(),
        ])
        .gap(10.0)
        .size(Some(width), Some(below));
        return Element::column(vec![header, body]).size(Some(width), Some(height));
    }
    let inner = width - 32.0;
    let row = |task: &crate::Task| {
        let kind = match task.kind.as_str() {
            "agent" => "Agent",
            "command" => "Bash",
            "workflow" => "Workflow",
            "monitor" => "Monitor",
            _ => "Task",
        };
        let ran = if task.running {
            Some(task.since.elapsed().as_secs())
        } else {
            task.usage.map(|u| u.duration_ms / 1000)
        };
        // One line under the title says what it is and what it used: a running one says so
        // by its shimmer, a finished one in words.
        let mut meta = vec![kind.to_owned()];
        if !task.running {
            meta.push(
                if task.stopped {
                    "Stopped"
                } else if task.failed {
                    "Failed"
                } else {
                    "Completed"
                }
                .into(),
            );
        }
        meta.extend(ran.map(elapsed));
        if let Some(usage) = task.usage.filter(|u| u.tokens > 0) {
            meta.push(tokens(usage.tokens));
            meta.push(match usage.tool_uses {
                1 => "1 tool use".into(),
                n => format!("{n} tool uses"),
            });
        }
        let meta = meta.join(" · ");
        let wide = inner - if task.running { 36.0 } else { 0.0 };
        let meta = if task.running {
            Element::shimmer_truncated_text(
                format!("task:{}:meta", task.id),
                meta,
                look::palette().text,
            )
        } else {
            Element::truncated_text(meta)
        }
        .font_family(FontFamily::Sans)
        .font_size(SMALL)
        .foreground(look::muted())
        .max_width(wide);
        let lines = Element::column(vec![
            Element::truncated_text(task.description.clone())
                .font_family(FontFamily::Sans)
                .font_size(BODY)
                .foreground(look::palette().text)
                .max_width(wide),
            meta,
        ])
        .gap(4.0);
        // A helper agent's row opens its transcript.
        let lines = match (&task.call, task.kind.as_str()) {
            (Some(call), "agent") => Element::clickable(format!("agent:{call}"), lines)
                .accessibility_label(format!("View transcript of {}", task.description)),
            _ => lines,
        };
        // The row's flexible child is what it holds, so Stop keeps to the right edge.
        let mut items = vec![lines.flex(1.0)];
        if task.running {
            items.push(stop_button(&format!("task:stop:{}", task.id)));
        }
        // Flush with the group's name, 10 above and below, over a hairline, as Desktop's.
        let gap = || Element::spacer().size(None, Some(10.0));
        Element::column(vec![
            gap(),
            Element::row(items).gap(8.0).size(Some(inner), None),
            gap(),
            hline(inner, look::divider()),
        ])
    };
    let mut column = vec![];
    for (key, label, running) in [
        ("tasks:running", "Running", true),
        ("tasks:finished", "Finished", false),
    ] {
        let group: Vec<&&crate::Task> = tasks.iter().filter(|t| t.running == running).collect();
        if group.is_empty() {
            continue;
        }
        // Open until folded, as Desktop's groups start.
        let open = !ui.expanded.iter().any(|k| k == key);
        column.push(Element::clickable(
            key,
            Element::row(vec![
                muted(format!("{label} {}", group.len())).centered_y(),
                caret(open),
            ])
            .gap(6.0)
            .size(None, Some(32.0)),
        ));
        if open {
            column.extend(group.into_iter().map(|t| row(t)));
        }
    }
    // Its bar over the padding, so a long list keeps Stop at the right edge.
    let body = Element::scroll(
        "tasks:scroll",
        Element::column(column)
            .padding(16.0)
            .size(Some(width), None),
    )
    .scrollbar_appearance(limn::ScrollbarAppearance::overlay())
    .size(Some(width), Some(below));
    Element::column(vec![header, body]).size(Some(width), Some(height))
}

/// The file on screen, as Desktop's: its path, folders muted leading to the tree and its name
/// copying itself, and its tools (Reload, Preview or source, Find, Copy; Cancel and Save while
/// edited), over the file in quill's editor, rendered for Markdown, or what keeps it from
/// showing. With no file, Desktop's words for an empty tile.
fn file_view(
    pane: &crate::files::FilesPane,
    width: f32,
    height: f32,
    palette: ComponentPalette,
) -> Element {
    use crate::files;
    use quill_components::{code, find};
    // Each line centred and cut to the width: limn centres a line, not a paragraph's lines.
    let note = |title: &str, lines: &[&str]| {
        let fill = || Element::spacer().flex(1.0);
        let line = |words: &str, size: f32, ink: limn::Color| {
            Element::truncated_text(words.to_owned())
                .font_family(FontFamily::Sans)
                .font_size(size)
                .foreground(ink)
                .max_width(width - 32.0)
                .centered_x()
        };
        let mut column = vec![fill(), line(title, BODY, look::palette().text)];
        column.extend(lines.iter().map(|l| line(l, SMALL, look::muted())));
        column.push(fill());
        Element::column(column)
            .gap(6.0)
            .size(Some(width), Some(height))
    };
    let Some(document) = pane.document() else {
        return note(
            "Open files appear here",
            &[
                "Pick a file in the tree,",
                "or click a file path in the conversation.",
            ],
        );
    };
    const BAR: f32 = 32.0;
    let root = pane.tree.root.clone();
    let file = document.path.clone();
    // The page's own fill for the fade where the path does not fit.
    let crumb_palette = ComponentPalette {
        surface: look::theme().background,
        ..look::palette()
    };
    let crumbs = limn::Breadcrumb::from_path("doc:crumbs", document.path.clone(), crumb_palette)
        .fade_overflow()
        .segment_action(move |segment| {
            Some(if segment == file {
                files::COPY_PATH.into()
            } else {
                format!("tree:reveal-dir:{}", root.join(segment).display())
            })
        });
    let tool = |id: &str, icon: &'static str, label: &str| {
        tool_surface(
            id,
            icons::of(icon),
            label,
            24.0,
            None,
            look::tool_hover(),
            None,
        )
        .centered_y()
    };
    let mut tools = vec![];
    if document.dirty {
        tools.push(keyed_button(files::REVERT, "Cancel", "", false, true, palette).centered_y());
        tools.push(keyed_button(DOC_SAVE, "Save", "Ctrl+S", true, true, palette).centered_y());
    } else {
        tools.push(tool(files::RELOAD, icons::RELOAD, "Reload file"));
    }
    if document.markdown() && document.editor.is_some() {
        tools.push(if document.preview {
            tool(DOC_PREVIEW, icons::CODE, "View source")
        } else {
            tool(DOC_PREVIEW, icons::EYE, "Preview")
        });
    }
    if document.editor.is_some() && !(document.markdown() && document.preview) {
        tools.push(tool(find::FIND_OPEN, icons::SEARCH, "Find in file"));
    }
    tools.push(tool(files::COPY, icons::COPY, "Copy file contents"));
    let crumbs = crumbs
        // The row's 8 and 6 at the ends and 4 between, the tools 24 but Cancel and Save.
        .width(
            (width - 26.0 - tools.len() as f32 * 28.0 - if document.dirty { 110.0 } else { 0.0 })
                .max(80.0),
        )
        .height(BAR - 1.0)
        .render();
    let mut row = vec![hgap(8.0), crumbs.centered_y(), Element::spacer().flex(1.0)];
    row.extend(tools);
    row.push(hgap(6.0));
    let mut column = vec![
        Element::row(row)
            .gap(4.0)
            .size(Some(width), Some(BAR - 1.0)),
        hline(width, look::pane_border()),
    ];
    let mut below = height - BAR;
    if let Some(error) = &document.error {
        column.push(
            Element::row(vec![
                muted(format!("Not saved: {error}")).foreground(look::failed())
            ])
            .padding(10.0),
        );
        below -= 40.0;
    }
    let path = pane.absolute(document);
    column.push(match &document.editor {
        None if document.unreadable => note("This file can't be shown as text.", &[]),
        None => Element::spacer().size(Some(width), Some(below)),
        Some(editor) if document.markdown() && document.preview => {
            let text = document.text().unwrap_or_default();
            let ids = code::PreviewIds {
                id: "doc:preview-text",
                scroll: "doc:scroll",
                link: "doc:link:",
                diagram: "doc:diagram:",
            };
            let _ = editor;
            code::markdown_preview(&text, ids, &[], [width, below])
        }
        Some(editor) => code::editor(files::editor_id(&document.path), editor, &path)
            .soft_wrap(document.markdown())
            .reveal_top_inset(if pane.find.is_some() { 48.0 } else { 0.0 })
            .search_matches(
                (pane.find.as_ref())
                    .and_then(|f| f.results.as_ref())
                    .map_or(0, |r| r.revision),
                (pane.find.as_ref())
                    .and_then(|f| f.results.as_ref())
                    .map(|r| r.matches.clone())
                    .unwrap_or_default(),
            )
            .height(below)
            .render(),
    });
    // The find bar floats under the end of the line over the file.
    if let Some(bar) = &pane.find {
        let anchor = Element::spacer().size(Some(width - 12.0), Some(0.0));
        column.insert(2, find::bar(bar, width, anchor));
    }
    Element::column(column).size(Some(width), Some(height))
}

/// The pane's dialogs over `base`: the tree's (a name, the trash's question) and Desktop's
/// question before a tab with edits not saved closes.
fn files_dialogs(pane: &crate::files::FilesPane, base: Element) -> Element {
    use crate::files;
    let base = quill_components::tree::dialogs(&pane.tree, base);
    let name = (pane.discarding)
        .and_then(|at| pane.tabs.get(at))
        .map(|d| d.path.rsplit('/').next().unwrap_or_default().to_owned())
        .unwrap_or_default();
    confirm(
        base,
        pane.discarding.is_some(),
        "Discard unsaved changes?",
        &format!("Your edits to {name} have not been saved. Leaving this tab discards them."),
        "Discard",
        files::DISCARD,
        files::KEEP,
    )
}

/// The session's browser: its tabs over a toolbar of back,
/// forward, reload and the address, then the page itself, which takes the pointer and the
/// keys while it has the focus. Desktop's texts, less the agent's name.
fn browser_pane(
    session: Option<&Session>,
    ui: &Ui,
    width: f32,
    height: f32,
    palette: ComponentPalette,
) -> Element {
    let Some(session) = session else {
        return Element::spacer().size(Some(width), Some(height));
    };
    let browser = &session.browser;
    let tabs = (browser.tabs.iter())
        .map(|tab| {
            let name = if !tab.title.is_empty() {
                tab.title.as_str()
            } else if tab.url == "about:blank" {
                "New tab"
            } else {
                tab.url.as_str()
            };
            let id = format!("browser:tab:{}", tab.id);
            let close = format!("browser:close:{}", tab.id);
            let active = Some(tab.id) == browser.active;
            let pill = pane_tab(&id, &close, "Close tab", name, None, active);
            (id, pill)
        })
        .collect();
    let new = icon_tool(
        BROWSER_NEW_TAB,
        icons::of(icons::ADD),
        "New tab",
        28.0,
        palette,
    );
    let active = (browser.tabs.iter()).position(|t| Some(t.id) == browser.active);
    let header = tabs_header(
        "browser:tabs",
        tabs,
        active,
        Some(new),
        pane_tools(Pane::Browser, ui, palette),
        width,
    );
    let tab = browser.tab();
    // What cannot be done now is drawn muted and takes no press.
    let tool = |enabled: bool, id: &str, icon: &'static str, label: &str| {
        if enabled {
            icon_tool(id, icons::of(icon), label, 28.0, palette)
        } else {
            Element::row(vec![Element::icon(icons::of(icon))
                .foreground(look::muted())
                .size(Some(16.0), Some(16.0))
                .centered()
                .flex(1.0)])
            .size(Some(28.0), Some(28.0))
            .centered_y()
        }
    };
    let reload = match tab {
        Some(tab) if tab.loading => icon_tool(
            BROWSER_STOP,
            icons::of(icons::CLOSE),
            "Stop loading",
            28.0,
            palette,
        ),
        _ => icon_tool(
            BROWSER_RELOAD,
            icons::of(icons::RELOAD),
            "Reload",
            28.0,
            palette,
        ),
    };
    let address = limn::TextField::new(
        BROWSER_URL,
        tab.map_or("", |t| {
            if t.url == "about:blank" {
                ""
            } else {
                t.url.as_str()
            }
        }),
        "Enter a URL",
        palette,
    )
    .size(ControlSize::Compact)
    // What the buttons leave, so the field ends 8 px from the pane's edge, as back starts.
    .width((width - 176.0).max(80.0))
    .render()
    .border(look::card_border(), 1.0)
    .focus_border(look::handle(), 1.0)
    .radius(8.0)
    .centered_y();
    let toolbar = Element::column(vec![
        Element::row(vec![
            hgap(8.0),
            tool(
                tab.is_some_and(|t| t.back),
                BROWSER_BACK,
                icons::ARROW_LEFT,
                "Go back",
            ),
            tool(
                tab.is_some_and(|t| t.forward),
                BROWSER_FORWARD,
                icons::ARROW_RIGHT,
                "Go forward",
            ),
            reload,
            address,
            viewport_menu(tab, ui),
            // Desktop's "Select element" / "Exit selection mode".
            toggle(
                BROWSER_SELECT,
                icons::of(icons::CURSOR),
                if browser.selecting {
                    "Exit selection mode"
                } else {
                    "Select element"
                },
                browser.selecting,
                None,
            ),
            hgap(4.0),
        ])
        .gap(4.0)
        .size(Some(width), Some(40.0)),
        hline(width, look::pane_border()),
    ]);
    let bar = browser_notice(session.id, ui, width, palette);
    let below =
        height - look::PANE_HEADER - 1.0 - 41.0 - if bar.is_some() { BAR + 1.0 } else { 0.0 };
    // A blank tab shows Desktop's empty state instead of its page, less the agent's name and the dev server it offers to find.
    let blank = |failed: Option<&String>| {
        let fill = || Element::spacer().flex(1.0);
        let mut column = vec![
            fill(),
            Element::icon(icons::of(icons::GLOBE))
                .foreground(look::muted())
                .large_icon(24.0)
                .size(Some(24.0), Some(24.0))
                .centered_x(),
            Element::spacer().size(None, Some(6.0)),
            text("Browse the web").centered_x(),
        ];
        // Two centred lines: limn centres a line, not the lines of a paragraph.
        let words = match failed {
            Some(failed) => vec![failed.clone()],
            None => vec![
                "Type a URL or ask the agent to open a site.".into(),
                "It can read, click, and type.".into(),
            ],
        };
        column.extend(words.into_iter().map(|line| {
            Element::truncated_text(line)
                .font_family(FontFamily::Sans)
                .font_size(SMALL)
                .foreground(look::muted())
                .max_width(width - 32.0)
                .centered_x()
        }));
        column.push(fill());
        Element::column(column)
            .gap(6.0)
            .size(Some(width), Some(below.max(0.0)))
    };
    let body = match tab {
        Some(tab) if tab.url != "about:blank" || tab.loading => Element::focus_area(
            BROWSER_VIEW,
            Element::surface(browser_surface(session.id, tab.id), "Page"),
        )
        .accessibility_label("Browser page")
        .size(Some(width), Some(below.max(0.0))),
        Some(_) => blank(None),
        None => blank(browser.failed.as_ref()),
    };
    let mut column = vec![header, toolbar, body];
    column.extend(bar);
    Element::column(column).size(Some(width), Some(height))
}

/// The height of the browser's notice strip, under its page.
const BAR: f32 = 32.0;

/// The strip under the page while the page or the browser has said something the person should
/// know (a dialog's words, a download saved, a failure with a warning): its icon, its words,
/// which fade when the pane is narrow, and its close. The page's size is the Viewport button's.
fn browser_notice(
    session: quill_model::SessionId,
    ui: &Ui,
    width: f32,
    palette: ComponentPalette,
) -> Option<Element> {
    let (words, failed) = ui.browser_notice.get(&session)?;
    let (icon, ink) = if *failed {
        (icons::WARNING, look::failed())
    } else {
        (icons::GLOBE, look::muted())
    };
    Some(Element::column(vec![
        hline(width, look::pane_border()),
        Element::row(vec![
            hgap(4.0),
            Element::icon(icons::of(icon))
                .foreground(ink)
                .size(Some(16.0), Some(16.0))
                .centered_y(),
            Element::faded_text(words.clone(), FADE)
                .font_family(FontFamily::Sans)
                .font_size(SMALL)
                .foreground(look::palette().text)
                .flex(1.0)
                .centered_y(),
            icon_tool(
                BROWSER_NOTICE_CLOSE,
                icons::of(icons::CLOSE),
                "Dismiss",
                24.0,
                palette,
            ),
            hgap(0.0),
        ])
        .gap(8.0)
        .size(Some(width), Some(BAR)),
    ]))
}

/// A tab's right-click menu, Desktop's four actions: rename, clear, close, and
/// close the others when there are others.
fn tab_menu(tab: usize, count: usize, child: Element, palette: ComponentPalette) -> Element {
    use limn::{ContextMenuEntry, ContextMenuItem};
    let item = |action: &str, label: &str| {
        ContextMenuEntry::Item(ContextMenuItem::new(
            format!("terminal:menu:{action}:{tab}"),
            label,
        ))
    };
    let mut entries = vec![
        item("rename", "Rename terminal"),
        item("clear", "Clear terminal"),
        ContextMenuEntry::Separator,
        item("close", "Close terminal"),
    ];
    if count > 1 {
        entries.push(item("others", "Close other terminals"));
    }
    limn::ContextMenu::new(format!("terminal:menu:{tab}"), child, entries, palette).render()
}

/// The name field a pill or the session's title turns into while the person renames it.
fn rename_field(id: &str, name: &str, width: f32, palette: ComponentPalette) -> Element {
    limn::TextField::new(id, name, "", palette)
        .size(ControlSize::Compact)
        .width(width)
        .render()
        .border(look::card_border(), 1.0)
        .focus_border(look::handle(), 1.0)
        .radius(8.0)
        .centered_y()
}

/// A session's name: the person's, the agent's or its first message, else "New session".
fn session_title(session: &Session) -> String {
    if session.title.is_empty() {
        "New session".into()
    } else {
        session.title.clone()
    }
}

/// The toggle's tooltip. Hidden with a version waiting it carries the dot's words (as in Desktop).
pub(crate) fn sidebar_toggle_label(ui: &Ui) -> &'static str {
    if ui.sidebar.open {
        "Hide sidebar"
    } else if crate::updates::ready(ui).is_some() {
        "Restart needed. Show sidebar for details."
    } else {
        "Show sidebar"
    }
}

/// The sidebar's toggle, "Hide sidebar Ctrl+B"; it stays at the bar's start while the sidebar
/// is hidden, on the active fill.
fn sidebar_toggle(ui: &Ui) -> Element {
    let toggle = toggle(
        SIDEBAR_TOGGLE,
        icons::of(icons::SIDEBAR),
        sidebar_toggle_label(ui),
        !ui.sidebar.open,
        Some("Ctrl+B"),
    );
    crate::updates::toggle_dot(ui, toggle)
}

/// "Open in" as Desktop lists it: quill-ide, then the
/// editors installed, by name, each with its own icon; then the desktop's default app and its
/// file manager. Each row is "open-in:<editor | default | files>:<target>", `target` a session
/// or "here", the folder on screen.
fn open_in_entries(target: &str, ui: &Ui) -> Vec<limn::DropdownMenuEntry> {
    open_in_rows("open-in:", target, ui)
}

/// The rows of Open in, each `{prefix}{place}:{target}`.
fn open_in_rows(prefix: &str, target: &str, ui: &Ui) -> Vec<limn::DropdownMenuEntry> {
    let mut entries = vec![
        limn::DropdownMenuEntry::Heading("Open in".into()),
        limn::DropdownMenuItem::new(format!("{prefix}{QUILL_IDE}:{target}"), "quill-ide")
            .icon(icons::of(icons::CODE))
            .into(),
    ];
    let mut editors: Vec<&crate::Editor> = ui.editors.iter().collect();
    editors.sort_by(|a, b| a.name.cmp(&b.name));
    for editor in editors {
        let item = limn::DropdownMenuItem::new(
            format!("{prefix}{}:{target}", editor.key),
            editor.name.clone(),
        );
        entries.push(
            match &editor.picture {
                Some(picture) => item.picture(picture.clone()),
                None => item.icon(icons::of(icons::CODE)),
            }
            .into(),
        );
    }
    entries.extend([
        limn::DropdownMenuEntry::Separator,
        limn::DropdownMenuItem::new(format!("{prefix}default:{target}"), "Default app").into(),
        limn::DropdownMenuItem::new(format!("{prefix}files:{target}"), "File manager").into(),
    ]);
    entries
}

/// "Open in ›" in a session's menu.
fn open_in(session: quill_model::SessionId, ui: &Ui) -> limn::DropdownMenuEntry {
    limn::DropdownMenuItem::new(format!("open-in:{session}"), "Open in")
        .submenu(open_in_entries(&session.to_string(), ui))
        .into()
}

/// A session menu's last rows, Archive (or Unarchive) and the red Delete.
fn archive_and_delete(session: &Session) -> [limn::DropdownMenuEntry; 2] {
    [
        lettered(
            format!("archive:{}", session.id),
            if session.archived {
                "Unarchive"
            } else {
                "Archive"
            },
            "A",
        )
        .into(),
        lettered(format!("delete:{}", session.id), "Delete", "D")
            .destructive(look::danger_ink())
            .into(),
    ]
}

/// The "Transcript view ›" submenu of a session's menus, and "Make … the default" once the
/// session's view differs from the default (docs/ui.md "Vistas del transcript").
fn transcript_view_entries(session: &Session, ui: &Ui) -> Vec<limn::DropdownMenuEntry> {
    let thinking = session
        .capabilities
        .as_ref()
        .is_some_and(|c| c.thinking_text);
    let thinking = crate::thinking_offered(session, thinking);
    let current = crate::current_view(session, &ui.settings);
    let mut views: Vec<limn::DropdownMenuEntry> = [
        (TranscriptView::Normal, "normal", "Normal"),
        (TranscriptView::Thinking, "thinking", "Thinking"),
        (TranscriptView::Verbose, "verbose", "Verbose"),
    ]
    .into_iter()
    .filter(|(view, ..)| *view != TranscriptView::Thinking || thinking)
    .map(|(view, key, label)| {
        limn::DropdownMenuItem::new(format!("view:{}:{key}", session.id), label)
            .radio(view == current)
            .into()
    })
    .collect();
    // Inside the submenu, after its views, as Desktop's.
    if current != ui.settings.transcript_view {
        let label = match current {
            TranscriptView::Normal => "Normal",
            TranscriptView::Thinking => "Thinking",
            TranscriptView::Verbose => "Verbose",
        };
        views.extend([
            limn::DropdownMenuEntry::Separator,
            limn::DropdownMenuItem::new(
                format!("view-default:{}", session.id),
                format!("Make {label} the default"),
            )
            .into(),
        ]);
    }
    vec![
        limn::DropdownMenuItem::new(format!("view-menu:{}", session.id), "Transcript view")
            .submenu(views)
            // Wider than the session's menu, for "Make Thinking the default".
            .submenu_width(250.0)
            .into(),
    ]
}

/// A menu row with Desktop's one-letter shortcut.
fn lettered(id: String, label: &str, key: &str) -> limn::DropdownMenuItem {
    limn::DropdownMenuItem::new(id, label).shortcut(key)
}

/// The width of the top bar's toggles at its end, with their margin.
const TOP_TOOLS: f32 = 178.0;

/// The session's title, Stop while it works, and the toggles of the panel and the panes.
/// The title's chevron opens its menu, and Rename turns it into a field.
/// The top bar: the session's title centered over the conversation, between the sidebars
/// that are open, and its tools at the ends. No line under it: the transcript fades out
/// beneath it instead.
fn top_bar(session: Option<&Session>, ui: &Ui, main: f32, beside: f32) -> Element {
    // An expanded pane leaves the bar empty, as Desktop's does.
    if ui.expanded_pane.is_some() {
        return Element::spacer().size(None, Some(look::TOP_BAR));
    }
    let mut row = if ui.sidebar.open {
        vec![hgap(16.0)]
    } else {
        vec![hgap(9.0), sidebar_toggle(ui), hgap(12.0)]
    };
    let mut heading = vec![];
    match (session, &ui.title_edit) {
        (Some(_), Some(value)) => {
            heading.push(rename_field(TITLE_FIELD, value, 320.0, look::palette()))
        }
        (Some(s), None) => {
            let title = session_title(s);
            let open = ui.open_menu == Some(TITLE_MENU);
            let chevron = tool_surface(
                TITLE_MENU,
                icons::of(icons::CARET_DOWN),
                &format!("More options for {title}"),
                22.0,
                open.then(look::active),
                look::tool_hover(),
                None,
            );
            let mut entries = vec![open_in(s.id, ui), limn::DropdownMenuEntry::Separator];
            entries.extend([
                lettered(format!("rename:{}", s.id), "Rename", "R").into(),
                lettered(format!("fork:{}", s.id), "Fork", "F").into(),
                limn::DropdownMenuEntry::Separator,
            ]);
            entries.extend(transcript_view_entries(s, ui));
            entries.push(limn::DropdownMenuEntry::Separator);
            entries.extend(archive_and_delete(s));
            // Centered, the title stays clear of the tools at either end: the sidebar's
            // toggle, and the toggles that sit over the conversation when nothing is beside it.
            let left: f32 = if ui.sidebar.open { 16.0 } else { 47.0 };
            let right = (TOP_TOOLS - beside).max(0.0);
            let room = (main - 2.0 * left.max(right) - 26.0).max(48.0);
            heading.push(
                Element::truncated_text(title)
                    .font_family(FontFamily::Sans)
                    .font_size(BODY)
                    .foreground(look::palette().text)
                    .max_width(room)
                    .centered_y(),
            );
            heading.push(
                menu(
                    chevron,
                    entries,
                    open,
                    TITLE_MENU,
                    160.0,
                    limn::PopoverPlacement::BottomStart,
                )
                .centered_y(),
            );
        }
        (None, _) => heading.push(muted("New session").centered_y()),
    }
    row.push(Element::spacer().flex(1.0));
    // Desktop's "Open in" by the panes' toggles: the folder on screen in an editor.
    if session.is_some() {
        let open = ui.open_menu == Some(OPEN_IN_MENU);
        let anchor = tool_surface(
            OPEN_IN_MENU,
            icons::of(icons::FOLDER_OPEN),
            "Open in",
            26.0,
            open.then(look::active),
            look::tool_hover(),
            None,
        );
        row.push(
            menu(
                anchor,
                open_in_entries("here", ui),
                open,
                OPEN_IN_MENU,
                200.0,
                limn::PopoverPlacement::BottomEnd,
            )
            .centered_y(),
        );
    }
    // Stop lives in the composer, as in Desktop.
    if session.is_some() {
        let shown = ui.panel_open && ui.panes.is_empty();
        row.push(toggle(
            PANEL_TOGGLE,
            icons::of(icons::PANEL_RIGHT),
            if shown {
                "Hide session rail"
            } else {
                "Show session rail"
            },
            shown,
            None,
        ));
        // A hairline parts the rail's toggle from the panes'.
        row.push(vline(16.0).centered_y());
    }
    if session.is_some() {
        // Desktop shows and hides its file tree with a list's glyph (`ListBullet`); the folder
        // is Open in's.
        row.push(toggle(
            FILES_TOGGLE,
            icons::of(icons::LIST),
            "Files",
            ui.pane_open(Pane::Files),
            Some("Ctrl+Shift+E"),
        ));
        row.push(toggle(
            BROWSER_TOGGLE,
            icons::of(icons::GLOBE),
            "Browser",
            ui.pane_open(Pane::Browser),
            Some("Ctrl+Shift+B"),
        ));
    }
    // Once the session has some, as Desktop's menu lists Background tasks then.
    if session.is_some_and(|s| !s.tasks.is_empty()) {
        row.push(toggle(
            TASKS_TOGGLE,
            icons::of(icons::AGENTS),
            "Background tasks",
            ui.pane_open(Pane::Tasks),
            None,
        ));
    }
    row.push(toggle(
        TERMINAL_TOGGLE,
        icons::of(icons::TERMINAL),
        "Terminal",
        ui.pane_open(Pane::Terminal),
        Some("Ctrl+`"),
    ));
    if session.is_some() {
        row.push(toggle(
            DIFF_TOGGLE,
            icons::of(icons::DIFF),
            "Changes",
            ui.pane_open(Pane::Diff),
            Some("Ctrl+Shift+D"),
        ));
    }
    row.push(hgap(8.0));
    let centered = Element::row(vec![
        Element::spacer().flex(1.0),
        Element::row(heading).gap(4.0),
        Element::spacer().flex(1.0),
    ])
    .size(Some(main), Some(look::TOP_BAR));
    Element::stack(vec![
        Element::row(row).gap(4.0).size(None, Some(look::TOP_BAR)),
        Element::row(vec![centered, Element::spacer().flex(1.0)]),
    ])
    .size(None, Some(look::TOP_BAR))
}

/// Desktop's sidebar: its toggle, New and Search, the pinned
/// sessions, then the rest under a header per folder that folds them, the folder with the
/// newest session first.
fn sidebar(store: &Store, ui: &Ui, height: f32, palette: ComponentPalette) -> Element {
    let nav = |id: &str, icon: &'static str, label: &str, keys: &str, selected: bool| {
        side_row(
            id.into(),
            vec![
                Element::icon(icons::of(icon))
                    .foreground(look::palette().text)
                    .size(Some(16.0), Some(16.0))
                    .centered_y(),
                text(label).centered_y(),
                Element::spacer().flex(1.0),
                muted(keys)
                    .font_size(12.0)
                    .centered_y()
                    .reveal_on_hover(format!("{id}:keys")),
            ],
            selected,
            palette,
        )
    };
    let mut rows = vec![
        // Desktop's band holds its menu, this toggle and its history; quill keeps the toggle,
        // centered on the top bar's middle.
        Element::row(vec![hgap(5.0), sidebar_toggle(ui)]).size(None, Some(look::TOP_BAR - 16.0)),
        Element::spacer().size(None, Some(12.0)),
        // New is filled while its page shows, as Desktop's is.
        nav(
            NEW_SESSION,
            icons::ADD,
            "New",
            "Ctrl+N",
            ui.selected.is_none(),
        ),
        nav(SEARCH, icons::SEARCH, "Search", "Ctrl+K", false),
        nav(CUSTOMIZE_OPEN, icons::BRIEFCASE, "Customize", "", false),
        Element::spacer().size(None, Some(16.0)),
    ];
    // Sessions another one started are listed under it, not on their own, unless it was put
    // away.
    let listed = |pinned: bool| {
        sorted(
            store
                .sessions
                .iter()
                .filter(|s| {
                    s.pinned == pinned
                        && !s.archived
                        && (tree_parent(store, s).and_then(|p| store.session(p)))
                            .is_none_or(|p| p.archived)
                })
                .collect(),
            ui,
        )
    };
    let pinned = listed(true);
    if !pinned.is_empty() {
        // "Pinned" folds under the empty folder, which no session works in.
        let key = std::path::PathBuf::new();
        let open = !ui.collapsed.contains(&key);
        let waiting = !open && pinned.iter().any(|s| s.status == SessionStatus::Awaiting);
        rows.push(
            group_header("Pinned", &key, open, waiting, None, ui, palette)
                .animate_move("group:pinned"),
        );
        if open {
            for s in &pinned {
                rows.push(with_children(store, s, 0, ui, palette));
            }
        }
        rows.push(Element::spacer().size(None, Some(8.0)));
    }
    // Each group in the order of its first session, so the one with the newest leads.
    let rest = listed(false);
    let mut groups: Vec<(String, std::path::PathBuf, Option<&std::path::Path>)> = vec![];
    for session in &rest {
        let group = match ui.group_by {
            crate::GroupBy::Folder => (
                folder_name(&session.cwd),
                session.cwd.clone(),
                Some(session.cwd.as_path()),
            ),
            crate::GroupBy::Status => {
                let label = status_group(session);
                (label.into(), format!("status:{label}").into(), None)
            }
            // One header still holds the filter menu (inferred: Desktop's was not captured).
            crate::GroupBy::None => ("Sessions".into(), "all:".into(), None),
        };
        if !groups.iter().any(|g| g.1 == group.1) {
            groups.push(group);
        }
    }
    if ui.group_by == crate::GroupBy::Status {
        groups.sort_by_key(|g| STATUS_GROUPS.iter().position(|s| g.0 == *s));
    }
    for (at, (label, key, folder)) in groups.into_iter().enumerate() {
        let open = !ui.collapsed.contains(&key);
        // A little air between groups, as Desktop's.
        if at > 0 {
            rows.push(Element::spacer().size(None, Some(8.0)));
        }
        let members: Vec<&&Session> = rest
            .iter()
            .filter(|s| match ui.group_by {
                crate::GroupBy::Folder => s.cwd == key,
                crate::GroupBy::Status => status_group(s) == label,
                crate::GroupBy::None => true,
            })
            .collect();
        let waiting = !open && members.iter().any(|s| s.status == SessionStatus::Awaiting);
        // As the sessions reorder by activity, the groups and their rows glide to their places.
        rows.push(
            group_header(&label, &key, open, waiting, folder, ui, palette)
                .animate_move(format!("group:{}", key.display())),
        );
        if !open {
            continue;
        }
        // Completed shows its first few until "Show more" (inferred: five, as the videos
        // show four or five before it).
        let limited = ui.group_by == crate::GroupBy::Status
            && label == STATUS_GROUPS[2]
            && members.len() > COMPLETED_SHOWN;
        let shown = if limited && !ui.show_more {
            COMPLETED_SHOWN
        } else {
            members.len()
        };
        for s in &members[..shown] {
            rows.push(with_children(store, s, 0, ui, palette));
        }
        if limited {
            let label = if ui.show_more {
                "Show less"
            } else {
                "Show more"
            };
            rows.push(
                side_row(
                    SHOW_MORE.into(),
                    vec![muted(label).centered_y()],
                    false,
                    palette,
                )
                .animate_move(SHOW_MORE),
            );
        }
    }
    // At its foot over a line, the person: their GitHub picture (else their initials) and
    // name, opening the menu that holds Settings, as Desktop's. Its other entries are claude.ai's pages; quill keeps none. GitHub says who
    // it is (`gh`); without it the foot has quill's user and the menu offers to connect.
    let (initial, name, avatar) = crate::github::identity(ui);
    let mut account = vec![hgap(0.0)];
    let picture = avatar.and_then(|path| ui.thumbs.get(&path));
    account.push(match picture {
        Some(picture) => limn::Avatar::new(name.clone(), palette)
            .image(picture.clone())
            .size(20.0)
            .render()
            .centered_y(),
        None => Element::row(vec![muted(initial).font_size(10.0).centered().flex(1.0)])
            .size(Some(20.0), Some(20.0))
            .background(look::active())
            .radius(10.0)
            .centered_y(),
    });
    account.push(text(name).centered_y());
    account.push(
        Element::icon(icons::of(icons::CARET_DOWN))
            .foreground(look::muted())
            .size(Some(12.0), Some(12.0))
            .centered_y(),
    );
    account.push(hgap(0.0));
    let account = ComponentPalette {
        hover: look::row_hover(),
        ..palette
    }
    .interactive_surface(Element::clickable(
        ACCOUNT_MENU,
        Element::row(account).gap(6.0).size(None, Some(32.0)),
    ))
    .radius(8.0);
    let mut entries = vec![
        crate::github::menu_header(ui),
        limn::DropdownMenuEntry::Separator,
    ];
    entries.push(
        limn::DropdownMenuItem::new(SETTINGS_OPEN, "Settings")
            .icon(icons::of(icons::SETTINGS))
            .shortcut("Ctrl+,")
            .into(),
    );
    entries.push(
        limn::DropdownMenuItem::new(crate::tour::START, "Take the tour")
            .icon(icons::of(icons::QUESTION))
            .into(),
    );
    entries.extend(crate::github::menu_end(ui));
    let account = menu(
        account,
        entries,
        ui.account_menu,
        ACCOUNT_MENU,
        260.0,
        limn::PopoverPlacement::TopStart,
    );
    Element::column(
        vec![
            Element::column(rows)
                .gap(2.0)
                .padding(8.0)
                .flex(1.0)
                .clip_children(),
            hline(look::SIDEBAR_WIDTH - 1.0, look::divider()),
        ]
        .into_iter()
        .chain(crate::updates::sidebar_strip(ui, palette))
        .chain([Element::row(vec![account, Element::spacer().flex(1.0)]).padding(8.0)])
        .collect(),
    )
    .background(look::sidebar())
    .size(Some(look::SIDEBAR_WIDTH - 1.0), Some(height))
}

/// The groups when the list goes by status, in their order, as the person's newer Desktop
/// names them: waiting for the person, at work, and the rest.
const STATUS_GROUPS: [&str; 3] = ["Needs input", "Working", "Completed"];
/// How many sessions Completed lists before "Show more".
const COMPLETED_SHOWN: usize = 5;

fn status_group(session: &Session) -> &'static str {
    match session.status {
        SessionStatus::Awaiting => STATUS_GROUPS[0],
        SessionStatus::Running | SessionStatus::Starting => STATUS_GROUPS[1],
        _ => STATUS_GROUPS[2],
    }
}

/// How deep the sidebar nests sessions started by sessions; a damaged log's loop of links
/// ends here too.
const TREE_DEPTH: usize = 8;

/// The session the sidebar nests `session` under: the one that started it, else the one it
/// waits for, unless that one is under it already, so the tree has no loop.
fn tree_parent(store: &Store, session: &Session) -> Option<quill_model::SessionId> {
    if session.parent.is_some() {
        return session.parent;
    }
    let on = session.waits.first()?.on;
    let mut at = store.session(on);
    for _ in 0..TREE_DEPTH {
        let Some(above) = at else { break };
        if above.id == session.id {
            return None;
        }
        let next = above.parent.or(above.waits.first().map(|w| w.on));
        at = next.and_then(|id| store.session(id));
    }
    Some(on)
}

/// The sessions under `parent` in the sidebar, in the order they were made: those it started
/// and those that wait for it.
fn started_by(store: &Store, parent: quill_model::SessionId) -> Vec<&Session> {
    (store.sessions.iter())
        .filter(|s| tree_parent(store, s) == Some(parent) && !s.archived)
        .collect()
}

/// How many rows a session takes in the sidebar with those it started, and theirs.
fn tree_rows(store: &Store, session: &Session, depth: usize) -> usize {
    let below = started_by(store, session.id).into_iter();
    1 + if depth < TREE_DEPTH {
        below.map(|s| tree_rows(store, s, depth + 1)).sum()
    } else {
        0
    }
}

/// A session's row and, under it, the sessions it started and theirs, each on a guide that
/// turns into its row as a file tree's does, so the person sees which sessions work together.
/// They move as one when the list reorders.
fn with_children(
    store: &Store,
    session: &Session,
    depth: usize,
    ui: &Ui,
    palette: ComponentPalette,
) -> Element {
    let id = format!("family:{}", session.id);
    let row = session_row(session, depth, ui, palette);
    let children = started_by(store, session.id);
    if children.is_empty() || depth >= TREE_DEPTH {
        return row.animate_move(id);
    }
    let count = children.len();
    let line = |width: f32, height: f32| {
        Element::spacer()
            .background(look::divider())
            .size(Some(width), Some(height))
    };
    let blocks = children.iter().enumerate().map(|(at, child)| {
        // Rows are 32 tall with 2 between: the guide runs on past the gap to the next one,
        // and at the last it stops at its row's middle, where it turns.
        let down = if at + 1 == count {
            17.0
        } else {
            tree_rows(store, child, depth + 1) as f32 * 34.0
        };
        let guide = Element::row(vec![
            line(1.0, down),
            Element::column(vec![
                Element::spacer().size(None, Some(16.0)),
                line(5.0, 1.0),
            ]),
        ])
        .size(Some(6.0), None);
        Element::row(vec![
            guide,
            with_children(store, child, depth + 1, ui, palette).flex(1.0),
        ])
    });
    Element::column(vec![
        row,
        Element::row(vec![
            hgap(16.0),
            Element::column(blocks.collect()).flex(1.0),
        ]),
    ])
    .gap(2.0)
    .animate_move(id)
}

/// Every session in the order the sidebar lists them now, which it keeps while the pointer
/// is over it (`Ui::sidebar_order`).
pub fn order(store: &Store, ui: &Ui) -> Vec<quill_model::SessionId> {
    let list = sorted(store.sessions.iter().collect(), ui);
    list.into_iter().map(|s| s.id).collect()
}

/// The sessions in the order the filter menu asks: by last change, by when they were made,
/// or by name, newest or Z first unless ascending. While the pointer is over the sidebar the
/// order it had then holds, and a session made since goes last.
fn sorted<'a>(mut list: Vec<&'a Session>, ui: &Ui) -> Vec<&'a Session> {
    if let Some((_, order)) = &ui.sidebar_order {
        list.sort_by_key(|s| {
            order
                .iter()
                .position(|id| *id == s.id)
                .unwrap_or(usize::MAX)
        });
        return list;
    }
    match ui.sort_by {
        crate::SortBy::Updated => list.sort_by_key(|s| (s.touched, s.id)),
        crate::SortBy::Created => list.sort_by_key(|s| s.id),
        crate::SortBy::Name => list.sort_by_key(|s| session_title(s).to_lowercase()),
    }
    if !ui.ascending {
        list.reverse();
    }
    list
}

/// The person's plan as `session`'s agent, or the last one that said, reports it.
fn account_plan(store: &Store, session: Option<&Session>) -> Option<String> {
    session
        .and_then(|s| s.capabilities.as_ref())
        .or_else(|| {
            store
                .sessions
                .iter()
                .rev()
                .find_map(|s| s.capabilities.as_ref())
        })
        .and_then(|c| c.plan.clone())
        // The agent says "Claude Max"; Desktop's popover says "Max".
        .map(|p| effort_label(p.trim_start_matches("Claude ").trim()))
}

/// The filter menu of a group's header: Group by, Sort by and Reset to
/// defaults, with the current choices checked. Desktop's Show, New section and View all,
/// and grouping by date or environment, are not here: quill keeps no dates or sections and
/// runs on this computer only.
fn filter_menu(key: &str, ui: &Ui) -> Element {
    let id = format!("filter-menu:{key}");
    let open = ui.filter_menu.as_deref() == Some(key);
    let button = tool_surface(
        &id,
        icons::of(icons::FILTER),
        "Group and sort",
        22.0,
        open.then(look::active),
        look::tool_hover(),
        None,
    )
    .centered_y();
    let button = if open {
        button
    } else {
        button.reveal_on_hover(format!("{id}:reveal"))
    };
    let check = |id: &str, label: &str, on: bool| {
        limn::DropdownMenuEntry::from(limn::DropdownMenuItem::new(id, label).selected(on))
    };
    use crate::{GroupBy, SortBy};
    let group = match ui.group_by {
        GroupBy::Folder => "Folder",
        GroupBy::Status => "Status",
        GroupBy::None => "None",
    };
    let sort = match ui.sort_by {
        SortBy::Updated => "Last updated",
        SortBy::Created => "Date created",
        SortBy::Name => "Name",
    };
    let entries = vec![
        limn::DropdownMenuItem::new("filter:group", "Group by")
            .annotation(group)
            .submenu(vec![
                check("group-by:status", "Status", ui.group_by == GroupBy::Status),
                check("group-by:folder", "Folder", ui.group_by == GroupBy::Folder),
                limn::DropdownMenuEntry::Separator,
                check("group-by:none", "None", ui.group_by == GroupBy::None),
            ])
            .into(),
        limn::DropdownMenuItem::new("filter:sort", "Sort by")
            .annotation(sort)
            .submenu(vec![
                check(
                    "sort-by:updated",
                    "Last updated",
                    ui.sort_by == SortBy::Updated,
                ),
                check(
                    "sort-by:created",
                    "Date created",
                    ui.sort_by == SortBy::Created,
                ),
                check("sort-by:name", "Name", ui.sort_by == SortBy::Name),
                limn::DropdownMenuEntry::Separator,
                check("sort-way:descending", "Descending", !ui.ascending),
                check("sort-way:ascending", "Ascending", ui.ascending),
            ])
            .into(),
        limn::DropdownMenuEntry::Separator,
        limn::DropdownMenuItem::new(FILTER_RESET, "Reset to defaults")
            .enabled((ui.group_by, ui.sort_by, ui.ascending) != Default::default())
            .into(),
    ];
    menu(
        button,
        entries,
        open,
        &id,
        220.0,
        // Its right edge under the button's, as Desktop's.
        limn::PopoverPlacement::BottomEnd,
    )
    .centered_y()
}

/// A group's header, which folds it: under the pointer it fills, and offers its filter menu
/// and, for a folder, a new session there.
fn group_header(
    label: &str,
    key: &std::path::Path,
    open: bool,
    waiting: bool,
    folder: Option<&std::path::Path>,
    ui: &Ui,
    palette: ComponentPalette,
) -> Element {
    let folder_key = key;
    // The right panel's chevrons, the person's choice over Desktop's solid triangles.
    let caret = caret(open);
    // Status groups read as plain words, Folder ones as small gray names with a caret.
    let status = key.to_string_lossy().starts_with("status:");
    let mut items = if status {
        vec![hgap(4.0), text(label).centered_y()]
    } else {
        vec![hgap(4.0), muted(label).font_size(SMALL).centered_y(), caret]
    };
    // Folded, a group that holds a session waiting on the person says so beside its name.
    if waiting {
        items.push(
            Element::icon(icons::of(icons::WARNING))
                .foreground(look::palette().text)
                .size(Some(14.0), Some(14.0))
                .centered_y(),
        );
    }
    items.push(Element::spacer().flex(1.0));
    // Every header but Pinned's offers the list's filter menu.
    if !key.as_os_str().is_empty() {
        items.push(filter_menu(&key.display().to_string(), ui));
    }
    if let Some(folder) = folder {
        let id = format!("new-in:{}", folder.display());
        items.push(
            tool_surface(
                &id,
                icons::of(icons::ADD),
                "New session",
                22.0,
                None,
                look::tool_hover(),
                None,
            )
            .centered_y()
            .reveal_on_hover(format!("{id}:reveal")),
        );
    }
    items.push(hgap(4.0));
    let header = Element::row(items).gap(6.0).size(None, Some(28.0));
    ComponentPalette {
        hover: look::row_hover(),
        ..palette
    }
    .interactive_surface(Element::clickable(
        format!("group:{}", folder_key.display()),
        header,
    ))
    .radius(8.0)
    .hover_region()
}

/// A session's row: its title, its status, and under the pointer "···" with Desktop's
/// Pin, Rename and Delete. As the newer Desktop draws them: a working session's title is gray with a light passing over it, one
/// that needs the person ends in an amber dot, one that finished unseen in a blue one, and a
/// finished top-level one in the place it ran (this computer). The title keeps its weight.
fn session_row(session: &Session, depth: usize, ui: &Ui, palette: ComponentPalette) -> Element {
    let id = format!("row-menu:{}", session.id);
    let open = ui.row_menu == Some(session.id);
    let dots = tool_surface(
        &id,
        icons::of(icons::DOTS_HORIZONTAL),
        "More options",
        24.0,
        open.then(look::active),
        look::tool_hover(),
        None,
    );
    let mut entries = vec![
        open_in(session.id, ui),
        limn::DropdownMenuEntry::Separator,
        lettered(
            format!("pin:{}", session.id),
            if session.pinned { "Unpin" } else { "Pin" },
            "P",
        )
        .into(),
        lettered(
            format!("unread:{}", session.id),
            if session.unread {
                "Mark as read"
            } else {
                "Mark as unread"
            },
            "U",
        )
        .into(),
        lettered(format!("rename:{}", session.id), "Rename", "R").into(),
        lettered(format!("fork:{}", session.id), "Fork", "F").into(),
        limn::DropdownMenuEntry::Separator,
    ];
    entries.extend(transcript_view_entries(session, ui));
    entries.push(limn::DropdownMenuEntry::Separator);
    entries.extend(archive_and_delete(session));
    let dots = menu(
        dots,
        entries,
        open,
        &id,
        180.0,
        limn::PopoverPlacement::BottomEnd,
    )
    .centered_y();
    let dots = if open {
        dots
    } else {
        dots.reveal_on_hover(format!("{id}:reveal"))
    };
    let working = matches!(
        session.status,
        SessionStatus::Running | SessionStatus::Starting
    );
    let title = if working {
        Element::shimmer_truncated_text(
            format!("row-shimmer:{}", session.id),
            session_title(session),
            look::palette().text,
        )
        .foreground(look::muted())
    } else {
        Element::truncated_text(session_title(session)).foreground(look::palette().text)
    }
    .font_family(FontFamily::Sans)
    .font_size(BODY)
    .flex(1.0)
    .centered_y();
    let mark = |icon: &'static str, color: limn::Color| {
        Element::icon(icons::of(icon))
            .foreground(color)
            .size(Some(14.0), Some(14.0))
            .centered_y()
    };
    let indicator = match session.status {
        SessionStatus::Awaiting | SessionStatus::Failed(_) => status_dot(session),
        _ if session.unread && !working => status_dot(session),
        // It waits for another session's task to end (`wait_for_session`).
        _ if !session.waits.is_empty() => mark(icons::HOURGLASS, look::muted()),
        // Grouped by folder Desktop marks no place.
        _ if depth == 0 && !working && ui.group_by != crate::GroupBy::Folder => {
            mark(icons::LAPTOP, look::muted())
        }
        _ => hgap(0.0),
    };
    side_row_with(
        format!("session:{}", session.id),
        vec![title, indicator, dots],
        ui.selected == Some(session.id),
        Some(&id),
        palette,
    )
}

/// The sessions whose title holds `query`, as the search lists them: those that need the
/// person, those at work, then the rest, newest first.
pub fn found(store: &Store, query: &str) -> Vec<quill_model::SessionId> {
    let query = query.to_lowercase();
    let mut found: Vec<&Session> = store
        .sessions
        .iter()
        .rev()
        .filter(|s| session_title(s).to_lowercase().contains(&query))
        .collect();
    found.sort_by_key(|s| search_section(s));
    found.into_iter().map(|s| s.id).collect()
}

/// Where the search lists a session; an archived one is found only here, at the end.
fn search_section(session: &Session) -> usize {
    match session.status {
        _ if session.archived => 3,
        SessionStatus::Awaiting => 0,
        SessionStatus::Running | SessionStatus::Starting => 1,
        _ => 2,
    }
}

/// Desktop's search, Ctrl+K: a field, the sessions under "Needs
/// attention", "Active" and "Recents", and "New session"; Enter opens the first, marked ↵.
fn search_dialog(store: &Store, ui: &Ui, base: Element, _palette: ComponentPalette) -> Element {
    use quill_components::search::{self, Found};
    let Some(query) = &ui.search else {
        return base;
    };
    let mut sections: Vec<(String, Vec<Found>)> = vec![];
    for id in found(store, query) {
        let Some(session) = store.session(id) else {
            continue;
        };
        let title = ["Needs attention", "Active", "Recents", "Archived"][search_section(session)];
        if sections.last().is_none_or(|(t, _)| t != title) {
            sections.push((title.into(), vec![]));
        }
        let row = Found::new(format!("search:{id}"), session_title(session));
        sections.last_mut().expect("just pushed").1.push(row);
    }
    // With nothing found, the first row is "New session", which Enter starts.
    sections.push((
        "Actions".into(),
        vec![Found::new(SEARCH_NEW, "New session")],
    ));
    search::dialog(
        base,
        SEARCH_FIELD,
        SEARCH_CLOSE,
        query,
        "Search",
        sections,
        0,
    )
}

/// A sidebar row: 32 px tall, 10 px in, rounded, filled while selected. The rows of the
/// right panel's cards that open something are these too.
pub(crate) fn side_row(
    id: String,
    content: Vec<Element>,
    selected: bool,
    palette: ComponentPalette,
) -> Element {
    side_row_with(id, content, selected, None, palette)
}

/// A sidebar row that stays lit while the pointer is on `peer`, its "···", as Desktop's
/// keeps both the row and the button lit.
fn side_row_with(
    id: String,
    content: Vec<Element>,
    selected: bool,
    peer: Option<&str>,
    palette: ComponentPalette,
) -> Element {
    // Empty ends: the row's own gap is the 10 px inset, as Desktop's.
    let mut items = vec![hgap(0.0)];
    items.extend(content);
    items.push(hgap(0.0));
    let row = Element::row(items).gap(10.0).size(None, Some(32.0));
    // Under the pointer a row turns #EDEDED, a selected one from #E0E0E0 to #D5D5D5.
    let palette = ComponentPalette {
        hover: if selected {
            look::selected_hover()
        } else {
            look::row_hover()
        },
        ..palette
    };
    let row = palette.interactive_surface(Element::clickable(id, row));
    let row = match peer {
        Some(peer) => row.hover_with(peer),
        None => row,
    }
    .radius(8.0)
    .hover_region();
    if selected {
        row.background(look::selected())
    } else {
        row
    }
}

/// Running and awaiting sessions carry a dot in the sidebar, as Desktop's rows do, and so
/// does an unread one, in the accent; each says what it means to a screen reader.
fn status_dot(session: &Session) -> Element {
    let status = &session.status;
    let color = match status {
        SessionStatus::Running | SessionStatus::Starting => look::running(),
        SessionStatus::Awaiting => look::awaiting(),
        SessionStatus::Failed(_) => look::removed(),
        // A turn that ended while the person looked elsewhere: "Unread response".
        _ if session.unread => look::theme().accent,
        _ => return Element::spacer().size(Some(6.0), Some(6.0)),
    };
    let said = match status {
        SessionStatus::Running | SessionStatus::Starting => "Running",
        SessionStatus::Awaiting => "Needs your input",
        SessionStatus::Failed(_) => "Failed",
        _ => "Unread response",
    };
    // A running session's dot pulses, as Desktop's does.
    let running = matches!(status, SessionStatus::Running | SessionStatus::Starting);
    limn::StatusDot::new(color)
        .pulse(running)
        .size(6.0)
        .id(format!("dot:{}", session.id))
        .description(said)
        .render()
        .centered_y()
}

/// A new session's page greets the person centered just over the composer, 26 px: quill's
/// pen and words that follow the local hour and day (`greeting`). The person's place and size
/// for it, not Desktop's top corner.
fn empty(ui: &Ui) -> Element {
    let greeting = crate::greeting::now(ui.utc_offset, ui.name.as_deref());
    let line = Element::row(vec![
        Element::icon(icons::of(include_str!(
            "../../../apps/quill/assets/icons/quill-mark.svg"
        )))
        .foreground(look::palette().text)
        .large_icon(36.0)
        .centered_y(),
        text(greeting).font_size(26.0).centered_y(),
    ])
    .gap(10.0);
    Element::column(vec![
        Element::spacer().flex(1.0),
        line.centered_x(),
        Element::spacer().size(None, Some(28.0)),
    ])
}

fn transcript(
    store: &Store,
    session: &Session,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    // What is inside the column's 16 px sides: Desktop's text stands in from the composer's
    // edges. A wider width laid an item out wider than its place.
    let outer = column;
    let column = column - 32.0;
    // Desktop opens a session with where it started.
    let mut items: Vec<Element> = vec![muted(format!(
        "Started on this computer · {}",
        folder_name(&session.cwd)
    ))
    .font_size(12.0)];
    let busy = matches!(
        session.status,
        SessionStatus::Starting | SessionStatus::Running | SessionStatus::Awaiting
    );
    let (entries, running) = entries(store, session, &session.items, busy, ui, column, palette);
    items.extend(entries);
    // A plan the agent ended its turn with waits under the turn for the person's word.
    if let Some(call) = proposed_plan(session, ui) {
        items.push(plan_card(store, call, Some(session), ui, column, palette));
    }
    if !busy && !session.rewound.is_empty() {
        items.push(rewound_line());
    }
    // While the turn runs, its task list sits over the running line; it goes when the
    // turn ends.
    let list = todos(&session.items);
    if busy && !list.is_empty() {
        items.push(task_card(&list, column, palette));
    }
    items.extend(waiting_cards(store, session, ui, column, palette));
    items.extend(wait_rows(store, session, ui, column, palette));
    if let Some(status) = status_line(session, running.as_ref(), ui, column, palette) {
        items.push(status);
    }
    let content = Element::column(items)
        .gap(14.0)
        .size(Some(outer), None)
        .padding(16.0)
        .centered_x();
    // What runs under the composer fades out over 48 px, as above Desktop's.
    let scroll = Element::scroll(format!("transcript:{}", session.id), content)
        .top_fade(look::theme().background, 24.0)
        .bottom_fade(look::theme().background, 48.0);
    if ui.at_end {
        return scroll;
    }
    // Away from the end, Desktop floats a round "↓" over the conversation: 36 px,
    // 20 px over the composer, on the
    // page's fill with its edge and no halo, at the person's word. The arrow is centered by
    // hand, as `tool_surface` does.
    let arrow = Element::icon(icons::of(icons::ARROW_DOWN))
        .foreground(look::palette().text)
        .size(Some(16.0), Some(16.0))
        .centered();
    let down = ComponentPalette {
        hover: look::tool_hover(),
        ..palette
    }
    .interactive_surface(
        Element::clickable(SCROLL_END, arrow).accessibility_label("Scroll to bottom"),
    )
    .radius(18.0)
    .size(Some(36.0), Some(36.0));
    let down = Element::row(vec![down])
        .background(look::theme().background)
        .border(look::card_border(), 1.0)
        .radius(18.0)
        .size(Some(36.0), Some(36.0));
    let float = Element::column(vec![
        Element::spacer().flex(1.0),
        Element::row(vec![
            Element::spacer().flex(1.0),
            down,
            Element::spacer().flex(1.0),
        ]),
        Element::spacer().size(None, Some(20.0)),
    ]);
    Element::overlay(scroll, float)
}

/// What a list of transcript items shows: messages, cards, and tool calls in a row folded
/// into one group, as Desktop shows a turn's work; an answered permission
/// between them is gone, a waiting one ends the group. While `busy`, a last step still
/// running leaves its group and is returned for the running line.
fn entries<'a>(
    store: &Store,
    session: &'a Session,
    list: &'a [Item],
    busy: bool,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> (Vec<Element>, Option<Step<'a>>) {
    let mut items: Vec<Element> = vec![];
    let mut steps: Vec<Step> = vec![];
    let transcript = crate::current_view(session, &ui.settings);
    let flush = |steps: &mut Vec<Step>, items: &mut Vec<Element>| {
        if !steps.is_empty() {
            items.push(step_group(steps, list, ui, transcript, column, palette));
            steps.clear();
        }
    };
    let main = std::ptr::eq(list, session.items.as_slice());
    let mut waiting = None;
    let new_from = ui.new_from.get(&session.id).copied().filter(|_| main);
    // A turn's changes drawn with the reply before them, which they are then skipped as.
    let mut drawn = None;
    for (at, item) in list.iter().enumerate() {
        if drawn == Some(at) {
            continue;
        }
        if new_from == Some(at) && at > 0 {
            flush(&mut steps, &mut items);
            items.push(new_divider(column));
        }
        match item {
            // Thinking and Verbose show the model's words in their place, which splits the group.
            Item::Thinking { text: thought, .. }
                if transcript != TranscriptView::Normal && !thought.trim().is_empty() =>
            {
                flush(&mut steps, &mut items);
                items.push(thinking_block(session, at, thought, main, ui, column));
            }
            Item::Tool { name, .. } if is_task_tool(name) => {}
            Item::Tool {
                call,
                name,
                input,
                result,
            } => {
                let step = Step {
                    call,
                    name,
                    input,
                    result: result.as_ref(),
                    running: result.is_none() || session.task_running(call),
                };
                // A session the person asked for is a card of its own, as Desktop's
                // threads in the chat.
                if name.ends_with("sessions__start_session")
                    && input["initiation"] == "user_asked"
                    && input.get("proposal_id").is_none()
                {
                    flush(&mut steps, &mut items);
                    items.push(started_card(store, session, input, column));
                } else if let (true, Some(child)) = (
                    name.ends_with("sessions__send_to_session")
                        && result.as_ref().is_some_and(|r| !r.is_error),
                    child_named(
                        store,
                        session,
                        input["session"].as_str().unwrap_or_default(),
                    ),
                ) {
                    // Desktop's "Sent your message over to" and the session.
                    flush(&mut steps, &mut items);
                    items.push(
                        Element::row(vec![
                            muted("Passed your message to")
                                .font_size(chat())
                                .centered_y(),
                            session_chip(store, child).shrink_to_fit(),
                        ])
                        .gap(8.0),
                    );
                } else if name == "WebSearch"
                    && result.as_ref().is_none_or(|r| !r.is_error && !r.stopped)
                {
                    // Its sources as a block of their own; an error or a stop stays a step.
                    flush(&mut steps, &mut items);
                    items.push(web_search_block(&step, ui, transcript, column, palette));
                } else if let Some(opened) = opened_tab(name, result.as_ref()) {
                    // A tab the agent opened shows as a card under its step, as Desktop's.
                    steps.push(step);
                    flush(&mut steps, &mut items);
                    items.push(browser_card(session, &opened, column, palette));
                } else if name == "Compact" {
                    // The context was cut down: a marker where the conversation continues.
                    flush(&mut steps, &mut items);
                    items.push(compaction_divider(result.is_none(), column));
                } else if name == "ImageGeneration"
                    && step_image(name, input, result.as_ref()).is_some()
                {
                    // The picture it made, on its own.
                    flush(&mut steps, &mut items);
                    items.push(image_block(&step, ui, transcript, column, palette));
                } else if stands_alone(name) {
                    flush(&mut steps, &mut items);
                    if let Some(element) = lone_step(&step, column, palette) {
                        items.push(element);
                    }
                } else {
                    steps.push(step);
                }
            }
            Item::Permission {
                decided: Some(_), ..
            }
            | Item::Thinking { .. } => {}
            other => {
                // A step that waits on this permission runs under its card.
                if busy
                    && matches!(other, Item::Permission { decided: None, .. })
                    && steps.last().is_some_and(|s| s.result.is_none())
                {
                    waiting = steps.pop();
                }
                flush(&mut steps, &mut items);
                // Desktop sets each message apart: about 60 px between turns and
                // 39 px under the person's words; the gap adds to the column's own 14.
                let user = matches!(other, Item::User { .. });
                // A forwarded message's line goes with it, after the turn's space.
                let after_forward = at
                    .checked_sub(1)
                    .is_some_and(|i| matches!(list.get(i), Some(Item::Forwarded { .. })));
                let opens = matches!(other, Item::Forwarded { .. }) || (user && !after_forward);
                if opens && !items.is_empty() {
                    items.push(Element::spacer().size(None, Some(32.0)));
                }
                let element = self::item(other, store, session, ui, column, palette);
                // Under a turn's last reply, once the turn ended, its actions show while the
                // pointer is on it; the replies between its steps have none and
                // keep no room for them (inferred).
                let ends_turn = list[at + 1..]
                    .iter()
                    .find(|item| {
                        matches!(
                            item,
                            Item::Assistant { .. } | Item::User { .. } | Item::TurnEnd { .. }
                        )
                    })
                    .map_or(!busy, |item| matches!(item, Item::TurnEnd { .. }));
                let element = match other {
                    Item::Assistant {
                        streaming: false,
                        id,
                        at: when,
                        ..
                    } if main && ends_turn => {
                        // The turn's changes go between the reply and its actions, as
                        // Desktop's.
                        let changes = (at + 1..list.len())
                            .take_while(|&i| {
                                matches!(list[i], Item::TurnEnd { .. } | Item::Changes { .. })
                            })
                            .find(|&i| matches!(list[i], Item::Changes { .. }));
                        let mut parts = vec![element];
                        if let Some(i) = changes {
                            drawn = Some(i);
                            parts.push(Element::spacer().size(None, Some(8.0)));
                            parts.push(self::item(&list[i], store, session, ui, column, palette));
                        }
                        parts.push(reply_actions(session, at, id.is_some(), *when, ui));
                        Element::column(parts).gap(4.0).hover_region()
                    }
                    // Under the person's words, theirs: the time, copy, rewind and fork.
                    Item::User { at: when, .. } if main => {
                        Element::column(vec![element, user_actions(list, session, at, *when, ui)])
                            .gap(4.0)
                            .hover_region()
                    }
                    _ => element,
                };
                items.push(element);
                if user {
                    let sent = sent_from(list, at);
                    if sent > 0 {
                        items.push(sent_line(sent));
                    }
                    items.push(Element::spacer().size(None, Some(11.0)));
                }
            }
        }
    }
    let running = if busy && steps.last().is_some_and(|s| s.result.is_none()) {
        steps.pop()
    } else {
        waiting
    };
    flush(&mut steps, &mut items);
    (items, running)
}

/// What to call `agent` in a sentence: the first word of its name ("Claude Code" is "Claude"),
/// which is how Desktop says it; "the agent" before quill knows it.
pub(crate) fn agent_word(ui: &Ui, agent: &str) -> String {
    // shortcut: a name whose first word is not what people call it reads oddly, when an
    // adapter needs a short name of its own.
    ui.agents
        .iter()
        .find(|a| a.id == agent)
        .and_then(|a| a.name.split_whitespace().next())
        .unwrap_or("the agent")
        .to_owned()
}

/// What a permission card asks, in Desktop's words: Bash by what it runs, an edit by its
/// file.
fn permission_heading(who: &str, tool: &str, input: &Value) -> String {
    // The browser's own question, Desktop's site card, less the agent's
    // name.
    if tool == quill_model::SITE_PERMISSION {
        if input["kind"] == "server" {
            let name = input["name"].as_str().unwrap_or_default();
            return format!("Allow the agent to start the dev server {name}?");
        }
        let site = input["site"].as_str().unwrap_or_default();
        return format!("Allow the agent to use the browser on {site}?");
    }
    let path = input["file_path"]
        .as_str()
        .or(input["notebook_path"].as_str());
    match (tool, input["description"].as_str()) {
        ("Bash", Some(what)) => format!("Allow {who} to run {what}?"),
        // An agent that gives no description (Codex's commands): the command is in the card.
        ("Bash", None) if input["command"].is_string() => {
            format!("Allow {who} to run this command?")
        }
        ("Edit" | "MultiEdit" | "NotebookEdit" | "Write", _) if path.is_some() => {
            format!("Allow {who} to edit {}?", file_of(input))
        }
        _ => format!("Allow {who} to use {tool}?"),
    }
}

/// What a session that waits on the person asks of them, in a line: its question, a plan to
/// review, or the permission its card asks for.
fn asks(session: &Session, ui: &Ui) -> Option<String> {
    session.items.iter().rev().find_map(|item| match item {
        Item::Permission {
            tool,
            input,
            decided: None,
            ..
        } => Some(match tool.as_str() {
            "AskUserQuestion" => input["questions"][0]["question"]
                .as_str()
                .unwrap_or("A question for you")
                .to_owned(),
            "ExitPlanMode" => "A plan for you to review".to_owned(),
            _ => permission_heading(&agent_word(ui, &session.agent), tool, input),
        }),
        _ => None,
    })
}

/// A session named in the conversation: a blue chip with a dot
/// that opens it beside, and over it a card with how it is doing, how many replies it gave
/// and when the last came.
fn session_chip(store: &Store, id: quill_model::SessionId) -> Element {
    let Some(session) = store.session(id) else {
        return Element::spacer().size(Some(0.0), Some(0.0));
    };
    let title = session_title(session);
    let chip = Element::clickable(
        format!("thread:{id}"),
        Element::row(vec![
            hgap(6.0),
            Element::spacer()
                .background(look::mention())
                .radius(2.5)
                .size(Some(5.0), Some(5.0))
                .centered_y(),
            one_line(title.clone(), look::mention()),
            hgap(6.0),
        ])
        .gap(6.0)
        .size(None, Some(24.0))
        .background(look::mention_fill())
        .radius(5.0),
    )
    .accessibility_label(format!("Open {title}"));
    let state = match session.status {
        SessionStatus::Running | SessionStatus::Starting => "Working",
        SessionStatus::Awaiting => "Waiting on you",
        SessionStatus::Failed(_) => "Failed",
        _ => "Idle",
    };
    let replies = session
        .items
        .iter()
        .filter(|i| matches!(i, Item::Assistant { .. }))
        .count();
    let last = session.items.iter().rev().find_map(|i| match i {
        Item::Assistant { at: Some(at), .. } => Some(*at),
        _ => None,
    });
    let mut said = format!(
        "{replies} {}",
        if replies == 1 { "reply" } else { "replies" }
    );
    if let Some(at) = last {
        said = format!("{said} \u{b7} {}", ago(at));
    }
    let card = Element::column(vec![
        Element::row(vec![
            muted(state).font_size(SMALL).centered_y(),
            Element::spacer().flex(1.0),
            muted(said).font_size(SMALL).centered_y(),
        ]),
        Element::truncated_text(title)
            .font_family(FontFamily::Sans)
            .font_size(BODY)
            .foreground(look::palette().text),
    ])
    .gap(6.0)
    .padding(12.0);
    // Open as far as the host says: the tree shows it while the chip or the card is hovered.
    limn::Popover::new(
        chip,
        card,
        true,
        format!("thread:{id}:card"),
        look::menu_palette(),
    )
    .width(260.0)
    .placement(limn::PopoverPlacement::BottomStart)
    .render()
    .popover_on_hover()
    .popover_gap(6.0)
}

/// The session `session` started under `title`, as the agent names it in its tools.
fn child_named(store: &Store, session: &Session, title: &str) -> Option<quill_model::SessionId> {
    store
        .sessions
        .iter()
        .find(|s| {
            s.parent == Some(session.id) && !s.aside && session_title(s).eq_ignore_ascii_case(title)
        })
        .map(|s| s.id)
}

/// A session started at the person's asking, in the conversation that started it: a dot, its title and what it does now; it opens beside. Until the
/// engine has made it, it is "Starting…".
fn started_card(store: &Store, session: &Session, input: &Value, column: f32) -> Element {
    let title = input["title"].as_str().unwrap_or_default();
    let child = child_named(store, session, title).and_then(|id| store.session(id));
    let doing = match child {
        Some(child) => activity(child),
        None => Some("Starting\u{2026}".to_owned()),
    };
    let dot = match child.map(|c| &c.status) {
        Some(SessionStatus::Awaiting) => look::awaiting(),
        Some(SessionStatus::Running | SessionStatus::Starting) | None => look::mention(),
        _ => look::muted(),
    };
    let mut row = vec![
        Element::spacer()
            .background(dot)
            .radius(2.5)
            .size(Some(5.0), Some(5.0))
            .centered_y(),
        one_line(title.to_owned(), look::palette().text),
    ];
    if let Some(doing) = doing {
        row.push(
            Element::faded_text(doing, FADE)
                .font_family(FontFamily::Sans)
                .font_size(SMALL)
                .foreground(look::muted())
                .flex(1.0)
                .centered_y(),
        );
    }
    let width = column.min(480.0);
    let card = Element::row(row)
        .gap(10.0)
        .padding(12.0)
        .size(Some(width), Some(40.0))
        .border(look::card_border(), 1.0)
        .radius(10.0);
    match child {
        Some(child) => ComponentPalette {
            hover: look::row_hover(),
            ..look::palette()
        }
        .interactive_surface(Element::clickable(format!("thread:{}", child.id), card))
        .radius(10.0)
        .size(Some(width), Some(40.0)),
        None => card,
    }
}

/// Under a session, the sessions it started that wait on the person: each with what it asks and "View session", which opens it beside.
fn waiting_cards(
    store: &Store,
    session: &Session,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Vec<Element> {
    let width = column.min(480.0);
    store
        .sessions
        .iter()
        .filter(|s| {
            s.parent == Some(session.id)
                && !s.aside
                && !s.archived
                && s.status == SessionStatus::Awaiting
        })
        .map(|child| {
            let mut lines = vec![text(session_title(child))];
            if let Some(asks) = asks(child, ui) {
                lines.push(
                    Element::faded_text(asks, FADE)
                        .font_family(FontFamily::Sans)
                        .font_size(SMALL)
                        .foreground(look::muted()),
                );
            }
            lines.push(Element::row(vec![keyed_button(
                &format!("thread:{}", child.id),
                "View session",
                "",
                false,
                true,
                palette,
            )]));
            Element::row(vec![
                Element::icon(icons::of(icons::WARNING))
                    .foreground(look::awaiting())
                    .size(Some(16.0), Some(16.0)),
                Element::column(lines).gap(4.0).flex(1.0),
            ])
            .gap(10.0)
            .padding(14.0)
            .border(look::card_border(), 1.0)
            .radius(12.0)
            .size(Some(width), None)
        })
        .collect()
}

/// What a "Cancel" of a wait reports: the waiting session and the one it waits for follow.
pub const WAIT_CANCEL: &str = "wait-cancel:";

/// What the session waits for (`wait_for_session`), under its turns until the wait ends: an
/// hourglass, "Waiting for «title» · Agent", which shimmers while that session works, or that
/// it asks the person something or stopped before its task was done; and Cancel. One line,
/// faded at its end where the column is narrow.
fn wait_rows(
    store: &Store,
    session: &Session,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Vec<Element> {
    (session.waits.iter())
        .map(|wait| {
            let other = store.session(wait.on);
            let title = other.map_or_else(|| wait.title.clone(), session_title);
            let agent = other.map(|o| {
                let info = ui.agents.iter().find(|a| a.id == o.agent);
                format!(
                    " \u{b7} {}",
                    info.map_or(o.agent.as_str(), |a| a.name.as_str())
                )
            });
            let agent = agent.unwrap_or_default();
            let words = match other.map(|o| &o.status) {
                Some(SessionStatus::Running | SessionStatus::Starting) => working_words(
                    &format!("wait-shimmer:{}:{}", session.id, wait.on),
                    format!("Waiting for \u{ab}{title}\u{bb}{agent}"),
                ),
                Some(SessionStatus::Awaiting) => one_line(
                    format!("\u{ab}{title}\u{bb} is asking you something{agent}"),
                    look::muted(),
                ),
                _ => one_line(
                    format!("\u{ab}{title}\u{bb} stopped before its task was done{agent}"),
                    look::muted(),
                ),
            };
            let cancel = keyed_button(
                &format!("{WAIT_CANCEL}{}:{}", session.id, wait.on),
                "Cancel",
                "",
                false,
                true,
                palette,
            );
            Element::row(vec![
                Element::icon(icons::of(icons::HOURGLASS))
                    .foreground(look::muted())
                    .size(Some(14.0), Some(14.0))
                    .centered_y(),
                words.flex(1.0).centered_y(),
                cancel.centered_y(),
            ])
            .gap(8.0)
            .size(Some(column), Some(30.0))
        })
        .collect()
}

/// How many sessions the turn after the person's message at `at` started or passed it to.
pub(crate) fn sent_from(list: &[Item], at: usize) -> usize {
    list.iter()
        .skip(at + 1)
        .take_while(|item| !matches!(item, Item::User { .. }))
        .filter(|item| match item {
            Item::Tool {
                name,
                input,
                result: Some(result),
                ..
            } if !result.is_error => {
                name.ends_with("sessions__send_to_session")
                    || (name.ends_with("sessions__start_session")
                        && input["initiation"] != "own_initiative")
            }
            _ => false,
        })
        .count()
}

/// Under the person's message, where it went: "Sent to one
/// session", "Sent to 3 sessions".
fn sent_line(sent: usize) -> Element {
    let words = if sent == 1 {
        "Sent to one session".to_owned()
    } else {
        format!("Sent to {sent} sessions")
    };
    Element::row(vec![
        Element::spacer().flex(1.0),
        Element::icon(icons::of(icons::RETURN))
            .foreground(look::muted())
            .size(Some(12.0), Some(12.0))
            .centered_y(),
        muted(words).font_size(SMALL).centered_y(),
    ])
    .gap(5.0)
}

/// Where the agent compacted its context, between two hairlines (inferred design: Claude
/// Code's `compact_boundary` is not drawn, and Desktop's is not captured).
fn compaction_divider(running: bool, column: f32) -> Element {
    labeled_divider(
        if running {
            "Compacting context"
        } else {
            "Context compacted"
        },
        column,
    )
}

/// Where what the person had not seen begins: "New" between two hairlines, in the muted ink
/// of the other dividers (Desktop draws its own in orange).
fn new_divider(column: f32) -> Element {
    labeled_divider("New", column)
}

/// `words` between two hairlines across the column, in a 24 px row of its own, as tall as
/// the step lines around it, so neither reaches into it.
fn labeled_divider(words: &str, column: f32) -> Element {
    let line = || {
        Element::spacer()
            .size(None, Some(1.0))
            .background(look::divider())
            .flex(1.0)
            .centered_y()
    };
    Element::row(vec![
        line(),
        muted(words).font_size(SMALL).centered_y(),
        line(),
    ])
    .gap(10.0)
    .size(Some(column), Some(24.0))
}

/// The annotation editor over the window [spec §3 "Editor"]: the picture under the marks on the
/// same dimmed backdrop as the viewer, and the toolbar 24 px above the bottom. A click on the
/// backdrop, Escape and ✕ all ask to close.
fn annotator(ui: &Ui, base: Element, window: Size, palette: ComponentPalette) -> Element {
    let Some(path) = &ui.annotating else {
        return base;
    };
    let aspect = (ui.sizes.get(path)).map_or(1.0, |(width, height)| width / height);
    let shown = annotate::shown(aspect, window);
    let mut board = vec![
        Sketch::new(annotate::SKETCH, &ui.marks, ui.tool, ui.ink, ui.stroke)
            .size(shown.width, shown.height)
            .render(),
    ];
    if let Some(picture) = ui.full.get(path).or(ui.thumbs.get(path)) {
        board.insert(
            0,
            Element::raster_image(picture.clone().full_resolution())
                .radius(5.0)
                .size(Some(shown.width), Some(shown.height)),
        );
    }
    let board = Element::stack(board).size(Some(shown.width), Some(shown.height));
    let top = ((window.height - shown.height) / 2.0).max(0.0);
    let mut column = vec![
        Element::spacer().size(None, Some(top)),
        Element::row(vec![
            Element::spacer().flex(1.0),
            board,
            Element::spacer().flex(1.0),
        ])
        .size(None, Some(shown.height)),
        Element::spacer().flex(1.0),
    ];
    if let Some(notice) = &ui.image_notice {
        column.push(
            Element::row(vec![
                Element::spacer().flex(1.0),
                text(notice)
                    .font_size(BODY)
                    .foreground(limn::Color::rgb(255, 255, 255)),
                Element::spacer().flex(1.0),
            ])
            .size(None, Some(24.0)),
        );
    }
    column.push(
        Element::row(vec![
            Element::spacer().flex(1.0),
            annotate_toolbar(ui, palette),
            Element::spacer().flex(1.0),
        ])
        .size(None, Some(ANNOTATE_TOOLBAR)),
    );
    column.push(Element::spacer().size(None, Some(24.0)));
    let content = Element::stack(vec![
        Element::clickable(
            annotate::BACKDROP,
            Element::spacer().size(Some(window.width), Some(window.height)),
        ),
        Element::column(column).size(Some(window.width), Some(window.height)),
    ])
    .size(Some(window.width), Some(window.height));
    let editor = Element::popover(
        base,
        content,
        annotate::CLOSE,
        limn::PopoverPlacement::Center,
    )
    .popover_scrim(limn::Color::rgba(0, 0, 0, 230));
    // The question sits over the editor, as Desktop's does.
    confirm_keeping(
        editor,
        ui.discarding,
        "Discard your annotations?",
        "You have unsaved marks on this image. Discarding removes them.",
        "Discard",
        annotate::DISCARD,
        "Keep editing",
        annotate::KEEP,
    )
}

/// The editor's toolbar's height: 40 px, its buttons 32.
const ANNOTATE_TOOLBAR: f32 = 40.0;

/// The floating toolbar: Pen, Shapes, a line, Ink color and Line width, a line, then Undo,
/// Redo and Clear all once there is history, then Save and ✕.
// The text tool is not here: its text has to be painted into the pixels, which the sketch does not do.
fn annotate_toolbar(ui: &Ui, palette: ComponentPalette) -> Element {
    let ink = look::palette().text;
    let armed = |tool: limn::SketchTool| ui.tool == tool;
    let shape_name = |tool: limn::SketchTool| {
        annotate::SHAPES
            .iter()
            .find(|(_, shape)| *shape == tool)
            .map_or("Shape", |(name, _)| name)
    };
    let shapes = menu(
        Element::row(vec![
            tool_button(
                annotate::SHAPE,
                icon_glyph(icons::of(shape_icon(ui.last_shape)), ink),
                shape_name(ui.last_shape),
                32.0,
                !armed(limn::SketchTool::Pen),
                true,
            ),
            tool_button(
                annotate::SHAPES_MENU,
                icon_glyph(icons::of(icons::CARET_DOWN), ink),
                "Shapes",
                14.0,
                false,
                true,
            ),
        ]),
        annotate::SHAPES
            .iter()
            .enumerate()
            .map(|(i, (name, tool))| {
                limn::DropdownMenuItem::new(format!("{}{i}", annotate::SHAPE_PICK), *name)
                    .icon(icons::of(shape_icon(*tool)))
                    .selected(ui.last_shape == *tool)
                    .into()
            })
            .collect(),
        ui.open_menu == Some(annotate::SHAPES_MENU),
        annotate::SHAPES_MENU,
        160.0,
        limn::PopoverPlacement::TopStart,
    );
    let inks = menu(
        tool_button(
            annotate::INK_MENU,
            Element::row(vec![
                Element::spacer()
                    .size(Some(14.0), Some(14.0))
                    .background(ui.ink)
                    .radius(7.0)
                    .centered_y(),
                icon_glyph(icons::of(icons::CARET_DOWN), ink).centered_y(),
            ])
            .gap(2.0)
            .size(None, Some(16.0))
            .centered(),
            "Ink color",
            40.0,
            false,
            true,
        ),
        vec![limn::DropdownMenuEntry::Custom(Box::new(
            Element::row(
                annotate::INKS
                    .iter()
                    .enumerate()
                    .map(|(i, (name, color))| {
                        let dot = Element::spacer()
                            .size(Some(20.0), Some(20.0))
                            .background(*color)
                            .radius(10.0);
                        let dot = if ui.ink == *color {
                            dot.border(look::handle(), 2.0)
                        } else {
                            dot
                        };
                        Element::clickable(format!("{}{i}", annotate::INK_PICK), dot)
                            .accessibility_label(*name)
                    })
                    .collect(),
            )
            .gap(8.0)
            .padding(8.0),
        ))],
        ui.open_menu == Some(annotate::INK_MENU),
        annotate::INK_MENU,
        180.0,
        limn::PopoverPlacement::TopStart,
    );
    let widths = menu(
        tool_button(
            annotate::WIDTH_MENU,
            // The bar and the caret on one middle line, whatever the width.
            Element::row(vec![
                Element::spacer()
                    .size(Some(16.0), Some(ui.stroke))
                    .background(ink)
                    .radius(ui.stroke / 2.0)
                    .centered_y(),
                icon_glyph(icons::of(icons::CARET_DOWN), ink).centered_y(),
            ])
            .gap(2.0)
            .size(None, Some(16.0))
            .centered(),
            "Line width",
            40.0,
            false,
            true,
        ),
        annotate::WIDTHS
            .iter()
            .enumerate()
            .map(|(i, (name, width))| {
                limn::DropdownMenuItem::new(
                    format!("{}{i}", annotate::WIDTH_PICK),
                    format!("{name} {width}"),
                )
                .selected(ui.stroke == *width)
                .into()
            })
            .collect(),
        ui.open_menu == Some(annotate::WIDTH_MENU),
        annotate::WIDTH_MENU,
        160.0,
        limn::PopoverPlacement::TopStart,
    );
    let history = !ui.marks.is_empty() || !ui.redo.is_empty();
    let mut items = vec![
        tool_button(
            annotate::PEN,
            icon_glyph(icons::of(icons::PENCIL), ink),
            "Pen",
            32.0,
            armed(limn::SketchTool::Pen),
            true,
        ),
        shapes,
        vline(24.0),
        inks,
        widths,
        vline(24.0),
    ];
    if history {
        let marks = !ui.marks.is_empty();
        items.extend([
            tool_button(
                annotate::UNDO,
                icon_glyph(icons::of(icons::UNDO), ink),
                "Undo",
                32.0,
                false,
                marks,
            ),
            tool_button(
                annotate::REDO,
                icon_glyph(icons::of(icons::REDO), ink),
                "Redo",
                32.0,
                false,
                !ui.redo.is_empty(),
            ),
            tool_button(
                annotate::CLEAR,
                icon_glyph(icons::of(icons::TRASH), ink),
                "Clear all",
                32.0,
                false,
                marks,
            ),
        ]);
    }
    items.extend([
        keyed_button(
            annotate::SAVE,
            "Save",
            "",
            true,
            !ui.marks.is_empty() && !ui.saving,
            palette,
        ),
        tool_button(
            annotate::CLOSE,
            icon_glyph(icons::of(icons::CLOSE), ink),
            "Close",
            32.0,
            false,
            true,
        ),
    ]);
    let panel = look::menu_palette();
    Element::row(items)
        .gap(4.0)
        .padding(4.0)
        .background(panel.surface)
        .border(panel.border, 1.0)
        .radius(10.0)
        .size(None, Some(ANNOTATE_TOOLBAR))
}

/// A toolbar button: its icon or other glyph, pressed when `pressed`, and its title as the
/// tooltip. Disabled it is drawn without a click.
fn tool_button(
    id: &str,
    glyph: Element,
    label: &str,
    width: f32,
    pressed: bool,
    enabled: bool,
) -> Element {
    let hover = look::tool_hover();
    let palette = ComponentPalette {
        hover,
        selected: hover,
        ..look::palette()
    };
    let target = if enabled {
        Element::clickable(id, glyph)
    } else {
        glyph
    };
    let button = palette
        .interactive_surface(target.accessibility_label(label))
        .radius(6.0)
        .size(Some(width), Some(32.0));
    let button = if pressed {
        button.background(look::active())
    } else {
        button
    };
    limn::Tooltip::new(button, id, label, look::palette()).render()
}

/// An icon drawn in a toolbar button, centred in it.
fn icon_glyph(icon: limn::Icon, ink: limn::Color) -> Element {
    Element::icon(icon)
        .foreground(ink)
        .size(Some(16.0), Some(16.0))
        .centered()
}

/// The icon of a shape, which the Shapes button shows and its menu lists.
fn shape_icon(tool: limn::SketchTool) -> &'static str {
    match tool {
        limn::SketchTool::Line => icons::SHAPE_LINE,
        limn::SketchTool::Arrow => icons::SHAPE_ARROW,
        limn::SketchTool::Ellipse => icons::SHAPE_ELLIPSE,
        limn::SketchTool::Pen | limn::SketchTool::Rectangle => icons::SHAPE_RECTANGLE,
    }
}

/// The image viewer over the window: the picture as large as the window allows, up to one of
/// its pixels on each of the screen's, zoomed and panned as limn's lightbox does. Its thumbnail
/// stands in while the whole picture decodes.
fn image_viewer(ui: &Ui, base: Element, window: Size, palette: ComponentPalette) -> Element {
    let Some(viewer) = &ui.viewer else {
        return base;
    };
    // A step's picture is named by its step, as Desktop's are; another by its file.
    let name = match viewer.gallery.get(viewer.index) {
        Some((_, caption)) => caption.clone(),
        None => (viewer.path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    // limn's lightbox draws only the toggle of the three zoom controls.
    let mut shown = limn::ImageViewer::new(base, true, name, IMAGE_CLOSE, palette)
        .viewport(window)
        .layout(limn::ViewerLayout::Lightbox)
        .zoom_actions(IMAGE_ZOOM, IMAGE_ZOOM, IMAGE_ZOOM);
    if viewer.gallery.len() > 1 {
        shown = (shown.gallery(viewer.index, viewer.gallery.len()))
            .previous_action(IMAGE_PREVIOUS)
            .next_action(IMAGE_NEXT);
        for (at, (path, caption)) in viewer.gallery.iter().enumerate() {
            shown = shown.thumbnail(
                format!("{IMAGE_AT}{at}"),
                ui.thumbs.get(path).cloned(),
                caption,
            );
        }
    }
    if let Some(picture) = ui.full.get(&viewer.path) {
        shown = shown.image(picture.clone());
    }
    if let Some(thumb) = ui.thumbs.get(&viewer.path) {
        shown = shown.placeholder(thumb.clone());
    }
    // 100 % is one of the picture's pixels on each of the screen's, so it fits without being
    // enlarged past them.
    if let Some(&(width, height)) = ui.sizes.get(&viewer.path) {
        let scale = ui.pixels_per_point.max(0.25);
        shown = shown.picture_size(Size::new(width / scale, height / scale));
    }
    if let Some((scale, fitted)) = viewer.zoom {
        shown = shown.zoom_level(scale, fitted);
    }
    if let Some(origin) = viewer.origin {
        shown = shown.origin(origin);
    }
    if viewer.annotatable {
        shown = shown.action(IMAGE_ANNOTATE, limn::Icon::Edit, "Annotate image");
    }
    shown.render()
}

/// A link's right-click menu, where it was clicked: the page in quill's browser, in the
/// computer's own, or its address copied.
fn link_menu(ui: &Ui, base: Element, window: Size) -> Element {
    let Some((url, at)) = &ui.link_menu else {
        return base;
    };
    let entries = vec![
        limn::DropdownMenuItem::new(format!("{LINK_BROWSER}{url}"), "Open in Browser")
            .icon(icons::of(icons::GLOBE))
            .into(),
        limn::DropdownMenuItem::new(format!("{LINK_EXTERNAL}{url}"), "Open in default browser")
            .icon(icons::of(icons::ARROW_OUT))
            .into(),
        limn::DropdownMenuEntry::Separator,
        limn::DropdownMenuItem::new(format!("{LINK_COPY}{url}"), "Copy link")
            .icon(icons::of(icons::COPY))
            .into(),
    ];
    let anchor = Element::spacer().size(Some(1.0), Some(1.0));
    let menu = menu(
        anchor,
        entries,
        true,
        LINK_MENU,
        240.0,
        limn::PopoverPlacement::BottomStart,
    );
    let placed = Element::column(vec![
        Element::spacer().size(None, Some(at.y.max(0.0))),
        Element::row(vec![hgap(at.x.max(0.0)), menu]),
    ]);
    Element::stack(vec![base, placed]).size(Some(window.width), Some(window.height))
}

/// How long a copy button shows its check, as Desktop's.
pub const COPIED_FOR: std::time::Duration = std::time::Duration::from_millis(1200);

/// The row under a reply while the pointer is on it, in Desktop's order: copy, fork from here, then the time it came. Read aloud is the person's newer
/// Desktop's, with the system's own voice; its pin, a chapter
/// mark, is left out while quill has no chapters.
fn reply_actions(
    session: &Session,
    at: usize,
    forkable: bool,
    when: Option<u64>,
    ui: &Ui,
) -> Element {
    let button = |action: &str, icon: &'static str, label: &str| {
        tool_surface(
            &format!("{action}:{}:{at}", session.id),
            icons::of(icon),
            label,
            24.0,
            None,
            look::tool_hover(),
            None,
        )
    };
    let copy = format!("copy:{}:{at}", session.id);
    let copied = ui
        .copied
        .as_ref()
        .is_some_and(|(id, since)| *id == copy && since.elapsed() < COPIED_FOR);
    let mut row = vec![if copied {
        button("copy", icons::CHECK, "Copied")
    } else {
        button("copy", icons::COPY, "Copy")
    }];
    if forkable {
        row.push(button("fork-at", icons::FORK, "Fork from here"));
    }
    let reading = ui.reading == Some((session.id, at));
    row.push(if reading {
        button("read", icons::STOP_CIRCLE, "Stop reading")
    } else {
        button("read", icons::SPEAKER, "Read aloud")
    });
    if let Some(when) = when {
        row.push(hgap(5.0));
        row.push(muted(ago(when)).font_size(SMALL).centered_y());
    }
    let row = Element::row(row).size(None, Some(24.0));
    if reading || copied {
        row
    } else {
        row.reveal_on_hover(format!("{copy}:reveal"))
    }
}

/// Where a rewind took the messages out: its undo, the way the changes card's Undo is drawn
/// (inferred: Desktop's placement of "Undo rewind" was not captured).
fn rewound_line() -> Element {
    let undo = Element::clickable(
        REWIND_UNDO,
        Element::row(vec![
            hgap(6.0),
            muted("Undo rewind").font_size(SMALL).centered_y(),
            hgap(6.0),
        ])
        .size(None, Some(24.0)),
    )
    .hover_background(look::tool_hover())
    .radius(6.0);
    Element::row(vec![
        muted("Rewound to an earlier message.")
            .font_size(SMALL)
            .centered_y(),
        hgap(4.0),
        undo,
    ])
    .size(None, Some(24.0))
}

/// The row under the person's message while the pointer is on it, as Desktop's: when it was sent, copy, "Rewind to
/// here" (not while the agent works) and "Fork from here", which both go on from the
/// agent's message before it; the first message has nothing to fork from.
fn user_actions(
    list: &[Item],
    session: &Session,
    at: usize,
    when: Option<u64>,
    ui: &Ui,
) -> Element {
    let id = |action: &str| format!("{action}:{}:{at}", session.id);
    let button = |action: &str, icon: &'static str, label: &str| {
        tool_surface(
            &id(action),
            icons::of(icon),
            label,
            24.0,
            None,
            look::tool_hover(),
            None,
        )
    };
    let copy = id("copy");
    let copied =
        (ui.copied.as_ref()).is_some_and(|(c, since)| *c == copy && since.elapsed() < COPIED_FOR);
    let mut row = vec![Element::spacer().flex(1.0)];
    if let Some(when) = when {
        row.push(muted(ago(when)).font_size(SMALL).centered_y());
        row.push(hgap(5.0));
    }
    row.push(if copied {
        button("copy", icons::CHECK, "Copied")
    } else {
        button("copy", icons::COPY, "Copy message")
    });
    row.push(if session.working() {
        let mark = Element::row(vec![Element::icon(icons::of(icons::REWIND))
            .foreground(look::muted())
            .size(Some(16.0), Some(16.0))
            .centered()
            .flex(1.0)])
        .size(Some(24.0), Some(24.0));
        limn::Tooltip::new(
            Element::panel(mark),
            id("rewind-off"),
            "Rewind is unavailable while the agent is working",
            look::palette(),
        )
        .render()
    } else {
        button("rewind-to", icons::REWIND, "Rewind to here")
    });
    let earlier = (list[..at].iter()).any(|i| matches!(i, Item::Assistant { id: Some(_), .. }));
    if earlier {
        row.push(button("fork-from", icons::FORK, "Fork from here"));
    }
    let row = Element::row(row).size(None, Some(24.0));
    if copied {
        row
    } else {
        row.reveal_on_hover(format!("{copy}:reveal"))
    }
}

/// How long ago `when` (seconds since 1970) was, in words, as Desktop's row time writes it
/// ("15 hours ago"); the wording under a minute is inferred.
pub(crate) fn ago(when: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(when, |d| d.as_secs());
    ago_from(now.saturating_sub(when))
}

pub(crate) fn ago_from(seconds: u64) -> String {
    let (count, unit) = match seconds {
        0..=59 => return "just now".into(),
        60..=3599 => (seconds / 60, "minute"),
        3600..=86_399 => (seconds / 3600, "hour"),
        86_400..=2_591_999 => (seconds / 86_400, "day"),
        2_592_000..=31_535_999 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    let s = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{s} ago")
}

/// How far a transcript line that does not fit fades out at its end.
const FADE: f32 = 24.0;

/// One line of a transcript row in `ink`: as wide as its words while the row has room, and
/// fading out at its end once it has not, never wrapping into the rows around it or running
/// past the column.
fn one_line(words: impl Into<String>, ink: Color) -> Element {
    Element::faded_text(words, FADE)
        .font_family(FontFamily::Sans)
        .font_size(chat())
        .foreground(ink)
        .shrink_to_fit()
}

/// Words about work under way, shimmering as a working session's title does in the sidebar,
/// on one line as `one_line` keeps one.
fn working_words(id: &str, words: String) -> Element {
    Element::shimmer_faded_text(id.to_owned(), words, look::palette().text, FADE)
        .foreground(look::muted())
        .font_family(FontFamily::Sans)
        .font_size(chat())
        .shrink_to_fit()
}

/// The line under the turn: quill's pen writing (in place of Desktop's orange
/// dots, as the person chose), then the step running, or the
/// turn's clock, its tokens, the work running beside it and whether the model thinks, muted,
/// at the transcript's size. Waiting on the person it says so; with only background work
/// left, it counts it.
fn status_line(
    session: &Session,
    running: Option<&Step>,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Option<Element> {
    let tasks = session.tasks.iter().filter(|t| t.running).count();
    let tasks = match tasks {
        0 => None,
        1 => Some("1 running task".to_owned()),
        n => Some(format!("{n} running tasks")),
    };
    Some(match &session.status {
        // "Reading saludo.py 2s ▸": the verb muted, what it acts on in the text's ink, and
        // the step's own clock; under a permission it waits on,
        // the same.
        SessionStatus::Starting | SessionStatus::Running | SessionStatus::Awaiting
            if running.is_some() =>
        {
            let step = running?;
            let key = format!("tool:{}", step.call);
            let view = crate::current_view(session, &ui.settings);
            let open = is_open(view, ui, step.call);
            let (verb, object) = step_label(step, false);
            let words = if object.is_empty() {
                verb
            } else {
                format!("{verb} {object}")
            };
            let mut line = vec![working_words("status:step", words)];
            let since = session.step_since.map_or(0, |t| t.elapsed().as_secs());
            line.push(muted(elapsed(since)).font_size(chat()).centered_y());
            line.push(caret(open));
            let line = fold(view, key, Element::row(line).gap(8.0));
            // A picture being made holds its place, pulsing, until it comes.
            if step.name == "ImageGeneration" {
                let wait = format!("image:wait:{}", step.call);
                let place = Skeleton::block(wait, palette, GENERATED_TILE, GENERATED_TILE);
                return Some(Element::column(vec![line, place.render()]).gap(8.0));
            }
            if !open {
                return Some(line);
            }
            Element::column(vec![line, step_body(step, ui, column, palette)]).gap(8.0)
        }
        SessionStatus::Starting | SessionStatus::Running => {
            // The clock shows from five seconds on.
            let secs = session.running_since.map_or(0, |t| t.elapsed().as_secs());
            let mut parts = vec![];
            if secs >= 5 {
                parts.push(elapsed(secs));
            }
            if session.tokens > 0 {
                parts.push(tokens(session.tokens));
            }
            if session.thinking {
                parts.push("Thinking".into());
            }
            // Before its clock shows, the turn still says it works.
            if parts.is_empty() {
                parts.push("Working".into());
            }
            let mut line = vec![working_words("status:turn", parts.join(" · "))];
            // The running tasks open their pane.
            line.extend(tasks.map(|tasks| {
                Element::clickable(TASKS_OPEN, working_words("status:tasks", tasks))
                    .underline_on_hover()
                    .shrink_to_fit()
            }));
            Element::row(line).gap(12.0)
        }
        // A ringed orange dot and a white pill with an orange edge.
        SessionStatus::Awaiting => Element::row(vec![
            Element::panel(
                Element::spacer()
                    .background(look::awaiting())
                    .radius(4.0)
                    .size(Some(8.0), Some(8.0)),
            )
            .border(look::awaiting_edge(), 2.0)
            .radius(7.0)
            .padding(1.0)
            .centered_y(),
            Element::panel(Element::row(vec![
                hgap(5.0),
                text("Needs your input")
                    .font_size(12.0)
                    .foreground(look::manual()),
                hgap(5.0),
            ]))
            .border(look::awaiting_edge(), 1.0)
            .radius(10.0)
            .padding(2.0)
            .centered_y(),
        ])
        .gap(8.0),
        SessionStatus::Failed(error) => text(error.clone()).foreground(look::removed()),
        SessionStatus::Idle | SessionStatus::Exited => {
            let tasks = tasks?;
            Element::clickable(
                TASKS_OPEN,
                TextShimmer::new("status:tasks", tasks, palette)
                    .render()
                    .font_size(chat()),
            )
        }
    })
}

/// A turn's running time as Desktop writes it: "14s", "9m 4s", "1h 2m 3s".
pub fn elapsed(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m {}s", secs / 3600, secs % 3600 / 60, secs % 60),
    }
}

/// A token count as the running line writes it: "140 tokens", "30.7k tokens".
pub fn tokens(count: u64) -> String {
    match count {
        0..1000 => format!("{count} tokens"),
        // shortcut: the millions form is a guess; Desktop's turns were never seen that long.
        1000..1_000_000 => format!("{:.1}k tokens", count as f64 / 1000.0),
        _ => format!("{:.1}M tokens", count as f64 / 1_000_000.0),
    }
}

fn item(
    item: &Item,
    store: &Store,
    session: &Session,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    match item {
        Item::Notice { text } => muted(text.clone()),
        // Above a message another session passed on, as Desktop's "Message forwarded from
        // project chat"; the name opens that session.
        Item::Forwarded { from } => Element::row(vec![
            muted("Message forwarded from")
                .font_size(SMALL)
                .centered_y(),
            Element::clickable(
                format!("session:{from}"),
                Element::faded_text(
                    store
                        .session(*from)
                        .map_or_else(|| "another session".to_owned(), session_title),
                    FADE,
                )
                .font_family(FontFamily::Sans)
                .font_size(SMALL)
                .foreground(look::palette().text)
                .max_width(column * 0.6),
            )
            .centered_y(),
            Element::icon(icons::of(icons::CARET_RIGHT))
                .foreground(look::muted())
                .size(Some(12.0), Some(12.0))
                .centered_y(),
        ])
        .gap(5.0),
        Item::User {
            request,
            text: t,
            images,
            ..
        } => {
            // As wide as its words, up to 3/4 of the column, 16 px of side padding.
            let line = text(t.clone())
                .font_size(chat())
                .max_width(column * 0.75 - 32.0);
            let bubble = Element::panel(Element::row(vec![hgap(4.0), line, hgap(4.0)]))
                .background(look::user_bubble())
                .radius(14.0)
                .padding(12.0);
            let bubble = Element::row(vec![Element::spacer().flex(1.0), bubble]);
            if images.is_empty() {
                return bubble;
            }
            // The images sit above the words, right-aligned.
            let tiles = images.iter().enumerate().map(|(index, path)| {
                let id = format!("image:msg:{request}:{index}");
                image_tile(&id, path, ui, palette).open_action(id).render()
            });
            let tiles = Element::row(vec![
                Element::spacer().flex(1.0),
                Element::row(tiles.collect()).wrap().gap(8.0),
            ]);
            Element::column(vec![tiles, bubble]).gap(8.0)
        }
        Item::Assistant {
            text: t, streaming, ..
        } => {
            // Desktop's code: blocks in a white bordered box as wide as their
            // code, no language line, a copy button, 13 px; inline code on a light fill.
            let code_palette = ComponentPalette {
                raised: look::palette().surface,
                border: look::divider(),
                ..palette
            };
            let colors = limn::MarkdownColors {
                code_ink: look::palette().text,
                code_fill: look::command_box(),
                ..limn::MarkdownColors::for_palette(code_palette)
            };
            Markdown::new(t, code_palette)
                .width(column)
                .font_size(chat())
                .colors(colors)
                .link_action_prefix(LINK)
                .code_label(false)
                .code_fit_content(true)
                .code_copy_action_prefix(COPY_CODE)
                .code_font_size(SMALL)
                .inline_code_size(SMALL)
                .code_syntax_colors(limn::CodeSyntaxColors {
                    text: look::palette().text,
                    string: look::added(),
                    keyword: look::keyword(),
                    function: Some(look::cursor()),
                    interpolation: Some(look::removed()),
                    ..limn::CodeSyntaxColors::for_palette(code_palette)
                })
                .streaming(*streaming)
                // New words come in out of a fade.
                .streaming_fade(true)
                .render()
                .element
        }
        Item::Tool { .. } => Element::spacer().size(None, Some(0.0)),
        Item::Permission {
            id,
            input,
            interactive: true,
            decided: None,
            ..
        } if input["questions"].is_array() => question_card(id, input, ui, column, palette),
        Item::Permission {
            id,
            tool,
            decided: None,
            ..
        } if tool == "ExitPlanMode" => plan_card(store, id, Some(session), ui, column, palette),
        Item::Permission { .. } => permission(item, &session.agent, ui, column, palette),
        Item::TurnEnd {
            ok: false,
            interrupted: true,
            ..
        } => muted("Interrupted"),
        Item::TurnEnd {
            ok: false,
            sign_in: true,
            ..
        } => Element::row(vec![
            text("Not signed in")
                .foreground(look::removed())
                .centered_y(),
            keyed_button(crate::account::SIGN_IN, "Sign in", "", false, true, palette),
        ])
        .gap(12.0),
        Item::TurnEnd {
            ok: false, error, ..
        } => text(error.clone().unwrap_or_else(|| "The turn failed".into()))
            .foreground(look::removed()),
        Item::TurnEnd { .. } | Item::Thinking { .. } => Element::spacer().size(None, Some(0.0)),
        Item::Changes {
            request,
            files,
            rewound,
        } => changes_card(
            request,
            files,
            *rewound,
            ui,
            crate::current_view(session, &ui.settings),
            column,
        ),
        Item::Proposals { tasks } => proposals_card(store, tasks, palette),
        Item::Reports { request, reports } => {
            reports_block(store, request, reports, session, ui, column)
        }
    }
}

/// One tool call of a turn, as its group lists it.
pub struct Step<'a> {
    pub call: &'a str,
    pub name: &'a str,
    pub input: &'a Value,
    pub result: Option<&'a ToolResult>,
    /// It has not answered yet, or the work it started still runs (a helper agent).
    pub running: bool,
}

/// The task tools keep the list shown over the running line; Desktop lists no step for them.
pub fn is_task_tool(name: &str) -> bool {
    matches!(
        name,
        "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskGet" | "TaskList"
    )
}

/// Steps Desktop sets on their own line, out of any group.
fn stands_alone(name: &str) -> bool {
    matches!(name, "AskUserQuestion" | "ExitPlanMode")
}

/// The picture a step made or looked at: a generated image's saved file (its result's file)
/// or the file a Read opened, when that is an image.
pub fn step_image(name: &str, input: &Value, result: Option<&ToolResult>) -> Option<PathBuf> {
    match name {
        "ImageGeneration" => result
            .filter(|r| !r.is_error)?
            .files
            .first()
            .map(|f| f.path.clone()),
        "Read" => {
            let path = Path::new(input["file_path"].as_str()?);
            let ext = path.extension()?.to_str()?.to_lowercase();
            ["png", "jpg", "jpeg", "gif", "webp"]
                .contains(&ext.as_str())
                .then(|| path.to_path_buf())
        }
        _ => None,
    }
}

/// The pictures of a step's row: the ones its result gave back, else the image file a Read
/// opened (Codex sends only its path). A failed step shows none, as Desktop's.
pub fn step_pictures(name: &str, input: &Value, result: Option<&ToolResult>) -> Vec<PathBuf> {
    match result {
        Some(r) if r.is_error => vec![],
        Some(r) if !r.images.is_empty() => r.images.clone(),
        Some(_) if name == "Read" => step_image(name, input, result).into_iter().collect(),
        _ => vec![],
    }
}

/// One picture of a run of steps: its step's call and its place among that step's, the file,
/// and the step's label done ("Captured page"), which names it.
pub struct StepPicture {
    pub call: String,
    pub at: usize,
    pub path: PathBuf,
    pub caption: String,
}

/// The pictures of the run of steps `call` is in: the tool calls between two messages, which
/// Desktop's viewer steps through and counts. `list` is the transcript or a helper's.
pub fn run_pictures(list: &[Item], call: &str) -> Vec<StepPicture> {
    let in_run = |item: &Item| {
        matches!(
            item,
            Item::Tool { .. } | Item::Thinking { .. } | Item::Permission { .. }
        )
    };
    let Some(at) = (list.iter()).position(|i| matches!(i, Item::Tool { call: c, .. } if c == call))
    else {
        return vec![];
    };
    let start = list[..at]
        .iter()
        .rposition(|i| !in_run(i))
        .map_or(0, |i| i + 1);
    let end = list[at..]
        .iter()
        .position(|i| !in_run(i))
        .map_or(list.len(), |i| at + i);
    let mut run = vec![];
    for item in &list[start..end] {
        let Item::Tool {
            call,
            name,
            input,
            result,
        } = item
        else {
            continue;
        };
        let pictures = step_pictures(name, input, result.as_ref());
        if pictures.is_empty() {
            continue;
        }
        let step = Step {
            call,
            name,
            input,
            result: result.as_ref(),
            running: false,
        };
        let (verb, object) = step_label(&step, true);
        let caption = format!("{verb} {object}").trim_end().to_owned();
        run.extend(
            pictures
                .into_iter()
                .enumerate()
                .map(|(at, path)| StepPicture {
                    call: call.clone(),
                    at,
                    path,
                    caption: caption.clone(),
                }),
        );
    }
    run
}

/// The pictures of `run` that `own` picks, as a row's end shows them: the last three, "+N"
/// before them for the rest. `None` when it picks none.
fn picture_strip(
    run: &[StepPicture],
    own: impl Fn(&StepPicture) -> bool,
    ui: &Ui,
) -> Option<Element> {
    let mine: Vec<usize> = (0..run.len()).filter(|&i| own(&run[i])).collect();
    if mine.is_empty() {
        return None;
    }
    let hidden = mine.len().saturating_sub(THUMBS_SHOWN);
    let mut strip = vec![];
    if hidden > 0 {
        strip.push(muted(format!("+{hidden}")).font_size(chat()).centered_y());
    }
    let thumbs = mine[hidden..].iter().map(|&i| thumbnail(run, i, ui));
    strip.push(Element::row(thumbs.collect()).gap(4.0).centered_y());
    Some(Element::row(strip).gap(8.0))
}

/// The picture `index` of `run` as a strip shows it, which a click opens in the viewer and the
/// pointer, resting on it, opens larger on a card above it: the step's label and the picture's
/// place in the run under it, as Desktop's.
fn thumbnail(run: &[StepPicture], index: usize, ui: &Ui) -> Element {
    let picture = &run[index];
    let id = format!("image:step:{}:{}", picture.call, picture.at);
    // A screenshot's shape until the host has read the file's.
    let aspect =
        (ui.sizes.get(&picture.path)).map_or(16.0 / 9.0, |&(width, height)| width / height);
    let fill = |image: Option<&limn::RasterImage>, cover: bool, width: f32, height: f32| {
        let image = match image {
            Some(image) if cover => Element::raster_image(image.clone().cover()),
            Some(image) => Element::raster_image(image.clone()),
            None => Element::spacer(),
        };
        Element::panel(image.size(Some(width), Some(height)))
            .background(look::command_box())
            .size(Some(width), Some(height))
    };
    let count = format!("{} of {}", index + 1, run.len());
    let named = if run.len() > 1 {
        format!("{}, {count}", picture.caption)
    } else {
        picture.caption.clone()
    };
    let width = (THUMB_HEIGHT * aspect).clamp(1.0, THUMB_WIDTH);
    let thumb = Element::clickable(
        &id,
        fill(ui.thumbs.get(&picture.path), true, width, THUMB_HEIGHT)
            .border(look::card_border(), 1.0)
            .radius(4.0)
            .clip_children(),
    )
    .accessibility_label(named);
    let mut line = vec![Element::truncated_text(picture.caption.clone())
        .font_family(FontFamily::Sans)
        .font_size(SMALL)
        .foreground(look::palette().text)
        .flex(1.0)];
    if run.len() > 1 {
        line.push(muted(count).font_size(SMALL));
    }
    // shortcut: the card enlarges the 256 px thumbnail a little, soft on a dense screen; a
    // larger decode of step pictures would sharpen it.
    let big = ui.full.get(&picture.path).or(ui.thumbs.get(&picture.path));
    // As wide as the card; a picture too tall for it is fitted in, on the neutral fill.
    let height = PREVIEW_WIDTH / aspect;
    let shown = fill(
        big,
        height <= PREVIEW_HEIGHT,
        PREVIEW_WIDTH,
        height.min(PREVIEW_HEIGHT),
    );
    let menu = look::menu_palette();
    let card = Element::panel(Element::column(vec![
        shown,
        pad(Element::row(line).gap(8.0), 12.0, 8.0),
    ]))
    .size(Some(PREVIEW_WIDTH), None)
    .background(menu.raised)
    .border(menu.border, 1.0)
    .radius(10.0)
    .shadow(menu.shadow, limn::Point::new(0.0, 9.0), 22.0, 0.0)
    .clip_children();
    Element::popover(
        thumb,
        card,
        format!("{id}:card"),
        limn::PopoverPlacement::TopEnd,
    )
    .popover_on_hover()
    .popover_gap(6.0)
}

/// A generated image: its line, which opens the prompt it was made from, over its picture,
/// which a click opens in the viewer (inferred design: Desktop makes no pictures).
fn image_block(
    step: &Step,
    ui: &Ui,
    view: TranscriptView,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let path = step_image(step.name, step.input, step.result).unwrap_or_default();
    let id = format!("image:tool:{}", step.call);
    let key = format!("tool:{}", step.call);
    let open = is_open(view, ui, step.call);
    let line = Element::row(vec![
        muted("Generated an image").font_size(chat()).centered_y(),
        caret(open),
    ])
    .gap(8.0)
    .size(None, Some(24.0));
    // Codex's prompt is a brief of a dozen lines: it waits behind the line.
    let prompt = (step.result.map(|r| r.output.trim()))
        .filter(|prompt| open && !prompt.is_empty())
        .map(|prompt| {
            Element::column(vec![
                Element::spacer().size(None, Some(4.0)),
                muted(prompt.to_owned())
                    .font_size(SMALL)
                    .size(Some(column.min(480.0)), None),
            ])
        });
    Element::column(vec![
        fold(view, key.clone(), line),
        limn::Collapse::new(format!("{key}:body"), prompt)
            .expand()
            .render(),
        Element::spacer().size(None, Some(6.0)),
        picture_tile(&id, &path, GENERATED_TILE, ui, palette),
    ])
}

/// quill's web search block: a pill that says what was searched, with the sources as cards
/// under it. The design follows rareui's WebSearch (rareui, `web-search.tsx`; its license is
/// not checked yet), with quill's own colors. Desktop draws a WebSearch as a plain step
/// instead. Motion, citation chips and favicons are not here yet.
fn web_search_block(
    step: &Step,
    ui: &Ui,
    transcript: TranscriptView,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let key = format!("web:{}", step.call);
    let more = format!("{key}:more");
    let sources: &[WebSource] = step
        .result
        .and_then(|r| r.web.as_ref())
        .map_or(&[][..], |w| w.sources.as_slice());
    let searching = step.running || step.result.is_none();
    // The panel starts open, so the key's presence means closed; Verbose holds it open.
    let open = transcript == TranscriptView::Verbose || !ui.expanded.contains(&key);
    let query = step.input["query"].as_str().unwrap_or_default();
    let count = sources.len();
    let sites = if count == 1 { "site" } else { "sites" };
    let label = if searching && query.is_empty() {
        "Searching the web".to_owned()
    } else if searching {
        format!("Searching \u{201c}{query}\u{201d}")
    } else {
        format!("Searched {count} {sites}")
    };
    let label = if searching {
        working_words(&format!("{key}:label"), label).font_size(13.0)
    } else {
        one_line(label, look::palette().text).font_family(FontFamily::SansMedium)
    };
    let mut globe = vec![Element::icon(icons::of(icons::GLOBE))
        .foreground(look::muted())
        .size(Some(15.0), Some(15.0))
        .centered()];
    if searching {
        globe.push(
            Element::progress_ring(Some(0.08), palette.selected, palette.text)
                .size(Some(28.0), Some(28.0)),
        );
    }
    let globe = Element::stack(globe)
        .background(look::composer_hover())
        .radius(14.0)
        .size(Some(28.0), Some(28.0));
    let mut parts = vec![globe, label];
    if !sources.is_empty() {
        let tiles = sources.iter().map(|s| letter_tile(&s.url, 20.0)).collect();
        parts.push(
            AvatarGroup::new(tiles, look::palette())
                .limit(4)
                .size(20.0)
                .ring(look::command_box())
                .render()
                .centered_y(),
        );
    }
    let caret = if open {
        icons::CARET_UP
    } else {
        icons::CARET_DOWN
    };
    parts.push(
        Element::icon(icons::of(caret))
            .foreground(look::muted())
            .size(Some(14.0), Some(14.0))
            .centered_y(),
    );
    // The pill is 40 tall with 6 padding and 14 on the right: 8 more past the chevron.
    // Each level gives way to a long query, so the pill keeps to the column.
    let content = Element::row(vec![
        Element::row(parts).gap(10.0).shrink_to_fit(),
        hgap(8.0),
    ])
    .padding(6.0);
    let hover = ComponentPalette {
        hover: look::tool_hover(),
        selected: look::tool_hover(),
        ..look::palette()
    };
    let pill = hover
        .interactive_surface(
            fold(transcript, key.clone(), content)
                .background(look::command_box())
                .size(None, Some(40.0)),
        )
        .radius(20.0)
        .shrink_to_fit();
    // The pill hugs what it says, as the reference's does.
    let mut block = vec![Element::row(vec![pill, Element::spacer().flex(1.0)])];
    if open && (searching || count > 0) {
        // Four columns, two in a narrow transcript; the block is at most 560 wide.
        let width = column.min(560.0);
        let columns = if width < 400.0 { 2 } else { 4 };
        let cells = if searching {
            (0..4)
                .map(|at| skeleton_cell(step.call, at, palette))
                .collect()
        } else {
            let expanded = ui.expanded.contains(&more);
            let shown = if count <= 4 || expanded { count } else { 3 };
            let mut cells: Vec<Element> = sources
                .iter()
                .enumerate()
                .take(shown)
                .map(|(at, source)| source_card(step.call, at, source, palette))
                .collect();
            if count > 4 {
                cells.push(if expanded {
                    show_less(&more)
                } else {
                    more_card(&more, &sources[3..], palette)
                });
            }
            cells
        };
        block.push(
            Element::column(vec![
                Element::spacer().size(None, Some(10.0)),
                grid(cells, columns),
            ])
            .size(Some(width), None),
        );
    }
    Element::column(block)
}

/// Cards in rows of `columns`, equal in width; a short last row keeps the same widths.
fn grid(cells: Vec<Element>, columns: usize) -> Element {
    let mut rows = vec![];
    let mut row = vec![];
    for cell in cells {
        row.push(cell);
        if row.len() == columns {
            rows.push(Element::row(std::mem::take(&mut row)).gap(8.0));
        }
    }
    if !row.is_empty() {
        while row.len() < columns {
            row.push(Element::spacer().flex(1.0));
        }
        rows.push(Element::row(row).gap(8.0));
    }
    Element::column(rows).gap(8.0)
}

/// A source's card: its title in two lines, then its site's letter, its name and its number.
/// A click opens the page as a link of the conversation does (`LINK`), so the right-click
/// menu of a link works on it too.
fn source_card(call: &str, at: usize, source: &WebSource, palette: ComponentPalette) -> Element {
    let ink = look::palette().text;
    let faint = look::muted();
    let title = if source.title.trim().is_empty() {
        host(&source.url).unwrap_or_default()
    } else {
        source.title.clone()
    };
    // Two lines at most, cut short: a long title never runs into the row under it.
    let content = Element::column(vec![
        Element::formatted_text(vec![limn::TextSpan::new(
            title.clone(),
            Color::rgba(ink.r, ink.g, ink.b, 217),
            12.5,
            FontFamily::SansMedium,
        )])
        .line_height(17.0)
        .max_lines(2)
        .size(None, Some(34.0))
        .clip_children(),
        Element::spacer().flex(1.0),
        Element::row(vec![
            letter_tile(&source.url, 16.0).render(),
            Element::truncated_text(site_name(&source.url))
                .font_family(FontFamily::Sans)
                .font_size(11.5)
                .foreground(faint)
                .flex(1.0)
                .centered_y(),
            Element::text((at + 1).to_string())
                .font_family(FontFamily::SansMedium)
                .font_size(11.0)
                .foreground(Color::rgba(faint.r, faint.g, faint.b, 178))
                .centered_y(),
        ])
        .gap(6.0),
    ])
    .size(None, Some(CARD_HEIGHT - 20.0));
    let card = Element::clickable(format!("{LINK}{call}:{at}:{}", source.url), content)
        .accessibility_label(title)
        .padding(10.0)
        .background(look::command_box())
        .radius(14.0)
        .size(None, Some(CARD_HEIGHT))
        .flex(1.0);
    ComponentPalette {
        hover: look::tool_hover(),
        selected: look::tool_hover(),
        ..palette
    }
    .interactive_surface(card)
    .radius(14.0)
}

/// "N more sources": the sites of the cards it hides, and a click shows them all.
fn more_card(key: &str, hidden: &[WebSource], palette: ComponentPalette) -> Element {
    let tiles = hidden.iter().map(|s| letter_tile(&s.url, 20.0)).collect();
    let count = hidden.len();
    let content = Element::column(vec![
        AvatarGroup::new(tiles, palette)
            .limit(4)
            .size(20.0)
            .ring(look::command_box())
            .render(),
        Element::spacer().flex(1.0),
        Element::text(format!(
            "{count} more {}",
            if count == 1 { "source" } else { "sources" }
        ))
        .font_family(FontFamily::SansMedium)
        .font_size(SMALL)
        .foreground(look::palette().text),
    ])
    .padding(10.0)
    .size(None, Some(CARD_HEIGHT));
    let card = Element::clickable(key, content)
        .accessibility_label(format!("{count} more sources"))
        .background(look::command_box())
        .radius(14.0)
        .size(None, Some(CARD_HEIGHT))
        .flex(1.0);
    ComponentPalette {
        hover: look::tool_hover(),
        selected: look::tool_hover(),
        ..palette
    }
    .interactive_surface(card)
    .radius(14.0)
}

/// The last cell of the expanded grid: an outlined "Show less" that folds the cards back.
fn show_less(key: &str) -> Element {
    let content = Element::row(vec![
        Element::icon(icons::of(icons::CARET_UP))
            .foreground(look::muted())
            .size(Some(12.0), Some(12.0))
            .centered_y(),
        muted("Show less")
            .font_family(FontFamily::SansMedium)
            .font_size(SMALL)
            .centered_y(),
    ])
    .gap(6.0)
    .centered();
    Element::clickable(key, content)
        .accessibility_label("Show less")
        .border(look::divider(), 1.0)
        .radius(14.0)
        .size(None, Some(CARD_HEIGHT))
        .flex(1.0)
}

/// A card while its search runs: two bars, and a dot with a short bar at the foot.
fn skeleton_cell(call: &str, at: usize, palette: ComponentPalette) -> Element {
    let id = |part: &str| format!("{call}:web-skeleton:{at}:{part}");
    let bar = |part: &str, share: f32| {
        Element::row(vec![
            Skeleton::new(id(part), palette)
                .height(10.0)
                .render()
                .flex(share),
            Element::spacer().flex(1.0 - share),
        ])
    };
    let foot = Element::row(vec![
        Skeleton::circle(id("dot"), palette, 14.0).render(),
        Skeleton::new(id("tag"), palette)
            .width(48.0)
            .height(8.0)
            .render(),
    ])
    .gap(6.0);
    Element::column(vec![
        bar("top", 0.88),
        bar("low", 0.6),
        Element::spacer().flex(1.0),
        foot,
    ])
    .gap(6.0)
    .padding(10.0)
    .background(look::command_box())
    .radius(14.0)
    .size(None, Some(CARD_HEIGHT))
    .flex(1.0)
}

/// A letter tile for a source's site: its initial on the site's own hue, white (limn's Avatar).
fn letter_tile(url: &str, size: f32) -> Avatar {
    let name = site_name(url);
    let palette = ComponentPalette {
        selected: hsl(site_hue(&name), 50.0, 48.0),
        text: Color::rgb(255, 255, 255),
        ..look::palette()
    };
    Avatar::new(name, palette).size(size)
}

/// The name of a source's site: the second-to-last label of its host, without `www.`
/// (`developer.mozilla.org` is `mozilla`), or the whole host when it has one label.
pub fn site_name(url: &str) -> String {
    let Some(host) = host(url) else {
        return String::new();
    };
    let labels: Vec<&str> = host.split('.').collect();
    match labels.len() {
        1 => host.clone(),
        count => labels[count - 2].to_owned(),
    }
}

/// The hue of a site's letter tile: FNV-1a of its name, in degrees. Stable across runs.
pub fn site_hue(name: &str) -> u32 {
    let hash = name.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    hash % 360
}

/// `hsl(hue, saturation, lightness)` as a color: the hue in degrees, the others in percent.
fn hsl(hue: u32, saturation: f32, lightness: f32) -> Color {
    let (s, l) = (saturation / 100.0, lightness / 100.0);
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = chroma * (1.0 - ((hue as f32 / 60.0) % 2.0 - 1.0).abs());
    let m = l - chroma / 2.0;
    let (r, g, b) = match hue / 60 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let byte = |value: f32| ((value + m) * 255.0).round() as u8;
    Color::rgb(byte(r), byte(g), byte(b))
}

pub use quill_model::Todo;

/// The task list as the task tools left it ([`quill_model::apply_task_tool`]).
pub fn todos(items: &[Item]) -> Vec<Todo> {
    let mut list: Vec<Todo> = vec![];
    for item in items {
        if let Item::Tool {
            name,
            input,
            result,
            ..
        } = item
        {
            let output = result.as_ref().map(|r| r.output.as_str());
            quill_model::apply_task_tool(&mut list, name, input, output);
        }
    }
    list
}

/// The task list: a card as wide as the column, a 28 px row per
/// task, its mark (a filled check when done, a turning arc while in progress, a dashed ring
/// while waiting) and its words, gray once done.
fn task_card(list: &[Todo], column: f32, palette: ComponentPalette) -> Element {
    let rows = list
        .iter()
        .enumerate()
        .map(|(i, todo)| {
            let done = todo.status == "completed";
            let mark = match todo.status.as_str() {
                // The icon's own white would take the tint: the check goes over a dot.
                "completed" => Element::overlay(
                    Element::spacer()
                        .background(look::muted())
                        .radius(7.0)
                        .size(Some(14.0), Some(14.0)),
                    Element::icon(icons::of(icons::CHECK))
                        .foreground(palette.surface)
                        .size(Some(10.0), Some(10.0))
                        .centered(),
                )
                .size(Some(14.0), Some(14.0)),
                "in_progress" => Spinner::new(format!("task:{i}"), look::muted())
                    .size(14.0)
                    .decorative()
                    .render(),
                _ => Element::icon(icons::of(icons::TASK_PENDING))
                    .foreground(look::muted())
                    .size(Some(14.0), Some(14.0)),
            };
            let ink = if done {
                look::muted()
            } else {
                look::palette().text
            };
            let words = one_line(todo.subject.clone(), ink).font_size(BODY);
            Element::row(vec![mark.centered_y(), words])
                .gap(6.0)
                .size(None, Some(28.0))
        })
        .collect();
    Element::panel(Element::column(rows))
        .background(palette.surface)
        .border(look::card_border(), 1.0)
        .radius(8.0)
        .padding(12.0)
        .size(Some(column), None)
}

/// A verb phrase put in the past, as Desktop names a finished step: "Run saludo.py" becomes
/// "Ran saludo.py", "Create atajo.txt…" "Created atajo.txt…".
pub fn past_tense(phrase: &str) -> String {
    inflect(phrase, |verb| {
        const IRREGULAR: &[(&str, &str)] = &[
            ("Run", "Ran"),
            ("Make", "Made"),
            ("Write", "Wrote"),
            ("Find", "Found"),
            ("Get", "Got"),
            ("Build", "Built"),
            ("Read", "Read"),
            ("Set", "Set"),
            ("Put", "Put"),
            ("Begin", "Began"),
        ];
        if let Some((_, past)) = IRREGULAR.iter().find(|(v, _)| *v == verb) {
            return (*past).to_owned();
        }
        if verb.ends_with('e') {
            format!("{verb}d")
        } else if verb.ends_with('y') && !verb.ends_with("ay") && !verb.ends_with("ey") {
            format!("{}ied", &verb[..verb.len() - 1])
        } else {
            format!("{verb}ed")
        }
    })
}

/// The same phrase while it runs: "Creating atajo.txt…".
pub fn progressive(phrase: &str) -> String {
    inflect(phrase, |verb| match verb {
        "Run" => "Running".to_owned(),
        "Set" => "Setting".to_owned(),
        "Get" => "Getting".to_owned(),
        "Put" => "Putting".to_owned(),
        v if v.ends_with("ie") => format!("{}ying", &v[..v.len() - 2]),
        v if v.ends_with('e') && !v.ends_with("ee") => format!("{}ing", &v[..v.len() - 1]),
        v => format!("{v}ing"),
    })
}

/// Change the phrase's first word when it reads as a verb (capitalized, letters only).
fn inflect(phrase: &str, verb: impl Fn(&str) -> String) -> String {
    let (first, rest) = phrase.split_once(' ').unwrap_or((phrase, ""));
    let is_verb = first.chars().next().is_some_and(char::is_uppercase)
        && first.chars().all(char::is_alphabetic);
    if !is_verb {
        return phrase.to_owned();
    }
    if rest.is_empty() {
        verb(first)
    } else {
        format!("{} {rest}", verb(first))
    }
}

/// A browser tool's step in Desktop's words: "Opening
/// localhost:3000", "Reading page", "Filling in a field", "Clicking", "Capturing page".
fn browser_step(tool: &str, input: &Value, done: bool) -> (String, String) {
    let said = |key: &str| input[key].as_str().unwrap_or_default().to_owned();
    let pick = |past: &str, present: &str| if done { past } else { present }.to_owned();
    let none = String::new();
    match tool {
        "navigate" => match said("url").as_str() {
            "back" => (pick("Went back", "Going back"), none),
            "forward" => (pick("Went forward", "Going forward"), none),
            url => match host(url) {
                Some(host) => (pick("Opened", "Opening"), host),
                None => (pick("Opened a page", "Opening a page"), none),
            },
        },
        "read_page" if input["filter"] == "interactive" => (
            pick("Read page (interactive)", "Reading page (interactive)"),
            none,
        ),
        "read_page" => (pick("Read page", "Reading page"), none),
        "get_page_text" => (pick("Read page text", "Reading page text"), none),
        "find" => (
            pick("Found", "Finding"),
            format!("\u{201c}{}\u{201d}", said("query")),
        ),
        "form_input" => (pick("Filled in a field", "Filling in a field"), none),
        "computer" => {
            let action = said("action");
            match action.as_str() {
                "screenshot" => (pick("Captured page", "Capturing page"), none),
                "left_click" | "right_click" | "double_click" | "triple_click" => {
                    (pick("Clicked", "Clicking"), none)
                }
                "type" => (pick("Typed", "Typing"), none),
                "key" => (pick("Pressed", "Pressing"), said("text")),
                "scroll" => (
                    pick("Scrolled", "Scrolling"),
                    input["scroll_direction"]
                        .as_str()
                        .unwrap_or("down")
                        .to_owned(),
                ),
                "scroll_to" => (pick("Scrolled to element", "Scrolling to element"), none),
                "left_click_drag" => (pick("Dragged", "Dragging"), none),
                "hover" => (pick("Hovered", "Hovering"), none),
                "wait" => (
                    pick("Waited", "Waiting"),
                    format!("{} seconds", input["duration"].as_f64().unwrap_or(1.0)),
                ),
                _ => (pick("Interacted with page", "Interacting with page"), none),
            }
        }
        "javascript_tool" => (pick("Ran page script", "Running page script"), none),
        "read_console_messages" => (pick("Read console", "Reading console"), none),
        "read_network_requests" => (
            pick("Read network activity", "Reading network activity"),
            none,
        ),
        "resize_window" => (pick("Resized browser", "Resizing browser"), none),
        "tabs_context" => (pick("Checked tabs", "Checking tabs"), none),
        "tabs_create" => (pick("Opened tab", "Opening tab"), none),
        "tabs_select" => (pick("Switched tabs", "Switching tabs"), none),
        "tabs_close" => (pick("Closed tab", "Closing tab"), none),
        // Desktop's own for dev servers: "Starting preview".
        "preview_start" => match (said("name").as_str(), host(&said("url"))) {
            ("", Some(host)) => (pick("Opened", "Opening"), host),
            (name, _) => (pick("Started preview", "Starting preview"), name.to_owned()),
        },
        "preview_stop" => (pick("Stopped server", "Stopping server"), none),
        "preview_list" => (pick("Listed servers", "Listing servers"), none),
        "preview_logs" => (pick("Read server logs", "Reading server logs"), none),
        "browser_batch" => (
            format!(
                "Batch \u{2014} {} actions",
                input["actions"].as_array().map_or(0, Vec::len)
            ),
            none,
        ),
        _ => (pick("Used the browser", "Using the browser"), none),
    }
}

/// The Viewport button's tooltip: the size set, and who set it when it was the agent.
pub(crate) fn viewport_label(tab: Option<&quill_model::BrowserTab>) -> String {
    match tab.and_then(|t| Some((t.emulated?, t.emulated_by_agent))) {
        Some(((w, h), true)) => format!("Viewport: {w} \u{d7} {h} \u{b7} set by the agent"),
        Some(((w, h), false)) => format!("Viewport: {w} \u{d7} {h}"),
        None => "Viewport".into(),
    }
}

/// The Viewport button's icon: a phone for a phone's width, else a monitor.
pub(crate) fn viewport_icon(emulated: Option<(u32, u32)>) -> &'static str {
    match emulated {
        Some((w, _)) if w <= 480 => icons::PHONE,
        _ => icons::MONITOR,
    }
}

/// Desktop's Viewport menu: the page at the pane's size, or as a phone or a
/// tablet draws it; "Set by the agent" under the size resize_window chose. Its button is
/// on while a size is set.
fn viewport_menu(tab: Option<&quill_model::BrowserTab>, ui: &Ui) -> Element {
    let emulated = tab.and_then(|t| t.emulated);
    let open = ui.open_menu == Some(BROWSER_VIEWPORT);
    let anchor = toggle(
        BROWSER_VIEWPORT,
        icons::of(viewport_icon(emulated)),
        &viewport_label(tab),
        open || emulated.is_some(),
        None,
    );
    let mut entries: Vec<limn::DropdownMenuEntry> = VIEWPORTS
        .iter()
        .map(|(name, size)| {
            let label = match size {
                Some((w, h)) => format!("{name} \u{b7} {w} \u{d7} {h}"),
                None => (*name).to_owned(),
            };
            let id = match size {
                Some((w, h)) => format!("{BROWSER_VIEWPORT}:{w}x{h}"),
                None => format!("{BROWSER_VIEWPORT}:responsive"),
            };
            let item = limn::DropdownMenuItem::new(id, label).selected(emulated == *size);
            match tab.filter(|t| t.emulated_by_agent && emulated == *size) {
                Some(_) => item.description("Set by the agent").into(),
                None => item.into(),
            }
        })
        .collect();
    if let Some((w, h)) = emulated.filter(|e| !VIEWPORTS.iter().any(|(_, s)| *s == Some(*e))) {
        entries.push(
            limn::DropdownMenuItem::new(
                format!("{BROWSER_VIEWPORT}:{w}x{h}"),
                format!("{w} \u{d7} {h}"),
            )
            .description("Set by the agent")
            .selected(true)
            .into(),
        );
    }
    menu(
        anchor,
        entries,
        open,
        BROWSER_VIEWPORT,
        220.0,
        limn::PopoverPlacement::BottomEnd,
    )
}

/// The tab a browser tool opened, from the line its answer leads with.
fn opened_tab(name: &str, result: Option<&ToolResult>) -> Option<Value> {
    if !matches!(
        name,
        "mcp__browser__navigate" | "mcp__browser__tabs_create" | "mcp__browser__preview_start"
    ) {
        return None;
    }
    let result = result.filter(|r| !r.is_error && !r.stopped)?;
    let first = result.output.lines().next()?;
    let card: Value = serde_json::from_str(first).ok()?;
    (card["type"] == "browser").then_some(card)
}

/// Desktop's "Opened in Browser" card: the page's title, "{host} · Opened in
/// Browser" (or that the tab closed), and "Open", which shows the pane on that tab. A globe
/// stands where Desktop has a thumbnail: a surface is not resampled, so a small one would
/// show a corner of the page.
fn browser_card(
    session: &Session,
    opened: &Value,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let id = opened["tabId"].as_u64().unwrap_or_default() as u32;
    let url = opened["url"].as_str().unwrap_or_default();
    let tab = session.browser.tabs.iter().find(|t| t.id == id);
    let title = tab
        .map(|t| t.title.clone())
        .filter(|t| !t.is_empty())
        .or_else(|| host(url))
        .unwrap_or_else(|| "Browser tab".into());
    let place = host(tab.map_or(url, |t| t.url.as_str()));
    let line = match (tab, place) {
        (None, _) => "Browser tab closed".to_owned(),
        (Some(_), Some(place)) => format!("{place} \u{b7} Opened in Browser"),
        (Some(_), None) => "Opened in Browser".to_owned(),
    };
    let width = column.min(480.0);
    let mut row = vec![
        // A stack centres on both axes; a row only across.
        Element::stack(vec![Element::icon(icons::of(icons::GLOBE))
            .foreground(look::muted())
            .size(Some(20.0), Some(20.0))
            .centered()])
        .background(look::command_box())
        .radius(8.0)
        .size(Some(40.0), Some(40.0))
        .centered_y(),
        // The words take what the globe and "Open" leave, fading out before the button.
        Element::column(vec![
            Element::faded_text(title, FADE)
                .font_family(FontFamily::Sans)
                .font_size(BODY)
                .foreground(look::palette().text),
            Element::faded_text(line, FADE)
                .font_family(FontFamily::Sans)
                .font_size(SMALL)
                .foreground(look::muted()),
        ])
        .gap(2.0)
        .flex(1.0)
        .centered_y(),
    ];
    if tab.is_some() {
        row.push(
            keyed_button(
                &format!("browser:card:{id}"),
                "Open",
                "",
                false,
                true,
                palette,
            )
            .centered_y(),
        );
    }
    Element::panel(Element::row(row).gap(12.0).padding(12.0))
        .background(palette.surface)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .size(Some(width), None)
}

/// A URL's host as Desktop shows it: without "www." and at most its last 48 characters.
fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|h| !h.is_empty())?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    let chars: Vec<char> = host.chars().collect();
    Some(chars[chars.len().saturating_sub(48)..].iter().collect())
}

fn file_of(input: &Value) -> String {
    input["file_path"]
        .as_str()
        .or(input["notebook_path"].as_str())
        .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned())
        .unwrap_or_default()
}

/// A step's verb and what it acted on, done or still running: ("Edited", "saludo.py"),
/// ("Reading", "saludo.py"); a command keeps its own words.
/// The verbs are Desktop's.
fn step_label(step: &Step, done: bool) -> (String, String) {
    let input = step.input;
    let said = |key: &str| input[key].as_str().unwrap_or_default().to_owned();
    let (past, present, object) = match step.name {
        "Bash" => {
            let said = input["description"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Run {}", said("command")));
            // Running it reads "Creating a.txt…"; stopped or failed, in its own words.
            let phrase = match step.result {
                None => progressive(&said),
                Some(r) if r.stopped || r.is_error => said,
                Some(_) if done => past_tense(&said),
                Some(_) => said,
            };
            return (phrase, String::new());
        }
        name if name.starts_with("mcp__browser__") => {
            let failed = step.result.is_some_and(|r| r.is_error);
            let (verb, object) =
                browser_step(&name["mcp__browser__".len()..], input, done || failed);
            return if failed {
                (format!("{verb} (failed)"), object)
            } else {
                (verb, object)
            };
        }
        "Read" => ("Read", "Reading", file_of(input)),
        "Edit" | "MultiEdit" | "NotebookEdit" => ("Edited", "Editing", file_of(input)),
        "Write" => {
            let created = step
                .result
                .and_then(|r| r.files.first())
                .is_none_or(|f| f.before.is_none());
            let past = if created { "Created" } else { "Updated" };
            (past, "Writing", file_of(input))
        }
        "Grep" | "Glob" => ("Searched", "Searching", said("pattern")),
        "LS" => ("Listed", "Listing", said("path")),
        "WebFetch" => ("Fetched", "Fetching", said("url")),
        "WebSearch" => ("Searched web", "Searching web", said("query")),
        "Agent" | "Task" => ("Ran agent", "Running agent", said("description")),
        // Codex's own steps, in the same voice (inferred: Desktop shows none of them).
        "Compact" => ("Compacted context", "Compacting context", String::new()),
        "EnterReviewMode" if input["review"].as_str().is_some_and(|r| !r.is_empty()) => (
            "Started a review of",
            "Starting a review of",
            said("review"),
        ),
        "EnterReviewMode" => ("Started a review", "Starting a review", String::new()),
        "ExitReviewMode" => ("Finished the review", "Finishing the review", String::new()),
        "ImageGeneration" => ("Generated an image", "Generating an image", String::new()),
        "Sleep" => (
            "Waited",
            "Waiting",
            waited(input["duration_ms"].as_u64().unwrap_or(0)),
        ),
        "WaitAgent" => ("Waited for agents", "Waiting for agents", String::new()),
        "MessageAgent" => ("Messaged an agent", "Messaging an agent", String::new()),
        "CloseAgent" => ("Closed an agent", "Closing an agent", String::new()),
        "Skill" => ("Ran skill", "Running skill", said("skill")),
        "ExitPlanMode" => ("Proposed plan", "Proposing plan", String::new()),
        // The rest in Desktop's words. What follows the
        // verb is quill's where Desktop shows none (inferred): the server, the resource, the
        // command stopped.
        "EnterPlanMode" => ("Started planning", "Making a plan", String::new()),
        "EnterWorktree" if input["path"].is_string() => {
            ("Entered a worktree", "Entering a worktree", said("name"))
        }
        "EnterWorktree" => ("Created a worktree", "Creating a worktree", said("name")),
        "ExitWorktree" => ("Left the worktree", "Leaving the worktree", String::new()),
        // The CLI's older names for both, which old transcripts still hold.
        "TaskOutput" | "BashOutput" | "AgentOutputTool" => {
            ("Read task output", "Reading task output", String::new())
        }
        "TaskStop" | "KillShell" | "KillBash" => {
            // "Successfully stopped task: <id> (<command>)" (CLI 2.1.312).
            let output = step.result.map(|r| r.output.as_str()).unwrap_or_default();
            let command = output
                .split_once(" (")
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .unwrap_or_default();
            ("Stopped task", "Stopping task", command.to_owned())
        }
        "Monitor" => (
            "Started watching background command",
            "Watching background command",
            said("description"),
        ),
        "ListMcpResourcesTool" => ("Listed resources", "Listing resources", said("server")),
        "ReadMcpResourceTool" => ("Read resource", "Reading resource", said("uri")),
        "ReadMcpResourceDirTool" => (
            "Listed resource folder",
            "Listing resource folder",
            said("uri"),
        ),
        "LSP" => ("Inspected code", "Inspecting code", String::new()),
        "CronCreate" if input["recurring"] == false => {
            ("Scheduled prompt", "Scheduling prompt", String::new())
        }
        "CronCreate" => ("Started loop", "Starting loop", String::new()),
        "CronList" => (
            "Listed scheduled prompts",
            "Listing scheduled prompts",
            String::new(),
        ),
        "CronDelete" => (
            "Stopped scheduled prompt",
            "Stopping scheduled prompt",
            String::new(),
        ),
        "ScheduleWakeup" if input["stop"] == true => {
            ("Stopped loop", "Stopping loop", String::new())
        }
        "ScheduleWakeup" => ("Scheduled check-in", "Scheduling check-in", said("reason")),
        // quill's own session tools, in Desktop's words.
        name if name.ends_with("sessions__start_session") => {
            if input["initiation"] == "own_initiative" {
                ("Suggested task", "Suggesting task", said("title"))
            } else {
                ("Started session", "Starting session", said("title"))
            }
        }
        name if name.ends_with("sessions__dismiss_task") => {
            ("Withdrew a task", "Withdrawing a task", String::new())
        }
        name if name.ends_with("sessions__send_to_session") => (
            "Passed the message to",
            "Passing the message to",
            said("session"),
        ),
        name if name.ends_with("sessions__wait_for_session") => {
            ("Set a wait", "Setting a wait", String::new())
        }
        name if name.ends_with("sessions__list_sessions") => {
            ("Listed sessions", "Listing sessions", String::new())
        }
        "AskUserQuestion" => (
            "Asked",
            "Asking",
            input["questions"][0]["header"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ),
        // A connector's tool by its own name: "mcp__server__tool" is "tool".
        name => (
            "Used",
            "Using",
            name.rsplit("__").next().unwrap_or(name).to_owned(),
        ),
    };
    let verb = if done { past } else { present };
    (verb.to_owned(), object)
}

/// A wait as it reads in a row: "1s", "2.5s", "300ms".
fn waited(ms: u64) -> String {
    match ms {
        0..=999 => format!("{ms}ms"),
        _ if ms.is_multiple_of(1000) => format!("{}s", ms / 1000),
        _ => format!("{:.1}s", ms as f64 / 1000.0),
    }
}

/// The line a group of steps folds into, as Desktop words it: one command says
/// what it did ("Ran saludo.py"); otherwise each file with what was done to it and how many
/// commands ran: "Read and edited saludo.py, ran a command". While a step runs, the group
/// reads as that step: "Listing files in folder".
pub fn group_summary(steps: &[Step]) -> String {
    if let Some(step) = steps.iter().rev().find(|s| s.running) {
        return match step.name {
            "Bash" | "Agent" | "Task" => progressive(
                step.input["description"]
                    .as_str()
                    .unwrap_or("Run a command"),
            ),
            _ => {
                let (verb, object) = step_label(step, false);
                format!("{verb} {object}").trim_end().to_owned()
            }
        };
    }
    if let [step] = steps {
        if matches!(step.name, "Bash" | "Agent" | "Task") {
            let said = step.input["description"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| "Run a command".into());
            return if step.result.is_some_and(|r| r.stopped || r.is_error) {
                said
            } else {
                past_tense(&said)
            };
        }
    }
    let mut files: Vec<(String, Vec<String>)> = vec![];
    // The browser's steps by what they did, as Desktop joins up to three ("Read page,
    // clicked…").
    let mut browsing: Vec<String> = vec![];
    // The only one says what it acted on too: "Opened example.com".
    let mut lone_browsing = None;
    let (mut commands, mut searches, mut agents, mut others) = (0, 0, 0, 0);
    for step in steps {
        match step.name {
            name if name.starts_with("mcp__browser__") => {
                let (verb, object) = step_label(step, true);
                lone_browsing = lone_browsing.is_none().then(|| {
                    format!("{} {object}", verb.to_lowercase())
                        .trim_end()
                        .to_owned()
                });
                let verb = verb.to_lowercase();
                if !browsing.contains(&verb) {
                    browsing.push(verb);
                }
            }
            "Bash" => commands += 1,
            "Grep" | "Glob" => searches += 1,
            "Agent" | "Task" => agents += 1,
            "Read" | "Edit" | "MultiEdit" | "NotebookEdit" | "Write" => {
                let (verb, file) = step_label(step, true);
                let verb = verb.to_lowercase();
                match files.iter_mut().find(|(f, _)| *f == file) {
                    Some((_, verbs)) if !verbs.contains(&verb) => verbs.push(verb),
                    Some(_) => {}
                    None => files.push((file, vec![verb])),
                }
            }
            _ => others += 1,
        }
    }
    // A lone tool of no kind above says what it did: "Stopped task npm run dev".
    if let ([step], 1) = (steps, others) {
        let (verb, object) = step_label(step, true);
        return format!("{verb} {object}").trim_end().to_owned();
    }
    let mut clauses: Vec<String> = files
        .into_iter()
        .map(|(file, verbs)| format!("{} {file}", verbs.join(" and ")))
        .collect();
    let single = steps
        .iter()
        .filter(|s| s.name.starts_with("mcp__browser__"))
        .count()
        == 1;
    if let (true, Some(lone)) = (single, lone_browsing) {
        clauses.push(lone);
    } else if !browsing.is_empty() {
        let mut clause = browsing[..browsing.len().min(3)].join(", ");
        if browsing.len() > 3 {
            clause.push('\u{2026}');
        }
        clauses.push(clause);
    }
    if searches > 0 {
        clauses.push("searched the code".into());
    }
    // "ran a command", "ran 2 commands"; "ran an agent" and
    // "used a tool" the same way.
    let count = |n: usize, verb: &str, one: &str, thing: &str| match n {
        0 => None,
        1 => Some(format!("{verb} {one} {thing}")),
        n => Some(format!("{verb} {n} {thing}s")),
    };
    clauses.extend(count(commands, "ran", "a", "command"));
    clauses.extend(count(agents, "ran", "an", "agent"));
    clauses.extend(count(others, "used", "a", "tool"));
    let mut summary = clauses.join(", ");
    if let Some(first) = summary.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    summary
}

/// The lines of `before` and `after` as a diff: an LCS walk, enough for the files a tool
/// edits. shortcut: quadratic in lines, capped at 400 a side; a real diff when files grow.
pub fn line_diff(before: &str, after: &str) -> Vec<limn::CodeDiffLine> {
    use limn::{CodeDiffLine, CodeDiffLineKind};
    let a: Vec<&str> = before.lines().take(400).collect();
    let b: Vec<&str> = after.lines().take(400).collect();
    let mut lcs = vec![vec![0u16; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut lines) = (0, 0, vec![]);
    let line = |kind, old: Option<usize>, new: Option<usize>, text: &str| CodeDiffLine {
        kind,
        old_line: old.map(|n| n + 1),
        new_line: new.map(|n| n + 1),
        text: text.to_owned(),
    };
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            lines.push(line(CodeDiffLineKind::Context, Some(i), Some(j), a[i]));
            i += 1;
            j += 1;
        // On a tie the removed line goes first, as a unified diff reads.
        } else if i < a.len() && (j == b.len() || lcs[i + 1][j] >= lcs[i][j + 1]) {
            lines.push(line(CodeDiffLineKind::Removed, Some(i), None, a[i]));
            i += 1;
        } else {
            lines.push(line(CodeDiffLineKind::Added, None, Some(j), b[j]));
            j += 1;
        }
    }
    lines
}

/// A run of steps as Desktop shows it: one muted line with the files' added and
/// removed lines, which unfolds into a card with a row per step; a row unfolds what it did.
fn step_group(
    steps: &[Step],
    list: &[Item],
    ui: &Ui,
    view: TranscriptView,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let key = format!("steps:{}", steps[0].call);
    let open = is_open(view, ui, &key);
    // The pictures its steps gave back, among the run's they are counted in.
    let pictured = (steps.iter()).any(|s| !step_pictures(s.name, s.input, s.result).is_empty());
    let run = if pictured {
        run_pictures(list, steps[0].call)
    } else {
        vec![]
    };
    let running = steps.iter().any(|s| s.running);
    let stopped = !running
        && steps
            .last()
            .is_some_and(|s| s.result.is_some_and(|r| r.stopped));
    let (added, removed) = steps
        .iter()
        .filter_map(|s| s.result)
        .fold((0, 0), |(a, r), res| (a + res.added, r + res.removed));
    let summary = group_summary(steps);
    let mut head = vec![if running {
        working_words(&format!("{key}:shimmer"), summary)
    } else {
        one_line(summary, look::muted())
    }];
    // A command left running in the background offers its output.
    if steps
        .iter()
        .any(|s| s.running && s.name == "Bash" && s.result.is_some())
    {
        head.push(text("View live output").font_size(chat()).centered_y());
    }
    if stopped {
        head.push(muted("Stopped").font_size(chat()).centered_y());
    } else if !running
        && steps
            .last()
            .is_some_and(|s| s.result.is_some_and(|r| r.is_error))
    {
        // Desktop marks a run whose last step failed.
        head.push(
            text("Failed")
                .font_size(chat())
                .foreground(look::failed())
                .centered_y(),
        );
    }
    if added + removed > 0 {
        head.push(
            text(format!("+{added}"))
                .font_size(chat())
                .foreground(look::added())
                .centered_y(),
        );
        head.push(
            text(format!("−{removed}"))
                .font_size(chat())
                .foreground(look::removed())
                .centered_y(),
        );
    }
    head.push(caret(open));
    let body_id = format!("{key}:body");
    let head = fold(
        view,
        key.clone(),
        Element::row(head).gap(8.0).size(None, Some(24.0)),
    )
    .shrink_to_fit();
    // Folded, they sit at the line's end; open, each step's row shows its own.
    let mine = |p: &StepPicture| steps.iter().any(|s| s.call == p.call);
    let strip = if open {
        None
    } else {
        picture_strip(&run, mine, ui)
    };
    let head = match strip {
        Some(strip) => Element::row(vec![head, Element::spacer().flex(1.0), strip])
            .size(Some(column), Some(24.0)),
        None => head,
    };
    // The steps open and close on limn's spring, as the panel's cards do; the gap above
    // them is inside what folds.
    let card = open.then(|| {
        let mut rows = vec![];
        for (i, step) in steps.iter().enumerate() {
            if i > 0 {
                rows.push(hline(column, look::card_border()));
            }
            rows.push(step_row(step, &run, ui, view, column, palette));
        }
        // A long group folds from its end too, by a chevron that shows while the pointer is
        // on the box; Verbose holds it open, so there is none.
        if view != TranscriptView::Verbose {
            let fold = icon_tool(
                &format!("{GROUP_FOLD}{key}"),
                icons::of(icons::CARET_UP),
                "Collapse",
                24.0,
                palette,
            )
            .reveal_on_hover(format!("{key}:fold"));
            rows.push(
                Element::row(vec![Element::spacer().flex(1.0), fold, hgap(4.0)])
                    .size(Some(column), Some(28.0)),
            );
        }
        let card = Element::panel(Element::column(rows))
            .background(palette.surface)
            .border(look::card_border(), 1.0)
            .radius(8.0)
            .size(Some(column), None)
            .hover_region();
        Element::column(vec![Element::spacer().size(None, Some(8.0)), card])
    });
    Element::column(vec![
        head,
        limn::Collapse::new(body_id, card).expand().render(),
    ])
}

/// Whether a collapsible is open: Verbose holds every one open, else the person's toggle.
fn is_open(view: TranscriptView, ui: &Ui, key: &str) -> bool {
    view == TranscriptView::Verbose || ui.expanded.iter().any(|k| k == key)
}

/// A collapsible's header: a click folds it, unless Verbose holds it open and it does nothing.
fn fold(view: TranscriptView, key: String, header: Element) -> Element {
    if view == TranscriptView::Verbose {
        header
    } else {
        Element::clickable(key, header)
    }
}

/// A thinking block, shown in Thinking and Verbose: the model's words, muted, under "Thought
/// process", with a copy button that copies them as a quote. Only the transcript's own items
/// have a place to copy from.
fn thinking_block(
    session: &Session,
    at: usize,
    thought: &str,
    main: bool,
    ui: &Ui,
    column: f32,
) -> Element {
    let label = muted("Thought process").font_size(SMALL).centered_y();
    let mut head = vec![label];
    if main {
        let copy = format!("copy:{}:{at}", session.id);
        let copied = ui
            .copied
            .as_ref()
            .is_some_and(|(id, since)| *id == copy && since.elapsed() < COPIED_FOR);
        let button = tool_surface(
            &copy,
            icons::of(if copied { icons::CHECK } else { icons::COPY }),
            if copied { "Copied" } else { "Copy" },
            24.0,
            None,
            look::tool_hover(),
            None,
        );
        head.push(if copied {
            button
        } else {
            button.reveal_on_hover(format!("{copy}:reveal"))
        });
    }
    let words = muted(thought.trim_end())
        .font_size(chat())
        .max_width(column);
    Element::column(vec![Element::row(head).gap(8.0), words])
        .gap(4.0)
        .hover_region()
}

/// The small triangle after a group or a step: right while folded, down while open.
/// Open or closed: a chevron, as the button menus draw theirs.
fn caret(open: bool) -> Element {
    Element::icon(icons::of(if open {
        icons::CARET_DOWN
    } else {
        icons::CARET_RIGHT
    }))
    .foreground(look::muted())
    .size(Some(12.0), Some(12.0))
    .centered_y()
}

/// One step in its group's card: "Read saludo.py", "Edited saludo.py +1 −1",
/// "Ran saludo.py to verify output", each opening what it did. A helper agent's row has no
/// caret: it opens the agent's own pane.
fn step_row(
    step: &Step,
    run: &[StepPicture],
    ui: &Ui,
    view: TranscriptView,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let agent = matches!(step.name, "Agent" | "Task");
    // Entering plan mode says so and nothing more: its output is the CLI's word to the model.
    let bare = matches!(
        step.name,
        "EnterPlanMode"
            | "Compact"
            | "EnterReviewMode"
            | "Sleep"
            | "WaitAgent"
            | "MessageAgent"
            | "CloseAgent"
    );
    let open = !agent && !bare && is_open(view, ui, step.call);
    let done = !step.running && step.result.is_some_and(|r| !r.stopped);
    let (verb, object) = step_label(step, done);
    let mut line = vec![hgap(10.0)];
    // A step still at work shimmers for as long as it runs, as the running line does.
    if step.running {
        let words = format!("{verb} {object}").trim_end().to_owned();
        line.push(working_words(&format!("tool:{}:shimmer", step.call), words).font_size(BODY));
    } else if object.is_empty() {
        line.push(one_line(verb, look::muted()).font_size(BODY));
    } else {
        line.push(muted(verb).centered_y());
        line.push(one_line(object, look::palette().text).font_size(BODY));
    }
    if let Some(r) = step.result {
        if r.added + r.removed > 0 {
            line.push(
                text(format!("+{}", r.added))
                    .foreground(look::added())
                    .centered_y(),
            );
            line.push(
                text(format!("−{}", r.removed))
                    .foreground(look::removed())
                    .centered_y(),
            );
        }
        if r.stopped {
            line.push(muted("Stopped").centered_y());
        }
    }
    if !agent && !bare {
        line.push(caret(open));
    }
    // Its pictures at the row's end.
    if let Some(strip) = picture_strip(run, |p| p.call == step.call, ui) {
        line.extend([Element::spacer().flex(1.0), strip.centered_y(), hgap(10.0)]);
    }
    let row_palette = ComponentPalette {
        hover: look::row_hover(),
        ..palette
    };
    let id = if agent {
        format!("agent:{}", step.call)
    } else {
        format!("tool:{}", step.call)
    };
    let line = Element::row(line).gap(6.0).size(Some(column), Some(31.0));
    let body_id = format!("{id}:body");
    // A helper's row opens its own pane, which Verbose leaves as it is.
    let line = if agent {
        Element::clickable(id, line)
    } else if bare {
        line
    } else {
        fold(view, id, line)
    };
    let row = row_palette.interactive_surface(line);
    // Its body opens and closes on limn's spring.
    let body = open.then(|| pad(step_body(step, ui, column - 24.0, palette), 12.0, 4.0));
    Element::column(vec![
        row,
        limn::Collapse::new(body_id, body).expand().render(),
    ])
}

/// What a step did, as its row unfolds it: a command and its output, a file's diff, or the
/// tool's output.
fn step_body(step: &Step, ui: &Ui, inner: f32, palette: ComponentPalette) -> Element {
    let mut body = vec![];
    let result = step.result;
    match step.name {
        "Bash" => {
            body.push(muted("Bash").font_size(SMALL));
            let command = step.input["command"].as_str().unwrap_or_default();
            body.push(
                Element::panel(
                    Element::row(vec![
                        mono("$").foreground(look::muted()),
                        mono(command).size(Some(inner - 40.0), None),
                    ])
                    .gap(8.0),
                )
                .background(palette.surface)
                .border(look::card_border(), 1.0)
                .radius(6.0)
                .padding(10.0)
                .size(Some(inner), None),
            );
            if let Some(r) = result.filter(|r| !r.output.trim().is_empty()) {
                body.push(
                    mono(r.output.trim_end())
                        .foreground(look::muted())
                        .size(Some(inner), None),
                );
            }
        }
        // An image it looked at, as the pictures of a message are.
        "Read" if step_image(step.name, step.input, result).is_some() => {
            let path = step_image(step.name, step.input, result).unwrap_or_default();
            let id = format!("image:tool:{}", step.call);
            body.push(picture_tile(&id, &path, IMAGE_TILE, ui, palette));
        }
        // The findings of a review, as the markdown they are written in.
        "ExitReviewMode" => {
            if let Some(r) = result.filter(|r| !r.output.trim().is_empty()) {
                body.push(
                    Markdown::new(&r.output, palette)
                        .width(inner)
                        .font_size(BODY)
                        .render()
                        .element,
                );
            }
        }
        // A connector's resources as a list, each by its name over its address.
        "ListMcpResourcesTool" | "ReadMcpResourceDirTool" => {
            let output = result.map(|r| r.output.as_str()).unwrap_or_default();
            // Not a list (none found, or an error): the CLI's own words.
            let Ok(listed) = serde_json::from_str::<Vec<Value>>(output) else {
                if !output.trim().is_empty() {
                    body.push(muted(output.trim().to_owned()).size(Some(inner), None));
                }
                return Element::column(body);
            };
            let rows: Vec<Element> = listed
                .iter()
                .enumerate()
                .flat_map(|(i, r)| {
                    let uri = r["uri"].as_str().unwrap_or_default();
                    let name = r["name"].as_str().unwrap_or(uri);
                    let row = Element::column(vec![
                        text(name.to_owned()).size(Some(inner - 24.0), None),
                        mono(uri)
                            .foreground(look::muted())
                            .size(Some(inner - 24.0), None),
                    ])
                    .gap(2.0)
                    .padding(12.0);
                    let line = (i > 0).then(|| hline(inner, look::card_border()));
                    line.into_iter().chain([row])
                })
                .collect();
            if rows.is_empty() {
                body.push(muted("No resources found"));
            } else {
                body.push(
                    Element::panel(Element::column(rows))
                        .background(palette.surface)
                        .border(look::card_border(), 1.0)
                        .radius(6.0)
                        .size(Some(inner), None),
                );
            }
        }
        // A background command's output without the CLI's tags: "Running" or "Completed ·
        // exit 0" over what it printed. The tags are the older CLI's, from memory: 2.1.312
        // no longer has the tool (inferred).
        "TaskOutput" | "BashOutput" | "AgentOutputTool"
            if result.is_some_and(|r| tagged(&r.output, "status").is_some()) =>
        {
            let output = result.map(|r| r.output.as_str()).unwrap_or_default();
            let mut status = tagged(output, "status").unwrap_or_default().to_owned();
            if let Some(first) = status.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            if let Some(code) = tagged(output, "exit_code") {
                status = format!("{status} · exit {code}");
            }
            body.push(muted(status).font_size(SMALL));
            let printed: Vec<&str> = ["output", "stdout", "stderr"]
                .into_iter()
                .filter_map(|tag| tagged(output, tag))
                .filter(|s| !s.is_empty())
                .collect();
            if !printed.is_empty() {
                body.push(
                    OutputBlock::new(format!("out:{}", step.call), printed.join("\n"), palette)
                        .render()
                        .size(Some(inner), None),
                );
            }
        }
        // A resource read: its text, not the JSON it came in; a binary one by where it went.
        "ReadMcpResourceTool" => {
            let read: Value = result
                .and_then(|r| serde_json::from_str(&r.output).ok())
                .unwrap_or_default();
            for (i, content) in read["contents"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Some(saved) = content["blobSavedTo"].as_str() {
                    body.push(muted(format!("Saved to {saved}")).font_size(SMALL));
                } else if let Some(said) = content["text"].as_str() {
                    body.push(
                        OutputBlock::new(
                            format!("out:{}:{i}", step.call),
                            said.to_owned(),
                            palette,
                        )
                        .render()
                        .size(Some(inner), None),
                    );
                }
            }
            if read.is_null() {
                if let Some(r) = result.filter(|r| !r.output.trim().is_empty()) {
                    body.push(
                        OutputBlock::new(format!("out:{}", step.call), r.output.clone(), palette)
                            .render()
                            .size(Some(inner), None),
                    );
                }
            }
        }
        _ => {
            let files = result.map(|r| r.files.as_slice()).unwrap_or_default();
            for file in files {
                let before = file.before.as_deref().unwrap_or_default();
                let after = file.after.as_deref().unwrap_or_default();
                // The first line that differs, where the editor opens.
                let line = before
                    .lines()
                    .zip(after.lines())
                    .position(|(a, b)| a != b)
                    .unwrap_or(before.lines().count().min(after.lines().count()))
                    + 1;
                let path = file.path.display().to_string();
                body.push(
                    Element::row(vec![
                        muted(path.clone()).font_size(SMALL).centered_y(),
                        open_file(&path, line as u32, ui, palette),
                    ])
                    .gap(4.0),
                );
                body.push(diff_box(
                    &format!("diff:{}:{}", step.call, file.path.display()),
                    file.before.as_deref().unwrap_or_default(),
                    file.after.as_deref().unwrap_or_default(),
                    inner,
                    palette,
                ));
            }
            if files.is_empty() {
                if let Some(r) = result.filter(|r| !r.output.trim().is_empty()) {
                    body.push(
                        OutputBlock::new(format!("out:{}", step.call), r.output.clone(), palette)
                            .render()
                            .size(Some(inner), None),
                    );
                }
            }
        }
    }
    Element::column(body).gap(8.0)
}

/// What `<name>…</name>` holds in a tool's output, trimmed.
pub fn tagged<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = output.find(&format!("<{name}>"))? + name.len() + 2;
    let end = output[start..].find(&format!("</{name}>"))? + start;
    Some(output[start..end].trim_matches('\n').trim_end())
}

/// Two versions of a file as a bordered diff, up to 480 px high.
fn diff_box(id: &str, before: &str, after: &str, width: f32, palette: ComponentPalette) -> Element {
    let lines = line_diff(before, after);
    let rows = lines.len().max(1) as f32;
    let words = word_ranges(&lines);
    Element::panel(
        limn::CodeDiff::new(id, lines)
            .word_ranges(words)
            .palette(palette)
            .font_size(12.0)
            .height((rows * 19.0 + 16.0).min(480.0))
            .render(),
    )
    .border(look::card_border(), 1.0)
    .radius(6.0)
    .size(Some(width), None)
}

/// A step Desktop sets on its own line: a proposed plan as "Proposed plan ▸",
/// which opens the plan, and answered questions as a card of
/// each question and its answer. Until they answer, their cards
/// are the permission's.
fn lone_step(step: &Step, column: f32, palette: ComponentPalette) -> Option<Element> {
    let result = step.result.filter(|r| !r.stopped && !r.is_error)?;
    if step.name == "ExitPlanMode" {
        let line = Element::row(vec![
            muted("Proposed plan").font_size(chat()).centered_y(),
            caret(false),
        ])
        .gap(8.0)
        .size(None, Some(24.0));
        return Some(Element::clickable(format!("plan:{}", step.call), line));
    }
    let questions = step.input["questions"].as_array()?;
    let rows: Vec<Element> = questions
        .iter()
        .enumerate()
        .flat_map(|(i, q)| {
            let question = q["question"].as_str().unwrap_or_default();
            let answer = answered(&result.output, question).unwrap_or_else(|| "Skipped".into());
            let row = Element::column(vec![
                text(question.to_owned()).font_size(BODY),
                muted(answer).font_size(BODY),
            ])
            .gap(4.0)
            .padding(16.0);
            let line = (i > 0).then(|| hline(column.min(480.0), look::card_border()));
            line.into_iter().chain([row])
        })
        .collect();
    Some(
        Element::panel(Element::column(rows))
            .background(palette.surface)
            .border(look::card_border(), 1.0)
            .radius(8.0)
            .size(Some(column.min(480.0)), None),
    )
}

/// The answer the agent was given to `question`, as the CLI words it:
/// `The user answered: "Pick red or blue?"="blue". …` ([F:ask]).
/// shortcut: an answer holding a quote is cut there; parse the tool's structured result when
/// one reaches the UI.
pub fn answered(output: &str, question: &str) -> Option<String> {
    let start = output.find(&format!("\"{question}\"=\""))? + question.len() + 4;
    let rest = &output[start..];
    Some(rest[..rest.find('"')?].to_owned())
}

fn mono(s: impl Into<String>) -> Element {
    Element::text(s.into())
        .font_family(FontFamily::Monospace)
        .font_size(SMALL)
        .foreground(look::palette().text)
}

/// The inline approval card, as Desktop draws it: a 480 px card with what
/// Claude wants to run, Deny, and the way the person allowed last as its main button with
/// the other in its menu. An edit names its file and shows its diff. Once answered it goes, leaving the tool's row.
fn permission(
    item: &Item,
    agent: &str,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let Item::Permission {
        id,
        tool,
        input,
        title,
        interactive,
        decided,
        suggestions,
        ..
    } = item
    else {
        return Element::spacer();
    };
    if decided.is_some() {
        return Element::spacer().size(None, Some(0.0));
    }
    let width = column.min(480.0);
    let inner = width - 32.0;
    let wrapped = |e: Element| e.size(Some(inner), None);
    if tool == quill_model::SITE_PERMISSION {
        return site_permission(id, input, ui, width, palette);
    }
    let command = input["command"].as_str();
    // Bash says what it runs in its own words: "Run saludo.py".
    let what = input["description"].as_str();
    let path = input["file_path"]
        .as_str()
        .or(input["notebook_path"].as_str());
    let file = file_of(input);
    let heading = permission_heading(&agent_word(ui, agent), tool, input);
    let mut rows = vec![wrapped(text(heading).font_family(FontFamily::SansBold))];
    if let Some(what) = what {
        rows.push(wrapped(muted(what.to_owned())));
    } else if path.is_some() {
        rows.push(wrapped(muted(file)));
    }
    if let Some(title) = title {
        rows.push(wrapped(muted(title.clone())));
    }
    let shown = command.or(path).map(str::to_owned).unwrap_or_else(|| {
        serde_json::to_string(input)
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect()
    });
    rows.push(
        Element::panel(
            text(shown)
                .font_family(FontFamily::Monospace)
                .font_size(SMALL)
                .foreground(look::command_ink())
                .size(Some(inner - 32.0), None),
        )
        .background(look::command_box())
        .radius(6.0)
        .padding(14.0),
    );
    // What the edit changes, before the person allows it.
    let change = match tool.as_str() {
        "Edit" => Some((
            input["old_string"].as_str().unwrap_or_default(),
            input["new_string"].as_str().unwrap_or_default(),
        )),
        "Write" => Some(("", input["content"].as_str().unwrap_or_default())),
        _ => None,
    };
    if let Some((before, after)) = change {
        rows.push(diff_box(
            &format!("perm-diff:{id}"),
            before,
            after,
            inner,
            palette,
        ));
    }
    if *interactive {
        rows.push(Button::new(format!("perm:{id}:once"), "Continue", palette).render());
    } else {
        rows.push(permission_actions(id, !suggestions.is_empty(), ui, palette));
    }
    Element::panel(Element::column(rows).gap(10.0))
        .background(palette.surface)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .padding(16.0)
        .size(Some(width), None)
}

/// The plan the agent ended its turn with, while the session plans: the last thing after the
/// person's last message, and not one the agent asks about (Claude Code asks with a permission,
/// whose card this is not).
pub(crate) fn proposed_plan<'a>(session: &'a Session, ui: &Ui) -> Option<&'a str> {
    let planning = mode_entry(ui, &session.agent, &session.mode)
        .is_some_and(|w| w.tone == quill_model::ModeTone::Plan);
    if !planning || session.working() {
        return None;
    }
    let call = (session.items.iter().rev())
        .take_while(|i| !matches!(i, Item::User { .. }))
        .find_map(|i| match i {
            Item::Tool {
                call,
                name,
                result: Some(r),
                ..
            } if name == "ExitPlanMode" && !r.is_error && !r.stopped => Some(call.as_str()),
            _ => None,
        })?;
    let asked = (session.items.iter())
        .any(|i| matches!(i, Item::Permission { call: Some(c), .. } if c == call));
    (!asked).then_some(call)
}

/// The mode a plan is carried out in: the one the session left to plan, else the agent's own
/// default, as long as it is offered; never a planning one.
fn implement_modes(store: &Store, session: &Session, ui: &Ui) -> (String, Vec<String>) {
    let planning = |m: &String| {
        mode_entry(ui, &session.agent, m).is_some_and(|w| w.tone == quill_model::ModeTone::Plan)
    };
    let mut modes = menu_modes(store, Some(session), ui);
    modes.retain(|m| !planning(m));
    let default = (ui.agents.iter().find(|a| a.id == session.agent)).map(|a| &a.default_mode);
    let back = [session.mode_before.as_ref(), default]
        .into_iter()
        .flatten()
        .find(|m| modes.contains(m))
        .or(modes.first())
        .cloned()
        .unwrap_or_default();
    modes.retain(|m| *m != back);
    (back, modes)
}

/// The plan the agent proposed, before the person decides: "{Agent} proposed a plan" and
/// "Open plan". While the agent waits on it (a permission): Reject, "Revise…", "Accept", and the
/// main button accepting into the most trusting mode offered, the other modes in its menu. A
/// plan it ended its turn with: "Revise…", which sends what should change, and "Implement in
/// {mode}", which leaves planning and has a turn carry it out, the other modes in its menu.
fn plan_card(
    store: &Store,
    id: &str,
    session: Option<&Session>,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let inner = column - 32.0;
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    let head = Element::row(vec![
        text(format!(
            "{} proposed a plan",
            capital(&agent_word(ui, agent))
        ))
        .font_family(FontFamily::SansBold)
        .centered_y(),
        Element::spacer().flex(1.0),
        limn::Link::new(format!("plan:{id}"), "Open plan", palette)
            .ink(look::handle())
            .font_size(BODY)
            .render()
            .centered_y(),
    ])
    .size(Some(inner), None);
    let revise = keyed_button(
        &format!("revise:{id}"),
        "Revise…",
        "Esc",
        false,
        true,
        palette,
    );
    let ended = session.filter(|s| proposed_plan(s, ui) == Some(id));
    let actions = if let Some(session) = ended {
        let (back, others) = implement_modes(store, session, ui);
        let label = |mode: &str| {
            let words = mode_entry(ui, agent, mode).map_or(mode, |w| w.label.as_str());
            format!("Implement in {words}")
        };
        let menu: Vec<limn::DropdownMenuEntry> = (others.iter())
            .map(|m| limn::DropdownMenuItem::new(format!("planok:{id}:{m}"), label(m)).into())
            .collect();
        let implement = limn::SplitButton::new(
            format!("planok:{id}:{back}"),
            label(&back),
            format!("perm:{id}:menu"),
            palette,
        )
        .menu(
            menu,
            ui.perm_menu.as_deref() == Some(id),
            format!("perm:{id}:menu-dismiss"),
        )
        .render();
        vec![revise, hgap(12.0), implement.centered_y()]
    } else {
        permission_plan_actions(store, id, session, ui, revise, palette)
    };
    let mut rows = vec![
        head,
        Element::row(actions)
            .gap(8.0)
            // In a narrow column the buttons go on a second line.
            .wrap()
            .size(Some(inner), None),
    ];
    if ui.revising.as_deref() == Some(id) {
        rows.push(
            limn::TextField::new(
                format!("revise-text:{id}"),
                ui.revision.as_str(),
                "What should change? (optional)",
                palette,
            )
            .width(inner)
            .render(),
        );
    }
    Element::panel(Element::column(rows).gap(14.0))
        .background(palette.surface)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .padding(16.0)
        .size(Some(column), None)
}

/// A plan the agent waits on: Reject, "Revise…", "Accept" into Manual, and the main button
/// accepting into the most trusting mode offered, the other in its menu.
fn permission_plan_actions(
    store: &Store,
    id: &str,
    session: Option<&Session>,
    ui: &Ui,
    revise: Element,
    palette: ComponentPalette,
) -> Vec<Element> {
    let modes = offered_modes(store, session, ui);
    let trusting = if modes.iter().any(|m| m == "auto") {
        ("auto", "Accept and auto mode")
    } else {
        ("acceptEdits", "Accept and allow edits")
    };
    let menu: Vec<limn::DropdownMenuEntry> = [
        ("acceptEdits", "Accept and allow edits"),
        ("auto", "Accept and auto mode"),
    ]
    .into_iter()
    .filter(|(mode, _)| modes.iter().any(|m| m == mode))
    .map(|(mode, label)| limn::DropdownMenuItem::new(format!("planok:{id}:{mode}"), label).into())
    .collect();
    let accept = limn::SplitButton::new(
        format!("planok:{id}:{}", trusting.0),
        trusting.1,
        format!("perm:{id}:menu"),
        palette,
    )
    .keys(&["Ctrl", "⏎"])
    .menu(
        menu,
        ui.perm_menu.as_deref() == Some(id),
        format!("perm:{id}:menu-dismiss"),
    )
    .render();
    vec![
        keyed_button(
            &format!("perm:{id}:deny"),
            "Reject",
            "",
            false,
            true,
            palette,
        ),
        revise,
        hgap(12.0),
        keyed_button(
            &format!("planok:{id}:default"),
            "Accept",
            "Ctrl+Shift+⏎",
            false,
            true,
            palette,
        ),
        accept.centered_y(),
    ]
}

/// Deny, then the last way of allowing as the main button with its keys, and the other way
/// in its menu.
/// The browser asks before the agent opens or uses a site that is not local nor allowed, as
/// Desktop's site card: "Allow once", "Always allow for this website" (kept in
/// Settings › Browser), "Don't allow".
fn site_permission(
    id: &str,
    input: &Value,
    ui: &Ui,
    width: f32,
    palette: ComponentPalette,
) -> Element {
    let inner = width - 32.0;
    // A dev server shows what it runs and where, as a command's card does.
    if input["kind"] == "server" {
        let rows = vec![
            text(permission_heading(
                "the agent",
                quill_model::SITE_PERMISSION,
                input,
            ))
            .font_family(FontFamily::SansBold)
            .size(Some(inner), None),
            muted(input["folder"].as_str().unwrap_or_default().to_owned())
                .font_size(SMALL)
                .size(Some(inner), None),
            Element::panel(
                text(input["command"].as_str().unwrap_or_default().to_owned())
                    .font_family(FontFamily::Monospace)
                    .font_size(SMALL)
                    .foreground(look::command_ink())
                    .size(Some(inner - 32.0), None),
            )
            .background(look::command_box())
            .radius(6.0)
            .padding(14.0),
            actions(
                id,
                true,
                ("Always allow this server", "Until quill quits"),
                "Don't allow",
                ui,
                palette,
            ),
        ];
        return Element::panel(Element::column(rows).gap(12.0))
            .background(palette.surface)
            .border(look::card_border(), 1.0)
            .radius(12.0)
            .padding(16.0)
            .size(Some(width), None);
    }
    let rows = vec![
        text(permission_heading(
            "the agent",
            quill_model::SITE_PERMISSION,
            input,
        ))
        .font_family(FontFamily::SansBold)
        .size(Some(inner), None),
        muted(
            input["url"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect::<String>(),
        )
        .font_size(SMALL)
        .size(Some(inner), None),
        actions(
            id,
            true,
            (
                "Always allow for this website",
                "Adds it to Settings \u{203a} Browser",
            ),
            "Don't allow",
            ui,
            palette,
        ),
    ];
    Element::panel(Element::column(rows).gap(12.0))
        .background(palette.surface)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .padding(16.0)
        .size(Some(width), None)
}

fn permission_actions(id: &str, can_always: bool, ui: &Ui, palette: ComponentPalette) -> Element {
    actions(
        id,
        can_always,
        ("Always allow", "Saves to local project settings"),
        "Deny",
        ui,
        palette,
    )
}

/// A permission's answers: deny, and a split button of allowing once or always, the person's
/// last choice first.
fn actions(
    id: &str,
    can_always: bool,
    (always_label, always_says): (&str, &str),
    deny_label: &str,
    ui: &Ui,
    palette: ComponentPalette,
) -> Element {
    let menu_open = ui.perm_menu.as_deref() == Some(id);
    let always_first = can_always && ui.last_allow == crate::Allow::Always;
    let once =
        limn::DropdownMenuItem::new(format!("perm:{id}:once"), "Allow once").shortcut("Ctrl+Enter");
    let always = limn::DropdownMenuItem::new(format!("perm:{id}:always"), always_label)
        .description(always_says)
        .shortcut("Ctrl+Shift+Enter");
    let (main, label, keys, others): (_, _, &[&str], Vec<_>) = if always_first {
        (
            "always",
            always_label,
            &["Ctrl", "Shift", "⏎"],
            vec![once.into()],
        )
    } else if can_always {
        ("once", "Allow once", &["Ctrl", "⏎"], vec![always.into()])
    } else {
        ("once", "Allow once", &["Ctrl", "⏎"], vec![])
    };
    let mut split = limn::SplitButton::new(
        format!("perm:{id}:{main}"),
        label,
        format!("perm:{id}:menu"),
        palette,
    )
    .keys(keys);
    if !others.is_empty() {
        split = split.menu(others, menu_open, format!("perm:{id}:menu-dismiss"));
    }
    let deny = Button::new(
        format!("perm:{id}:deny"),
        deny_label,
        ComponentPalette {
            raised: look::cancel(),
            ..palette
        },
    )
    .variant(ButtonVariant::Secondary)
    .button_size(ButtonSize::Small)
    .render();
    ButtonGroup::new(vec![deny, split.render()])
        .align_end()
        .render()
}

/// Desktop's composer, as Desktop lays it out: a new
/// session's place and folder as filled chips over a 44 px box that holds only the message
/// and its return arrow; under the box "+" and the mode on the left, the model, its effort
/// and the plan's usage on the right.
fn composer(
    store: &Store,
    session: Option<&Session>,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let running = session.is_some_and(|s| {
        matches!(
            s.status,
            SessionStatus::Running | SessionStatus::Starting | SessionStatus::Awaiting
        )
    });
    let trailing = if running && ui.prompt.trim().is_empty() {
        stop_button(STOP)
    } else {
        send_button(!ui.prompt.trim().is_empty())
    };
    // While a plan waits, its card is the way to answer: Desktop hides the box and keeps
    // the footer under it.
    let plan_waiting = session.is_some_and(|s| {
        s.items.iter().any(
            |i| matches!(i, Item::Permission { tool, decided: None, .. } if tool == "ExitPlanMode"),
        )
    });
    if plan_waiting {
        return composer_footer(store, session, ui, palette).size(Some(column), None);
    }
    let placeholder = match session {
        _ if running => "Enter to send, Ctrl+Enter to queue for later",
        // The agent's suggested next prompt stands where the placeholder is, as in Desktop.
        Some(s) => s.suggestion.as_deref().unwrap_or("Type / for commands"),
        None => "Describe a task or ask a question",
    };
    let box_ = Composer::new(
        COMPOSER,
        ui.prompt.clone(),
        placeholder,
        column,
        look::composer_palette(),
    )
    .enter_submits(true)
    // Desktop's hint stays in the focused box, as it is right after sending.
    .persistent_placeholder(true)
    .auto_grow(28.0, 184.0)
    .editor_font_size(15.0)
    .pill_controls(Element::spacer().size(Some(0.0), None), trailing)
    .inline_controls(true)
    .border_color(look::composer_border())
    .shadow_color(look::composer_shadow())
    .card_radius(14.0)
    .context(composer_footer(store, session, ui, palette))
    .attachments(composer_images(ui, palette))
    .render();
    let box_ = crate::slash::list(store, ui, box_, column);
    let box_ = crate::mention::list(store, ui, box_, column);
    if let Some(session) = session {
        // The queue sits above the branch bar, so the composer's stack reads top to bottom.
        let mut stack: Vec<Element> = queue_bar(session, ui, column, palette)
            .into_iter()
            .chain(branch_bar(session, ui, column, palette))
            .collect();
        if stack.is_empty() {
            return box_;
        }
        stack.push(box_);
        return Element::column(stack).gap(8.0);
    }
    // quill runs sessions on this computer only, so "Local" is a label, not a menu.
    let local = Element::row(vec![
        hgap(8.0),
        chip_icon(icons::of(icons::LAPTOP)),
        chip_text("Local", look::palette().text),
        hgap(8.0),
    ])
    .gap(6.0)
    .background(look::cancel())
    .radius(6.0)
    .size(None, Some(24.0));
    Element::column(vec![
        Element::row(
            [local]
                .into_iter()
                .chain(agent_menu(ui))
                .chain([folder_menu(store, ui)])
                .collect(),
        )
        .gap(8.0),
        box_,
    ])
    .gap(13.0)
}

/// The composer's image tiles, each with its remove and open actions, and the note why an
/// image was left out.
fn composer_images(ui: &Ui, palette: ComponentPalette) -> Vec<Element> {
    let mut stacked = vec![];
    if !ui.images.is_empty() {
        let tiles = ui.images.iter().enumerate().map(|(index, path)| {
            let tile = image_tile(&format!("image:tile:{index}"), path, ui, palette)
                .remove_action(format!("image:remove:{index}"))
                .open_action(format!("image:open:{index}"));
            tile.render()
        });
        stacked.push(Element::row(tiles.collect()).wrap().gap(8.0));
    }
    if let Some(notice) = &ui.image_notice {
        stacked.push(
            text(notice.clone())
                .font_size(SMALL)
                .foreground(look::muted()),
        );
    }
    stacked
}

/// An image as a square tile: its thumbnail once decoded, else its file icon.
fn image_tile(id: &str, path: &Path, ui: &Ui, palette: ComponentPalette) -> Attachment {
    tile_of(id, path, IMAGE_TILE, ui, palette)
}

fn tile_of(id: &str, path: &Path, side: f32, ui: &Ui, palette: ComponentPalette) -> Attachment {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tile = Attachment::new(id, name, palette)
        .preview_only()
        .width(side);
    match ui.thumbs.get(path) {
        Some(thumb) => tile.preview(thumb.clone().cover()),
        None => tile,
    }
}

/// A picture of a step, opened in the viewer by a click on it.
fn picture_tile(id: &str, path: &Path, side: f32, ui: &Ui, palette: ComponentPalette) -> Element {
    tile_of(id, path, side, ui, palette)
        .open_action(id.to_owned())
        .render()
}

/// The logo of the connector or plugin `key`, once the host has decoded it, for the light or
/// dark theme; `None` while it is not, or logos are off, and the caller shows its icon.
pub(crate) fn logo_picture<'a>(ui: &'a Ui, key: &str) -> Option<&'a limn::RasterImage> {
    if !ui.settings.logos {
        return None;
    }
    let (light, dark) = ui.logos.get(key)?;
    let path = if look::is_dark() { dark } else { light };
    ui.thumbs.get(path.as_ref()?)
}

/// A logo on a square tile of the item's colour, as the Connectors rows show one.
fn logo_tile(logo: &limn::RasterImage, side: f32) -> Element {
    Element::row(vec![Element::raster_image(logo.clone())
        .size(Some(side * 0.75), Some(side * 0.75))
        .centered()
        .flex(1.0)])
    .size(Some(side), Some(side))
    .background(look::customize_tile())
    .radius(8.0)
    .centered_y()
}

/// Lines added and removed as figures that count up or down to each new one on the frames
/// between, rounded on each, as Desktop's do; under reduced motion they change at once. `key`
/// keeps their counts across rebuilds.
fn counting(key: &str, added: u32, removed: u32, size: f32) -> [Element; 2] {
    let figure = |what: &str, value: u32, format: fn(f32) -> String, color: limn::Color| {
        Element::rolling_number(format!("{key}:{what}"), value as f32, format)
            .foreground(color)
            .font_size(size)
            .centered_y()
    };
    [
        figure(
            "added",
            added,
            |n| format!("+{}", thousands(n.round() as u32)),
            look::added(),
        ),
        figure(
            "removed",
            removed,
            |n| format!("\u{2212}{}", thousands(n.round() as u32)),
            look::removed(),
        ),
    ]
}

/// What [`counting`] counts to, as a screen reader says it while the figures move.
fn counted(added: u32, removed: u32) -> String {
    format!("+{} \u{2212}{}", thousands(added), thousands(removed))
}

/// A count with commas between its thousands, as the branch bar writes them.
pub(crate) fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Why the branch bar's pull request cannot be opened now, in Desktop's words.
pub fn pr_blocked(session: &Session) -> Option<&'static str> {
    let repo = session.repo.as_ref()?;
    if repo.remote.is_none() {
        Some("Add a GitHub remote to create a pull request")
    } else if repo.base.as_ref() == Some(&repo.branch) {
        Some("Already on the base branch \u{2014} check out a feature branch first")
    } else if matches!(
        session.status,
        SessionStatus::Running | SessionStatus::Starting | SessionStatus::Awaiting
    ) {
        Some("The agent is working \u{2014} wait for the turn to finish")
    } else {
        None
    }
}

/// The queue bar above the composer: the messages sent while the turn runs, waiting for it to
/// end. One shows its row; two or more fold into a stack that expands to a list, as Desktop's
/// queued-message bar does [queue-spec §4].
fn queue_bar(
    session: &Session,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Option<Element> {
    let first = session.queue.first()?;
    let fill = look::palette().hover;
    let strip = |row: Vec<Element>| {
        Element::row(row)
            .gap(6.0)
            .padding(8.0)
            .size(Some(column), None)
    };
    let bar = if session.queue.len() == 1 {
        strip(vec![
            hgap(4.0),
            text("1 message queued").centered_y(),
            queue_text(&queue_preview(first)),
            queue_tool(&first.id, "send", icons::RETURN, "Send now", palette),
            queue_tool(
                &first.id,
                "take",
                icons::PENCIL,
                "Edit in composer",
                palette,
            ),
            queue_tool(
                &first.id,
                "remove",
                icons::CLOSE,
                "Remove from queue",
                palette,
            ),
            hgap(4.0),
        ])
        .background(fill)
        .radius(10.0)
    } else if ui.queue_expanded.contains(&session.id) {
        let mut list = vec![];
        for entry in &session.queue {
            if !list.is_empty() {
                list.push(hline(column, look::divider()));
            }
            list.push(queue_row(entry, ui, column, palette));
        }
        // The list scrolls past 352 px, Desktop's cap, so a long queue keeps the composer on screen.
        let header = strip(vec![
            hgap(4.0),
            text(format!("{} messages queued", session.queue.len()))
                .flex(1.0)
                .centered_y(),
            icon_tool(
                QUEUE_TOGGLE,
                icons::of(icons::CARET_UP),
                "Collapse queued messages",
                28.0,
                palette,
            ),
            hgap(4.0),
        ]);
        let clear = Element::row(vec![
            Element::spacer().flex(1.0),
            Element::clickable(
                QUEUE_CLEAR,
                text("Clear all")
                    .font_size(SMALL)
                    .foreground(look::danger_ink()),
            ),
        ])
        .padding(8.0)
        .size(Some(column), None);
        Element::column(vec![
            header,
            hline(column, look::divider()),
            Element::scroll("queue:list", Element::column(list)).max_height(352.0),
            hline(column, look::divider()),
            clear,
        ])
        .size(Some(column), None)
        .background(fill)
        .radius(10.0)
        .clip_children()
    } else {
        let head = strip(vec![
            hgap(4.0),
            text(format!("{} messages queued", session.queue.len())).centered_y(),
            queue_text(&format!("Next: {}", queue_preview(first))),
            Element::icon(icons::of(icons::CARET_DOWN))
                .foreground(look::palette().text)
                .size(Some(16.0), Some(16.0))
                .centered_y(),
            hgap(4.0),
        ])
        .background(fill)
        .radius(10.0)
        .hover_background(look::palette().selected);
        Element::clickable(QUEUE_TOGGLE, head).accessibility_label("Show the queued messages")
    };
    Some(bar)
}

/// One message of an expanded queue: its text in two lines at most, and its "⋮" menu.
fn queue_row(entry: &QueuedMessage, ui: &Ui, column: f32, palette: ComponentPalette) -> Element {
    let id = format!("{QUEUE_MENU}{}", entry.id);
    let open = ui.queue_menu.as_deref() == Some(entry.id.as_str());
    let anchor = icon_tool(
        &id,
        icons::of(icons::DOTS_VERTICAL),
        "Queued message actions",
        28.0,
        palette,
    );
    let items = vec![
        limn::DropdownMenuItem::new(format!("queue:take:{}", entry.id), "Edit in composer").into(),
        limn::DropdownMenuItem::new(format!("queue:send:{}", entry.id), "Send now").into(),
        limn::DropdownMenuEntry::Separator,
        limn::DropdownMenuItem::new(format!("queue:remove:{}", entry.id), "Remove from queue")
            .destructive(look::danger_ink())
            .into(),
    ];
    let actions = menu(
        anchor,
        items,
        open,
        &id,
        200.0,
        limn::PopoverPlacement::TopEnd,
    );
    Element::row(vec![
        hgap(8.0),
        text(queue_preview(entry))
            .max_lines(2)
            .flex(1.0)
            .centered_y(),
        actions,
        hgap(4.0),
    ])
    .gap(6.0)
    .padding(4.0)
    .size(Some(column), None)
}

/// A queue bar's icon button for one message: its `queue:<action>:<id>` id, as `queue_row_intent`
/// reads it.
fn queue_tool(
    id: &str,
    action: &str,
    icon: &'static str,
    label: &str,
    palette: ComponentPalette,
) -> Element {
    icon_tool(
        &format!("queue:{action}:{id}"),
        icons::of(icon),
        label,
        28.0,
        palette,
    )
}

/// The muted, one-line text of a queue bar, cut short with an ellipsis.
fn queue_text(value: &str) -> Element {
    Element::truncated_text(value.to_owned())
        .font_family(FontFamily::Sans)
        .font_size(BODY)
        .foreground(look::muted())
        .flex(1.0)
        .centered_y()
}

/// What a queued message shows of itself: its words in one run, then "[Image]" for each
/// image, or "…" when there is nothing to show.
pub fn queue_preview(entry: &QueuedMessage) -> String {
    let mut words: Vec<&str> = entry.text.split_whitespace().collect();
    words.extend(entry.images.iter().map(|_| "[Image]"));
    if words.is_empty() {
        "…".to_owned()
    } else {
        words.join(" ")
    }
}

/// A chip of the branch bar: 28 px, the bar's lighter fill.
fn bar_chip(content: Element) -> Element {
    Element::row(vec![content])
        .size(None, Some(28.0))
        .background(look::branch_chip())
        .border(look::branch_chip_edge(), 1.0)
        .radius(6.0)
        .clip_children()
        .centered_y()
}

/// The bar over a session's composer, as the person's Desktop keeps it: the folder and its branch, the lines added and
/// removed since the last commit counting to each new figure, the pull
/// request in the kind last chosen, and a close. It shows while the folder is a repository.
fn branch_bar(
    session: &Session,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Option<Element> {
    let repo = session.repo.as_ref()?;
    if ui.bar_closed.contains(&session.id) {
        return None;
    }
    // Narrow, the bar gives up the folder's name first, then the counts, so the pull request
    // and the close always fit; the branch takes what is left, cut short with an ellipsis.
    let mut row = vec![hgap(5.0)];
    if column >= 520.0 {
        row.push(muted(folder_name(&session.cwd)).centered_y());
    }
    row.push(
        Element::truncated_text(repo.branch.clone())
            .font_family(FontFamily::Monospace)
            .font_size(BODY)
            .foreground(look::muted())
            .flex(1.0)
            .centered_y(),
    );
    let (added, removed) = session
        .working
        .iter()
        .flatten()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    if added + removed > 0 && column >= 420.0 {
        let mut figures = vec![hgap(6.0)];
        figures.extend(counting(
            &format!("bar:{}", session.id),
            added,
            removed,
            BODY,
        ));
        figures.push(hgap(6.0));
        // A click shows the diff, with Desktop's tooltip.
        let counts = ComponentPalette {
            hover: look::branch_chip_hover(),
            ..palette
        }
        .interactive_surface(
            Element::clickable(VIEW_CHANGES, Element::row(figures).gap(4.0))
                .accessibility_label(counted(added, removed)),
        )
        .size(None, Some(28.0));
        row.push(bar_chip(
            limn::Tooltip::new(counts, VIEW_CHANGES, "View changes", look::palette()).render(),
        ));
    }
    let label = if ui.settings.pr_draft {
        "Create draft PR"
    } else {
        "Create PR"
    };
    let blocked = pr_blocked(session);
    // Space at the sides only: the label sits in the middle of the 28 px chip.
    let main = Element::row(vec![
        hgap(10.0),
        text(label)
            .foreground(if blocked.is_some() {
                look::muted()
            } else {
                look::palette().text
            })
            .centered_y(),
        hgap(10.0),
    ])
    .size(None, Some(28.0));
    let hover = ComponentPalette {
        hover: look::branch_chip_hover(),
        ..palette
    };
    let main = match blocked {
        Some(reason) => limn::Tooltip::new(main, "pr:why", reason, look::palette()).render(),
        None => hover
            .interactive_surface(Element::clickable(PR_CREATE, main))
            .size(None, Some(28.0)),
    };
    let open = ui.open_menu == Some(PR_MENU);
    let chevron = hover
        .interactive_surface(Element::clickable(
            PR_MENU,
            Element::icon(icons::of(icons::CARET_DOWN))
                .foreground(look::palette().text)
                .size(Some(14.0), Some(14.0))
                .centered(),
        ))
        .accessibility_label("Pull request options")
        .size(Some(28.0), Some(28.0));
    let chevron = if open {
        chevron.background(look::branch_chip_hover())
    } else {
        chevron
    };
    let kinds = vec![
        limn::DropdownMenuItem::new("pr-kind:ready", "Create PR")
            .radio(!ui.settings.pr_draft)
            .into(),
        limn::DropdownMenuItem::new("pr-kind:draft", "Create draft PR")
            .radio(ui.settings.pr_draft)
            .into(),
    ];
    let chevron = menu(
        chevron,
        kinds,
        open,
        PR_MENU,
        200.0,
        limn::PopoverPlacement::TopEnd,
    );
    row.push(bar_chip(Element::row(vec![
        main,
        Element::spacer()
            .background(look::branch_bar())
            .size(Some(1.0), Some(28.0)),
        chevron,
    ])));
    row.push(icon_tool(
        BAR_CLOSE,
        icons::of(icons::CLOSE),
        "Close",
        28.0,
        palette,
    ));
    Some(
        Element::row(row)
            .gap(8.0)
            .padding(8.0)
            .size(Some(column), Some(45.0))
            .background(look::branch_bar())
            .radius(12.0),
    )
}

/// A round-cornered 34 px icon button inside the composer, gray under the pointer and while
/// its menu is open.
fn composer_button(id: &str, icon: limn::Icon, label: &str, open: bool, side: f32) -> Element {
    let palette = ComponentPalette {
        hover: look::composer_hover(),
        selected: look::composer_open(),
        ..look::palette()
    };
    let glyph = Element::icon(icon)
        .foreground(look::palette().text)
        .size(Some(16.0), Some(16.0))
        .centered();
    let button = palette
        .interactive_surface(Element::clickable(id, glyph).accessibility_label(label))
        .radius(8.0)
        .size(Some(side), Some(side));
    let button = if open {
        button.background(look::composer_open())
    } else {
        button
    };
    limn::Tooltip::new(button, id, label, look::palette()).render()
}

/// The composer's Stop: its white rounded square. Every Stop of a running thing is this one.
pub(crate) fn stop_button(id: &str) -> Element {
    composer_button(id, icons::of(icons::STOP), "Stop", false, 28.0)
}

/// Send, drawn as Desktop's return arrow; limn has no such icon, so it is the character.
fn send_button(ready: bool) -> Element {
    let palette = ComponentPalette {
        hover: look::composer_hover(),
        selected: look::composer_open(),
        ..look::palette()
    };
    // Muted while there is nothing to send, as Desktop's.
    let ink = if ready {
        look::palette().text
    } else {
        look::muted()
    };
    let glyph = Element::icon(icons::of(icons::RETURN))
        .foreground(ink)
        .size(Some(22.0), Some(22.0))
        .centered();
    let button = palette
        .interactive_surface(Element::clickable(SEND, glyph))
        .radius(8.0)
        // 28 px: limn centers it in the first line's 28 px, so it sits mid-box.
        .size(Some(28.0), Some(28.0));
    limn::Tooltip::new(button, SEND, "Send", look::palette()).render()
}

/// "+": what can go into the message, as far as quill can: files and folders
/// as @-mentions the agent reads, and the slash that opens its commands.
fn plus_menu(session: Option<&Session>, ui: &Ui) -> Element {
    let open = ui.open_menu == Some(PLUS_MENU);
    let anchor = composer_button(PLUS_MENU, icons::of(icons::ADD), "Add", open, 28.0);
    let mut entries = vec![
        limn::DropdownMenuItem::new(ATTACH_FILES, "Add files or photos")
            .icon(icons::of(icons::PAPERCLIP))
            .shortcut("Ctrl+U")
            .into(),
        limn::DropdownMenuItem::new(ATTACH_FOLDER, "Add folder")
            .icon(icons::of(icons::FOLDER))
            .into(),
        limn::DropdownMenuItem::new(SLASH, "Slash commands")
            .icon(icons::of(icons::SLASH))
            .into(),
    ];
    // Desktop's Connectors and Plugins: ways to find and
    // manage them, then each one with its switch. A session's connectors are its agent's MCP
    // servers; the plugins are its agent's.
    let mut connectors = vec![
        limn::DropdownMenuItem::new(CONNECTORS_BROWSE, "Browse connectors")
            .icon(icons::of(icons::CONNECTORS))
            .into(),
        limn::DropdownMenuItem::new(CONNECTORS_MANAGE, "Manage connectors")
            .icon(icons::of(icons::BRIEFCASE))
            .into(),
    ];
    let servers = session.and_then(|s| s.mcp.as_ref());
    // The agent says its servers only while it runs.
    if servers.is_none() && session.is_some() {
        connectors.push(limn::DropdownMenuEntry::Separator);
        connectors.push(limn::DropdownMenuEntry::Message(
            "The session's connectors show once its agent runs.".into(),
        ));
    }
    if let Some(servers) = servers.filter(|s| !s.is_empty()) {
        connectors.push(limn::DropdownMenuEntry::Separator);
        // An agent that cannot turn a server off for the session shows it without a switch.
        let can_toggle = session
            .and_then(|s| s.capabilities.as_ref())
            .is_some_and(|c| c.mcp_toggle);
        connectors.extend(servers.iter().map(|server| {
            let on = server.status != quill_model::McpStatus::Disabled;
            let id = format!("{CONNECTOR_SWITCH}{}:{}", u8::from(!on), server.name);
            let item =
                limn::DropdownMenuItem::new(id, server.label.clone()).icon(icons::of(icons::PLUG));
            if can_toggle {
                item.switch(on).into()
            } else {
                item.description(if on { "On" } else { "Off in its config" })
                    .disabled()
                    .into()
            }
        }));
    }
    // Desktop's Plugins: each plugin on with skills, its one skill
    // or a menu of them, which goes into the message as `/plugin:skill`; then managing and
    // finding plugins.
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    let known = ui.extensions.get(agent).map(|e| &e.list);
    let mut plugins: Vec<limn::DropdownMenuEntry> = vec![];
    for plugin in known
        .map_or(&[][..], |k| &k.plugins)
        .iter()
        .filter(|p| p.enabled)
    {
        let prefix = format!("{}:", plugin.name);
        let skills: Vec<&quill_model::Skill> = known
            .map_or(&[][..], |k| &k.skills)
            .iter()
            .filter(|s| s.name.starts_with(&prefix))
            .collect();
        let item = |skill: &quill_model::Skill, label: &str| {
            limn::DropdownMenuItem::new(format!("{PLUGIN_SKILL}{}", skill.name), label)
                .description(skill.description.clone())
                .truncate()
        };
        match skills.as_slice() {
            [] => {}
            [skill] => plugins.push(
                item(skill, &plugin.name)
                    .icon(icons::of(icons::PLUG))
                    .into(),
            ),
            many => plugins.push(
                limn::DropdownMenuItem::new(
                    format!("{PLUS_MENU}:plugin:{}", plugin.id),
                    plugin.name.clone(),
                )
                .icon(icons::of(icons::PLUG))
                .description(format!("{} skills", many.len()))
                .submenu(
                    many.iter()
                        .map(|s| item(s, s.name.trim_start_matches(&prefix)).into())
                        .collect(),
                )
                .into(),
            ),
        }
    }
    plugins.push(limn::DropdownMenuEntry::Separator);
    plugins.push(
        limn::DropdownMenuItem::new(PLUGINS_MANAGE, "Manage plugins")
            .icon(icons::of(icons::BRIEFCASE))
            .into(),
    );
    plugins.push(
        limn::DropdownMenuItem::new(PLUGINS_BROWSE, "Browse plugins")
            .icon(icons::of(icons::CONNECTORS))
            .into(),
    );
    entries.push(
        limn::DropdownMenuItem::new(format!("{PLUS_MENU}:connectors"), "Connectors")
            .icon(icons::of(icons::CONNECTORS))
            .submenu(connectors)
            .into(),
    );
    entries.push(
        limn::DropdownMenuItem::new(format!("{PLUS_MENU}:plugins"), "Plugins")
            .icon(icons::of(icons::PLUG))
            .submenu(plugins)
            .into(),
    );
    menu(
        anchor,
        entries,
        open,
        PLUS_MENU,
        320.0,
        limn::PopoverPlacement::TopStart,
    )
}

/// A footer chip: an icon and words, gray under the pointer and darker while its menu is
/// open, 24 px tall.
fn chip(
    id: &str,
    content: Vec<Element>,
    open: bool,
    tooltip: &str,
    shortcut: Option<&str>,
) -> Element {
    let palette = ComponentPalette {
        hover: look::chip_hover(),
        selected: look::chip_open(),
        ..look::palette()
    };
    let mut items = vec![hgap(6.0)];
    items.extend(content);
    items.push(hgap(6.0));
    let row = Element::row(items).gap(6.0).size(None, Some(24.0));
    let chip = palette
        .interactive_surface(Element::clickable(id, row))
        .radius(6.0);
    // Above the composer the chips are filled at rest.
    let chip = if open {
        chip.background(look::chip_open())
    } else if id == FOLDER_MENU || id == AGENT_MENU {
        chip.background(look::cancel())
    } else {
        chip
    };
    let tip = limn::Tooltip::new(chip, id, tooltip, look::palette());
    match shortcut {
        Some(keys) => tip.shortcut(keys),
        None => tip,
    }
    .render()
}

fn chip_text(s: impl Into<String>, ink: limn::Color) -> Element {
    text(s).font_size(SMALL).foreground(ink).centered_y()
}

fn chip_icon(icon: limn::Icon) -> Element {
    Element::icon(icon)
        .foreground(look::palette().text)
        .size(Some(14.0), Some(14.0))
        .centered_y()
}

/// The row under the composer: "+" and the mode on the left; the model, its
/// effort and the plan's usage ring on the right.
fn composer_footer(
    store: &Store,
    session: Option<&Session>,
    ui: &Ui,
    palette: ComponentPalette,
) -> Element {
    let row = Element::row(vec![
        plus_menu(session, ui),
        mode_menu(store, session, ui),
        Element::spacer().flex(1.0),
        model_menu(store, session, ui, palette),
        effort_menu(store, ui),
        usage_menu(store, session, ui),
    ])
    .gap(2.0)
    .size(None, Some(28.0));
    // 20 px under the box, as Desktop's, so the row's tooltips clear the message.
    Element::column(vec![Element::spacer().size(None, Some(6.0)), row])
}

/// When a window starts again, as Desktop says it: "Resets in 4 hr 53 min"
/// within a day, else the local day and hour, "Resets Sun 3:00 AM". `offset` is the local
/// time's distance from UTC in seconds.
pub fn resets_in(resets_at: u64, now: u64, offset: i64) -> String {
    // A window already over says nothing until the agent reports the next one.
    if resets_at <= now {
        return String::new();
    }
    let left = resets_at - now;
    if left < 86_400 {
        let (hours, minutes) = (left / 3_600, left % 3_600 / 60);
        return if hours == 0 {
            format!("Resets in {minutes} min")
        } else {
            format!("Resets in {hours} hr {minutes} min")
        };
    }
    let local = resets_at as i64 + offset;
    // 1 January 1970 was a Thursday.
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let day = DAYS[local.div_euclid(86_400).rem_euclid(7) as usize];
    let seconds = local.rem_euclid(86_400);
    let (hour, minute) = (seconds / 3_600, seconds % 3_600 / 60);
    let (twelve, half) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!("Resets {day} {twelve}:{minute:02} {half}")
}

/// The share of a plan window spent: none once it started again, until the agent reports
/// the new one.
pub(crate) fn spent(window: &quill_model::UsageWindow, now: u64) -> f32 {
    if window.resets_at <= now {
        0.0
    } else {
        window.utilization
    }
}

/// A 4 px bar filled blue to `share`, on Desktop's chart ground so it shows on
/// the page, a card and a menu alike.
fn usage_bar(share: f32, width: f32) -> Element {
    let fill = (width * share.clamp(0.0, 1.0)).max(if share > 0.0 { 4.0 } else { 0.0 });
    Element::row(vec![Element::spacer()
        .background(look::handle())
        .radius(2.0)
        .size(Some(fill), Some(4.0))])
    .background(look::chart_tint())
    .radius(2.0)
    .size(Some(width), Some(4.0))
}

/// One labelled row of the usage popover, over its bar.
fn usage_row(label: &str, detail: String, share: f32, width: f32, muted_label: bool) -> Element {
    let label = if muted_label {
        muted(label.to_owned())
    } else {
        text(label.to_owned())
    };
    Element::column(vec![
        Element::row(vec![
            label.font_size(SMALL),
            Element::spacer().flex(1.0),
            muted(detail).font_size(SMALL),
        ])
        .size(Some(width), Some(20.0)),
        usage_bar(share, width),
    ])
    .gap(8.0)
}

/// The plan limits of the agent in view: the open session's, else the one a new session runs.
pub(crate) fn agent_limits(store: &Store, ui: &Ui) -> Option<quill_model::PlanLimits> {
    let session = ui.selected.and_then(|id| store.session(id));
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    store.limits_by_agent.get(agent).copied()
}

/// The usage ring and its popover: the context window, then the plan's session
/// and weekly limits with when they reset, from what the agent reports.
fn usage_menu(store: &Store, session: Option<&Session>, ui: &Ui) -> Element {
    let open = ui.open_menu == Some(USAGE_MENU);
    let limits = agent_limits(store, ui).unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // A 14 px ring, blue over gray, as Desktop's, filled as the Context card is.
    let share = session.and_then(context_share).unwrap_or(0.0);
    let ring_palette = ComponentPalette {
        text: look::handle(),
        // Seen on the hover's #E5E5E5 too; Desktop darkens it there.
        selected: look::ring_track(),
        ..look::palette()
    };
    let ring = limn::Progress::new(share, ring_palette)
        .ring(14.0)
        .render()
        .centered();
    let palette = ComponentPalette {
        hover: look::tool_hover(),
        ..look::palette()
    };
    let anchor = palette
        .interactive_surface(Element::clickable(USAGE_MENU, ring))
        .radius(6.0)
        .size(Some(28.0), Some(28.0));
    let anchor = if open {
        anchor.background(look::tool_hover())
    } else {
        anchor
    };
    let width = 328.0;
    let tokens = session.map_or(0, |s| s.context_tokens);
    let tokens = if tokens >= 1000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        tokens.to_string()
    };
    let plan = account_plan(store, session);
    let mut rows = vec![
        usage_row("Context window", tokens.clone(), share, width, true),
        hline(width, look::divider()),
        Element::row(vec![
            muted(match &plan {
                Some(plan) => format!("Plan usage limits · {plan}"),
                None => "Plan usage limits".into(),
            })
            .font_size(SMALL),
            Element::spacer().flex(1.0),
            Element::clickable(USAGE_ALL, text("See all →").font_size(SMALL)),
        ])
        .size(Some(width), Some(20.0)),
    ];
    for (label, window) in [
        ("Session limit", limits.session),
        ("Weekly · all models", limits.weekly),
    ] {
        if let Some(w) = window {
            rows.push(usage_row(
                label,
                format!(
                    "{}  {}%",
                    resets_in(w.resets_at, now, ui.utc_offset),
                    (spent(&w, now) * 100.0).round() as u32
                ),
                spent(&w, now),
                width,
                false,
            ));
        }
    }
    let card = Element::column(rows).gap(14.0).padding(8.0);
    // Its hint sums it up, as Desktop's: "Context 30.0k", the week's share and reset; with
    // the window known, as the Context card says it: "51% · 512.9K / 1M".
    let title = match session.map(context_fill) {
        Some((used, max)) if max > 0 => format!(
            "Context {}% \u{b7} {} / {}",
            (share * 100.0).round().min(100.0) as u32,
            compact(used),
            compact(max)
        ),
        _ => format!("Context {tokens}"),
    };
    let mut tip = limn::Tooltip::new(anchor, USAGE_MENU, title, look::palette())
        // At the window's right edge it opens to the left, to stay whole.
        .placement(limn::TooltipPlacement::Left);
    if let Some(w) = limits.weekly {
        tip = tip.description(format!(
            "Weekly · all models: {}% · {}",
            (spent(&w, now) * 100.0).round() as u32,
            resets_in(w.resets_at, now, ui.utc_offset)
        ));
    }
    let anchor = tip.render();
    menu(
        anchor,
        vec![limn::DropdownMenuEntry::Custom(Box::new(card))],
        open,
        USAGE_MENU,
        width + 32.0,
        limn::PopoverPlacement::TopEnd,
    )
}

/// The agent a new session runs [inferred: a chip beside "Local" and the folder]: its logo and
/// name, opening the agents available. Only offered when there is more than one.
fn agent_menu(ui: &Ui) -> Option<Element> {
    if ui.agents.len() < 2 {
        return None;
    }
    let open = ui.open_menu == Some(AGENT_MENU);
    let chosen = ui.agents.iter().find(|a| a.id == ui.agent)?;
    let logo = |a: &quill_model::AgentInfo| icons::of(icons::agent(a.icon.as_deref()));
    let anchor = chip(
        AGENT_MENU,
        vec![
            chip_icon(logo(chosen)),
            chip_text(&chosen.name, look::palette().text),
        ],
        open,
        "Agent",
        None,
    );
    let mut entries = vec![limn::DropdownMenuEntry::Heading("Agent".into())];
    entries.extend(ui.agents.iter().map(|a| {
        limn::DropdownMenuItem::new(format!("pick-agent:{}", a.id), a.name.clone())
            .icon(logo(a))
            .selected(a.id == ui.agent)
            .into()
    }));
    Some(menu(
        anchor,
        entries,
        open,
        AGENT_MENU,
        200.0,
        limn::PopoverPlacement::TopStart,
    ))
}

/// The folder a new session works in: the recent ones and "Open folder…".
fn folder_menu(store: &Store, ui: &Ui) -> Element {
    let open = ui.open_menu == Some(FOLDER_MENU);
    let anchor = chip(
        FOLDER_MENU,
        vec![
            chip_icon(icons::of(icons::FOLDER)),
            chip_text(folder_name(&ui.cwd), look::palette().text),
        ],
        open,
        "Folder",
        None,
    );
    let mut recent: Vec<&std::path::PathBuf> = vec![&ui.cwd];
    for session in store.sessions.iter().rev() {
        if !recent.contains(&&session.cwd) {
            recent.push(&session.cwd);
        }
    }
    let mut entries = vec![limn::DropdownMenuEntry::Heading("Recent".into())];
    entries.extend(recent.into_iter().take(6).map(|folder| {
        limn::DropdownMenuItem::new(format!("folder:{}", folder.display()), folder_name(folder))
            .selected(*folder == ui.cwd)
            .into()
    }));
    entries.push(limn::DropdownMenuEntry::Separator);
    entries.push(limn::DropdownMenuItem::new(PICK_FOLDER, "Open folder…").into());
    menu(
        anchor,
        entries,
        open,
        FOLDER_MENU,
        220.0,
        limn::PopoverPlacement::TopStart,
    )
}

/// The modes the menu lists, in its order: those the agent offers, then those the chosen
/// model adds (Auto), that quill can name.
pub(crate) fn offered_modes(store: &Store, session: Option<&Session>, ui: &Ui) -> Vec<String> {
    let capabilities = agent_capabilities(store, session, ui);
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    // Before the agent has said what it offers: the modes its adapter lists at the start.
    let mut modes = capabilities
        .map(|c| c.modes.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| {
            let words = ui.agents.iter().find(|a| a.id == agent).map(|a| &a.modes);
            (words.into_iter().flatten())
                .filter(|m| m.listed)
                .map(|m| m.id.clone())
                .collect()
        });
    // The chosen model, or the agent's first, its default.
    let models = offered_models(store, session, ui);
    let model = ui
        .model
        .as_deref()
        .and_then(|id| (models.iter()).find(|m| m.id == id || m.name.to_lowercase().contains(id)))
        .or(models.first());
    modes.extend(model.into_iter().flat_map(|m| m.modes.iter().cloned()));
    modes.retain(|m| mode_entry(ui, agent, m).is_some());
    modes
}

/// The modes the mode menu lists: those offered, and those that skip every approval once the
/// person allowed them for the agent in Settings.
pub(crate) fn menu_modes(store: &Store, session: Option<&Session>, ui: &Ui) -> Vec<String> {
    let mut modes = offered_modes(store, session, ui);
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    if ui.settings.agent(agent).allow_unsafe {
        // The session's word, else the adapter's: before any session of the agent, too.
        let mut unsafe_modes = agent_capabilities(store, session, ui)
            .map(|c| c.unsafe_modes.clone())
            .unwrap_or_default();
        let words = ui.agents.iter().find(|a| a.id == agent).map(|a| &a.modes);
        for word in words.into_iter().flatten() {
            if matches!(word.tone, quill_model::ModeTone::Unsafe)
                && !unsafe_modes.contains(&word.id)
            {
                unsafe_modes.push(word.id.clone());
            }
        }
        for mode in unsafe_modes {
            if !modes.contains(&mode) && mode_entry(ui, agent, &mode).is_some() {
                modes.push(mode);
            }
        }
    }
    modes
}

/// What the agent can do: the session's, or for a new session the last session of the same
/// agent that heard it, so the composer offers its models and modes before the first message.
pub(crate) fn agent_capabilities<'a>(
    store: &'a Store,
    session: Option<&'a Session>,
    ui: &Ui,
) -> Option<&'a quill_model::AgentCapabilities> {
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    session.and_then(|s| s.capabilities.as_ref()).or_else(|| {
        (store.sessions.iter().rev())
            .filter(|s| s.agent == agent)
            .find_map(|s| s.capabilities.as_ref())
    })
}

/// The models the menu lists: the session's agent's, for a new session the last session's,
/// The stop past max that the slider draws alive: Claude Code's `ultracode` and Codex's
/// `ultra` (model/list, account.jsonl:7) are the same top setting under each program's name.
pub(crate) fn ultra_effort(effort: &str) -> bool {
    matches!(effort, "ultracode" | "ultra")
}

/// and before any session of the agent what its program offered when asked.
pub(crate) fn offered_models<'a>(
    store: &'a Store,
    session: Option<&'a Session>,
    ui: &'a Ui,
) -> &'a [quill_model::ModelInfo] {
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    agent_capabilities(store, session, ui)
        .map(|c| c.models.as_slice())
        .filter(|models| !models.is_empty())
        .or_else(|| ui.agent_models.get(agent).map(Vec::as_slice))
        .unwrap_or_default()
}

/// The models Desktop lists by number, the newest of each family, and the rest it keeps
/// under "More models". The agent lists each family newest first.
pub(crate) fn split_models(
    models: &[quill_model::ModelInfo],
) -> (Vec<&quill_model::ModelInfo>, Vec<&quill_model::ModelInfo>) {
    let mut families: Vec<&str> = vec![];
    let (mut latest, mut more) = (vec![], vec![]);
    for model in models {
        let family = model.name.split_whitespace().next().unwrap_or_default();
        if family == "Default" || families.contains(&family) {
            more.push(model);
        } else {
            families.push(family);
            latest.push(model);
        }
    }
    (latest, more)
}

/// The row a typed digit picks in the open mode or model menu, as Desktop's 1–4 do, or a
/// typed letter in a session's menu, as its P, R and D.
pub fn menu_digit(store: &Store, ui: &Ui, typed: &str) -> Option<String> {
    let menu_session = ui
        .row_menu
        .or(ui.selected.filter(|_| ui.open_menu == Some(TITLE_MENU)));
    if let Some(session) = menu_session {
        let action = match typed {
            "p" | "P" if ui.row_menu.is_some() => "pin",
            "u" | "U" if ui.row_menu.is_some() => "unread",
            "r" | "R" => "rename",
            "f" | "F" => "fork",
            "a" | "A" => "archive",
            "d" | "D" => "delete",
            _ => return None,
        };
        return Some(format!("{action}:{session}"));
    }
    let n: usize = typed.parse().ok().filter(|n| (1..=9).contains(n))?;
    let session = ui.selected.and_then(|id| store.session(id));
    match ui.open_menu? {
        MODE_MENU => menu_modes(store, session, ui)
            .get(n - 1)
            .map(|m| format!("mode:{m}")),
        MODEL_MENU => split_models(offered_models(store, session, ui))
            .0
            .get(n - 1)
            .map(|m| format!("model:{}", m.id)),
        _ => None,
    }
}

/// What `mode` is called and does for `agent`, in the words its adapter gave; `None` for a
/// mode the adapter has no words for.
pub(crate) fn mode_entry<'a>(
    ui: &'a Ui,
    agent: &str,
    mode: &str,
) -> Option<&'a quill_model::ModeWords> {
    (ui.agents.iter().find(|a| a.id == agent))?
        .modes
        .iter()
        .find(|m| m.id == mode)
}

fn mode_icon(tone: quill_model::ModeTone) -> &'static str {
    use quill_model::ModeTone::*;
    match tone {
        Edits => icons::CODE,
        Plan => icons::SCROLL,
        Auto => icons::LIGHTNING,
        Read => icons::EYE,
        Unsafe => icons::WARNING,
        Ask => icons::HAND,
    }
}

/// The permission mode: Manual reads amber, as Desktop's chip does.
fn mode_menu(store: &Store, session: Option<&Session>, ui: &Ui) -> Element {
    let open = ui.open_menu == Some(MODE_MENU);
    let current = session.map_or(ui.mode.as_str(), |s| s.mode.as_str());
    let agent = session.map_or(ui.agent.as_str(), |s| s.agent.as_str());
    let words = mode_entry(ui, agent, current);
    let label = words.map_or(current, |w| w.label.as_str());
    // A mode that skips approvals reads as the warning it is, as Desktop marks it
    // (`isWarning`); one that asks reads amber.
    let ink = match words.map(|w| w.tone) {
        Some(quill_model::ModeTone::Ask) => look::manual(),
        Some(quill_model::ModeTone::Unsafe) => look::failed(),
        _ => look::palette().text,
    };
    // Desktop's tooltip says what the mode does.
    let about = words.map_or("", |w| w.about.as_str());
    let anchor = chip(
        MODE_MENU,
        vec![chip_text(label, ink)],
        open,
        about,
        Some("Ctrl+Shift+M"),
    );
    let mut entries = vec![limn::DropdownMenuEntry::Heading("Mode".into())];
    for (i, mode) in menu_modes(store, session, ui).iter().enumerate() {
        let Some(words) = mode_entry(ui, agent, mode) else {
            continue;
        };
        let n = i + 1;
        entries.push(
            limn::DropdownMenuItem::new(format!("mode:{mode}"), words.label.clone())
                .icon(icons::of(mode_icon(words.tone)))
                .description(words.about.clone())
                .shortcut(n.to_string())
                .selected(mode == current)
                .into(),
        );
    }
    menu(
        anchor,
        entries,
        open,
        MODE_MENU,
        // Wide enough for the longest words, "The agent handles permission decisions".
        340.0,
        limn::PopoverPlacement::TopStart,
    )
}

/// Desktop's names for effort levels.
pub(crate) fn effort_label(effort: &str) -> String {
    match effort {
        "xhigh" => "Extra".into(),
        "medium" => "Medium".into(),
        other => {
            let mut s = other.to_owned();
            if let Some(first) = s.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            s
        }
    }
}

/// The model and its effort: the models numbered, the current one checked, then
/// Effort with the levels that model accepts.
fn model_menu(
    store: &Store,
    session: Option<&Session>,
    ui: &Ui,
    _palette: ComponentPalette,
) -> Element {
    let open = ui.open_menu == Some(MODEL_MENU);
    let models = offered_models(store, session, ui);
    let current = current_model(store, session, ui);
    // The agent's own default says which model it is.
    let named = |model: &quill_model::ModelInfo| match model.default {
        true => format!("{} (default)", model.name),
        false => model.name.clone(),
    };
    let name = match current {
        Some(model) if ui.model.is_none() => named(model),
        Some(model) => model.name.clone(),
        None => ui.model.clone().unwrap_or_else(|| "Default".into()),
    };
    let content = vec![chip_text(name, look::palette().text)];
    let anchor = chip(MODEL_MENU, content, open, "Model", Some("Ctrl+Shift+I"));
    // The person's Desktop lists the models bare; effort has its own chip.
    let mut entries = vec![];
    let (latest, more) = split_models(models);
    let item = |model: &quill_model::ModelInfo| {
        limn::DropdownMenuItem::new(format!("model:{}", model.id), named(model))
            .selected(current.is_some_and(|c| c.id == model.id))
    };
    for (n, model) in latest.iter().enumerate() {
        // The current model shows its check in place of its number.
        let row = item(model);
        let row = if current.is_some_and(|c| c.id == model.id) {
            row
        } else {
            row.shortcut((n + 1).to_string())
        };
        entries.push(row.into());
    }
    if !more.is_empty() {
        entries.push(limn::DropdownMenuEntry::Separator);
        let rest = more.iter().map(|m| item(m).into()).collect();
        entries.push(
            limn::DropdownMenuItem::new(format!("{MODEL_MENU}:more"), "More models")
                .submenu(rest)
                .into(),
        );
    }
    menu(
        anchor,
        entries,
        open,
        MODEL_MENU,
        200.0,
        limn::PopoverPlacement::TopEnd,
    )
}

/// The model the composer runs: the one picked, matched by id or by name, else the agent's
/// default.
pub(crate) fn current_model<'a>(
    store: &'a Store,
    session: Option<&'a Session>,
    ui: &'a Ui,
) -> Option<&'a quill_model::ModelInfo> {
    let models = offered_models(store, session, ui);
    match ui.model.as_deref() {
        Some(id) => (models.iter()).find(|m| m.id == id || m.name.to_lowercase().contains(id)),
        None => models.iter().find(|m| m.default),
    }
}

/// The efforts the composer's model accepts, the slider's stops in order.
pub fn efforts(store: &Store, ui: &Ui) -> Vec<String> {
    let session = ui.selected.and_then(|id| store.session(id));
    current_model(store, session, ui)
        .map(|m| m.efforts.clone())
        .unwrap_or_default()
}

/// Where the effort slider stands: its stops, the level chosen, where the thumb is, and
/// the level it is nearest, which the heading names as it goes.
struct EffortState {
    efforts: Vec<String>,
    chosen: usize,
    thumb_x: f32,
    shown: usize,
}

fn effort_state(store: &Store, ui: &Ui) -> Option<EffortState> {
    let efforts = efforts(store, ui);
    let n = efforts.len();
    if n == 0 {
        return None;
    }
    let last = n.saturating_sub(1).max(1) as f32;
    let chosen = ui
        .effort
        .as_ref()
        .and_then(|e| efforts.iter().position(|x| x == e))
        .unwrap_or(0);
    let rest = if n > 1 { chosen as f32 / last } else { 0.5 };
    // The thumb is where the pointer holds it, or on its way to its stop, else on the level.
    let now = std::time::Instant::now();
    let thumb_x = match ui.effort_thumb {
        Some(thumb) if ui.effort_dragging || thumb.is_active(now) => thumb.value(now),
        _ => rest * EFFORT_TRAVEL,
    }
    // The spring does not overshoot the track's ends.
    .clamp(0.0, EFFORT_TRAVEL);
    let shown = ((thumb_x / EFFORT_TRAVEL * last).round().max(0.0) as usize).min(n - 1);
    Some(EffortState {
        efforts,
        chosen,
        thumb_x,
        shown,
    })
}

/// The level the effort slider's thumb is nearest, while its popover is open.
pub fn effort_shown(store: &Store, ui: &Ui) -> Option<String> {
    if ui.open_menu != Some(EFFORT_MENU) {
        return None;
    }
    effort_state(store, ui).map(|s| s.efforts[s.shown].clone())
}

/// The effort chip and its popover, as the person's Desktop has them: "Effort  High", "Faster … Smarter" over a slider with a stop
/// per level the model accepts, and "Recommended" under medium. The slider is Desktop's: a 28 px control whose 24 px track fills from its start to the
/// thumb's middle, a white 20 × 24 thumb that grows to 1.12 while held, 3 px dots and a
/// 3 × 9 tick. On Ultracode the heading turns violet and the track is the reference
/// selector's pixel field.
fn effort_menu(store: &Store, ui: &Ui) -> Element {
    let Some(state) = effort_state(store, ui) else {
        return Element::spacer().size(Some(0.0), Some(0.0));
    };
    let EffortState {
        efforts,
        chosen,
        thumb_x,
        shown,
    } = state;
    let open = ui.open_menu == Some(EFFORT_MENU);
    // None chosen, the agent runs at its own default, which it does not tell: "Default", not
    // the first stop.
    let unchosen = ui.effort.is_none() && !ui.effort_dragging;
    let label = |effort: &str| match unchosen {
        true => "Default".to_owned(),
        false => effort_label(effort),
    };
    let anchor = chip(
        EFFORT_MENU,
        vec![chip_text(label(&efforts[chosen]), look::palette().text)],
        open,
        "Effort",
        None,
    );
    let width = EFFORT_WIDTH;
    let n = efforts.len();
    let last = n.saturating_sub(1).max(1) as f32;
    let rest = if n > 1 { chosen as f32 / last } else { 0.5 };
    let recommended = efforts.iter().position(|e| e == "medium");
    let ultra = ultra_effort(&efforts[shown]);
    let stop_x = |i: usize| {
        if n > 1 {
            i as f32 / last * EFFORT_TRAVEL
        } else {
            EFFORT_TRAVEL / 2.0
        }
    };
    let at_x = |x: f32, side: f32, height: f32, mark: Element| {
        Element::row(vec![hgap(EFFORT_INSET + x - side / 2.0), mark.centered_y()])
            .size(Some(width), Some(height))
    };
    let mut track: Vec<Element> = vec![];
    if ultra {
        track.push(
            Element::spacer()
                .horizontal_gradient(look::ultra_track())
                // As the track: its clip keeps corners square.
                .radius(6.0)
                .size(Some(width), Some(24.0)),
        );
        track.push(pixel_field(ui, width, 24.0));
    } else {
        // The fill runs from the start to the thumb's middle, under it.
        track.push(
            Element::row(vec![Element::spacer()
                .background(look::slider_fill())
                .radius(6.0)
                .size(Some(EFFORT_INSET + thumb_x), Some(24.0))])
            .size(Some(width), Some(24.0)),
        );
        for i in 0..n {
            let (w, h) = if Some(i) == recommended {
                (3.0, 9.0)
            } else {
                (3.0, 3.0)
            };
            track.push(at_x(
                stop_x(i),
                w,
                24.0,
                Element::spacer()
                    .background(look::slider_mark())
                    .radius(1.5)
                    .size(Some(w), Some(h)),
            ));
        }
    }
    let track = Element::stack(track)
        .background(look::slider_track())
        .radius(6.0)
        .clip_children()
        .size(Some(width), Some(24.0));
    // Held, the thumb grows a little, as Desktop's does.
    // 21 wide on a 20 px travel: half a pixel over each end, so at a fractional scale the
    // track's last column does not show beside it.
    let thumb = at_x(
        thumb_x,
        21.0,
        28.0,
        Element::spacer()
            .background(limn::Color::WHITE)
            .border(look::slider_thumb_edge(), 1.0)
            .radius(6.0)
            .size(Some(21.0), Some(24.0))
            .animate_scale_about(
                "effort:thumb",
                if ui.effort_dragging { 1.12 } else { 1.0 },
                limn::Point::new(0.5, 0.5),
            ),
    );
    let control = Element::stack(vec![
        Element::column(vec![Element::spacer().size(None, Some(2.0)), track])
            .size(Some(width), Some(28.0)),
        thumb,
    ]);
    let control = Element::clickable(EFFORT_SLIDER, control)
        .slider(rest, n, EFFORT_INSET)
        .accessibility_label("Effort")
        .size(Some(width), Some(28.0));
    let effort = efforts[shown].clone();
    let name = text(label(&effort)).font_size(BODY);
    // Ultracode reads in Desktop's "pro" accent.
    let name = if ultra {
        name.foreground(look::pro())
    } else {
        name
    };
    let mut rows = vec![
        Element::row(vec![
            muted("Effort").font_size(BODY),
            // The level's name slides in as the thumb reaches it [reference selector].
            limn::Ticker::new("effort:level", effort.clone(), name.size(None, Some(18.0)))
                .height(18.0)
                .render(),
            Element::spacer().flex(1.0),
            limn::Tooltip::new(
                Element::icon(icons::of(icons::QUESTION))
                    .foreground(look::muted())
                    .size(Some(16.0), Some(16.0)),
                "effort:help",
                "How long Claude thinks before it answers",
                look::palette(),
            )
            .render(),
        ])
        .gap(10.0)
        .size(Some(width), None),
        Element::spacer().size(None, Some(6.0)),
        Element::row(vec![
            muted("Faster").font_size(SMALL),
            Element::spacer().flex(1.0),
            muted("Smarter").font_size(SMALL),
        ])
        .size(Some(width), None),
        control,
    ];
    if let Some(recommended) = recommended {
        // Under its stop, as Desktop labels the recommended level.
        let x = (EFFORT_INSET + stop_x(recommended) - 42.0).max(0.0_f32);
        rows.push(Element::row(vec![
            hgap(x),
            muted("Recommended").font_size(SMALL),
        ]));
    }
    // The menu adds 8 px of its own: 16 in all, as Desktop's.
    let card = Element::column(rows).gap(10.0).padding(8.0);
    menu(
        anchor,
        vec![limn::DropdownMenuEntry::Custom(Box::new(card))],
        open,
        EFFORT_MENU,
        width + 32.0,
        limn::PopoverPlacement::TopEnd,
    )
}

/// The Ultracode track's lilac cells, sweeping in from the right over its first second
/// (`pixels.rs`, after the reference selector); still and whole under reduced motion.
fn pixel_field(ui: &Ui, width: f32, height: f32) -> Element {
    let elapsed = ui
        .ultra_since
        .map_or(0.0, |since| since.elapsed().as_secs_f32() * 1000.0);
    let t = (elapsed / 1000.0).clamp(0.0, 1.0);
    let reveal = t * t * (3.0 - 2.0 * t);
    let left = look::ultra_left();
    let (columns, cells) = crate::pixels::field(width, height, elapsed, reveal, left);
    // The track is rounded and its clip is not: a cell in a corner, outside the 6 px arc,
    // would show beside the thumb's rounded corner, so it is left out.
    const RADIUS: f32 = 6.0;
    let inside = |x: f32, y: f32| {
        let dx = (RADIUS - x).max(x - (width - RADIUS)).max(0.0);
        let dy = (RADIUS - y).max(y - (height - RADIUS)).max(0.0);
        dx * dx + dy * dy <= RADIUS * RADIUS
    };
    let cell = crate::pixels::CELL;
    let rows = cells
        .chunks(columns)
        .enumerate()
        .map(|(i, row)| {
            Element::row(
                row.iter()
                    .enumerate()
                    .map(|(j, cell_color)| {
                        let middle = (j as f32 * cell + cell / 2.0, i as f32 * cell + cell / 2.0);
                        let cell_color = cell_color.filter(|_| inside(middle.0, middle.1));
                        match cell_color {
                            Some(color) => Element::panel(Element::spacer().background(color))
                                .padding(crate::pixels::INSET),
                            None => Element::spacer(),
                        }
                        .size(Some(crate::pixels::CELL), Some(crate::pixels::CELL))
                    })
                    .collect(),
            )
        })
        .collect();
    Element::column(rows).size(Some(width), Some(height))
}

/// What one request changed, under its turn, as Desktop draws it: one file is a single row, its name with Undo, its counts and a chevron;
/// more are "Edited N files" with them over the first three files and "N more". Every row
/// opens the request's diff, and Undo is a real one (Desktop's asks the agent instead,
/// engine.md §6).
fn changes_card(
    request: &str,
    files: &[FileEffect],
    rewound: bool,
    ui: &Ui,
    transcript: TranscriptView,
    column: f32,
) -> Element {
    let (added, removed) = files
        .iter()
        .fold((0, 0), |(a, r), f| (a + f.added, r + f.removed));
    let view = format!("changes:{request}:view");
    let undo = if rewound {
        muted("Undone").font_size(SMALL).centered_y()
    } else {
        let id = format!("changes:{request}:undo");
        let button = Element::clickable(
            id.clone(),
            Element::row(vec![
                hgap(6.0),
                muted("Undo").font_size(SMALL).centered_y(),
                hgap(6.0),
            ])
            .size(None, Some(24.0)),
        )
        .hover_background(look::tool_hover())
        .radius(6.0);
        limn::Tooltip::new(
            button,
            id,
            "Undo this request's file changes",
            look::palette(),
        )
        .render()
        .centered_y()
    };
    let counts = |added: u32, removed: u32| {
        vec![
            text(format!("+{added}"))
                .font_size(SMALL)
                .foreground(look::added())
                .centered_y(),
            text(format!("\u{2212}{removed}"))
                .font_size(SMALL)
                .foreground(look::removed())
                .centered_y(),
        ]
    };
    // Each row keeps to the card: the name and its folder give way, fading out at their end,
    // so the counts stay inside however narrow the column gets.
    let inner = column - 10.0;
    let row = |icon: &'static str, label: String, folder: Option<String>, end: Vec<Element>| {
        let mut items = vec![
            hgap(4.0),
            Element::icon(icons::of(icon))
                .foreground(look::muted())
                .size(Some(14.0), Some(14.0))
                .centered_y(),
            Element::faded_text(label, FADE)
                .font_family(FontFamily::Sans)
                .font_size(BODY)
                .foreground(look::palette().text)
                .max_width(inner / 2.0),
        ];
        items.push(match folder {
            Some(f) => Element::faded_text(f, FADE)
                .font_family(FontFamily::Sans)
                .font_size(SMALL)
                .foreground(look::muted())
                .flex(1.0),
            None => Element::spacer().flex(1.0),
        });
        items.extend(end);
        items.push(hgap(4.0));
        Element::row(items).gap(8.0).size(Some(inner), Some(36.0))
    };
    let chevron = || caret(false);
    let opens = |id: String, row: Element| {
        Element::clickable(id, row)
            .hover_background(look::row_hover())
            .radius(8.0)
    };
    let name = |f: &FileEffect| f.path.rsplit('/').next().unwrap_or(&f.path).to_owned();
    let mut rows = vec![];
    if let [file] = files {
        let mut end = vec![undo];
        end.extend(counts(file.added, file.removed));
        end.push(chevron());
        rows.push(opens(view.clone(), row(icons::CODE, name(file), None, end)));
    } else {
        let mut end = vec![undo];
        end.extend(counts(added, removed));
        end.push(chevron());
        let label = format!("Edited {} files", files.len());
        rows.push(opens(view.clone(), row(icons::DIFF, label, None, end)));
        let more = format!("changes:{request}:more");
        let all = is_open(transcript, ui, &more);
        let shown = if all { files.len() } else { files.len().min(3) };
        for file in &files[..shown] {
            let folder = file.path.rsplit_once('/').map(|(dir, _)| dir.to_owned());
            let end = counts(file.added, file.removed);
            // Each row its own id, so it lights alone under the pointer.
            let id = format!("{view}:{}", file.path);
            rows.push(opens(id, row(icons::CODE, name(file), folder, end)));
        }
        if files.len() > 3 {
            let label = if all {
                "Show less".to_owned()
            } else {
                format!("{} more", files.len() - 3)
            };
            let line = Element::row(vec![hgap(26.0), muted(label).font_size(SMALL).centered_y()])
                .size(None, Some(28.0));
            rows.push(if transcript == TranscriptView::Verbose {
                line
            } else {
                opens(more, line)
            });
        }
    }
    Element::column(rows)
        .padding(4.0)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .size(Some(column), None)
}

/// A diff with more files or changed lines than these starts with every file folded and says
/// so, as Desktop's does (the numbers are inferred).
const LARGE_DIFF_FILES: usize = 300;
const LARGE_DIFF_LINES: u32 = 20_000;

/// A changed file as the diff pane's list builds it. The list makes only the rows in view,
/// after the view is built, so each row owns what it shows.
struct DiffRow {
    path: String,
    added: u32,
    removed: u32,
    /// Where its first change is, which "Open in Files" opens.
    line: u32,
    /// Its patch while it is open; `None` folded.
    patch: Option<String>,
    /// Whether it has lines to show: a binary file has none, and its band does not open.
    body: bool,
    binary: bool,
    /// Its open chip, built with the view while its "Open in" menu is open.
    menu: Option<Element>,
}

/// The changes of one request, file by file, as unified diffs. A request can touch hundreds
/// of files: the list builds only those in view, and a large diff starts folded.
fn diff_pane(
    store: &Store,
    ui: &Ui,
    width: f32,
    height: f32,
    palette: ComponentPalette,
) -> Element {
    let files = diff_files(store, ui).map_or(&[][..], Vec::as_slice);
    // "master → working tree" for what is uncommitted, as Desktop's; a request's
    // own changes when the person opened them from its card.
    let title = if ui.diff.is_some() {
        text("Changes in this request").centered_y()
    } else {
        let branch = ui
            .selected
            .and_then(|id| store.session(id))
            .and_then(|s| s.branch.clone())
            .unwrap_or_else(|| "HEAD".into());
        Element::row(vec![
            text(branch).centered_y(),
            muted("→").centered_y(),
            text("working tree").centered_y(),
        ])
        .gap(8.0)
    };
    let header = pane_header(title, pane_tools(Pane::Diff, ui, palette), width);
    let changed: u32 = files.iter().map(|f| f.added + f.removed).sum();
    let large = files.len() > LARGE_DIFF_FILES || changed > LARGE_DIFF_LINES;
    let toggled: std::collections::HashSet<&str> = (ui.expanded.iter())
        .filter_map(|k| k.strip_prefix(DIFF_FOLD))
        .collect();
    let rows: Vec<DiffRow> = files
        .iter()
        .map(|file| {
            let body = file.patch.lines().any(|l| l.starts_with("@@ "));
            // A click flips a file from where it starts: open, or folded in a large diff.
            let folded = large != toggled.contains(file.path.as_str());
            let open = body && !folded;
            let line = first_changed_line(&file.patch);
            DiffRow {
                path: file.path.clone(),
                added: file.added,
                removed: file.removed,
                line,
                patch: open.then(|| file.patch.clone()),
                body,
                binary: !body
                    && (file.patch.contains("Binary files ")
                        || file.patch.contains("GIT binary patch")),
                menu: open_in_menu(&file.path, line, ui)
                    .map(|rows| open_file_chip(&file.path, line, Some(rows), palette)),
            }
        })
        .collect();
    let mut pane = vec![header];
    if files.is_empty() {
        pane.push(pad(muted("No uncommitted changes"), 8.0, 8.0));
    }
    if large {
        let note = "Files are collapsed for large diffs. Select a file to expand it.";
        pane.push(pad(muted(note).font_size(SMALL), 16.0, 8.0));
    }
    let rows = std::sync::Arc::new(rows);
    let keys = rows.clone();
    let list = Element::measured_list(
        "diff:scroll",
        rows.len(),
        32.0,
        move |at| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            keys[at].path.hash(&mut hasher);
            hasher.finish()
        },
        move |at| diff_row(&rows[at], at, width, palette),
    );
    pane.push(list.flex(1.0));
    Element::column(pane)
        .background(look::theme().background)
        .size(Some(width), Some(height))
}

/// One file of the diff pane: its band, and its lines under it while it is open.
fn diff_row(row: &DiffRow, at: usize, width: f32, palette: ComponentPalette) -> Element {
    let band = diff_file_band(row, width, palette);
    let Some(patch) = &row.patch else {
        return band;
    };
    let lines = parse_patch(patch);
    let height = (lines.len().max(1) as f32 * 19.0 + 16.0).min(600.0);
    let words = word_ranges(&lines);
    Element::column(vec![
        band,
        limn::CodeDiff::new(format!("diff:{at}"), lines)
            .word_ranges(words)
            .palette(palette)
            .font_size(12.0)
            .height(height)
            .render(),
    ])
    .size(Some(width), None)
}

/// A changed file's open, a split button drawn as Create PR's: the file at `line` in quill's
/// own Files pane, and its menu with every other place, "Open in" as a session's menu lists
/// them (quill-ide, the editors found, the default app, the file manager).
fn open_file(path: &str, line: u32, ui: &Ui, palette: ComponentPalette) -> Element {
    open_file_chip(path, line, open_in_menu(path, line, ui), palette)
}

/// The "Open in" rows of `path`'s menu, aimed at its file and line, while that menu is open.
fn open_in_menu(path: &str, line: u32, ui: &Ui) -> Option<Vec<limn::DropdownMenuEntry>> {
    (ui.diff_menu.as_deref() == Some(path))
        .then(|| open_in_rows(OPEN_FILE_IN, &format!("{line}:{path}"), ui))
}

/// [`open_file`] with its menu's rows: `Some` while the menu is open.
fn open_file_chip(
    path: &str,
    line: u32,
    rows: Option<Vec<limn::DropdownMenuEntry>>,
    palette: ComponentPalette,
) -> Element {
    let hover = ComponentPalette {
        hover: look::branch_chip_hover(),
        ..palette
    };
    let glyph = |icon: &'static str, size: f32| {
        Element::icon(icons::of(icon))
            .foreground(look::palette().text)
            .size(Some(size), Some(size))
            .centered()
    };
    let open = limn::Tooltip::new(
        hover
            .interactive_surface(Element::clickable(
                format!("{OPEN_HERE}{line}:{path}"),
                glyph(icons::ARROW_OUT, 14.0),
            ))
            .size(Some(28.0), Some(28.0)),
        format!("{OPEN_HERE}{line}:{path}"),
        "Open in Files",
        look::palette(),
    )
    .render();
    let id = format!("{OPEN_FILE_MENU}{path}");
    let shown = rows.is_some();
    let caret = hover
        .interactive_surface(Element::clickable(
            id.clone(),
            glyph(icons::CARET_DOWN, 12.0),
        ))
        .accessibility_label("Open in")
        .size(Some(24.0), Some(28.0));
    let caret = if shown {
        caret.background(look::branch_chip_hover())
    } else {
        caret
    };
    let caret = menu(
        caret,
        rows.unwrap_or_default(),
        shown,
        &id,
        220.0,
        limn::PopoverPlacement::BottomEnd,
    );
    bar_chip(Element::row(vec![
        open,
        Element::spacer()
            .background(look::branch_bar())
            .size(Some(1.0), Some(28.0)),
        caret,
    ]))
    .centered_y()
}

/// The first line a patch's new side has, from its first hunk's header, "@@ -a,b +c,d @@".
fn first_changed_line(patch: &str) -> u32 {
    patch
        .lines()
        .find_map(|line| {
            let new = line
                .strip_prefix("@@ ")?
                .split(' ')
                .nth(1)?
                .strip_prefix('+')?;
            new.split(',').next()?.parse().ok()
        })
        .unwrap_or(1)
        .max(1)
}

/// A file's band over its diff, as Desktop's: #F2F2F2, the name in #616161 after
/// a caret and the code mark, then its added and removed lines. A file with no lines to show
/// (a binary one) keeps its band, dimmed caret and counts, and does not open.
fn diff_file_band(row: &DiffRow, width: f32, palette: ComponentPalette) -> Element {
    let name = row.path.rsplit('/').next().unwrap_or(&row.path).to_owned();
    let dim = look::muted();
    let mut words = vec![
        if row.body {
            caret(row.patch.is_some())
        } else {
            caret(false).foreground(limn::Color {
                a: dim.a / 3,
                ..dim
            })
        },
        Element::icon(icons::of(icons::CODE))
            .foreground(look::muted())
            .size(Some(14.0), Some(14.0))
            .centered_y(),
        text(name).foreground(look::command_ink()).centered_y(),
    ];
    // A file with no lines to show says why: a binary one, or one too long to show (a new
    // file past its limit keeps its count).
    if row.binary {
        words.push(muted("Binary file").font_size(SMALL).centered_y());
    } else if !row.body && row.added + row.removed > 0 {
        words.push(muted("Too large to show").font_size(SMALL).centered_y());
    }
    let words = Element::row(words).gap(6.0).size(None, Some(32.0));
    // The caret and the name fold the file's lines away and back, as Desktop's band does.
    let fold = if row.body {
        Element::clickable(format!("{DIFF_FOLD}{}", row.path), words)
    } else {
        words
    };
    Element::row(vec![
        hgap(16.0),
        fold,
        Element::spacer().flex(1.0),
        text(format!("+{}", row.added))
            .foreground(look::added())
            .centered_y(),
        text(format!("−{}", row.removed))
            .foreground(look::removed())
            .centered_y(),
        (row.menu.clone()).unwrap_or_else(|| open_file_chip(&row.path, row.line, None, palette)),
        hgap(10.0),
    ])
    .gap(6.0)
    .background(look::cancel())
    .size(Some(width), Some(32.0))
}

/// Lines of a unified diff, numbered from its hunk headers.
pub fn parse_patch(patch: &str) -> Vec<limn::CodeDiffLine> {
    use limn::{CodeDiffLine, CodeDiffLineKind};
    let (mut old, mut new) = (0usize, 0usize);
    let mut lines = vec![];
    let mut in_hunk = false;
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            // "@@ -a,b +c,d @@": the first line numbers of the hunk.
            let mut parts = header.split_whitespace();
            let start = |p: Option<&str>| {
                p.and_then(|p| p[1..].split(',').next())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1)
            };
            old = start(parts.next());
            new = start(parts.next());
            in_hunk = true;
            continue;
        }
        if !in_hunk || line.starts_with("\\ ") {
            continue;
        }
        let (kind, text) = match line.chars().next() {
            Some('+') => (CodeDiffLineKind::Added, &line[1..]),
            Some('-') => (CodeDiffLineKind::Removed, &line[1..]),
            Some(' ') => (CodeDiffLineKind::Context, &line[1..]),
            _ => (CodeDiffLineKind::Context, line),
        };
        let (old_line, new_line) = match kind {
            CodeDiffLineKind::Added => (None, Some(new)),
            CodeDiffLineKind::Removed => (Some(old), None),
            // The parser makes no folded rows; they are the panel's own (plan Q5.5).
            CodeDiffLineKind::Context | CodeDiffLineKind::Folded => (Some(old), Some(new)),
        };
        if old_line.is_some() {
            old += 1;
        }
        if new_line.is_some() {
            new += 1;
        }
        lines.push(CodeDiffLine {
            kind,
            old_line,
            new_line,
            text: text.to_owned(),
        });
    }
    lines
}

/// A question Claude asks, one at a time: a 480 px card with the
/// question on a gray band and "‹ 1 of 2 ›" when there are more; each option a row with its
/// mark (a radio, or a box when several may be chosen), label and muted description, lines
/// between; "Other" with a field for the person's own words; Skip, and Next or
/// Submit with Ctrl ⏎. A single choice starts on its first option, as Desktop's did.
/// A question may say `other: false` (no "Other"), `secret: true` (the words are masked) or
/// have no options (only a field for words); one that is not `blocking` counts down to going
/// unanswered once it waited untouched a while.
fn question_card(
    id: &str,
    input: &Value,
    ui: &Ui,
    column: f32,
    palette: ComponentPalette,
) -> Element {
    let questions = input["questions"].as_array().cloned().unwrap_or_default();
    let width = column.min(480.0);
    let count = questions.len().max(1);
    let page = ui.question.get(id).copied().unwrap_or(0).min(count - 1);
    let question = &questions.get(page).cloned().unwrap_or_default();
    let multi = question["multiSelect"] == true;
    let chosen = picked(ui, id, page, question);
    let secret = question["secret"] == true;
    let typed = ui
        .other
        .get(&(id.to_owned(), page))
        .map_or("", String::as_str);
    let field = |placeholder: &str| {
        limn::TextField::new(
            format!("ask-other:{id}:{page}"),
            typed,
            placeholder,
            palette,
        )
        .password(secret)
        .width(width - 74.0)
        .render()
    };
    // The question takes the band but the pager's room, and as many lines as it needs.
    let pager = if count > 1 { 140.0 } else { 26.0 };
    let mut band = vec![
        text(question["question"].as_str().unwrap_or_default().to_owned())
            .font_size(BODY)
            .size(Some(width - pager), None)
            .centered_y(),
        Element::spacer().flex(1.0),
    ];
    if count > 1 {
        let arrow = |dir: &str, icon: &'static str, label: &str, on: bool| {
            let glyph = Element::icon(icons::of(icon))
                .foreground(if on {
                    look::palette().text
                } else {
                    look::muted()
                })
                .size(Some(16.0), Some(16.0))
                .centered();
            let button = Element::panel(glyph)
                .size(Some(24.0), Some(24.0))
                .centered_y();
            if on {
                Element::clickable(format!("ask:{id}:{dir}"), button).accessibility_label(label)
            } else {
                button
            }
        };
        band.push(arrow("prev", icons::CARET_LEFT, "Back", page > 0));
        band.push(
            muted(format!("{} of {count}", page + 1))
                .font_size(12.0)
                .centered_y(),
        );
        band.push(arrow("next", icons::CARET_RIGHT, "Next", page + 1 < count));
    }
    let mut rows = vec![
        Element::panel(Element::row(band).gap(6.0).min_size(0.0, 24.0))
            .background(look::user_bubble())
            // Inside the card's rounded edge.
            .corner_radii(limn::CornerRadii {
                top_left: 11.0,
                top_right: 11.0,
                ..Default::default()
            })
            .padding(10.0)
            .size(Some(width), None),
    ];
    let mut options: Vec<(String, String)> = question["options"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| {
            (
                o["label"].as_str().unwrap_or_default().to_owned(),
                o["description"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let words_only = options.is_empty();
    if question["other"] != false && !words_only {
        options.push(("Other".into(), String::new()));
    }
    if words_only {
        rows.push(
            Element::column(vec![field("Type your answer")])
                .padding(16.0)
                .size(Some(width), None),
        );
    }
    for (o, (label, description)) in options.iter().enumerate() {
        if o > 0 {
            rows.push(hline(width, look::card_border()));
        }
        let action = format!("ask:{id}:{page}:{o}");
        let on = chosen.contains(label);
        let mark = if multi {
            limn::Checkbox::new(&action, on, palette)
                .label(label.clone())
                // Desktop's chosen marks are its blue.
                .accent(look::handle())
                .render()
        } else {
            let option =
                limn::RadioOption::new(&action, label.clone()).description(description.clone());
            limn::RadioGroup::new(
                format!("{action}:group"),
                vec![option],
                on.then_some(action.as_str()),
                palette,
            )
            .accent(look::handle())
            .render()
        };
        let mut row = vec![mark];
        if multi && !description.is_empty() {
            row.push(pad(muted(description.clone()).font_size(BODY), 26.0, 0.0));
        }
        if label == "Other" && on {
            row.push(pad(field("Type your own answer here"), 26.0, 0.0));
        }
        rows.push(
            Element::column(row)
                .gap(6.0)
                .padding(16.0)
                .size(Some(width), None),
        );
    }
    let last = page + 1 == count;
    let (go, label) = if last {
        ("submit", "Submit")
    } else {
        ("next", "Next")
    };
    // The agent goes on without an answer once this ends.
    let left = ui.question_countdown(id).map(|left| {
        let seconds = left.as_secs_f32().ceil() as u64;
        muted(format!("Skipped in {seconds}s"))
            .font_size(SMALL)
            .centered_y()
    });
    let answered = !chosen.is_empty() || (words_only && !typed.trim().is_empty());
    let mut footer: Vec<Element> = left.into_iter().collect();
    footer.extend([
        Element::spacer().flex(1.0),
        keyed_button(&format!("ask:{id}:skip"), "Skip", "", false, true, palette),
        keyed_button(
            &format!("ask:{id}:{go}"),
            label,
            "Ctrl+⏎",
            true,
            answered,
            palette,
        ),
    ]);
    rows.push(
        Element::row(footer)
            .gap(8.0)
            .padding(16.0)
            .size(Some(width), None),
    );
    Element::panel(Element::column(rows))
        .background(palette.surface)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .size(Some(width), None)
}

/// The labels the person chose for question `page` of `permission`; a single choice not yet
/// touched is its first option.
pub fn picked(ui: &Ui, permission: &str, page: usize, question: &Value) -> Vec<String> {
    match ui.answers.get(&(permission.to_owned(), page)) {
        Some(labels) => labels.clone(),
        None if question["multiSelect"] != true => question["options"][0]["label"]
            .as_str()
            .map(|l| vec![l.to_owned()])
            .unwrap_or_default(),
        None => vec![],
    }
}

/// One card of the right panel, as Desktop's: a 280 px white card with a #EBEBEB
/// edge, radius 12 and 18 px in, a 13 px title with its caret, and what it holds.
fn panel_card(
    title: &str,
    id: &str,
    open: bool,
    trailing: Vec<Element>,
    rows: Vec<Element>,
    width: f32,
) -> Element {
    let mut head = vec![text(title).font_size(SMALL).centered_y(), caret(open)];
    head.push(Element::spacer().flex(1.0));
    head.extend(trailing);
    let head = Element::clickable(id, Element::row(head).gap(6.0).size(None, Some(20.0)));
    // The rows open and close on limn's spring, the gap above them inside the slot.
    let body = open.then(|| {
        Element::column(
            std::iter::once(Element::spacer().size(None, Some(0.0)))
                .chain(rows)
                .collect(),
        )
        .gap(14.0)
    });
    let content = vec![
        head,
        limn::Collapse::new(format!("{id}:body"), body)
            .expand()
            .render(),
    ];
    Element::panel(Element::column(content))
        .background(look::palette().surface)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .padding(18.0)
        .size(Some(width), None)
}

/// A row of a panel card: a 16 px icon and its words.
fn panel_row(icon: limn::Icon, words: Element) -> Element {
    Element::row(vec![
        Element::icon(icon)
            .foreground(look::palette().text)
            .size(Some(16.0), Some(16.0))
            .centered_y(),
        words.centered_y(),
    ])
    .gap(10.0)
    .size(None, Some(20.0))
}

/// Where the session works, how full its context is and what it changed.
fn right_panel(
    store: &Store,
    session: &Session,
    ui: &Ui,
    width: f32,
    height: f32,
    _palette: ComponentPalette,
) -> Element {
    // The cards leave the scrollbar a gutter of its own at the right, off their edges.
    let inner = width - 26.0;
    let open = |key: &str, default: bool| ui.expanded.iter().any(|k| k == key) != default;
    let mut environment = vec![
        panel_row(icons::of(icons::LAPTOP), text("This computer")),
        panel_row(icons::of(icons::FOLDER), text(folder_name(&session.cwd))),
    ];
    if let Some(branch) = &session.branch {
        environment.push(panel_row(
            icons::of(icons::GIT_BRANCH),
            text(branch.clone()),
        ));
    }
    environment.extend(crate::remote::row(session, ui, inner - 36.0));
    // How full the context is, as Desktop's percentage and bar while the card is folded: the
    // window comes from the agent's own report of its model.
    let share = context_share(session);
    let mut context_head = vec![];
    if let Some(share) = share.filter(|_| !open(CONTEXT_CARD, true)) {
        context_head.push(
            muted(format!("{}%", (share * 100.0).round() as u32))
                .font_size(SMALL)
                .centered_y(),
        );
        context_head.push(context_bar(share));
    }
    let mut context_rows = context_breakdown(session, ui, inner - 36.0);
    if session.cost > 0.0 {
        context_rows.push(muted(format!("${:.4} this session", session.cost)).font_size(SMALL));
    }
    let (added, removed, files) = match &session.working {
        Some(working) => (
            working.iter().map(|f| f.added).sum::<u32>(),
            working.iter().map(|f| f.removed).sum::<u32>(),
            working.len(),
        ),
        None => session
            .items
            .iter()
            .fold((0, 0, 0), |(a, r, n), item| match item {
                Item::Changes {
                    files,
                    rewound: false,
                    ..
                } => (
                    a + files.iter().map(|f| f.added).sum::<u32>(),
                    r + files.iter().map(|f| f.removed).sum::<u32>(),
                    n + files.len(),
                ),
                _ => (a, r, n),
            }),
    };
    let changes = if files == 0 {
        muted("Nothing changed yet")
    } else {
        let mut row = vec![Element::icon(icons::of(icons::DIFF))
            .foreground(look::palette().text)
            .size(Some(16.0), Some(16.0))
            .centered_y()];
        row.extend(counting(
            &format!("panel:{}", session.id),
            added,
            removed,
            SMALL,
        ));
        // The row opens the diff, as Desktop's does.
        Element::clickable(
            CHANGES_ROW,
            Element::row(row).gap(8.0).size(None, Some(24.0)),
        )
        .accessibility_label(counted(added, removed))
    };
    let mut cards = vec![
        panel_card(
            "Environment",
            "panel:environment",
            open("panel:environment", true),
            vec![],
            environment,
            inner,
        ),
        panel_card(
            "Context",
            CONTEXT_CARD,
            open(CONTEXT_CARD, true),
            context_head,
            context_rows,
            inner,
        ),
    ];
    // What the agent left running beside its turns, as Desktop's rail lists it.
    if let Some(rows) = crate::terminals::rows(session, inner - 36.0) {
        cards.push(panel_card(
            "Background tasks",
            "panel:terminals",
            open("panel:terminals", true),
            vec![],
            // Packed as the sidebar's rows are, 2 px apart.
            vec![Element::column(rows).gap(2.0)],
            inner,
        ));
    }
    if let Some(rows) = usage_rows(store, ui, inner - 36.0) {
        cards.push(panel_card(
            "Usage",
            "panel:usage",
            open("panel:usage", true),
            vec![],
            rows,
            inner,
        ));
    }
    cards.extend([panel_card(
        "Changes",
        "panel:changes",
        open("panel:changes", true),
        vec![],
        vec![changes],
        inner,
    )]);
    let outputs = outputs(session);
    if !outputs.is_empty() {
        cards.push(panel_card(
            "Outputs",
            "panel:outputs",
            open("panel:outputs", true),
            vec![],
            outputs
                .into_iter()
                .map(|path| {
                    let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                    Element::clickable(
                        format!("doc:open:{path}"),
                        panel_row(
                            icons::of(icons::CODE),
                            Element::truncated_text(name)
                                .font_family(FontFamily::Sans)
                                .font_size(BODY)
                                .foreground(look::palette().text)
                                .max_width(inner - 72.0),
                        ),
                    )
                    .accessibility_label(format!("Open {path}"))
                })
                .collect(),
            inner,
        ));
    }
    // Taller than the window, as an open breakdown gets, the cards scroll.
    Element::scroll("panel:scroll", Element::column(cards).gap(8.0).padding(8.0))
        .scrollbar_appearance(limn::ScrollbarAppearance::overlay())
        .size(Some(width), Some(height))
}

/// The Usage card's rows, as Desktop's rail draws them (screenshot the person shared,
/// threads.com/@testingcatalog/post/DdutukbgswG): each plan window's name and share, when
/// it resets, and its bar. `None` until the agent reported its limits.
fn usage_rows(store: &Store, ui: &Ui, width: f32) -> Option<Vec<Element>> {
    let limits = agent_limits(store, ui)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let rows: Vec<Element> = [("5-hour limit", limits.session), ("Weekly", limits.weekly)]
        .into_iter()
        .filter_map(|(label, window)| {
            let w = window?;
            Some(
                Element::column(vec![
                    Element::row(vec![
                        Element::row(vec![
                            text(label).font_size(SMALL).centered_y(),
                            muted(format!("{}%", (spent(&w, now) * 100.0).round() as u32))
                                .font_size(SMALL)
                                .centered_y(),
                        ])
                        .gap(5.0),
                        Element::spacer().flex(1.0),
                        // What does not fit beside the name and share is cut.
                        Element::truncated_text(resets_in(w.resets_at, now, ui.utc_offset))
                            .font_family(FontFamily::Sans)
                            .font_size(SMALL)
                            .foreground(look::muted())
                            .max_width((width - 130.0).max(60.0))
                            .centered_y(),
                    ])
                    .gap(8.0)
                    .size(Some(width), Some(20.0)),
                    usage_bar(spent(&w, now), width),
                ])
                .gap(6.0),
            )
        })
        .collect();
    (!rows.is_empty()).then_some(rows)
}

/// The files the session made or changed and still has, newest first: the panel's Outputs, as
/// Desktop lists them.
fn outputs(session: &Session) -> Vec<String> {
    let mut paths: Vec<String> = vec![];
    for item in session.items.iter().rev() {
        if let Item::Changes {
            files,
            rewound: false,
            ..
        } = item
        {
            for file in files {
                if !paths.contains(&file.path) {
                    paths.push(file.path.clone());
                }
            }
        }
    }
    // A file it deleted last is not an output; one deleted then made again is.
    let gone = |path: &String| {
        session.items.iter().rev().find_map(|item| match item {
            Item::Changes {
                files,
                rewound: false,
                ..
            } => files
                .iter()
                .find(|f| f.path == *path)
                .map(|f| f.kind == ChangeKind::Deleted),
            _ => None,
        }) == Some(true)
    };
    paths.retain(|p| !gone(p));
    paths
}

/// The right panel's Context card, open until folded, as Desktop's rail shows it.
pub const CONTEXT_CARD: &str = "panel:context";

/// A count of tokens as Desktop writes it: "254", "15.8K", "1M".
pub(crate) fn compact(n: u64) -> String {
    // Halves round up, as JavaScript's `toFixed` does here, not to even.
    let short = |value: f64, unit: &str| {
        let value = format!("{:.1}", (value * 10.0).round() / 10.0);
        format!("{}{unit}", value.strip_suffix(".0").unwrap_or(&value))
    };
    let thousands = (n as f64 / 100.0).round() / 10.0;
    if thousands >= 1000.0 {
        short(n as f64 / 1e6, "M")
    } else if n >= 1000 {
        short(thousands, "K")
    } else {
        n.to_string()
    }
}

/// How full the context is, used of the window: the agent's own breakdown when it gave one,
/// else what its turns reported. The Context card and the composer's ring both show this.
fn context_fill(session: &Session) -> (u64, u64) {
    match session.context_usage.as_ref().filter(|u| u.max > 0) {
        Some(u) => (u.used, u.max),
        None => (session.context_tokens, session.context_window),
    }
}

/// [`context_fill`] as a share, `None` while the window is not known.
fn context_share(session: &Session) -> Option<f32> {
    let (used, max) = context_fill(session);
    (max > 0).then(|| used as f32 / max as f32)
}

/// The Context card open, as Desktop's rail draws it: "N% used" and
/// "used / max", a meter of what fills the context, a row per category with its swatch,
/// tokens and share (the deferred last, with "—"), and the lists under some categories,
/// each folded. Until the agent told what fills it, the meter is one share of the whole.
fn context_breakdown(session: &Session, ui: &Ui, width: f32) -> Vec<Element> {
    use quill_model::ContextKind::*;
    let usage = session.context_usage.as_ref().filter(|u| u.max > 0);
    let (used, max) = context_fill(session);
    if used == 0 || max == 0 {
        return vec![muted("Send a message to see context usage.").font_size(SMALL)];
    }
    let percent = (used as f32 / max as f32 * 100.0).round().min(100.0) as u32;
    let headline = Element::row(vec![
        text(format!("{percent}% used")).centered_y(),
        Element::spacer().flex(1.0),
        muted(format!("{} / {}", compact(used), compact(max)))
            .font_size(SMALL)
            .centered_y(),
    ])
    .size(None, Some(20.0));
    let Some(usage) = usage else {
        return vec![headline, meter(&[(used, look::handle())], max, width)];
    };
    let of = |kind| {
        let mut list: Vec<_> = usage
            .categories
            .iter()
            .filter(|c| c.kind == kind && c.tokens > 0)
            .collect();
        list.sort_by_key(|c| std::cmp::Reverse(c.tokens));
        list
    };
    let (taken, buffer, free, deferred) = (of(Used), of(Buffer), of(Free), of(Deferred));
    // At most six in the meter: past that, the first five and "Other".
    let colored = if taken.len() + buffer.len() > 6 {
        5 - buffer.len().min(1)
    } else {
        taken.len()
    };
    let color = |at: usize| {
        if at < colored {
            look::chart_series(at)
        } else {
            look::chart_reference()
        }
    };
    let mut segments: Vec<(u64, limn::Color)> = taken[..colored]
        .iter()
        .enumerate()
        .map(|(at, c)| (c.tokens, color(at)))
        .collect();
    let other: u64 = taken[colored..].iter().map(|c| c.tokens).sum();
    if other > 0 {
        segments.push((other, look::chart_reference()));
    }
    segments.extend(buffer.iter().map(|c| (c.tokens, look::chart_muted())));
    let total = max.max(
        taken
            .iter()
            .chain(&buffer)
            .chain(&free)
            .map(|c| c.tokens)
            .sum(),
    );
    let mut rows = vec![headline, meter(&segments, total, width)];
    let row = |swatch: limn::Color, name: &str, tokens: u64, share: Option<f32>| {
        let ink = if share.is_some() {
            look::palette().text
        } else {
            look::muted()
        };
        Element::row(vec![
            Element::spacer()
                .background(swatch)
                .radius(2.0)
                .size(Some(10.0), Some(10.0))
                .centered_y(),
            Element::truncated_text(name.to_owned())
                .font_family(FontFamily::Sans)
                .font_size(SMALL)
                .foreground(ink)
                .max_width(width - NAME_ROOM)
                .centered_y(),
            Element::spacer().flex(1.0),
            to_right(muted(compact(tokens)).font_size(SMALL), 44.0),
            to_right(
                text(share.map_or("—".into(), |s| format!("{:.1}%", s * 100.0)))
                    .font_size(SMALL)
                    .foreground(ink),
                44.0,
            ),
        ])
        .gap(6.0)
        .size(None, Some(20.0))
    };
    let share = |tokens: u64| Some(tokens as f32 / max as f32);
    let mut legend: Vec<Element> = taken
        .iter()
        .enumerate()
        .map(|(at, c)| row(color(at), &c.name, c.tokens, share(c.tokens)))
        .collect();
    legend.extend(
        buffer
            .iter()
            .map(|c| row(look::chart_muted(), &c.name, c.tokens, share(c.tokens))),
    );
    legend.extend(
        free.iter()
            .map(|c| row(look::chart_tint(), &c.name, c.tokens, share(c.tokens))),
    );
    legend.extend(
        deferred
            .iter()
            .map(|c| row(look::chart_muted(), &c.name, c.tokens, None)),
    );
    rows.push(Element::column(legend).gap(4.0));
    for (name, entries) in &usage.groups {
        let id = format!("{CONTEXT_CARD}:{name}");
        let open = ui.expanded.contains(&id);
        let tokens: u64 = entries.iter().map(|(_, t)| *t).sum();
        let toggle = Element::clickable(
            id.clone(),
            Element::row(vec![
                caret(open).size(Some(10.0), Some(12.0)),
                text(name.clone()).font_size(SMALL).centered_y(),
                Element::spacer().flex(1.0),
                to_right(muted(compact(tokens)).font_size(SMALL), 44.0),
                to_right(muted(entries.len().to_string()).font_size(SMALL), 44.0),
            ])
            .gap(6.0)
            .size(None, Some(20.0)),
        );
        let list = open.then(|| {
            let list = Element::column(
                entries
                    .iter()
                    .map(|(entry, tokens)| {
                        Element::row(vec![
                            Element::spacer().size(Some(10.0), None),
                            Element::truncated_text(tilde(entry))
                                .font_family(FontFamily::Sans)
                                .font_size(SMALL)
                                .foreground(look::muted())
                                .max_width(width - NAME_ROOM)
                                .centered_y(),
                            Element::spacer().flex(1.0),
                            to_right(muted(compact(*tokens)).font_size(SMALL), 44.0),
                            Element::spacer().size(Some(44.0), None),
                        ])
                        .gap(6.0)
                        .size(None, Some(20.0))
                    })
                    .collect(),
            );
            // A long list scrolls in its own box, as Desktop's past twelve rows (`max-h-60`).
            if entries.len() > 12 {
                Element::scroll(format!("{id}:scroll"), list).size(None, Some(240.0))
            } else {
                list
            }
        });
        rows.push(Element::column(vec![
            toggle,
            limn::Collapse::new(format!("{id}:body"), list)
                .expand()
                .render(),
        ]));
    }
    rows
}

/// A path under the person's home as "~/…", so a long one keeps what tells it apart.
fn tilde(path: &str) -> String {
    match quill_files::home_dir() {
        Some(home) => match path.strip_prefix(&*home.to_string_lossy()) {
            Some(rest) if rest.starts_with(['/', '\\']) => format!("~{rest}"),
            _ => path.to_owned(),
        },
        None => path.to_owned(),
    }
}

/// What a breakdown row keeps beside its name: the swatch, tokens, share and their gaps.
const NAME_ROOM: f32 = 10.0 + 44.0 + 44.0 + 4.0 * 6.0;

/// `element` at the right end of a box `width` wide, for numbers in a column.
fn to_right(element: Element, width: f32) -> Element {
    Element::row(vec![Element::spacer().flex(1.0), element.centered_y()])
        .size(Some(width), Some(20.0))
}

/// Desktop's context meter, its `StackedMeter` at size "sm": 4 px on a tinted ground, each
/// share in its color with a 1 px seam, cut to `width`.
fn meter(segments: &[(u64, limn::Color)], total: u64, width: f32) -> Element {
    let total = total.max(1) as f32;
    let parts = segments
        .iter()
        .map(|(tokens, color)| ((*tokens as f32 / total * width).round(), *color))
        .filter(|(w, _)| *w >= 1.0)
        .map(|(w, color)| Element::spacer().background(color).size(Some(w), Some(4.0)))
        .collect();
    Element::row(parts)
        .gap(1.0)
        .background(look::chart_tint())
        .radius(2.0)
        .size(Some(width), Some(4.0))
        .clip_children()
}

/// Desktop's small context bar: a 24 × 6 pill filled blue to the share used.
fn context_bar(share: f32) -> Element {
    let fill = (24.0 * share.clamp(0.0, 1.0)).max(3.0);
    Element::row(vec![Element::spacer()
        .background(look::handle())
        .radius(3.0)
        .size(Some(fill), Some(6.0))])
    .background(look::active())
    .radius(3.0)
    .size(Some(24.0), Some(6.0))
    .centered_y()
}

/// What changed inside each rewritten line, for the word highlights of the diff:
/// each run of removed lines is paired with the added run after it, line by line, and the
/// characters between their common start and common end are marked.
pub fn word_ranges(lines: &[limn::CodeDiffLine]) -> Vec<Vec<std::ops::Range<usize>>> {
    use limn::CodeDiffLineKind::{Added, Removed};
    let mut ranges = vec![Vec::new(); lines.len()];
    let mut i = 0;
    while i < lines.len() {
        let removed: Vec<usize> = (i..lines.len())
            .take_while(|&j| lines[j].kind == Removed)
            .collect();
        let after = i + removed.len();
        let added: Vec<usize> = (after..lines.len())
            .take_while(|&j| lines[j].kind == Added)
            .collect();
        for (&r, &a) in removed.iter().zip(&added) {
            let old: Vec<char> = lines[r].text.chars().collect();
            let new: Vec<char> = lines[a].text.chars().collect();
            let start = old.iter().zip(&new).take_while(|(x, y)| x == y).count();
            let end = old[start..]
                .iter()
                .rev()
                .zip(new[start..].iter().rev())
                .take_while(|(x, y)| x == y)
                .count();
            if start + end < old.len() {
                ranges[r].push(start..old.len() - end);
            }
            if start + end < new.len() {
                ranges[a].push(start..new.len() - end);
            }
        }
        i = (after + added.len()).max(i + 1);
    }
    ranges
}

/// A helper agent's own pane: "‹" and what it was asked to do,
/// then its prompt as the person's bubble, the model it ran on, and its steps and words.
fn agent_pane(
    store: &Store,
    session: Option<&Session>,
    ui: &Ui,
    width: f32,
    height: f32,
    palette: ComponentPalette,
) -> Element {
    let found = session.zip(ui.helper.as_deref()).and_then(|(s, call)| {
        s.items.iter().find_map(|i| match i {
            Item::Tool { call: c, input, .. } if c == call => Some((s, c.as_str(), input)),
            _ => None,
        })
    });
    let title = found.map_or("", |(_, _, input)| {
        input["description"].as_str().unwrap_or_default()
    });
    let back = icon_tool(
        "pane:agent:close",
        icons::of(icons::CARET_LEFT),
        "Back",
        28.0,
        palette,
    );
    // The title is cut before the pane's tools.
    let lead = Element::row(vec![
        back,
        Element::truncated_text(title.to_owned())
            .font_family(FontFamily::Sans)
            .font_size(BODY)
            .foreground(look::palette().text)
            .max_width(width - 160.0)
            .centered_y(),
    ])
    .gap(4.0);
    let header = pane_header(lead, pane_tools(Pane::Agent, ui, palette), width);
    let Some((session, call, input)) = found else {
        return Element::column(vec![header]).size(Some(width), Some(height));
    };
    let inner = width - 48.0;
    // Codex's helper has no prompt to show: the CLI does not say it.
    let prompt = input["prompt"].as_str().unwrap_or_default();
    let mut body = vec![];
    if !prompt.is_empty() {
        body.push(
            Element::panel(
                text(prompt.to_owned())
                    .font_size(BODY)
                    .max_width(inner - 32.0),
            )
            .background(look::user_bubble())
            .radius(14.0)
            .padding(16.0),
        );
    }
    // "Model Haiku 5.5": the model it was given, else the session's.
    let models = offered_models(store, Some(session), ui);
    let model = input["model"]
        .as_str()
        .and_then(|id| {
            models
                .iter()
                .find(|m| m.id == id || m.name.to_lowercase().contains(id))
        })
        .or_else(|| current_model(store, Some(session), ui));
    body.push(Element::spacer().size(None, Some(16.0)));
    if let Some(model) = model {
        let effort = input["effort"].as_str().map(|e| format!(" \u{b7} {e}"));
        body.push(
            muted(format!(
                "Model {}{}",
                model.name,
                effort.unwrap_or_default()
            ))
            .font_size(SMALL),
        );
    }
    let running = session.task_running(call);
    let helper = session.helpers.get(call).map_or(&[][..], Vec::as_slice);
    body.extend(entries(store, session, helper, running, ui, inner, palette).0);
    if running {
        // As the session's own running line: its words, shimmering.
        body.push(working_words("agent:working", "Working".into()));
    }
    let content = Element::column(body).gap(12.0).padding(24.0);
    // Its bar over the padding: a rail taken from the width would put the rows, laid out
    // `inner` wide, in less room than they were measured in, and their lines over the next.
    let scroll = Element::scroll(format!("agent:{call}:scroll"), content)
        .scrollbar_appearance(limn::ScrollbarAppearance::overlay());
    Element::column(vec![header, scroll.flex(1.0)]).size(Some(width), Some(height))
}

/// The plan Claude proposed, in its own pane: "Plan" over the
/// plan's markdown.
fn plan_pane(session: Option<&Session>, ui: &Ui, width: f32, height: f32) -> Element {
    let palette = look::palette();
    let header = pane_header(
        text("Plan").centered_y(),
        pane_tools(Pane::Plan, ui, palette),
        width,
    );
    let plan = session
        .zip(ui.plan.as_deref())
        .and_then(|(s, id)| plan_text(&s.items, id))
        .unwrap_or_default();
    let inner = width - 48.0;
    let content = Markdown::new(&plan, palette)
        .width(inner)
        .font_size(BODY)
        .render()
        .element
        .padding(24.0);
    // Its bar over the padding, as the agent pane's.
    let scroll = Element::scroll("plan:scroll", content)
        .scrollbar_appearance(limn::ScrollbarAppearance::overlay());
    Element::column(vec![header, scroll.flex(1.0)]).size(Some(width), Some(height))
}

/// The plan a permission or a tool call `id` carries.
pub fn plan_text(items: &[Item], id: &str) -> Option<String> {
    items.iter().find_map(|i| match i {
        Item::Permission { id: p, input, .. } | Item::Tool { call: p, input, .. } if p == id => {
            input["plan"].as_str().map(str::to_owned)
        }
        _ => None,
    })
}

/// What a session is doing now, as a card row under its title says it: the step it runs
/// ("Reading CartLineItem.tsx"), or nothing once its turn is over.
fn activity(session: &Session) -> Option<String> {
    match session.status {
        SessionStatus::Running | SessionStatus::Starting => {}
        // Inferred: the videos show no session waiting from a card.
        SessionStatus::Awaiting => return Some("Needs your input".into()),
        _ => return None,
    }
    let running = session.items.iter().rev().find_map(|item| match item {
        Item::Tool {
            call,
            name,
            input,
            result: None,
        } if !is_task_tool(name) => Some(Step {
            call,
            name,
            input,
            result: None,
            running: true,
        }),
        _ => None,
    });
    Some(match running {
        Some(step) => {
            let (verb, object) = step_label(&step, false);
            format!("{verb} {object}").trim_end().to_owned()
        }
        None if session.thinking => "Thinking".into(),
        None => "Working".into(),
    })
}

/// The card of the tasks a turn proposed for sessions of their own:
/// "New session" over a single one, "3 suggested tasks" and "Start all" over several. A
/// waiting task shows its title, its brief in two lines, a close, "Do it here" and "Start";
/// a started one is a row that follows its session, with what it does now, and opens it.
/// Desktop's "Options" (where and how it runs) is not here: quill starts every session on
/// this computer, as the one that proposed it.
fn proposals_card(
    store: &Store,
    tasks: &[(quill_model::Proposal, Option<quill_model::ProposalOutcome>)],
    palette: ComponentPalette,
) -> Element {
    use quill_model::ProposalOutcome;
    let width = 440.0;
    let shown: Vec<_> = tasks
        .iter()
        .filter(|(_, outcome)| !matches!(outcome, Some(ProposalOutcome::Dismissed)))
        .collect();
    if shown.is_empty() {
        return Element::spacer().size(None, Some(0.0));
    }
    let waiting: Vec<&str> = shown
        .iter()
        .filter(|(_, outcome)| outcome.is_none())
        .map(|(p, _)| p.id.as_str())
        .collect();
    let started = shown
        .iter()
        .filter(|(_, o)| matches!(o, Some(ProposalOutcome::Started { .. })))
        .count();
    let mut head = vec![if shown.len() == 1 && started == 0 {
        text("New session")
    } else {
        text(format!("{} suggested tasks", shown.len()))
    }
    .centered_y()];
    if started > 0 {
        head.push(muted(format!("{started} started")).centered_y());
    }
    head.push(Element::spacer().flex(1.0));
    if waiting.len() > 1 {
        head.push(
            Element::clickable(
                format!("proposal-all:{}", waiting.join(",")),
                text("Start all").centered_y(),
            )
            .centered_y(),
        );
    }
    let mut rows = vec![Element::row(head)
        .gap(12.0)
        .padding(14.0)
        .size(Some(width), Some(42.0))
        .background(look::command_box())];
    for (at, (proposal, outcome)) in shown.iter().enumerate() {
        if at > 0 {
            rows.push(hline(width, look::divider()));
        }
        rows.push(match outcome {
            None => {
                let action = |what: &str| format!("proposal:{what}:{}", proposal.id);
                Element::column(vec![
                    Element::row(vec![
                        text(proposal.title.clone()).flex(1.0),
                        tool_surface(
                            &action("dismiss"),
                            icons::of(icons::CLOSE),
                            "Dismiss",
                            22.0,
                            None,
                            look::tool_hover(),
                            None,
                        ),
                    ])
                    .gap(8.0),
                    // The brief in two lines at most, as Desktop's card cuts it.
                    Element::formatted_text(vec![limn::TextSpan::new(
                        proposal.prompt.clone(),
                        look::muted(),
                        BODY,
                        FontFamily::Sans,
                    )])
                    .max_width(width - 28.0)
                    .max_lines(2),
                    Element::row(vec![
                        Element::spacer().flex(1.0),
                        Element::clickable(action("here"), text("Do it here").centered_y())
                            .centered_y(),
                        keyed_button(&action("start"), "Start", "", true, true, palette),
                    ])
                    .gap(16.0),
                ])
                .gap(6.0)
                .padding(14.0)
                .size(Some(width), None)
            }
            Some(ProposalOutcome::Started { session }) => {
                let child = store.session(*session);
                let doing = child.and_then(activity);
                let mut lines = vec![text(proposal.title.clone())];
                if let Some(doing) = &doing {
                    lines.push(muted(doing.clone()).font_size(SMALL));
                }
                let mut row = vec![Element::column(lines).gap(2.0).flex(1.0).centered_y()];
                if doing.is_some() {
                    row.push(
                        Element::spacer()
                            .background(look::muted())
                            .radius(2.5)
                            .size(Some(5.0), Some(5.0))
                            .centered_y(),
                    );
                }
                row.push(
                    Element::icon(icons::of(icons::CARET_RIGHT))
                        .foreground(look::muted())
                        .size(Some(14.0), Some(14.0))
                        .centered_y(),
                );
                ComponentPalette {
                    hover: look::row_hover(),
                    ..palette
                }
                .interactive_surface(Element::clickable(
                    format!("thread:{session}"),
                    Element::row(row)
                        .gap(12.0)
                        .padding(14.0)
                        .size(Some(width), None),
                ))
            }
            Some(ProposalOutcome::Kept) => Element::row(vec![
                text(proposal.title.clone()).flex(1.0),
                muted("Kept in this session").font_size(SMALL),
            ])
            .gap(12.0)
            .padding(14.0)
            .size(Some(width), None),
            Some(ProposalOutcome::Dismissed) => Element::spacer(),
        });
    }
    Element::column(rows)
        .border(look::card_border(), 1.0)
        .radius(12.0)
        .clip_children()
        .size(Some(width), None)
}

/// What the sessions it started said, as the session hears it:
/// "Received message from" and the session's name, which opens it, and ▸ to read what it
/// said. The newest shimmers while the session works on it.
fn reports_block(
    store: &Store,
    request: &str,
    reports: &[quill_model::Report],
    session: &Session,
    ui: &Ui,
    column: f32,
) -> Element {
    let working = matches!(
        session.status,
        SessionStatus::Running | SessionStatus::Starting
    ) && session
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            Item::Reports { request: r, .. } => Some(r == request),
            Item::User { .. } => Some(false),
            _ => None,
        })
        .unwrap_or(false);
    let rows = reports
        .iter()
        .enumerate()
        .map(|(at, report)| {
            let key = format!("report:{request}:{at}");
            let view = crate::current_view(session, &ui.settings);
            let open = is_open(view, ui, &key);
            // A wait that ended says so; one on a session gone names it as it was.
            let said = match report.waited {
                Some(_) => "Waited for",
                None => "Received message from",
            };
            let lead = if working {
                Element::shimmer_text(
                    format!("report-shimmer:{request}:{at}"),
                    said,
                    look::palette().text,
                )
                .foreground(look::muted())
                .font_family(FontFamily::Sans)
                .font_size(chat())
            } else {
                muted(said).font_size(chat())
            };
            let from = match store.session(report.from) {
                Some(_) => session_chip(store, report.from),
                None => one_line(format!("\u{ab}{}\u{bb}", report.title), look::muted()),
            };
            let line = Element::row(vec![
                lead.centered_y(),
                from.shrink_to_fit(),
                fold(view, key.clone(), caret(open)).centered_y(),
            ])
            .gap(8.0);
            if !open {
                return line;
            }
            Element::column(vec![
                line,
                Markdown::new(&report.text, look::palette())
                    .width(column - 16.0)
                    .font_size(BODY)
                    .render()
                    .element,
            ])
            .gap(8.0)
        })
        .collect();
    Element::column(rows).gap(10.0)
}

/// A session beside the one in view: a side chat (`/btw`), or a session
/// it started, as Desktop opens a thread. Its title, after its
/// parent's for a started one, with "open on its own", expand and close; its own transcript
/// and running line; and a field under it: "Reply in this session…" for a side chat, "Steer
/// this session…" for a started one. The session in view keeps working.
fn side_pane(
    store: &Store,
    ui: &Ui,
    width: f32,
    height: f32,
    palette: ComponentPalette,
) -> Element {
    let side = ui.side_chat.and_then(|id| store.session(id));
    let title = side.map_or_else(|| "Side chat".to_owned(), session_title);
    let parent = side
        .filter(|s| !s.aside)
        .and_then(|s| s.parent)
        .and_then(|p| store.session(p));
    let room = width - 160.0;
    let name = |t: String, ink: limn::Color, room: f32| {
        Element::truncated_text(t)
            .font_family(FontFamily::Sans)
            .font_size(BODY)
            .foreground(ink)
            .max_width(room)
            .centered_y()
    };
    let lead = match parent {
        Some(parent) => Element::row(vec![
            name(session_title(parent), look::muted(), room * 0.4),
            muted("\u{203a}").centered_y(),
            name(title, look::palette().text, room * 0.6),
        ])
        .gap(6.0),
        None => name(title, look::palette().text, room),
    };
    let mut tools = vec![icon_tool(
        SIDE_OPEN,
        icons::of(icons::ARROW_OUT),
        "Open on its own",
        28.0,
        palette,
    )];
    tools.extend(pane_tools(Pane::Side, ui, palette));
    let header = pane_header(lead, tools, width);
    let Some(side) = side else {
        return Element::column(vec![header]).size(Some(width), Some(height));
    };
    let inner = width - 32.0;
    let busy = matches!(
        side.status,
        SessionStatus::Starting | SessionStatus::Running | SessionStatus::Awaiting
    );
    let (mut body, running) = entries(store, side, &side.items, busy, ui, inner, palette);
    if let Some(line) = status_line(side, running.as_ref(), ui, inner, palette) {
        body.push(line);
    }
    let content = Element::column(body).gap(12.0).padding(16.0);
    // Its bar over the padding, as the agent pane's.
    let scroll = Element::scroll(format!("side:{}:scroll", side.id), content)
        .scrollbar_appearance(limn::ScrollbarAppearance::overlay());
    let reply = limn::TextField::new(
        SIDE_FIELD,
        ui.side_reply.as_str(),
        if side.aside {
            "Reply in this session…"
        } else {
            "Steer this session…"
        },
        look::composer_palette(),
    )
    .render()
    .border(look::composer_border(), 1.0)
    .radius(12.0)
    .size(Some(width - 24.0), Some(40.0));
    Element::column(vec![
        header,
        scroll.flex(1.0),
        Element::row(vec![hgap(12.0), reply]),
        Element::spacer().size(None, Some(12.0)),
    ])
    .size(Some(width), Some(height))
}

/// The value of a notification select: its choice's name.
fn notify_value(level: NotifyLevel) -> &'static str {
    match level {
        NotifyLevel::Off => "off",
        NotifyLevel::Badge => "badge",
        NotifyLevel::Banner => "banner",
    }
}

/// A select of Settings, open while its menu is.
fn setting_select(
    ui: &Ui,
    name: &str,
    choices: &[(String, String)],
    current: &str,
    _palette: ComponentPalette,
) -> Element {
    settings::select(
        name,
        choices,
        current,
        ui.settings_menu.as_deref() == Some(name),
    )
}

/// Settings' pages, in the order the column lists them: quill's own, then one per agent
/// quill can run, built from what the agent says it offers, then this computer's. Desktop's
/// pages for its account, its cloud and its own features are not here: quill has none of
/// them (docs/ui.md §2.11).
fn settings_pages(store: &Store, ui: &Ui, palette: ComponentPalette) -> Vec<SettingsPage> {
    use quill_model::{InterfaceFont, Motion, TextSize, Theme, TranscriptView};
    let s = &ui.settings;
    let theme = settings::theme_control(match s.theme {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    });
    let interface_font = match s.interface_font {
        InterfaceFont::System => "system",
        InterfaceFont::Ubuntu => "ubuntu",
    };
    let text_size = match s.text_size {
        TextSize::Small => "small",
        TextSize::Medium => "medium",
        TextSize::Large => "large",
    };
    let transcript_view = match s.transcript_view {
        TranscriptView::Normal => "normal",
        TranscriptView::Thinking => "thinking",
        TranscriptView::Verbose => "verbose",
    };
    let motion = match s.motion {
        Motion::System => "system",
        Motion::Reduced => "reduced",
        Motion::Full => "full",
    };
    let mut pages = vec![
        SettingsPage {
            key: "general".into(),
            title: "General".into(),
            icon: icons::SETTINGS,
            groups: vec![(
                "Appearance".into(),
                vec![
                    setting("Theme", "", theme),
                    // Desktop's own words and choices; its
                    // Anthropic Sans and OpenDyslexic are not quill's (docs/ui.md §2.11).
                    setting(
                        "Interface font",
                        "Font for the whole interface \u{2014} menus, sidebars, and panels.",
                        setting_select(
                            ui,
                            "interface_font",
                            &choices(&[("system", "System"), ("ubuntu", "Ubuntu")]),
                            interface_font,
                            palette,
                        ),
                    ),
                    setting(
                        "Transcript text size",
                        "The size of the conversation's words.",
                        setting_select(
                            ui,
                            "text_size",
                            &choices(&[("small", "Small"), ("medium", "Medium"), ("large", "Large")]),
                            text_size,
                            palette,
                        ),
                    ),
                    setting(
                        "Transcript width",
                        "How wide the conversation and the message box get.",
                        setting_select(
                            ui,
                            "width",
                            &choices(&[("narrow", "Narrow"), ("medium", "Medium"), ("wide", "Wide")]),
                            match s.width {
                                quill_model::Width::Narrow => "narrow",
                                quill_model::Width::Medium => "medium",
                                quill_model::Width::Wide => "wide",
                            },
                            palette,
                        ),
                    ),
                    setting(
                        "Default transcript view",
                        "The view sessions open in. Picking a view from a session's Transcript view menu changes only that session.",
                        setting_select(
                            ui,
                            "transcript_view",
                            &choices(&[
                                ("normal", "Normal"),
                                ("thinking", "Thinking"),
                                ("verbose", "Verbose"),
                            ]),
                            transcript_view,
                            palette,
                        ),
                    ),
                    setting(
                        "Motion",
                        "Reduced keeps panels, sliders and menus from moving.",
                        setting_select(
                            ui,
                            "motion",
                            &choices(&[("system", "System"), ("reduced", "Reduced"), ("full", "Full")]),
                            motion,
                            palette,
                        ),
                    ),
                ],
            ),
            (
                "Logos".into(),
                vec![setting(
                    "Logos of connectors and plugins",
                    "Looked up by name on svgl.app, else the server's own site, and kept on this computer.",
                    setting_switch("logos", s.logos, palette),
                )],
            ),
            (
                "Code".into(),
                vec![setting(
                    "Code font",
                    "Set a custom monospace font for code and terminal.",
                    limn::TextField::new(
                        "set:code_font",
                        s.code_font.as_str(),
                        "Default monospace",
                        palette,
                    )
                    .render()
                    .size(Some(240.0), Some(32.0)),
                )],
            )],
        },
        SettingsPage {
            key: "sessions".into(),
            title: "Sessions".into(),
            icon: icons::LIST,
            groups: vec![
                (
                    "While agents work".into(),
                    vec![
                        setting(
                            "Keep computer awake",
                            "Keep this computer from sleeping while an agent works, so long tasks can finish. The display can still turn off, and closing the lid still puts it to sleep.",
                            setting_switch("keep_awake", s.keep_awake, palette),
                        ),
                        setting(
                            "Keep awake on battery power",
                            "",
                            setting_switch("keep_awake_battery", s.keep_awake_on_battery, palette),
                        ),
                        setting(
                            "Connect new sessions to Remote Control",
                            "Reach every new session from your phone or a browser as it starts, when its agent offers it. Each session's switch is in its Environment card.",
                            setting_switch("remote_new", s.remote_new_sessions, palette),
                        ),
                        // Desktop's own words and choices.
                        setting(
                            "Archive inactive sessions",
                            "Automatically archive sessions after a period of no activity. Pinned sessions stay.",
                            setting_select(
                                ui,
                                "archive",
                                &choices(&[("0", "Never"), ("1", "After 1 day"), ("7", "After 7 days"), ("30", "After 30 days")]),
                                &s.archive_after_days.to_string(),
                                palette,
                            ),
                        ),
                    ],
                ),
                (
                    // Desktop's own words and choices; quill has no dock badge,
                    // so Badge is its own marks in the sidebar and on the window (inferred).
                    "Notifications".into(),
                    vec![
                        setting(
                            "Permission requests",
                            NOTICE_PERMISSION,
                            setting_select(
                                ui,
                                "notify_permission",
                                &choices(&[("off", "Off"), ("badge", "Badge only"), ("banner", "Banners")]),
                                notify_value(s.notify_permission),
                                palette,
                            ),
                        ),
                        setting(
                            "Questions",
                            NOTICE_QUESTION,
                            setting_select(
                                ui,
                                "notify_question",
                                &choices(&[("off", "Off"), ("badge", "Badge only"), ("banner", "Banners")]),
                                notify_value(s.notify_question),
                                palette,
                            ),
                        ),
                        setting(
                            "Task complete",
                            NOTICE_DONE,
                            setting_select(
                                ui,
                                "notify_done",
                                &choices(&[("off", "Off"), ("banner", "Banners")]),
                                match s.notify_done {
                                    NotifyLevel::Off => "off",
                                    _ => "banner",
                                },
                                palette,
                            ),
                        ),
                        // quill's own sounds in place of Desktop's "System sound".
                        setting(
                            "Notification sound",
                            "Played with an OS banner: one sound when a task completes, another when a session needs you.",
                            setting_select(
                                ui,
                                "notification_sound",
                                &choices(&[("quill", "quill"), ("none", "None")]),
                                if s.notification_sound { "quill" } else { "none" },
                                palette,
                            ),
                        ),
                        setting(
                            "Draw attention",
                            "Ask the desktop to flag quill when a session finishes or needs you while quill isn't in front.",
                            setting_switch("attention", s.attention, palette),
                        ),
                    ],
                ),
                (
                    "Pull requests".into(),
                    vec![
                        // Desktop's "Branch prefix" and its own default.
                        setting(
                            "Branch prefix",
                            "Prefix added to the branch names quill asks agents to create.",
                            limn::TextField::new(
                                "set:branch_prefix",
                                s.branch_prefix.as_str(),
                                "quill",
                                palette,
                            )
                            .render()
                            .size(Some(220.0), Some(32.0)),
                        ),
                        setting(
                            "Pull request kind",
                            "What the branch bar's button opens. Picking a kind in its menu changes this too.",
                            setting_select(
                                ui,
                                "pr_kind",
                                &choices(&[("ready", "Ready for review"), ("draft", "Draft")]),
                                if s.pr_draft { "draft" } else { "ready" },
                                palette,
                            ),
                        ),
                        // Desktop's "Open pull requests in browser".
                        setting(
                            "Open pull requests in browser",
                            "Open pull request links in your default browser instead of the built-in browser.",
                            setting_switch("prs_in_browser", s.prs_in_browser, palette),
                        ),
                        setting(
                            "Auto-archive after PR merge",
                            "Automatically archive sessions when the associated pull request is merged.",
                            setting_switch("archive_on_merge", s.archive_on_merge, palette),
                        ),
                    ],
                ),
            ],
        },
    ];
    // The sites the agent may use without asking, which "Always allow for this website" adds
    // to.
    let sites: Vec<Setting> = if s.browser_sites.is_empty() {
        vec![setting(
            "No allowed sites yet.",
            "The agent asks before it opens or uses a site. Local addresses never ask.",
            Element::spacer().size(Some(0.0), Some(0.0)),
        )]
    } else {
        s.browser_sites
            .iter()
            .map(|site| {
                setting(
                    site,
                    "",
                    setting_button(&format!("set:browser_site_remove={site}"), "Remove"),
                )
            })
            .collect()
    };
    let program = match &s.browser_program {
        Some(path) => setting(
            "Chromium",
            path,
            Element::row(vec![
                setting_button("set:browser_program=", "Use PATH"),
                setting_button(BROWSER_PROGRAM_CHOOSE, "Choose\u{2026}"),
            ])
            .gap(8.0),
        ),
        None => setting(
            "Chromium",
            "QUILL_CHROME, else the first chromium or google-chrome on PATH. A new choice runs from the browser's next start.",
            setting_button(BROWSER_PROGRAM_CHOOSE, "Choose\u{2026}"),
        ),
    };
    pages.push(SettingsPage {
        key: "browser".into(),
        title: "Browser".into(),
        icon: icons::GLOBE,
        groups: vec![
            (
                "Chromium".into(),
                vec![
                    program,
                    setting(
                        "Native view",
                        "Pages draw in quill's window as they would in Chromium's, with no delay. \
                         From quill's next start.",
                        setting_switch("browser_native", s.browser_native, palette),
                    ),
                    // Desktop's "Open links in built-in browser".
                    setting(
                        "Open links in built-in browser",
                        "A link in a conversation opens in the session's browser. Off, it opens in this computer's default browser.",
                        setting_switch("links_in_browser", s.links_in_browser, palette),
                    ),
                ],
            ),
            ("Allowed sites".into(), sites),
        ],
    });
    for agent in &ui.agents {
        pages.push(agent_page(store, ui, agent, palette));
    }
    pages.push(crate::accounts::page(store, ui, palette));
    let kept = match &ui.storage {
        Some((path, bytes)) => format!(
            "Sessions and their file history are kept in {} ({}).",
            path.display(),
            human_bytes(*bytes)
        ),
        None => "Sessions and their file history are kept on this computer.".to_owned(),
    };
    pages.push(SettingsPage {
        key: "system".into(),
        title: "System".into(),
        icon: icons::MONITOR,
        groups: vec![
            (
                "quill".into(),
                vec![
                    setting("Version", "", text(env!("CARGO_PKG_VERSION")).centered_y()),
                    setting(
                        "Open at login",
                        "Start quill when you log in to this computer.",
                        setting_switch("login", s.open_at_login, palette),
                    ),
                ],
            ),
            (
                "Updates".into(),
                crate::updates::settings(ui, s.auto_update, palette),
            ),
            (
                "Storage".into(),
                vec![setting(
                    "Data folder",
                    &kept,
                    setting_button(SETTINGS_STORAGE_OPEN, "Open"),
                )],
            ),
        ],
    });
    // Customize is a page of its own; Settings keeps the way there.
    if let Some(general) = pages.iter_mut().find(|p| p.key == SETTINGS_FIRST) {
        general.groups.push((
            "Customize".into(),
            vec![setting(
                "Skills, connectors and plugins",
                "What each agent can use, on a page of its own.",
                setting_button(CUSTOMIZE_OPEN, "Customize\u{2026}"),
            )],
        ));
    }
    pages
}

/// "Add custom connector": its name, the address it is reached at or the command that starts
/// it, its environment and headers, and where it goes, as the agent's program adds it.
pub(crate) fn server_dialog(ui: &Ui, base: Element, palette: ComponentPalette) -> Element {
    let Some(adding) = &ui.connectors.adding else {
        return base;
    };
    let field = |key: &str, value: &str, hint: &str| {
        limn::TextField::new(format!("{SRV_FIELD}{key}"), value, hint, palette)
            .render()
            .size(Some(460.0), Some(32.0))
    };
    let area = |key: &str, value: &str, hint: &str| {
        Element::text_area(format!("{SRV_FIELD}{key}"), value, hint)
            .size(Some(460.0), Some(64.0))
            .background(look::palette().raised)
            .border(look::settings_field_edge(), 1.0)
            .radius(8.0)
    };
    let label = |words: &str| text(words).font_size(BODY);
    let note = |words: &str| muted(words).font_size(SMALL);
    let mut body = vec![note(
        "Make sure you trust a server before you add it: it runs on this computer.",
    )];
    // For which agents: its own program adds it to its own config, which the terminal sees too;
    // "All agents" is quill's, added to each session.
    body.push(label("Add to"));
    let mut targets = vec![keyed_button(
        "srv:to:*",
        "All agents",
        "",
        adding.all,
        true,
        palette,
    )];
    targets.extend(ui.agents.iter().map(|agent| {
        keyed_button(
            &format!("srv:to:{}", agent.id),
            &agent.name,
            "",
            !adding.all && adding.agents.contains(&agent.id),
            true,
            palette,
        )
    }));
    body.push(Element::row(targets).gap(8.0).wrap().max_width(460.0));
    body.push(note(if adding.all {
        "quill adds it to each session, so the agents\u{2019} own files stay as they are."
    } else {
        "Each one adds it with its own command, so it shows in its terminal too."
    }));
    body.extend(vec![
        label("Name"),
        field("name", &adding.name, "my-server"),
        label("Address or command"),
        field(
            "target",
            &adding.target,
            "https://\u{2026}/mcp, or npx -y package",
        ),
        note("An https:// address, or the command that starts it with its arguments."),
        label("Environment variables"),
        area("env", &adding.env, "KEY=value, one a line"),
    ]);
    let remote = adding.target.trim_start().starts_with("http");
    if remote {
        body.push(label("Headers"));
        body.push(area("headers", &adding.headers, "Name: value, one a line"));
    }
    // A secret is kept in the system's keychain and reaches the agent only as a variable of
    // its process: only quill's own layer has them.
    if adding.all {
        body.push(label("Secret variables"));
        body.push(area(
            "secret-env",
            &adding.secret_env,
            "KEY=value, one a line",
        ));
        if remote {
            body.push(label("Secret headers"));
            body.push(area(
                "secret-headers",
                &adding.secret_headers,
                "Name: value, one a line",
            ));
            body.push(Element::row(vec![keyed_button(
                "srv:sse",
                "Older SSE transport",
                "",
                adding.sse,
                true,
                palette,
            )]));
        }
        let kept = if ui.global.keychain {
            "Secrets are kept in the system keychain, not in a file."
        } else {
            "The system keychain isn\u{2019}t answering: secrets can\u{2019}t be kept."
        };
        body.push(note(kept));
    }
    // Where it goes in each of them, in that agent's words.
    for agent in ui
        .agents
        .iter()
        .filter(|a| !adding.all && adding.agents.contains(&a.id))
    {
        let scopes = (ui.connectors.listed.get(&agent.id)).map_or(&[][..], |l| &l.scopes[..]);
        if scopes.len() < 2 {
            continue;
        }
        let chosen = adding
            .scopes
            .get(&agent.id)
            .or_else(|| scopes.first().map(|s| &s.0));
        if adding.agents.len() > 1 {
            body.push(label(&format!("Where in {}", agent.name)));
        } else {
            body.push(label("Where"));
        }
        body.push(
            Element::row(
                scopes
                    .iter()
                    .map(|(scope, words)| {
                        keyed_button(
                            &format!("srv:scope:{}:{scope}", agent.id),
                            words,
                            "",
                            chosen == Some(scope),
                            true,
                            palette,
                        )
                    })
                    .collect(),
            )
            .gap(8.0)
            .wrap()
            .max_width(460.0),
        );
    }
    let footer = Element::row(vec![
        Element::spacer().flex(1.0),
        keyed_button("srv:cancel", "Cancel", "", false, true, palette),
        keyed_button(
            "srv:save",
            "Add",
            "",
            true,
            adding.server().is_some(),
            palette,
        ),
    ])
    .gap(8.0);
    limn::Modal::new(
        base,
        true,
        "Add custom connector",
        "",
        "srv:cancel",
        look::palette(),
    )
    .width(520.0)
    .body(Element::column(body).gap(8.0))
    .footer(footer)
    .backdrop_dismiss(true)
    .scrim(look::scrim())
    .render()
}

/// The "All agents" section of Connectors (`skills` false) or Skills: quill's own items, with
/// how each agent treats each (docs/agents.md "All agents"). Nothing while there are none and
/// only one agent: it would say nothing the agent's own section does not.
pub(crate) fn global_section(ui: &Ui, skills: bool, width: f32) -> Option<Element> {
    use quill_components::customize::{self, Standing};
    use quill_model::Applies;
    let g = &ui.global;
    let names: Vec<(&str, String, &[String])> = if skills {
        (g.skills.iter())
            .map(|s| (s.name.as_str(), cut(&s.description, 200), &s.except[..]))
            .collect()
    } else {
        (g.servers.iter())
            .map(|s| {
                let mut target = vec![s.target.clone()];
                target.extend(s.args.iter().cloned());
                (s.name.as_str(), target.join(" "), &s.except[..])
            })
            .collect()
    };
    let mut column = vec![customize::agent_heading(
        icons::GLOBE,
        "All agents",
        Some(names.len()),
        width,
    )];
    if let Some(error) = &g.error {
        column.push(
            text(format!("Couldn\u{2019}t do that. {error}"))
                .font_size(BODY)
                .foreground(look::failed()),
        );
    }
    if names.is_empty() {
        column.push(customize::empty(
            if skills { icons::SKILL } else { icons::PLUG },
            if skills {
                "No skills for all agents yet. quill adds them to each session."
            } else {
                "No connectors for all agents yet. Add one with \u{201c}All agents\u{201d}."
            },
            width,
        ));
    }
    let kind = if skills { "skill" } else { "server" };
    for (name, words, _) in &names {
        let mut notes = vec![];
        let mut marks = vec![];
        let mut entries: Vec<limn::DropdownMenuEntry> = vec![];
        for agent in &ui.agents {
            let how = (g.applied.iter())
                .find(|a| a.skill == skills && a.name == *name && a.agent == agent.id)
                .map(|a| a.applies.clone());
            let (standing, why) = match how {
                Some(Applies::Excluded) => (Standing::Off, format!("Off in {}", agent.name)),
                Some(Applies::Replaced) => (
                    Standing::Replaced,
                    format!("Replaced in {}: it has its own {name}", agent.name),
                ),
                Some(Applies::Refused(why)) => (Standing::Refused, why),
                _ => (Standing::On, format!("Used in {}", agent.name)),
            };
            match standing {
                Standing::Off => notes.push(format!("off in {}", agent.name)),
                Standing::Replaced => notes.push(format!("replaced in {}", agent.name)),
                Standing::Refused => notes.push(format!("not in {}", agent.name)),
                Standing::On => {}
            }
            marks.push((icons::agent(agent.icon.as_deref()), why, standing));
            entries.push(
                limn::DropdownMenuItem::new(
                    format!("glob:use:{kind}:{name}:{}", agent.id),
                    format!("Use in {}", agent.name),
                )
                .radio(standing != Standing::Off)
                .into(),
            );
        }
        entries.push(limn::DropdownMenuEntry::Separator);
        entries.push(
            limn::DropdownMenuItem::new(format!("glob:remove:{kind}:{name}"), "Remove")
                .icon(icons::of(icons::CLOSE))
                .destructive(look::failed())
                .into(),
        );
        let menu = format!("cz:more:glob:{kind}:{name}");
        let open = ui.settings_menu.as_deref() == Some(menu.as_str());
        let opens = crate::customize::opener(if skills { "skill" } else { "srv" }, "", name);
        let mut said = format!("from quill \u{b7} {words}");
        if !notes.is_empty() {
            said = format!("{} \u{b7} {}", capital(&notes.join(", ")), said);
        }
        column.push(customize::item(
            if skills { icons::SKILL } else { icons::PLUG },
            name,
            &said,
            Some(customize::agent_standing(
                &format!("glob:marks:{kind}:{name}"),
                &marks,
            )),
            ends(Some(customize::more(&menu, entries, open)), Some(opens)),
            width,
        ));
    }
    Some(Element::column(column))
}

pub(crate) fn capital(words: &str) -> String {
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// "New skill" and "Import skill" for "All agents".
pub(crate) fn skill_dialog(ui: &Ui, base: Element, palette: ComponentPalette) -> Element {
    use crate::SkillDialog;
    let Some(dialog) = &ui.skill_dialog else {
        return base;
    };
    let field = |key: &str, value: &str, hint: &str| {
        limn::TextField::new(format!("{GSK_FIELD}{key}"), value, hint, palette)
            .render()
            .size(Some(460.0), Some(32.0))
    };
    let label = |words: &str| text(words).font_size(BODY);
    let note = |words: &str| muted(words).font_size(SMALL);
    let (title, body, ready) = match dialog {
        SkillDialog::New {
            name,
            description,
            instructions,
        } => (
            "Write a skill",
            vec![
                label("Name"),
                field("name", name, "my-skill"),
                note("Lowercase words joined by dashes."),
                label("Description"),
                field(
                    "description",
                    description,
                    "What it does, and when to use it",
                ),
                label("Instructions"),
                Element::text_area(
                    format!("{GSK_FIELD}instructions"),
                    instructions,
                    "What the agent should do when this skill applies",
                )
                .size(Some(460.0), Some(120.0))
                .background(look::palette().raised)
                .border(look::settings_field_edge(), 1.0)
                .radius(8.0),
                note("quill keeps it for all agents and adds it to each session."),
            ],
            !name.trim().is_empty(),
        ),
        SkillDialog::Import { path } => (
            "Import skill",
            vec![
                label("Folder"),
                field("path", path, "/home/you/skills/release-notes"),
                note("A folder with a SKILL.md. quill keeps a copy."),
            ],
            !path.trim().is_empty(),
        ),
    };
    let footer = Element::row(vec![
        Element::spacer().flex(1.0),
        keyed_button("glob:skill-cancel", "Cancel", "", false, true, palette),
        keyed_button("glob:skill-save", "Add", "", true, ready, palette),
    ])
    .gap(8.0);
    limn::Modal::new(base, true, title, "", "glob:skill-cancel", look::palette())
        .width(520.0)
        .body(Element::column(body).gap(8.0))
        .footer(footer)
        .backdrop_dismiss(true)
        .scrim(look::scrim())
        .render()
}

/// The agents an item is on, as logos at its end, when it is on in several: those where `on`
/// says it is. `None` otherwise: one agent's section already says it.
pub(crate) fn agent_marks(ui: &Ui, id: &str, on: impl Fn(&str) -> bool) -> Option<Element> {
    let who: Vec<(&'static str, String)> = marked_agents(ui, on)
        .map(|a| (icons::agent(a.icon.as_deref()), a.name.clone()))
        .collect();
    // On in one agent, its section says it; the marks say where else it is.
    (who.len() > 1).then(|| quill_components::customize::agent_marks(id, &who))
}

/// The agents where `on` holds, when there are several agents; none with one.
pub(crate) fn marked_agents<'a>(
    ui: &'a Ui,
    on: impl Fn(&str) -> bool + 'a,
) -> impl Iterator<Item = &'a quill_model::AgentInfo> {
    let several = ui.agents.len() > 1;
    ui.agents.iter().filter(move |a| several && on(&a.id))
}

/// Whether `agent` has a skill of this name that is not off.
pub(crate) fn skill_on_in(ui: &Ui, agent: &str, name: &str) -> bool {
    (ui.extensions.get(agent))
        .is_some_and(|k| (k.list.skills.iter()).any(|s| s.name == name && s.state != "off"))
}

/// Whether `agent` has a connector of this name that is not off.
pub(crate) fn server_on_in(ui: &Ui, agent: &str, label: &str) -> bool {
    use quill_model::McpStatus;
    let listed = ui
        .connectors
        .listed
        .get(agent)
        .and_then(|l| l.servers.as_ref());
    listed.is_some_and(|all| {
        all.iter()
            .any(|s| s.label == label && s.status != McpStatus::Disabled)
    })
}

/// Whether `agent` has a plugin of this name that is on.
pub(crate) fn plugin_on_in(ui: &Ui, agent: &str, name: &str) -> bool {
    (ui.extensions.get(agent))
        .is_some_and(|k| (k.list.plugins.iter()).any(|p| p.name == name && p.enabled))
}

/// `first` and `then` side by side at an item's end.
pub(crate) fn ends(first: Option<Element>, then: Option<Element>) -> Option<Element> {
    match (first, then) {
        (Some(a), Some(b)) => Some(Element::row(vec![a, b]).gap(12.0).centered_y()),
        (a, b) => a.or(b),
    }
}

/// What a Customize page says over its lists: why the last change failed, or what it did.
pub(crate) fn customize_said(known: &crate::Extensions) -> Option<Element> {
    let (words, color) = match (&known.error, &known.notice) {
        (Some(error), _) => (
            format!("Couldn\u{2019}t make that change. {error}"),
            look::failed(),
        ),
        (None, Some(notice)) => (notice.clone(), look::muted()),
        (None, None) => return None,
    };
    Some(text(words).font_size(BODY).foreground(color))
}

pub(crate) fn cut(words: &str, most: usize) -> String {
    let mut cut: String = words.chars().take(most).collect();
    if words.chars().count() > most {
        cut.push('\u{2026}');
    }
    cut
}

/// Skills: by where they come from, each with its state; its "⋯" changes the state, in a
/// project folder (the header's chip), or says why it cannot.
pub(crate) fn skills_page(ui: &Ui, width: f32) -> Element {
    use quill_components::customize;
    let folder = crate::customize_folder(ui);
    let project = crate::is_project(&folder);
    let query = ui.customize_query.trim().to_lowercase();
    let page = ui.customize.clone().unwrap_or_default();
    let mut column = vec![];
    if page.layer.is_none() {
        column.extend(global_section(ui, true, width));
    }
    for agent in crate::customize::agents(ui) {
        column.push(Element::spacer().size(None, Some(16.0)));
        let known = ui.extensions.get(&agent.id).filter(|k| k.loaded);
        column.push(customize::agent_heading(
            icons::agent(agent.icon.as_deref()),
            &agent.name,
            known.map(|k| k.list.skills.len()),
            width,
        ));
        let Some(known) = known else {
            column.push(customize::loading(
                "cz:skills:loading",
                "Looking for skills\u{2026}",
            ));
            continue;
        };
        if known.working.as_deref() == Some("") {
            column.push(customize::loading(
                "cz:skills:loading",
                "Reading again\u{2026}",
            ));
        }
        column.extend(customize_said(known));
        let mut found: Vec<&quill_model::Skill> = (known.list.skills.iter())
            .filter(|s| page.source.as_ref().is_none_or(|from| &s.source == from))
            .filter(|s| {
                query.is_empty()
                    || s.name.to_lowercase().contains(&query)
                    || s.description.to_lowercase().contains(&query)
            })
            .collect();
        // By name, one list; else a section for each place they come from.
        let mut sources: Vec<&str> = vec![];
        if page.by_name {
            found.sort_by_key(|s| s.name.to_lowercase());
            sources.extend((!found.is_empty()).then_some(""));
        }
        for skill in found.iter().filter(|_| !page.by_name) {
            if !sources.contains(&skill.source.as_str()) {
                sources.push(&skill.source);
            }
        }
        for source in sources {
            let group: Vec<&&quill_model::Skill> = (found.iter())
                .filter(|s| page.by_name || s.source == source)
                .collect();
            let mut title = if page.by_name {
                "By name".to_owned()
            } else {
                source.to_owned()
            };
            if let Some(first) = title.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            column.push(Element::spacer().size(None, Some(12.0)));
            column.push(customize::section(&title, Some(group.len()), None, width));
            for skill in group {
                let label = (known.list.skill_states.iter())
                    .find(|(state, _)| *state == skill.state)
                    .map_or(skill.state.clone(), |(_, label)| label.clone());
                let first = known.list.skill_states.first().map(|(s, _)| s.as_str());
                let end = if known.working.as_deref() == Some(skill.name.as_str()) {
                    Some(
                        limn::Spinner::new(format!("cz:wait:{}", skill.name), look::muted())
                            .size(14.0)
                            .render(),
                    )
                } else {
                    (Some(skill.state.as_str()) != first).then(|| muted(label).font_size(SMALL))
                };
                let marks = agent_marks(
                    ui,
                    &format!("cz:marks:{}:{}", agent.id, skill.name),
                    |other| skill_on_in(ui, other, &skill.name),
                );
                // Its states, each a choice unless the agent or the folder keeps it as it is;
                // some agents keep a skill's state per folder, others for the person.
                let why = skill.locked.clone().or_else(|| {
                    (known.list.kinds.skills_per_folder && !project).then(|| {
                        "Choose a project folder to change a skill\u{2019}s state there.".to_owned()
                    })
                });
                let mut entries: Vec<limn::DropdownMenuEntry> = (known.list.skill_states.iter())
                    .map(|(state, label)| {
                        let item = limn::DropdownMenuItem::new(
                            format!("ext:skill:{}:{}={state}", agent.id, skill.name),
                            label.clone(),
                        )
                        .radio(*state == skill.state);
                        if why.is_some() {
                            item.disabled().into()
                        } else {
                            item.into()
                        }
                    })
                    .collect();
                if let Some(why) = &why {
                    entries.push(limn::DropdownMenuEntry::Separator);
                    entries.push(
                        limn::DropdownMenuItem::new("cz:why", "")
                            .description(why.clone())
                            .disabled()
                            .into(),
                    );
                }
                let menu = format!("cz:more:{}:{}", agent.id, skill.name);
                let open = ui.settings_menu.as_deref() == Some(menu.as_str());
                // On or off from its row; its other states in its "⋯".
                let first = known.list.skill_states.first().map(|(s, _)| s.as_str());
                let switch = limn::Switch::new(
                    format!("czp:skill-switch:{}:{}", agent.id, skill.name),
                    Some(skill.state.as_str()) == first,
                    look::palette(),
                )
                .size(limn::ControlSize::Dense)
                .accessibility_label(format!("{} on", skill.name))
                .enabled(why.is_none() && known.list.skill_states.len() > 1)
                .render();
                let opens = crate::customize::opener("skill", &agent.id, &skill.name);
                column.push(customize::item(
                    icons::SKILL,
                    &skill.name,
                    &format!(
                        "from {} \u{b7} {}",
                        skill.source,
                        cut(&skill.description, 200)
                    ),
                    ends(ends(marks, end), Some(switch)),
                    ends(Some(customize::more(&menu, entries, open)), Some(opens)),
                    width,
                ));
            }
        }
        if found.is_empty() {
            let words = if query.is_empty() {
                format!("No skills in {} yet.", agent.name)
            } else {
                "No skills found.".to_owned()
            };
            column.push(customize::empty(icons::SKILL, &words, width));
        }
    }
    Element::column(column).size(Some(width), None)
}

/// Plugins: Desktop's header (search, Discover, + Add ▾ for the marketplaces), its card while
/// none is installed, then the project's plugins under its folder and the person's own, each
/// with its "⋯" (update, turn off or on, remove).
pub(crate) fn plugins_page(ui: &Ui, width: f32) -> Element {
    use quill_components::customize;
    let folder = crate::customize_folder(ui);
    let project = crate::is_project(&folder);
    let query = ui.customize_query.trim().to_lowercase();
    let mut column = vec![];
    for (at, agent) in crate::customize::agents(ui).into_iter().enumerate() {
        if at > 0 {
            column.push(Element::spacer().size(None, Some(16.0)));
        }
        let known = ui.extensions.get(&agent.id).filter(|k| k.loaded);
        column.push(customize::agent_heading(
            icons::agent(agent.icon.as_deref()),
            &agent.name,
            known.map(|k| k.list.plugins.len()),
            width,
        ));
        let Some(known) = known else {
            column.push(customize::loading(
                "cz:plugins:loading",
                "Looking for plugins\u{2026}",
            ));
            continue;
        };
        if known.working.as_deref() == Some("") {
            column.push(customize::loading(
                "cz:plugins:loading",
                "Reading again\u{2026}",
            ));
        }
        column.extend(customize_said(known));
        if known.list.plugins.is_empty() {
            column.push(customize::hero(
                "Add your first plugins",
                &format!(
                    "Give {} new skills, commands and tools with plugins. Add them from Discover, or from a marketplace.",
                    agent.name
                ),
                Some(customize::button(
                    &format!("cz:discover:{}", agent.id),
                    "Discover plugins",
                    None,
                    false,
                    true,
                )),
                width,
            ));
        }
        let shown =
            |p: &&quill_model::Plugin| query.is_empty() || p.name.to_lowercase().contains(&query);
        let item = |plugin: &quill_model::Plugin| {
            let offer = known.list.offers.iter().find(|o| o.id == plugin.id);
            let market = plugin.id.split_once('@').map_or("", |(_, m)| m);
            let mut words = vec![];
            words.extend(plugin.version.as_ref().map(|v| format!("v{v}")));
            words.push(format!("from {market}"));
            words.extend(offer.map(|o| cut(&o.description, 160)));
            let end = if known.working.as_deref() == Some(plugin.id.as_str()) {
                Some(
                    limn::Spinner::new(format!("cz:wait:{}", plugin.id), look::muted())
                        .size(14.0)
                        .render(),
                )
            } else {
                (!plugin.enabled).then(|| muted("Off").font_size(SMALL))
            };
            let id = &agent.id;
            let kinds = known.list.kinds;
            let mut entries: Vec<limn::DropdownMenuEntry> = vec![];
            if kinds.update_plugins {
                entries.push(
                    limn::DropdownMenuItem::new(format!("ext:update:{id}:{}", plugin.id), "Update")
                        .icon(icons::of(icons::RELOAD))
                        .into(),
                );
            }
            if kinds.switch_plugins {
                entries.push(
                    limn::DropdownMenuItem::new(
                        format!("ext:switch:{id}:{}", plugin.id),
                        if plugin.enabled {
                            "Turn off"
                        } else {
                            "Turn on"
                        },
                    )
                    .icon(icons::of(icons::PLUG))
                    .into(),
                );
            }
            if !entries.is_empty() {
                entries.push(limn::DropdownMenuEntry::Separator);
            }
            entries.push(
                limn::DropdownMenuItem::new(format!("ext:remove:{id}:{}", plugin.id), "Remove")
                    .icon(icons::of(icons::CLOSE))
                    .destructive(look::failed())
                    .into(),
            );
            let menu = format!("cz:more:{id}:{}", plugin.id);
            let open = ui.settings_menu.as_deref() == Some(menu.as_str());
            let words = words.join(" \u{b7} ");
            let more = ends(
                Some(customize::more(&menu, entries, open)),
                Some(crate::customize::opener("plugin", id, &plugin.id)),
            );
            let marks = agent_marks(ui, &format!("cz:marks:{id}:{}", plugin.id), |other| {
                plugin_on_in(ui, other, &plugin.name)
            });
            let end = ends(marks, end);
            match logo_picture(ui, &format!("plugin:{}", plugin.name)) {
                Some(logo) => customize::item_logo(logo, &plugin.name, &words, end, more, width),
                None => customize::item(icons::PLUGIN, &plugin.name, &words, end, more, width),
            }
        };
        let here: Vec<&quill_model::Plugin> = (known.list.plugins.iter())
            .filter(|p| p.scope != "user")
            .filter(shown)
            .collect();
        let yours: Vec<&quill_model::Plugin> = (known.list.plugins.iter())
            .filter(|p| p.scope == "user")
            .filter(shown)
            .collect();
        if project {
            column.push(Element::spacer().size(None, Some(16.0)));
            column.push(customize::section(
                "In this project",
                Some(here.len()),
                Some(customize::folder_chip(PICK_FOLDER, &folder_name(&folder))),
                width,
            ));
            if here.is_empty() {
                column.push(customize::empty(
                    icons::FOLDER,
                    "No plugins installed for this project yet.",
                    width,
                ));
            }
            column.extend(here.into_iter().map(item));
        }
        if !yours.is_empty() {
            column.push(Element::spacer().size(None, Some(16.0)));
            let trailing =
                (!project).then(|| customize::folder_chip(PICK_FOLDER, "Choose a project"));
            column.push(customize::section(
                "Your plugins",
                Some(yours.len()),
                trailing,
                width,
            ));
            column.extend(yours.into_iter().map(item));
        }
    }
    Element::column(column).size(Some(width), None)
}

/// Discover's directory, as Desktop's: a modal of its own over
/// Settings, "Plugins" in its column, a search across all that the marketplaces offer, a
/// filter by marketplace and an add of one, over cards two to a row (the most installed
/// first), each with its marketplace and installs and "+" to install it (where, in a project).
fn directory_dialog(ui: &Ui, base: Element, window: Size, palette: ComponentPalette) -> Element {
    use quill_components::customize;
    let Some(agent) = &ui.directory else {
        return base;
    };
    let width = limn::Modal::fit_width(window.width, 1024.0);
    let height = (window.height - 72.0).clamp(320.0, 772.0);
    // Desktop's column of 200 px items 16 in, and the content 16 in from both sides.
    let inner = width - 2.0 - 232.0 - 32.0;
    let known = ui.extensions.get(agent);
    let project = crate::is_project(&crate::customize_folder(ui));
    let query = ui.directory_query.trim().to_lowercase();
    let search = limn::SearchField::new(
        CZ_DIR_SEARCH,
        &ui.directory_query,
        "Search plugins\u{2026}",
        ComponentPalette {
            surface: look::customize_card(),
            raised: look::customize_card(),
            border: look::settings_field_edge(),
            ..look::palette()
        },
    )
    .width(inner)
    .render();
    let markets: Vec<String> = known.map_or(vec![], |k| {
        let mut all: Vec<String> = vec![];
        for offer in &k.list.offers {
            if !all.contains(&offer.marketplace) {
                all.push(offer.marketplace.clone());
            }
        }
        all
    });
    let filter_menu = "cz:filter-menu".to_owned();
    let mut entries = vec![
        limn::DropdownMenuItem::new("cz:filter:", "All marketplaces")
            .radio(ui.directory_market.is_none())
            .into(),
    ];
    entries.extend(markets.iter().map(|m| {
        limn::DropdownMenuItem::new(format!("cz:filter:{m}"), m.clone())
            .radio(ui.directory_market.as_deref() == Some(m.as_str()))
            .into()
    }));
    let filter = customize::menu_button(
        customize::button(
            &filter_menu,
            ui.directory_market.as_deref().unwrap_or("Filter by"),
            None,
            true,
            false,
        ),
        &filter_menu,
        entries,
        ui.settings_menu.as_deref() == Some(filter_menu.as_str()),
    );
    let add = tool_surface(
        &format!("cz:add-marketplace:{agent}"),
        icons::of(icons::ADD),
        "Add marketplace",
        28.0,
        None,
        look::tool_hover(),
        None,
    );
    // Whose directory it is, when several agents have one: plugins install into that agent.
    let agents = (ui.agents.len() > 1).then(|| {
        Element::row(
            ui.agents
                .iter()
                .map(|a| {
                    keyed_button(
                        &format!("cz:dir:{}", a.id),
                        &a.name,
                        "",
                        &a.id == agent,
                        true,
                        palette,
                    )
                })
                .collect(),
        )
        .gap(8.0)
    });
    let mut cards = vec![];
    if let Some(known) = known {
        let installed: Vec<&str> = known.list.plugins.iter().map(|p| p.id.as_str()).collect();
        let mut offers: Vec<&quill_model::PluginOffer> = (known.list.offers.iter())
            .filter(|o| {
                ui.directory_market
                    .as_ref()
                    .is_none_or(|m| &o.marketplace == m)
            })
            .filter(|o| {
                query.is_empty()
                    || o.name.to_lowercase().contains(&query)
                    || o.description.to_lowercase().contains(&query)
            })
            .collect();
        offers.sort_by(|a, b| b.installs.cmp(&a.installs).then(a.name.cmp(&b.name)));
        let card_width = ((inner - 12.0) / 2.0).floor();
        for offer in offers
            .into_iter()
            .take(if query.is_empty() { 60 } else { 120 })
        {
            let mut meta = offer.marketplace.clone();
            if let Some(n) = offer.installs {
                meta.push_str(&format!(" \u{b7} {} installs", human_count(n)));
            }
            let id = format!("ext:install:{agent}:{}", offer.id);
            let action = if installed.contains(&offer.id.as_str()) {
                muted("Added").font_size(SMALL)
            } else if known.working.as_deref() == Some(offer.id.as_str()) {
                limn::Spinner::new(format!("cz:wait:{}", offer.id), look::muted())
                    .size(14.0)
                    .render()
            } else if project {
                let open = ui.settings_menu.as_deref() == Some(id.as_str());
                let trigger = tool_surface(
                    &id,
                    icons::of(icons::ADD),
                    "Install",
                    28.0,
                    None,
                    look::tool_hover(),
                    None,
                );
                let entries = (known.list.scopes.iter())
                    .map(|(scope, label)| {
                        limn::DropdownMenuItem::new(format!("{id}={scope}"), label.clone()).into()
                    })
                    .collect();
                customize::menu_button(trigger, &id, entries, open)
            } else {
                tool_surface(
                    &id,
                    icons::of(icons::ADD),
                    "Install",
                    28.0,
                    None,
                    look::tool_hover(),
                    None,
                )
            };
            let tile = Element::row(vec![Element::icon(icons::of(icons::PLUG))
                .foreground(look::palette().text)
                .size(Some(16.0), Some(16.0))
                .centered()
                .flex(1.0)])
            .size(Some(36.0), Some(36.0))
            .background(look::customize_tile())
            .radius(8.0);
            let copy = Element::column(vec![
                Element::truncated_text(offer.name.clone())
                    .font_family(FontFamily::SansBold)
                    .font_size(BODY)
                    .foreground(look::palette().text),
                muted(cut(&offer.description, 90)).font_size(SMALL),
                muted(meta).font_size(SMALL),
            ])
            .gap(3.0)
            .flex(1.0);
            cards.push(
                Element::row(vec![tile, copy, action])
                    .gap(12.0)
                    .padding(12.0)
                    .size(Some(card_width), Some(104.0))
                    .background(look::customize_card())
                    .radius(10.0),
            );
        }
    }
    let mut grid = vec![];
    let mut cards = cards.into_iter();
    while let Some(left) = cards.next() {
        let mut row = vec![left];
        row.extend(cards.next());
        grid.push(Element::row(row).gap(12.0));
    }
    let content: Element = match known.filter(|k| k.loaded) {
        None => customize::loading("cz:dir:loading", "Looking for plugins\u{2026}"),
        Some(_) if grid.is_empty() => muted("No plugins available.").font_size(BODY).centered_x(),
        Some(_) => Element::column(grid).gap(12.0),
    };
    let nav = Element::column(vec![side_row(
        "cz:directory:plugins".into(),
        vec![
            Element::icon(icons::of(icons::PLUG))
                .size(Some(16.0), Some(16.0))
                .centered_y(),
            text("Plugins").font_size(BODY).centered_y(),
        ],
        true,
        palette,
    )])
    .padding(16.0)
    .size(Some(232.0), None);
    let body = Element::row(vec![
        nav,
        Element::column(
            agents
                .into_iter()
                .chain([
                    search,
                    Element::row(vec![Element::spacer().flex(1.0), filter, add]).gap(8.0),
                    Element::scroll("cz:directory", content).flex(1.0),
                ])
                .collect(),
        )
        .gap(12.0)
        .padding(16.0)
        .flex(1.0),
    ])
    .size(Some(width - 2.0), Some(height - 47.0));
    limn::Modal::new(
        base,
        true,
        "Directory",
        "",
        "cz:close",
        ComponentPalette {
            surface: look::settings_page(),
            border: look::settings_title_line(),
            ..look::palette()
        },
    )
    .width(width)
    .height(height)
    .outline(look::settings_outline())
    .flush_body(true)
    .body(body)
    .scrim(look::scrim())
    .render()
}

/// The marketplaces' dialogs, as Desktop's: adding one by its source,
/// warned to trust it first; and those added, each with its "⋯" (check for updates, remove).
fn marketplace_dialog(ui: &Ui, base: Element, palette: ComponentPalette) -> Element {
    use quill_components::customize;
    let Some(dialog) = &ui.marketplaces else {
        return base;
    };
    let (title, body, footer) = match dialog {
        crate::MarketDialog::Add => {
            let field = limn::TextField::new(
                format!("{EXT_MKT_FIELD}all"),
                &ui.marketplace_field,
                "owner/repo",
                palette,
            )
            .render()
            .size(Some(440.0), Some(32.0));
            // Each agent adds it with its own program; Codex reads Claude's marketplace files
            // too, but quill asks each one.
            let to = Element::row(
                ui.agents
                    .iter()
                    .map(|a| {
                        keyed_button(
                            &format!("cz:to:{}", a.id),
                            &a.name,
                            "",
                            ui.market_targets.contains(&a.id),
                            true,
                            palette,
                        )
                    })
                    .collect(),
            )
            .gap(8.0);
            let body = Element::column(vec![
                muted("Make sure you trust a plugin before you install it: it can run commands on this computer.")
                    .font_size(BODY),
                text("URL").font_size(BODY),
                field,
                muted("A GitHub owner/repo or a Git repository URL.").font_size(SMALL),
                text("Add to").font_size(BODY),
                to,
            ])
            .gap(8.0);
            let footer = Element::row(vec![
                Element::spacer().flex(1.0),
                keyed_button("cz:close", "Cancel", "", false, true, palette),
                keyed_button(
                    "ext:mkt-add",
                    "Add",
                    "",
                    true,
                    !ui.marketplace_field.trim().is_empty() && !ui.market_targets.is_empty(),
                    palette,
                ),
            ])
            .gap(8.0);
            ("Add marketplace", body, footer)
        }
        crate::MarketDialog::Manage => {
            let mut rows = vec![muted(
                "Marketplaces added on this computer, by agent. Removing one also uninstalls its plugins.",
            )
            .font_size(BODY)];
            for agent in &ui.agents {
                let known = ui.extensions.get(&agent.id);
                let agent = &agent.id;
                let name = (ui.agents.iter())
                    .find(|a| &a.id == agent)
                    .map_or(agent.clone(), |a| a.name.clone());
                rows.push(text(name).font_family(FontFamily::SansBold).font_size(BODY));
                let markets = known.map_or(&[][..], |k| &k.list.marketplaces[..]);
                if markets.is_empty() {
                    rows.push(muted("No marketplaces added yet.").font_size(BODY));
                }
                for market in markets {
                    let menu = format!("cz:more:{agent}:{}", market.name);
                    let open = ui.settings_menu.as_deref() == Some(menu.as_str());
                    let entries = vec![
                        limn::DropdownMenuItem::new(
                            format!("ext:mkt-update:{agent}:{}", market.name),
                            "Check for updates",
                        )
                        .icon(icons::of(icons::RELOAD))
                        .into(),
                        limn::DropdownMenuItem::new(
                            format!("ext:mkt-remove:{agent}:{}", market.name),
                            "Remove",
                        )
                        .icon(icons::of(icons::CLOSE))
                        .destructive(look::failed())
                        .into(),
                    ];
                    let end = known
                        .filter(|k| k.working.as_deref() == Some(market.name.as_str()))
                        .map(|_| {
                            limn::Spinner::new(format!("cz:wait:{}", market.name), look::muted())
                                .size(14.0)
                                .render()
                        });
                    rows.push(customize::item(
                        icons::GLOBE,
                        &market.name,
                        &market.source,
                        end,
                        Some(customize::more(&menu, entries, open)),
                        500.0,
                    ));
                }
            }
            let footer = Element::row(vec![
                Element::spacer().flex(1.0),
                keyed_button("cz:close", "Done", "", true, true, palette),
            ]);
            (
                "Manage marketplaces",
                Element::column(rows).gap(8.0),
                footer,
            )
        }
    };
    limn::Modal::new(base, true, title, "", "cz:close", look::palette())
        .width(540.0)
        .body(body)
        .footer(footer)
        .backdrop_dismiss(true)
        .scrim(look::scrim())
        .render()
}

/// 2900 as "2.9K", 1200000 as "1.2M", as Desktop counts installs.
pub(crate) fn human_count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}K", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// Customize's question before it removes a plugin, or a marketplace with its plugins,
/// in Desktop's words.
fn removing_dialog(ui: &Ui, base: Element) -> Element {
    let (title, words) = match &ui.removing {
        Some(crate::Removing::Plugin(_, plugin)) => (
            "Remove plugin?".to_owned(),
            format!("{} will be uninstalled from this computer.", plugin.name),
        ),
        Some(crate::Removing::Marketplace(_, name)) => (
            "Remove marketplace?".to_owned(),
            format!(
                "{name} is removed from this computer. Removing one also uninstalls its plugins."
            ),
        ),
        None => (String::new(), String::new()),
    };
    confirm(
        base,
        ui.removing.is_some(),
        &title,
        &words,
        "Remove",
        EXT_REMOVE_YES,
        EXT_REMOVE_NO,
    )
}

/// An agent's page: its program and the version it runs, what new sessions start with, and
/// its modes that skip approvals, from what its sessions said it offers.
fn agent_page(
    store: &Store,
    ui: &Ui,
    agent: &quill_model::AgentInfo,
    palette: ComponentPalette,
) -> SettingsPage {
    let mine = agent.id.as_str();
    let chosen = ui.settings.agent(mine);
    // What the agent offers, as its latest session heard it.
    let latest = store
        .sessions
        .iter()
        .rev()
        .find(|s| s.agent == mine && s.capabilities.is_some());
    let capabilities = latest.and_then(|s| s.capabilities.as_ref());
    let version = store
        .sessions
        .iter()
        .rev()
        .filter(|s| s.agent == mine)
        .find_map(|s| s.agent_version.clone());
    let tested = agent.tested.join(", ");
    let (version_value, version_about) = match &version {
        Some(v) if agent.tested.contains(v) => {
            (v.clone(), format!("quill was tested with {tested}."))
        }
        Some(v) => (
            v.clone(),
            format!("quill was tested with {tested}; this version may behave differently."),
        ),
        None => (
            "Not run yet".to_owned(),
            format!("quill was tested with {tested}."),
        ),
    };
    let program = limn::TextField::new(
        format!("set:agent:{mine}:program"),
        chosen.program.as_deref().unwrap_or_default(),
        mine,
        palette,
    )
    .render()
    .size(Some(240.0), Some(32.0));
    let mut groups = vec![(
        "Program".to_owned(),
        vec![
            setting(
                "Program",
                &format!(
                    "The {} program quill runs. Empty runs the one on your PATH. Applies to new sessions.",
                    agent.name
                ),
                program,
            ),
            setting(
                "Version",
                &version_about,
                if version.is_some() {
                    text(version_value)
                } else {
                    muted(version_value)
                }
                .centered_y(),
            ),
        ],
    )];
    let defaults = match capabilities {
        None => vec![setting(
            "Model, effort and mode",
            &format!(
                "Start a session with {} once, and its models, efforts and modes are listed here.",
                agent.name
            ),
            Element::spacer().size(Some(0.0), Some(0.0)),
        )],
        Some(capabilities) => {
            let leave = (String::new(), "Agent's default".to_owned());
            let mut models = vec![leave.clone()];
            models.extend(
                capabilities
                    .models
                    .iter()
                    .map(|m| (m.id.clone(), m.name.clone())),
            );
            let model = chosen.model.clone().unwrap_or_default();
            let efforts: Vec<String> = capabilities
                .models
                .iter()
                .find(|m| m.id == model)
                .or_else(|| capabilities.models.first())
                .map(|m| m.efforts.clone())
                .unwrap_or_default();
            let mut effort_choices = vec![leave.clone()];
            effort_choices.extend(efforts.iter().map(|e| (e.clone(), effort_label(e))));
            let mut modes = vec![leave];
            let unsafe_modes = if chosen.allow_unsafe {
                capabilities.unsafe_modes.clone()
            } else {
                vec![]
            };
            modes.extend(
                capabilities
                    .modes
                    .iter()
                    .chain(&unsafe_modes)
                    .filter_map(|m| mode_entry(ui, mine, m).map(|w| (m.clone(), w.label.clone()))),
            );
            let mut rows = vec![
                setting(
                    "Model",
                    "What a new session starts with; the message box changes it for one session.",
                    setting_select(ui, &format!("agent:{mine}:model"), &models, &model, palette),
                ),
                setting(
                    "Effort",
                    "",
                    setting_select(
                        ui,
                        &format!("agent:{mine}:effort"),
                        &effort_choices,
                        chosen.effort.as_deref().unwrap_or_default(),
                        palette,
                    ),
                ),
                setting(
                    "Permission mode",
                    "",
                    setting_select(
                        ui,
                        &format!("agent:{mine}:mode"),
                        &modes,
                        chosen.mode.as_deref().unwrap_or_default(),
                        palette,
                    ),
                ),
            ];
            if !capabilities.unsafe_modes.is_empty() {
                rows.push(setting(
                    "Allow bypass permissions mode",
                    &format!(
                        "Bypass all permission checks and let {} work uninterrupted. Letting an agent run any command is risky and can lose data, damage your system or leak it (through prompt injection, for example). An open session restarts in it when you choose it.",
                        agent.name
                    ),
                    setting_switch(&format!("agent:{mine}:unsafe"), chosen.allow_unsafe, palette),
                ));
            }
            rows
        }
    };
    groups.push(("New sessions".to_owned(), defaults));
    // What the agent's adapter declares as its own, then the environment its sessions get.
    let mut own: Vec<Setting> = (agent.options.iter())
        .map(|option| {
            let value = chosen
                .options
                .get(&option.key)
                .cloned()
                .unwrap_or_else(|| option.default.clone());
            let name = format!("agent:{mine}:opt.{}", option.key);
            let control = if option.choices.is_empty() {
                setting_switch(&name, value == "true", palette)
            } else {
                setting_select(ui, &name, &option.choices, &value, palette)
            };
            setting(&option.label, &option.description, control)
        })
        .collect();
    own.push(setting(
        "Environment variables",
        "KEY=value, one a line. Every new session of this agent starts with them.",
        Element::text_area(
            format!("set:agent:{mine}:env"),
            chosen.env.join("\n"),
            "KEY=value",
        )
        .size(Some(260.0), Some(72.0))
        .background(look::palette().raised)
        .border(look::settings_field_edge(), 1.0)
        .radius(8.0),
    ));
    groups.push(("Sessions".to_owned(), own));
    SettingsPage {
        key: format!("agent:{mine}"),
        title: agent.name.clone(),
        icon: icons::agent(agent.icon.as_deref()),
        groups,
    }
}

/// A size in bytes as people read it.
fn human_bytes(bytes: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < units.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{size:.1} {}", units[unit])
    }
}

/// The session's MCP servers (`/mcp`), as Desktop's dialog lists them: each server with what ships it, where it was configured and how it is,
/// and what can be done about it: connect (sign in), reconnect, turn on. A row opens the
/// server's details: its sign-in (re-authenticate, clear its credentials after asking) and,
/// quill's own, its tools by what they do. Its words are Desktop's; while a turn runs the
/// actions wait for it to end.
fn mcp_dialog(
    store: &Store,
    ui: &Ui,
    base: Element,
    window: Size,
    palette: ComponentPalette,
) -> Element {
    use crate::McpPhase;
    use quill_model::{McpSource, McpStatus};
    let Some(mcp) = &ui.mcp else {
        return base;
    };
    let session = store.session(mcp.session);
    let width = limn::Modal::fit_width(window.width, 720.0);
    let inner = limn::Modal::body_width(width);
    let waits = session.is_some_and(|s| s.running_since.is_some());
    let action = |id: String, label: &str| {
        let button = keyed_button(&id, label, "", false, !waits, palette);
        if waits {
            limn::Tooltip::new(
                button,
                format!("{id}:why"),
                "Available when the current turn finishes",
                look::palette(),
            )
            .render()
        } else {
            button
        }
    };
    let words = |s: String| muted(s).font_size(BODY);
    // Desktop shows each server's favicon; quill shows the logo it found, else the initial.
    let avatar = |server: &quill_model::McpServer, side: f32| match logo_picture(
        ui,
        &format!(
            "mcp:{}",
            crate::server_logo_name(ui, &server.name, &server.label)
        ),
    ) {
        Some(logo) => logo_tile(logo, side),
        None => limn::Avatar::new(&server.label, look::palette())
            .size(side)
            .render(),
    };
    // Where it was configured, as Desktop's outlined pill beside the type.
    let chip = |label: &str| {
        Element::row(vec![
            hgap(6.0),
            muted(label.to_owned()).font_size(11.0).centered_y(),
            hgap(6.0),
        ])
        .size(None, Some(18.0))
        .border(look::customize_edge(), 1.0)
        .radius(9.0)
    };
    let spinner = |name: &str| {
        limn::Spinner::new(format!("mcp:wait:{name}"), look::muted())
            .size(14.0)
            .render()
    };
    let kind = |source: McpSource| match source {
        McpSource::User => "User",
        McpSource::Project => "Project",
        McpSource::Local => "Local",
        McpSource::Managed => "Managed",
        McpSource::Plugin => "Plugin",
        // Desktop lists the account's connectors apart; quill names them (inferred).
        McpSource::Account => "Account",
        McpSource::App => "App",
        McpSource::Global => "All agents",
        McpSource::Other => "Other",
    };
    let tools = |n: usize| match n {
        1 => "Connected \u{b7} 1 tool".to_owned(),
        n => format!("Connected \u{b7} {n} tools"),
    };
    let needs_auth = |server: &quill_model::McpServer| {
        if server.source == McpSource::Account {
            "Sign in to it where your account's connectors are managed, then reconnect here."
        } else {
            "Sign in with /mcp in a terminal where this session runs, then reconnect here."
        }
    };
    let caps = session.and_then(|s| s.capabilities.as_ref());
    let (can_toggle, can_clear) = (
        caps.is_some_and(|c| c.mcp_toggle),
        caps.is_some_and(|c| c.mcp_clear_auth),
    );
    let servers = session.and_then(|s| s.mcp.as_ref());
    let shown = (mcp.detail.as_ref())
        .and_then(|name| servers.and_then(|all| all.iter().find(|s| &s.name == name)));
    let mut body = vec![];
    if let Some(notice) = &mcp.notice {
        body.push(words(notice.clone()).padding(8.0));
    }
    match (servers, shown) {
        (_, Some(server)) => {
            let phase = mcp.phases.get(&server.name).copied();
            let back = tool_surface(
                MCP_BACK,
                icons::of(icons::ARROW_LEFT),
                "All servers",
                28.0,
                None,
                look::tool_hover(),
                None,
            );
            body.push(
                Element::row(vec![back, text("All servers").font_size(BODY).centered_y()])
                    .gap(6.0),
            );
            let mut who = vec![text(server.label.clone())
                .font_family(FontFamily::SansBold)
                .font_size(16.0)];
            if let Some(plugin) = &server.plugin {
                who.push(muted(format!("Provided by the {plugin} plugin")).font_size(SMALL));
            }
            let status = match (phase, server.status) {
                (Some(McpPhase::Opening), _) => "Opening sign-in\u{2026}".to_owned(),
                (Some(McpPhase::InBrowser), _) => {
                    "Complete sign-in in your browser\u{2026}".to_owned()
                }
                (None, McpStatus::Connected) => tools(server.tools.len()),
                (None, McpStatus::Pending) => "Connecting\u{2026}".to_owned(),
                (None, McpStatus::NeedsAuth) => "Needs authentication".to_owned(),
                (None, McpStatus::Failed) => match &server.error {
                    Some(error) => format!("Failed to connect: {error}"),
                    None => "Failed to connect".to_owned(),
                },
                (None, McpStatus::Disabled) => "Turned off".to_owned(),
            };
            who.push(words(format!("{} \u{b7} {status}", kind(server.source))));
            let mut buttons = vec![];
            let name = &server.name;
            match (phase, server.status) {
                (Some(McpPhase::InBrowser), _) => {
                    buttons.push(action(format!("{MCP_RECONNECT}{name}"), "Reconnect"));
                }
                (Some(McpPhase::Opening), _) => {}
                (None, McpStatus::Connected) if server.sign_in => {
                    buttons.push(action(format!("{MCP_CONNECT}{name}"), "Re-authenticate"));
                    if can_clear {
                        buttons.push(action(format!("{MCP_CLEAR}{name}"), "Clear authentication"));
                    }
                }
                (None, McpStatus::NeedsAuth | McpStatus::Failed) => {
                    if server.sign_in {
                        buttons.push(action(format!("{MCP_CONNECT}{name}"), "Connect"));
                    }
                    buttons.push(action(format!("{MCP_RECONNECT}{name}"), "Reconnect"));
                }
                (None, McpStatus::Disabled) if can_toggle => {
                    buttons.push(action(format!("{MCP_TURN_ON}{name}"), "Turn on"))
                }
                _ => {}
            }
            body.push(
                Element::row(vec![
                    avatar(server, 32.0).centered_y(),
                    Element::column(who).gap(4.0).flex(1.0),
                    Element::row(buttons).gap(8.0).centered_y(),
                ])
                .gap(12.0)
                .padding(8.0),
            );
            let note = match (phase, server.status) {
                (Some(McpPhase::InBrowser), _) => Some(
                    "Finished signing in from your browser? Reconnect to pick up the new credentials.",
                ),
                (None, McpStatus::Connected) if server.sign_in => Some(
                    "Re-authenticate and Clear authentication apply to servers you signed in to with OAuth.",
                ),
                (None, McpStatus::NeedsAuth) if !server.sign_in => Some(needs_auth(server)),
                _ => None,
            };
            if let Some(note) = note {
                body.push(words(note.to_owned()).padding(8.0));
            }
            // quill's own: the tools, by what their server says they do (Desktop's groups
            // for its connectors' tools).
            let groups = [
                ("Read-only tools", 0),
                ("Write/delete tools", 1),
                ("Other tools", 2),
            ];
            let group_of = |tool: &quill_model::McpTool| {
                if tool.read_only {
                    0
                } else if tool.destructive {
                    1
                } else {
                    2
                }
            };
            let mut list = vec![];
            for (title, at) in groups {
                let rows: Vec<Element> = (server.tools.iter())
                    .filter(|t| group_of(t) == at)
                    .map(|tool| {
                        let mut row = vec![text(tool.name.clone()).font_size(BODY)];
                        if let Some(about) = &tool.description {
                            let about: String = about.chars().take(140).collect();
                            row.push(muted(about).font_size(SMALL));
                        }
                        Element::column(row).gap(2.0).padding(6.0)
                    })
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                list.push(
                    muted(format!("{title} ({})", rows.len()))
                        .font_size(SMALL)
                        .padding(6.0),
                );
                list.extend(rows);
            }
            if !list.is_empty() {
                body.push(
                    Element::scroll("mcp:tools", Element::column(list).size(Some(inner), None))
                        .max_height(window.height * 0.45),
                );
            }
        }
        (None, _) => body.push(
            words("Loading this session\u{2019}s MCP servers\u{2026} If they don\u{2019}t appear, send a message in the session to start it.".into())
                .padding(12.0),
        ),
        (Some(servers), _) if servers.is_empty() => {
            body.push(words("This session has no MCP servers.".into()).padding(12.0))
        }
        (Some(servers), _) => {
            let columns = vec![
                // Desktop's columns: Type and Status 160 each (`w-40`).
                limn::TableColumn::flexible("server", "Server", 200.0),
                limn::TableColumn::fixed("type", "Type", 160.0),
                limn::TableColumn::fixed("status", "Status", 160.0),
            ];
            let rows = servers
                .iter()
                .map(|server| {
                    let mut copy = vec![Element::truncated_text(server.label.clone())
                        .font_family(FontFamily::Sans)
                        .font_size(BODY)
                        .foreground(look::palette().text)];
                    if let Some(plugin) = &server.plugin {
                        copy.push(
                            muted(format!("Provided by the {plugin} plugin")).font_size(SMALL),
                        );
                    }
                    let who = vec![
                        avatar(server, 24.0).centered_y(),
                        Element::column(copy).gap(2.0).flex(1.0).centered_y(),
                    ];
                    let note = |said: String, why: Option<&str>| {
                        let said = words(said);
                        match why {
                            Some(why) => limn::Tooltip::new(
                                said,
                                format!("mcp:why:{}", server.name),
                                why,
                                look::palette(),
                            )
                            .render(),
                            None => said,
                        }
                    };
                    let name = &server.name;
                    // Desktop's status cell: a check, a spinner, or the one thing to
                    // do; what it says more is in the server's details.
                    let check = || {
                        Element::icon(icons::of(icons::CHECK))
                            .foreground(look::palette().text)
                            .size(Some(16.0), Some(16.0))
                    };
                    let status = match (mcp.phases.get(name), server.status) {
                        (Some(McpPhase::Opening), _) => spinner(name),
                        (Some(McpPhase::InBrowser), _) => {
                            action(format!("{MCP_RECONNECT}{name}"), "Reconnect")
                        }
                        (None, McpStatus::Connected) => check(),
                        (None, McpStatus::Pending) => spinner(name),
                        (None, McpStatus::NeedsAuth) if server.sign_in => {
                            action(format!("{MCP_CONNECT}{name}"), "Connect")
                        }
                        (None, McpStatus::NeedsAuth) => {
                            note("Needs authentication".into(), Some(needs_auth(server)))
                        }
                        (None, McpStatus::Failed) => limn::Tooltip::new(
                            action(format!("{MCP_RECONNECT}{name}"), "Reconnect"),
                            format!("mcp:failed:{name}"),
                            server.error.as_deref().unwrap_or("Failed to connect"),
                            look::palette(),
                        )
                        .render(),
                        (None, McpStatus::Disabled) if can_toggle => {
                            action(format!("{MCP_TURN_ON}{name}"), "Turn on")
                        }
                        (None, McpStatus::Disabled) => note("Off in its config".into(), None),
                    };
                    limn::TableRow::new(
                        format!("{MCP_ROW}{name}"),
                        vec![
                            Element::row(who).gap(10.0).flex(1.0).centered_y(),
                            chip(kind(server.source)).centered_y(),
                            status.centered_y(),
                        ],
                    )
                    .controls([MCP_CONNECT, MCP_RECONNECT, MCP_TURN_ON].map(|a| format!("{a}{name}")))
                })
                .collect();
            // Two lines for a plugin's server, as Desktop's rows (`min-h-14`) hold.
            let sizing = limn::TableSizing {
                row_height: 44.0,
                ..limn::TableSizing::COMPACT
            };
            let table = limn::Table::new("mcp:servers", columns, inner, look::palette())
                .sizing(sizing)
                .rows(rows)
                .render();
            body.push(Element::scroll("mcp:scroll", table).max_height(window.height * 0.6));
        }
    }
    let footer = Element::row(vec![
        Element::spacer().flex(1.0),
        keyed_button(MCP_DONE, "Done", "", true, true, palette),
    ]);
    let dialog = limn::Modal::new(
        base,
        true,
        "MCP servers in this session",
        "",
        MCP_DONE,
        look::palette(),
    )
    .width(width)
    .body(Element::column(body).gap(4.0))
    .footer(footer)
    .backdrop_dismiss(true)
    .scrim(look::scrim())
    .render();
    // Desktop's question before forgetting a server's credentials.
    let clearing = (mcp.clearing.as_ref())
        .map(|name| {
            let found = servers.and_then(|all| all.iter().find(|s| &s.name == name));
            found.map_or(name.clone(), |s| s.label.clone())
        })
        .unwrap_or_default();
    confirm(
        dialog,
        mcp.clearing.is_some(),
        &format!("Clear authentication for {clearing}?"),
        "The stored credentials for this server are revoked and deleted. You can authenticate again afterwards.",
        "Clear authentication",
        MCP_CLEAR_YES,
        MCP_CLEAR_NO,
    )
}

/// Settings, in Desktop's measures at 1×: a 1024 × 772 card whose body
/// reaches its edges under a 44-high title; a 212-wide column of pages on its own ground, a
/// 28-high search over groups of 32-high pages; and the page's sections, each a 16 px title
/// over rows apart by hairlines, a 15 px title and its 14 px muted words on the left and the
/// control on the right. With a search, every page's settings that match.
fn settings_dialog(
    store: &Store,
    ui: &Ui,
    base: Element,
    window: Size,
    palette: ComponentPalette,
) -> Element {
    let Some(current) = &ui.settings_page else {
        return base;
    };
    let pages = settings_pages(store, ui, palette);
    let owned = |keys: &[&str]| keys.iter().map(|k| (*k).to_owned()).collect();
    let mut groups = vec![(
        "Preferences",
        owned(&["general", "sessions", "browser", SETTINGS_ACCOUNTS]),
    )];
    let agents: Vec<String> = pages
        .iter()
        .filter(|p| p.key.starts_with("agent:"))
        .map(|p| p.key.clone())
        .collect();
    if !agents.is_empty() {
        groups.push(("Agents", agents));
    }
    groups.push(("This computer", owned(&["system"])));
    settings::dialog(
        base,
        pages,
        groups,
        current,
        &ui.settings_query,
        ui.settings_error.as_deref(),
        window,
    )
}

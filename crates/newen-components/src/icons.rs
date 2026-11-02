//! quill's own icons: line drawings on a 20 × 20 grid with a 1.25 stroke ([`STROKE`]), round
//! caps and joins and no fill, in the quiet, thin style of Claude Desktop's icons. They are drawn for quill, not
//! taken from that font, which is Anthropic's own. Each is an SVG in `currentColor`, which
//! limn tints like its own icons through `Icon::Custom`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

macro_rules! icon {
    ($body:expr) => {
        concat!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20' fill='none' ",
            "stroke='currentColor' stroke-width='1.25' stroke-linecap='round' stroke-linejoin='round'>",
            $body,
            "</svg>"
        )
    };
}

/// The width of every line, on the 20 grid: about 1/16 of the icon, so a 16 px icon draws
/// 1 px lines.
pub const STROKE: f32 = 1.25;

/// A drawing as limn takes it.
pub fn of(svg: &'static str) -> limn::Icon {
    limn::Icon::Custom(svg)
}

/// An agent's logo, as its adapter ships it, or the code icon when it ships none.
pub fn agent(svg: Option<&str>) -> &'static str {
    static LOGOS: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let Some(svg) = svg else { return CODE };
    // shortcut: the cache leaks one string per distinct logo, at most one per installed agent.
    LOGOS
        .get_or_init(Default::default)
        .lock()
        .expect("agent logos")
        .entry(svg.to_owned())
        .or_insert_with(|| Box::leak(svg.to_owned().into_boxed_str()))
}

// Actions.
pub const ADD: &str = icon!("<path d='M10 2.75v14.5M2.75 10h14.5'/>");
pub const CLOSE: &str = icon!("<path d='M4.5 4.5l11 11M15.5 4.5l-11 11'/>");
pub const CHECK: &str = icon!("<path d='M3.5 10.5l4.25 4.25 8.75-9.5'/>");
pub const CHECK_CIRCLE: &str =
    icon!("<circle cx='10' cy='10' r='7.5'/><path d='M6.75 10.25l2.25 2.25 4.25-4.75'/>");
pub const COPY: &str = icon!(
    "<rect x='6.5' y='6.5' width='11' height='11' rx='2'/><path d='M13.5 3.75v-.25a1.75 1.75 0 0 0-1.75-1.75h-7.5A1.75 1.75 0 0 0 2.5 3.5v7.5a1.75 1.75 0 0 0 1.75 1.75h.25'/>"
);
pub const SEARCH: &str =
    icon!("<circle cx='8.75' cy='8.75' r='6'/><path d='M13.25 13.25l4.25 4.25'/>");
pub const FILTER: &str = icon!("<path d='M2.75 5.5h14.5M5.5 10h9M8.25 14.5h3.5'/>");
pub const SHARE: &str =
    icon!("<path d='M10 2.5v10M6 6.5l4-4 4 4M4.5 10v5.5a2 2 0 0 0 2 2h7a2 2 0 0 0 2-2V10'/>");
pub const PIN: &str =
    icon!("<path d='M7 2.5h6M8.25 2.5v5L5 11.25h10L11.75 7.5v-5M10 11.25v6.25'/>");
/// Angle brackets: a file of code.
pub const CODE: &str =
    icon!("<path d='M6.25 5.5L2 10l4.25 4.5M13.75 5.5L18 10l-4.25 4.5M11.5 3.5l-3 13'/>");
/// A path that parts in two going right: a new session from a point of this one.
pub const FORK: &str = icon!(
    "<path d='M2.5 10h3.5c2.75 0 3.75-5 7-5h4M6 10c2.75 0 3.75 5 7 5h4M14.75 2.75L17 5l-2.25 2.25M14.75 12.75L17 15l-2.25 2.25'/>"
);
/// A speaker and its sound: read aloud.
pub const SPEAKER: &str = icon!(
    "<path d='M2.5 7.5h3L10 4v12l-4.5-3.5h-3zM13 7.25a3.75 3.75 0 0 1 0 5.5M15.5 4.75a7.25 7.25 0 0 1 0 10.5'/>"
);
pub const STOP: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20'><rect x='2.5' y='2.5' width='15' height='15' rx='3.75' fill='currentColor'/></svg>";
pub const STOP_CIRCLE: &str = icon!(
    "<circle cx='10' cy='10' r='7.5'/><rect x='7.5' y='7.5' width='5' height='5' rx='0.75' fill='currentColor' stroke='none'/>"
);
pub const DOTS_HORIZONTAL: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20' fill='currentColor'><circle cx='4' cy='10' r='1.4'/><circle cx='10' cy='10' r='1.4'/><circle cx='16' cy='10' r='1.4'/></svg>";
pub const DOTS_VERTICAL: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 20 20' fill='currentColor'><circle cx='10' cy='4' r='1.4'/><circle cx='10' cy='10' r='1.4'/><circle cx='10' cy='16' r='1.4'/></svg>";

// Arrows and carets.
pub const ARROW_LEFT: &str = icon!("<path d='M17 10H3M8 5l-5 5 5 5'/>");
pub const ARROW_RIGHT: &str = icon!("<path d='M3 10h14M12 5l5 5-5 5'/>");
pub const ARROW_UP: &str = icon!("<path d='M10 17V3M5 8l5-5 5 5'/>");
pub const ARROW_DOWN: &str = icon!("<path d='M10 3v14M5 12l5 5 5-5'/>");
/// Return: what sends a message.
pub const RETURN: &str =
    icon!("<path d='M16.5 3.5v5.75a2.25 2.25 0 0 1-2.25 2.25H3.5M7.75 7.25L3.5 11.5l4.25 4.25'/>");
pub const ARROW_OUT: &str = icon!(
    "<path d='M11.5 2.75h5.75V8.5M17.25 2.75l-7.5 7.5M15 11.75v3.5a2 2 0 0 1-2 2H4.75a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h3.5'/>"
);
pub const CARET_RIGHT: &str = icon!("<path d='M7.5 4.75L12.75 10 7.5 15.25'/>");
pub const CARET_LEFT: &str = icon!("<path d='M12.5 4.75L7.25 10l5.25 5.25'/>");
pub const CARET_DOWN: &str = icon!("<path d='M4.75 7.5L10 12.75 15.25 7.5'/>");
pub const CARET_UP: &str = icon!("<path d='M4.75 12.5L10 7.25l5.25 5.25'/>");
pub const EXPAND: &str = icon!("<path d='M11.5 3h5.5v5.5M17 3l-6 6M8.5 17H3v-5.5M3 17l6-6'/>");
pub const COLLAPSE: &str = icon!("<path d='M16.5 9h-5.5V3.5M11 9l6-6M3.5 11H9v5.5M9 11l-6 6'/>");

// Places and things.
pub const FOLDER: &str = icon!(
    "<path d='M2.25 5.5a2 2 0 0 1 2-2h3.6l2.15 2.25h5.75a2 2 0 0 1 2 2v7.25a2 2 0 0 1-2 2h-11.5a2 2 0 0 1-2-2z'/>"
);
/// The folder opened: its front leant forward off the back.
pub const FOLDER_OPEN: &str = icon!(
    "<path d='M2.25 14.75V5.5a2 2 0 0 1 2-2h3.6l2.15 2.25h5.25a2 2 0 0 1 2 2v1.25'/><path d='M4.4 9.9a1.75 1.75 0 0 1 1.62-1.15h11.1a.9.9 0 0 1 .84 1.23l-1.9 5.05a1.75 1.75 0 0 1-1.64 1.12h-10.67a1 1 0 0 1-.94-1.35z'/>"
);
/// A sheet with its corner folded: a file.
pub const FILE: &str = icon!(
    "<path d='M11.25 2.5h-5.5a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h8.5a2 2 0 0 0 2-2v-8.5z'/><path d='M11.25 2.5v3.5a1 1 0 0 0 1 1h4'/>"
);
pub const LAPTOP: &str = icon!(
    "<rect x='3.5' y='3.75' width='13' height='9.25' rx='1.75'/><path d='M1.75 16.25h16.5'/>"
);
/// A phone: the session reached from another device.
pub const PHONE: &str =
    icon!("<rect x='5.75' y='2' width='8.5' height='16' rx='2'/><path d='M8.75 15h2.5'/>");
/// The permission modes' marks, as Desktop's menu gives each one:
/// a raised hand to ask, a scroll to plan, a bolt to decide alone.
pub const HAND: &str = icon!(
    "<path d='M6.5 10.5V5a1.25 1.25 0 0 1 2.5 0v4.5M9 9V3.75a1.25 1.25 0 0 1 2.5 0V9M11.5 9V4.75a1.25 1.25 0 0 1 2.5 0v5.5c0 4-2.25 7-6 7-2.5 0-3.75-1.25-5-3.5L1.75 11a1.2 1.2 0 0 1 2-1.25L6.5 13'/>"
);
pub const SCROLL: &str = icon!(
    "<path d='M5 3.25h9.5a1.75 1.75 0 0 1 1.75 1.75v1.25H13.5M5 3.25a1.75 1.75 0 0 0-1.75 1.75v1.25H5M5 3.25v12a1.75 1.75 0 0 0 1.75 1.75h8.5a1.75 1.75 0 0 0 1.75-1.75v-1H8.5v1A1.75 1.75 0 0 1 6.75 17M8 7.5h4.5M8 10.5h4.5'/>"
);
pub const LIGHTNING: &str = icon!("<path d='M11 2.5 4.5 11h5l-1 6.5L15 9h-5z'/>");
pub const BRIEFCASE: &str = icon!(
    "<rect x='2.75' y='6' width='14.5' height='10.5' rx='2'/><path d='M7 6V4.75a1.5 1.5 0 0 1 1.5-1.5h3a1.5 1.5 0 0 1 1.5 1.5V6'/>"
);
pub const CLOUD: &str = icon!(
    "<path d='M5.75 16a3.75 3.75 0 0 1-.5-7.47A5.25 5.25 0 0 1 15.3 7.4 4.3 4.3 0 0 1 14.5 16z'/>"
);
/// Reload: a circle's arc and its arrowhead, at the set's scale.
pub const RELOAD: &str =
    icon!("<path d='M16.25 10a6.25 6.25 0 1 1-1.83-4.42M16.25 3.25v3.5h-3.5'/>");
/// A bin with its lid, for clearing what is done.
pub const TRASH: &str = icon!(
    "<path d='M3.5 5.5h13M8 5.5V3.75h4V5.5M5 5.5l.75 10.25a1.5 1.5 0 0 0 1.5 1.5h5.5a1.5 1.5 0 0 0 1.5-1.5L15 5.5M8.5 8.75v5M11.5 8.75v5'/>"
);
/// A pencil: the annotation editor's pen.
pub const PENCIL: &str = icon!(
    "<path d='M12.75 4.25l3 3M3.75 16.25l.75-3.25L13.5 3.75a1.75 1.75 0 0 1 2.5 2.5L7 15.5z'/>"
);
/// An arc turning back, for undo.
pub const UNDO: &str = icon!("<path d='M7.5 6.5H13a3.75 3.75 0 0 1 0 7.5H8M7.5 3.5l-3 3 3 3'/>");
/// The same arc turning forward, for redo.
pub const REDO: &str = icon!("<path d='M12.5 6.5H7a3.75 3.75 0 0 0 0 7.5h5M12.5 3.5l3 3-3 3'/>");
/// A straight line, for the annotation editor's line.
pub const SHAPE_LINE: &str = icon!("<path d='M4 16L16 4'/>");
/// A line with a head at its end, for the annotation editor's arrow.
pub const SHAPE_ARROW: &str = icon!("<path d='M4 16L15 5M9.5 5H15v5.5'/>");
/// An outlined rectangle, for the annotation editor.
pub const SHAPE_RECTANGLE: &str = icon!("<rect x='3.5' y='5' width='13' height='10' rx='1'/>");
/// An outlined ellipse, for the annotation editor.
pub const SHAPE_ELLIPSE: &str = icon!("<ellipse cx='10' cy='10' rx='6.5' ry='4.5'/>");
/// Two figures, one behind the other: work that runs on its own beside the turn.
pub const AGENTS: &str = icon!(
    "<circle cx='8' cy='7' r='3'/><path d='M2.75 16.5a5.25 5.25 0 0 1 10.5 0M12.5 4.25a3 3 0 0 1 0 5.5M14.75 12a5.25 5.25 0 0 1 2.5 4.5'/>"
);
/// Rewind: Reload's arc turned back, as Desktop's "Rewind to here".
pub const REWIND: &str = icon!("<path d='M3.75 10a6.25 6.25 0 1 0 1.83-4.42M3.75 3.25v3.5h3.5'/>");
pub const GLOBE: &str = icon!(
    "<circle cx='10' cy='10' r='7.5'/><path d='M2.5 10h15M10 2.5c2.1 2.1 3 4.6 3 7.5s-.9 5.4-3 7.5c-2.1-2.1-3-4.6-3-7.5s.9-5.4 3-7.5z'/>"
);
pub const EYE: &str = icon!(
    "<path d='M1.75 10S4.75 4 10 4s8.25 6 8.25 6-3 6-8.25 6S1.75 10 1.75 10z'/><circle cx='10' cy='10' r='2.75'/>"
);
pub const MICROPHONE: &str = icon!(
    "<rect x='7' y='2.25' width='6' height='10' rx='3'/><path d='M4.25 9.25a5.75 5.75 0 0 0 11.5 0M10 15v2.75'/>"
);
pub const PAPERCLIP: &str = icon!(
    "<path d='M16.25 9.25l-6.4 6.4a3.75 3.75 0 0 1-5.3-5.3l6.9-6.9a2.5 2.5 0 0 1 3.55 3.55l-6.9 6.9a1.25 1.25 0 0 1-1.8-1.8l6.1-6.1'/>"
);
pub const PLUG: &str = icon!(
    "<path d='M7.25 2.5v3.75M12.75 2.5v3.75M4.75 6.25h10.5v2.5a5.25 5.25 0 0 1-10.5 0zM10 14v3.5'/>"
);
pub const CONNECTORS: &str = icon!(
    "<rect x='2.5' y='2.5' width='6' height='6' rx='1.5'/><rect x='11.5' y='2.5' width='6' height='6' rx='1.5'/><rect x='2.5' y='11.5' width='6' height='6' rx='1.5'/><path d='M14.5 11.5v6M11.5 14.5h6'/>"
);
pub const CURSOR: &str = icon!("<path d='M4 2.75l11.75 6.5-5 1.5-2.5 5z'/>");
pub const SETTINGS: &str = icon!(
    "<path d='M8.10 3.78 L8.76 2.20 L11.24 2.20 L11.90 3.78 L13.05 4.26 L14.64 3.61 L16.39 5.36 L15.74 6.95 L16.22 8.10 L17.80 8.76 L17.80 11.24 L16.22 11.90 L15.74 13.05 L16.39 14.64 L14.64 16.39 L13.05 15.74 L11.90 16.22 L11.24 17.80 L8.76 17.80 L8.10 16.22 L6.95 15.74 L5.36 16.39 L3.61 14.64 L4.26 13.05 L3.78 11.90 L2.20 11.24 L2.20 8.76 L3.78 8.10 L4.26 6.95 L3.61 5.36 L5.36 3.61 L6.95 4.26Z'/><circle cx='10' cy='10' r='2.75'/>"
);
pub const LIST: &str = icon!(
    "<path d='M7.25 5h10M7.25 10h10M7.25 15h10'/><circle cx='3.25' cy='5' r='0.9' fill='currentColor' stroke='none'/><circle cx='3.25' cy='10' r='0.9' fill='currentColor' stroke='none'/><circle cx='3.25' cy='15' r='0.9' fill='currentColor' stroke='none'/>"
);
pub const MENU: &str = icon!("<path d='M2.75 5h14.5M2.75 10h14.5M2.75 15h9'/>");

/// Settings' theme: the system's, light, dark.
pub const MONITOR: &str = icon!(
    "<rect x='2.25' y='3' width='15.5' height='10.75' rx='2'/><path d='M10 13.75v3M6.75 17h6.5'/>"
);
pub const SUN: &str = icon!(
    "<circle cx='10' cy='10' r='3.4'/><path d='M15.90 10.00L17.60 10.00M14.17 14.17L15.37 15.37M10.00 15.90L10.00 17.60M5.83 14.17L4.63 15.37M4.10 10.00L2.40 10.00M5.83 5.83L4.63 4.63M10.00 4.10L10.00 2.40M14.17 5.83L15.37 4.63'/>"
);
pub const MOON: &str =
    icon!("<path d='M16.75 12.4A7 7 0 0 1 7.6 3.25a7.25 7.25 0 1 0 9.15 9.15z'/>");

// Code.
pub const TERMINAL: &str = icon!("<path d='M3.25 5l5 5-5 5M10.75 15.5h6'/>");
pub const DIFF: &str = icon!(
    "<rect x='2.5' y='2.5' width='15' height='15' rx='2.75'/><path d='M10 5.75v5.5M7.25 8.5h5.5M7.25 14h5.5'/>"
);
pub const GIT_BRANCH: &str = icon!(
    "<circle cx='5.25' cy='4.25' r='1.75'/><circle cx='5.25' cy='15.75' r='1.75'/><circle cx='14.75' cy='4.25' r='1.75'/><path d='M5.25 6v8M14.75 6v1.75a2.5 2.5 0 0 1-2.5 2.5h-4.5a2.5 2.5 0 0 0-2.5 2.5'/>"
);
pub const SLASH: &str = icon!(
    "<rect x='2.5' y='2.5' width='15' height='15' rx='2.75'/><path d='M12.25 5.75l-4.5 8.5'/>"
);
pub const SIDEBAR: &str =
    icon!("<rect x='1.75' y='3.5' width='16.5' height='13' rx='2.75'/><path d='M6 7v6'/>");
pub const PANEL_RIGHT: &str =
    icon!("<rect x='1.75' y='3.5' width='16.5' height='13' rx='2.75'/><path d='M14 7v6'/>");

/// An arrow leaving a door: signing out.
pub const LOGOUT: &str =
    icon!("<path d='M8 3.5H5.5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2H8M12.5 6.5L16 10l-3.5 3.5M16 10H8'/>");
pub const QUESTION: &str = icon!(
    "<circle cx='10' cy='10' r='7.5'/><path d='M7.75 7.75a2.35 2.35 0 0 1 4.55.75c0 1.6-2.3 2-2.3 3.25'/><circle cx='10' cy='14.25' r='0.7' fill='currentColor' stroke='none'/>"
);

/// A session that needs the person: a triangle with a mark as in Desktop.
pub const WARNING: &str = icon!(
    "<path d='M10 2.75L18 16.75H2z'/><path d='M10 8v4'/><circle cx='10' cy='14.25' r='0.7' fill='currentColor' stroke='none'/>"
);

/// An error: a ring with a cross, as `CHECK_CIRCLE` holds a check.
pub const ERROR: &str =
    icon!("<circle cx='10' cy='10' r='7.5'/><path d='M7.5 7.5l5 5M12.5 7.5l-5 5'/>");

/// A waiting task's dashed ring.
pub const TASK_PENDING: &str = icon!("<circle cx='10' cy='10' r='7' stroke-dasharray='2.4 2.3'/>");

/// A session waiting for another's task to end: an hourglass.
pub const HOURGLASS: &str = icon!(
    "<path d='M5 2.75h10M5 17.25h10'/><path d='M6.25 2.75v1.5c0 2.25 3.75 3.5 3.75 5.75s-3.75 3.5-3.75 5.75v1.5M13.75 2.75v1.5c0 2.25-3.75 3.5-3.75 5.75s3.75 3.5 3.75 5.75v1.5'/>"
);

/// The greeting's mark: an eight-ray spark, quill's own.
pub const SPARK: &str =
    icon!("<path d='M10 2.5v15M2.5 10h15M4.7 4.7l10.6 10.6M15.3 4.7L4.7 15.3'/>");

/// A conversation: a chat bubble with its tail, for the "@" list's sessions.
pub const CHAT: &str = icon!(
    "<path d='M4.25 4.25h11.5a1.5 1.5 0 0 1 1.5 1.5v6.5a1.5 1.5 0 0 1-1.5 1.5H9l-3.5 2.75v-2.75h-1.25a1.5 1.5 0 0 1-1.5-1.5v-6.5a1.5 1.5 0 0 1 1.5-1.5z'/>"
);

// Customize.
/// An open book, a line of words on each page: a skill, instructions the agent reads.
pub const SKILL: &str = icon!(
    "<path d='M10 5.5c-1.75-1.5-4.25-2-7-1.75v11.5c2.75-.25 5.25.25 7 1.75 1.75-1.5 4.25-2 7-1.75V3.75c-2.75-.25-5.25.25-7 1.75zM10 5.5v11.5M5.5 8h2.25M5.5 10.75h2.25M12.25 8h2.25M12.25 10.75h2.25'/>"
);
/// A puzzle piece, a knob on its top and its right side: a plugin.
pub const PLUGIN: &str =
    icon!("<path d='M2.5 7h4.25a1.9 1.9 0 1 1 3.5 0h3.25v3a1.9 1.9 0 1 1 0 3.5v3.5h-11z'/>");
/// A wrench, its open jaw up and to the right: a connector's tool.
pub const TOOL: &str = icon!(
    "<path d='M14.25 2.9a4 4 0 0 0-4.9 4.95L3.4 13.8a1.85 1.85 0 0 0 2.6 2.6l5.95-5.95a4 4 0 0 0 4.95-4.9l-2.45 2.45-2.35-.25-.25-2.35z'/>"
);
/// A closed padlock: what someone else set and the person cannot change.
pub const LOCK: &str = icon!(
    "<rect x='4.25' y='8.75' width='11.5' height='8.5' rx='1.75'/><path d='M6.75 8.75V6.25a3.25 3.25 0 0 1 6.5 0v2.5M10 12.25v1.75'/>"
);
/// A ring crossed by one bar: blocked.
pub const BLOCKED: &str = icon!("<circle cx='10' cy='10' r='7.5'/><path d='M4.7 4.7l10.6 10.6'/>");
/// An arrow up off a line: a file sent from this computer.
pub const UPLOAD: &str = icon!("<path d='M10 13.5V3.25M5.75 7.5L10 3.25l4.25 4.25M3.5 16.75h13'/>");
/// An arrow down onto a line: a file saved to this computer.
pub const DOWNLOAD: &str =
    icon!("<path d='M10 3.25V13.5M5.75 9.25L10 13.5l4.25-4.25M3.5 16.75h13'/>");
/// A ring with an "i": more about what a line says.
pub const INFO: &str = icon!(
    "<circle cx='10' cy='10' r='7.5'/><path d='M10 9.25v4.75'/><circle cx='10' cy='6.4' r='0.8' fill='currentColor' stroke='none'/>"
);
/// A compass, its needle a narrow diamond: what there is to find.
pub const DISCOVER: &str =
    icon!("<circle cx='10' cy='10' r='7.5'/><path d='M12.9 7.1l-1.6 4.2-4.2 1.6 1.6-4.2z'/>");
/// A shop with a scalloped awning and a door: a marketplace.
pub const STOREFRONT: &str = icon!(
    "<path d='M3.5 9v6.75a1.25 1.25 0 0 0 1.25 1.25h10.5a1.25 1.25 0 0 0 1.25-1.25V9M2.75 7.5L4 3.25h12l1.25 4.25a2.4 2.4 0 0 1-4.8 0 2.45 2.45 0 0 1-4.9 0 2.4 2.4 0 0 1-4.8 0zM8 17v-4h4v4'/>"
);
/// Three lines getting shorter beside an arrow down: the order of a list.
pub const SORT: &str = icon!("<path d='M3 5h8M3 10h6M3 15h4M15 3.75v12.5M12 13.25l3 3 3-3'/>");

/// Every icon by name, for the preview sheet and its test.
pub const ALL: &[(&str, &str)] = &[
    ("add", ADD),
    ("close", CLOSE),
    ("check", CHECK),
    ("check_circle", CHECK_CIRCLE),
    ("error", ERROR),
    ("copy", COPY),
    ("search", SEARCH),
    ("filter", FILTER),
    ("share", SHARE),
    ("pin", PIN),
    ("chat", CHAT),
    ("code", CODE),
    ("monitor", MONITOR),
    ("sun", SUN),
    ("moon", MOON),
    ("fork", FORK),
    ("speaker", SPEAKER),
    ("stop", STOP),
    ("stop_circle", STOP_CIRCLE),
    ("dots_horizontal", DOTS_HORIZONTAL),
    ("dots_vertical", DOTS_VERTICAL),
    ("arrow_left", ARROW_LEFT),
    ("arrow_right", ARROW_RIGHT),
    ("arrow_up", ARROW_UP),
    ("arrow_down", ARROW_DOWN),
    ("return", RETURN),
    ("arrow_out", ARROW_OUT),
    ("caret_right", CARET_RIGHT),
    ("caret_left", CARET_LEFT),
    ("caret_down", CARET_DOWN),
    ("caret_up", CARET_UP),
    ("expand", EXPAND),
    ("collapse", COLLAPSE),
    ("folder", FOLDER),
    ("folder_open", FOLDER_OPEN),
    ("file", FILE),
    ("laptop", LAPTOP),
    ("rewind", REWIND),
    ("trash", TRASH),
    ("pencil", PENCIL),
    ("undo", UNDO),
    ("redo", REDO),
    ("shape_line", SHAPE_LINE),
    ("shape_arrow", SHAPE_ARROW),
    ("shape_rectangle", SHAPE_RECTANGLE),
    ("shape_ellipse", SHAPE_ELLIPSE),
    ("agents", AGENTS),
    ("briefcase", BRIEFCASE),
    ("hand", HAND),
    ("scroll", SCROLL),
    ("lightning", LIGHTNING),
    ("cloud", CLOUD),
    ("globe", GLOBE),
    ("eye", EYE),
    ("microphone", MICROPHONE),
    ("paperclip", PAPERCLIP),
    ("plug", PLUG),
    ("connectors", CONNECTORS),
    ("cursor", CURSOR),
    ("settings", SETTINGS),
    ("list", LIST),
    ("menu", MENU),
    ("terminal", TERMINAL),
    ("diff", DIFF),
    ("git_branch", GIT_BRANCH),
    ("slash", SLASH),
    ("sidebar", SIDEBAR),
    ("panel_right", PANEL_RIGHT),
    ("question", QUESTION),
    ("task_pending", TASK_PENDING),
    ("hourglass", HOURGLASS),
    ("warning", WARNING),
    ("spark", SPARK),
    ("skill", SKILL),
    ("plugin", PLUGIN),
    ("tool", TOOL),
    ("lock", LOCK),
    ("blocked", BLOCKED),
    ("upload", UPLOAD),
    ("download", DOWNLOAD),
    ("info", INFO),
    ("discover", DISCOVER),
    ("storefront", STOREFRONT),
    ("sort", SORT),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_logo_is_kept_once_and_no_logo_is_the_code_icon() {
        let svg = "<svg xmlns='http://www.w3.org/2000/svg'><path d='M0 0'/></svg>";
        let first = agent(Some(svg));
        assert_eq!(first, svg);
        assert!(std::ptr::eq(first, agent(Some(svg))));
        assert_eq!(agent(None), CODE);
    }

    /// Every line drawing draws at [`STROKE`], none heavier on its own; the filled ones
    /// (dots, the stop square) draw no line at all.
    #[test]
    fn every_line_icon_draws_at_the_set_stroke() {
        let set = format!("stroke-width='{STROKE}'");
        for (name, svg) in ALL {
            assert_eq!(
                svg.matches("stroke-width").count(),
                svg.matches(&set).count(),
                "{name}"
            );
            if svg.contains("stroke='currentColor'") {
                assert!(svg.contains(&set), "{name} draws its lines at {STROKE}");
            }
        }
        assert_eq!(STROKE, 1.25);
    }
}

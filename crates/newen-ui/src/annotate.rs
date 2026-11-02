//! The image annotation editor over a composer image (docs/ui.md §2.6; spec §3 "Editor"): the
//! marks drawn over the picture, the toolbar's tools, undo and redo, closing with marks, and the
//! save the host makes off the window's thread. The text tool needs text painted into the pixels
//! and is not here yet.

use crate::Ui;
use limn::{Color, Key, Size, SketchMark, SketchTool, UiAction};
use std::path::PathBuf;

/// The sketch over the picture, and the toolbar's controls.
pub const SKETCH: &str = "annotate:sketch";
pub const PEN: &str = "annotate:pen";
/// Arms the last shape; its ▾ opens the shapes menu.
pub const SHAPE: &str = "annotate:shape";
pub const SHAPE_PICK: &str = "annotate:shape:";
pub const INK_PICK: &str = "annotate:ink:";
pub const WIDTH_PICK: &str = "annotate:width:";
pub const UNDO: &str = "annotate:undo";
pub const REDO: &str = "annotate:redo";
pub const CLEAR: &str = "annotate:clear";
/// The save is the host's: it reads the marks and answers with `finish_save`.
pub const SAVE: &str = "annotate:save";
/// ✕, Escape and a click on the backdrop; with marks they ask first.
pub const CLOSE: &str = "annotate:close";
pub const BACKDROP: &str = "annotate:backdrop";
pub const DISCARD: &str = "annotate:discard";
pub const KEEP: &str = "annotate:keep";
/// The menus the toolbar opens, which are also the `open_menu` values.
pub const SHAPES_MENU: &str = "annotate:shapes";
pub const INK_MENU: &str = "annotate:ink";
pub const WIDTH_MENU: &str = "annotate:width";

/// The shapes the Shapes menu offers, with their names.
pub const SHAPES: [(&str, SketchTool); 4] = [
    ("Line", SketchTool::Line),
    ("Arrow", SketchTool::Arrow),
    ("Rectangle", SketchTool::Rectangle),
    ("Ellipse", SketchTool::Ellipse),
];
/// The ink colors, as Desktop offers them; Red is the first and the default.
pub const INKS: [(&str, Color); 5] = [
    ("Red", Color::rgb(0xE0, 0x31, 0x31)),
    ("Blue", Color::rgb(0x19, 0x71, 0xC2)),
    ("Green", Color::rgb(0x2F, 0x9E, 0x44)),
    ("Black", Color::rgb(0x1F, 0x1E, 0x1D)),
    ("White", Color::rgb(0xFF, 0xFF, 0xFF)),
];
/// The line widths in logical pixels; Medium is the default.
pub const WIDTHS: [(&str, f32); 4] = [
    ("Thin", 2.0),
    ("Medium", 4.0),
    ("Thick", 8.0),
    ("Extra thick", 12.0),
];
/// The picture's widest, and the room the window keeps above and below it.
const MAX_WIDTH: f32 = 640.0;
const SIDE_ROOM: f32 = 32.0;
const TOP_BOTTOM_ROOM: f32 = 160.0;
/// The notice the save leaves when the copy could not be written.
const SAVE_FAILED: &str = "Couldn't save your annotations. Try again.";

/// The size the picture is drawn at in a window: fitted to its aspect (width over height) within
/// the window's width less 32 px, at most 640 px wide, and its height less 160 px. The host uses
/// the same function for the scale of a saved copy.
pub fn shown(aspect: f32, window: Size) -> Size {
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        1.0
    };
    let width = MAX_WIDTH
        .min(window.width - SIDE_ROOM)
        .min((window.height - TOP_BOTTOM_ROOM) * aspect)
        .max(0.0);
    Size::new(width, width / aspect)
}

/// The shortcuts of the editor: Ctrl+Z undoes, Ctrl+Shift+Z redoes.
pub fn key(ui: &Ui, key: Key, ctrl: bool, shift: bool) -> Option<String> {
    if ui.annotating.is_none() || !ctrl || key != Key::Z {
        return None;
    }
    Some(if shift { REDO } else { UNDO }.to_owned())
}

/// Handles the editor's actions. Returns whether it took the action.
pub fn action(ui: &mut Ui, action: &UiAction) -> bool {
    match action {
        UiAction::Sketched { id, mark } if id == SKETCH => {
            if ui.annotating.is_some() {
                ui.marks.push(mark.clone());
                ui.redo.clear();
            }
            true
        }
        UiAction::Clicked { id } => clicked(ui, id),
        _ => false,
    }
}

fn clicked(ui: &mut Ui, id: &str) -> bool {
    match id {
        PEN => ui.tool = SketchTool::Pen,
        SHAPE => ui.tool = ui.last_shape,
        SHAPES_MENU => toggle_menu(ui, SHAPES_MENU),
        INK_MENU => toggle_menu(ui, INK_MENU),
        WIDTH_MENU => toggle_menu(ui, WIDTH_MENU),
        UNDO => {
            if let Some(mark) = ui.marks.pop() {
                ui.redo.push(mark);
            }
        }
        REDO => {
            if let Some(mark) = ui.redo.pop() {
                ui.marks.push(mark);
            }
        }
        // The marks go to `redo` in reverse, so Redo brings them back in their order.
        CLEAR => {
            while let Some(mark) = ui.marks.pop() {
                ui.redo.push(mark);
            }
        }
        SAVE => {}
        CLOSE | BACKDROP => {
            if !ui.saving {
                if ui.marks.is_empty() {
                    close(ui);
                } else {
                    ui.discarding = true;
                }
            }
        }
        DISCARD => close(ui),
        KEEP => ui.discarding = false,
        _ => {
            // Only its own menus: another menu's dismiss is the composer's to hear.
            if let Some(menu) = id.strip_suffix(":dismiss") {
                if [SHAPES_MENU, INK_MENU, WIDTH_MENU].contains(&menu) {
                    ui.open_menu = None;
                    return true;
                }
                return false;
            }
            if let Some(i) = id
                .strip_prefix(SHAPE_PICK)
                .and_then(|i| i.parse::<usize>().ok())
            {
                let Some((_, shape)) = SHAPES.get(i) else {
                    return true;
                };
                ui.last_shape = *shape;
                ui.tool = *shape;
                ui.open_menu = None;
            } else if let Some(i) = id
                .strip_prefix(INK_PICK)
                .and_then(|i| i.parse::<usize>().ok())
            {
                let Some((_, ink)) = INKS.get(i) else {
                    return true;
                };
                ui.ink = *ink;
                ui.open_menu = None;
            } else if let Some(i) = id
                .strip_prefix(WIDTH_PICK)
                .and_then(|i| i.parse::<usize>().ok())
            {
                let Some((_, width)) = WIDTHS.get(i) else {
                    return true;
                };
                ui.stroke = *width;
                ui.open_menu = None;
            } else {
                return false;
            }
        }
    }
    true
}

fn toggle_menu(ui: &mut Ui, menu: &'static str) {
    ui.open_menu = (ui.open_menu != Some(menu)).then_some(menu);
}

/// Closes the editor and forgets its marks; the ink and the width stay for the app run.
fn close(ui: &mut Ui) {
    ui.annotating = None;
    ui.marks.clear();
    ui.redo.clear();
    ui.discarding = false;
    ui.tool = SketchTool::Pen;
    ui.open_menu = None;
}

/// Starts a save: the picture's path, the marks and the window, for the host to make the copy
/// from. `None` when there is nothing to save or one is running already.
pub fn start_save(ui: &mut Ui) -> Option<(PathBuf, Vec<SketchMark>, Size)> {
    let path = ui.annotating.clone()?;
    if ui.saving || ui.marks.is_empty() {
        return None;
    }
    ui.saving = true;
    Some((path, ui.marks.clone(), ui.window))
}

/// The host's answer to a save: the copy's path, or `None` when it could not be written. The
/// copy takes the picture's place in the composer and the editor closes; otherwise it stays open.
pub fn finish_save(ui: &mut Ui, original: PathBuf, saved: Option<PathBuf>) {
    ui.saving = false;
    match saved {
        Some(copy) => {
            if let Some(slot) = ui.images.iter_mut().find(|path| **path == original) {
                *slot = copy;
            }
            ui.image_notice = None;
            close(ui);
        }
        None => ui.image_notice = Some(SAVE_FAILED.to_owned()),
    }
}

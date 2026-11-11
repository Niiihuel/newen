//! The browser pane's page takes the pointer where it is on top and the keys while it has the
//! focus (limn's `Element::focus_area`, backend.md "Host-fed surfaces"). The window's own
//! input is read here, before limn's, because the page wants what limn does not translate:
//! every key with Alt, the other buttons, the wheel's travel.

use eframe::egui;
use newen_model::{BrowserInput, Modifiers, PointerButton};
use std::time::{Duration, Instant};

/// What the page asked of the window besides input.
#[derive(Debug, PartialEq)]
pub enum Ask {
    Input(BrowserInput),
    /// Copy what is selected in the page; with `cut`, delete it after.
    Copy {
        cut: bool,
    },
    FocusAddress,
    NewTab,
    CloseTab,
    Reload,
    /// Show the next tab, or the one before.
    NextTab(bool),
    ToggleBrowser,
    /// Take the focus, for a press of a button limn leaves alone.
    Focus,
}

/// The pointer heard apart from the window's frames (pointer.rs), where the system has a way:
/// it gives the page its moves and its wheel as they come, and says of each press and
/// release whether it gave the page that one too, so none reaches it twice.
pub trait Apart {
    /// Whether it gave the page this press or release; asked once of each, in order.
    fn gave(&self, button: egui::PointerButton, pressed: bool) -> bool;
    /// Whether the page's moves and wheel are its own to give for now; off, they are the
    /// window's, as without it.
    fn moves(&self) -> bool;
}

/// Clicks in a row: the last press, to count double and triple clicks.
#[derive(Default)]
pub struct Clicks(Option<(Instant, egui::Pos2, egui::PointerButton, u8)>);

impl Clicks {
    /// How many this press makes: one more when it is soon after the last, near it, with the
    /// same button.
    pub fn count(&mut self, pos: egui::Pos2, button: egui::PointerButton) -> u8 {
        let now = Instant::now();
        let clicks = match self.0 {
            Some((at, last, held, clicks))
                if held == button
                    && now.duration_since(at) < Duration::from_millis(500)
                    && last.distance(pos) < 5.0 =>
            {
                clicks % 3 + 1
            }
            _ => 1,
        };
        self.0 = Some((now, pos, button, clicks));
        clicks
    }
}

#[derive(Default)]
pub struct Page {
    /// The button held since a press on the page: its moves and release go to the page even
    /// off it.
    held: Option<egui::PointerButton>,
    clicks: Clicks,
    /// Where the pointer was last seen in the window, kept across frames: a wheel often comes
    /// in a frame of its own, without a move (Hyprland's), and goes where the pointer is.
    pointer: Option<egui::Pos2>,
}

impl Page {
    /// Read this frame's input. `area` is the page's rectangle in the window when it is on
    /// top of what is under the pointer (`None` elsewhere), `focused` whether it has the
    /// keys. With the pointer heard `apart`, the page has its moves and wheel already, and
    /// the presses `apart` says it gave. Returns what to do and the indices of the events
    /// taken from limn.
    pub fn route(
        &mut self,
        events: &[egui::Event],
        over: impl Fn(egui::Pos2) -> Option<egui::Rect>,
        page: Option<egui::Rect>,
        focused: bool,
        apart: Option<&dyn Apart>,
    ) -> (Vec<Ask>, Vec<usize>) {
        let (mut asks, mut taken) = (vec![], vec![]);
        let at = |rect: egui::Rect, pos: egui::Pos2| (pos.x - rect.min.x, pos.y - rect.min.y);
        let moves = apart.is_some_and(|apart| apart.moves());
        for (index, event) in events.iter().enumerate() {
            // Asked of every press and release, the page's or not: each is told once.
            let given = match (event, apart) {
                (
                    egui::Event::PointerButton {
                        button, pressed, ..
                    },
                    Some(apart),
                ) => apart.gave(*button, *pressed),
                _ => false,
            };
            match event {
                egui::Event::PointerMoved(pos) | egui::Event::PointerButton { pos, .. } => {
                    self.pointer = Some(*pos)
                }
                egui::Event::PointerGone => self.pointer = None,
                _ => {}
            }
            match event {
                egui::Event::PointerMoved(_) | egui::Event::PointerGone if moves => {
                    if matches!(event, egui::Event::PointerGone) {
                        self.held = None;
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    let rect = over(*pos).or(page.filter(|_| self.held.is_some()));
                    if let Some(rect) = rect {
                        let (x, y) = at(rect, *pos);
                        asks.push(Ask::Input(BrowserInput::Move { x, y }));
                    }
                }
                egui::Event::PointerGone if self.held.take().is_some() || page.is_some() => {
                    asks.push(Ask::Input(BrowserInput::Leave));
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers,
                } => {
                    let Some(rect) = over(*pos) else {
                        // The page has it, though something else is here by now: not both.
                        if given {
                            self.held = Some(*button);
                            taken.push(index);
                        }
                        continue;
                    };
                    let Some(pressed) = pointer_button(*button) else {
                        continue;
                    };
                    self.held = Some(*button);
                    if !given {
                        let clicks = self.clicks.count(*pos, *button);
                        let (x, y) = at(rect, *pos);
                        asks.push(Ask::Input(BrowserInput::Down {
                            x,
                            y,
                            button: pressed,
                            clicks,
                            keys: keys(*modifiers),
                        }));
                    }
                    // limn focuses the page on a primary press; it would open its own menu on
                    // a secondary one, which the page answers instead.
                    if *button != egui::PointerButton::Primary {
                        asks.push(Ask::Focus);
                        taken.push(index);
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: false,
                    modifiers,
                } if self.held == Some(*button) => {
                    self.held = None;
                    if given {
                        continue;
                    }
                    let (Some(rect), Some(released)) =
                        (over(*pos).or(page), pointer_button(*button))
                    else {
                        continue;
                    };
                    let (x, y) = at(rect, *pos);
                    asks.push(Ask::Input(BrowserInput::Up {
                        x,
                        y,
                        button: released,
                        keys: keys(*modifiers),
                    }));
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } => {
                    // Where the pointer is now, not where the page was last pressed.
                    let Some((rect, pos)) =
                        self.pointer.and_then(|pos| over(pos).map(|r| (r, pos)))
                    else {
                        continue;
                    };
                    // A notch moves 100 px, as Chromium's own wheel does on Linux.
                    let scale = match unit {
                        egui::MouseWheelUnit::Point => 1.0,
                        egui::MouseWheelUnit::Line => 100.0,
                        egui::MouseWheelUnit::Page => rect.height(),
                    };
                    // Shift turns a vertical wheel sideways, as browsers do.
                    let (dx, dy) = if modifiers.shift && delta.x == 0.0 {
                        (-delta.y * scale, 0.0)
                    } else {
                        (-delta.x * scale, -delta.y * scale)
                    };
                    taken.push(index);
                    if moves {
                        continue;
                    }
                    let (x, y) = at(rect, pos);
                    let precise = *unit == egui::MouseWheelUnit::Point;
                    asks.push(Ask::Input(BrowserInput::Wheel {
                        x,
                        y,
                        dx,
                        dy,
                        precise,
                    }));
                }
                _ if !focused => {}
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    taken.push(index);
                    if let Some(ask) = shortcut(*key, *modifiers) {
                        asks.push(ask);
                    } else if let Some((name, code, key_code)) = dom_key(*key, modifiers.shift) {
                        let text = if *key == egui::Key::Enter { "\r" } else { "" };
                        asks.push(Ask::Input(BrowserInput::Key {
                            key: name,
                            code,
                            key_code,
                            text: text.into(),
                            keys: keys(*modifiers),
                        }));
                    }
                }
                egui::Event::Key { pressed: false, .. } => taken.push(index),
                // Enter types its own line break above.
                egui::Event::Text(text) if matches!(text.as_str(), "\n" | "\r" | "\r\n") => {
                    taken.push(index)
                }
                egui::Event::Text(text) | egui::Event::Paste(text) => {
                    taken.push(index);
                    asks.push(Ask::Input(BrowserInput::Text(text.clone())));
                }
                egui::Event::Copy => {
                    taken.push(index);
                    asks.push(Ask::Copy { cut: false });
                }
                egui::Event::Cut => {
                    taken.push(index);
                    asks.push(Ask::Copy { cut: true });
                }
                egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    taken.push(index);
                    asks.push(Ask::Input(BrowserInput::Composition(text.clone())));
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    taken.push(index);
                    asks.push(Ask::Input(BrowserInput::Text(text.clone())));
                }
                _ => {}
            }
        }
        (asks, taken)
    }
}

pub fn pointer_button(button: egui::PointerButton) -> Option<PointerButton> {
    match button {
        egui::PointerButton::Primary => Some(PointerButton::Left),
        egui::PointerButton::Secondary => Some(PointerButton::Right),
        egui::PointerButton::Middle => Some(PointerButton::Middle),
        _ => None,
    }
}

pub fn keys(modifiers: egui::Modifiers) -> Modifiers {
    Modifiers {
        shift: modifiers.shift,
        ctrl: modifiers.ctrl || modifiers.command,
        alt: modifiers.alt,
    }
}

/// The browser's own keys while the page has the focus, Desktop's.
pub fn shortcut(key: egui::Key, modifiers: egui::Modifiers) -> Option<Ask> {
    use egui::Key;
    let ctrl = modifiers.ctrl || modifiers.command;
    Some(match (key, ctrl, modifiers.shift) {
        (Key::L, true, false) | (Key::F6, false, _) => Ask::FocusAddress,
        (Key::T, true, false) => Ask::NewTab,
        (Key::W, true, false) => Ask::CloseTab,
        (Key::R, true, false) | (Key::F5, false, false) => Ask::Reload,
        (Key::Tab, true, shift) => Ask::NextTab(!shift),
        (Key::B, true, true) => Ask::ToggleBrowser,
        _ => return None,
    })
}

/// A key as the DOM names it: its `key`, its `code`, and the Windows key code Chromium edits
/// by. Characters arrive again as text, which the page types.
fn dom_key(key: egui::Key, shift: bool) -> Option<(String, String, u32)> {
    use egui::Key::*;
    let named =
        |name: &str, code: &str, key_code: u32| Some((name.to_owned(), code.to_owned(), key_code));
    let name = key.name();
    match key {
        A | B | C | D | E | F | G | H | I | J | K | L | M | N | O | P | Q | R | S | T | U | V
        | W | X | Y | Z => {
            let letter = if shift {
                name.to_owned()
            } else {
                name.to_lowercase()
            };
            Some((letter, format!("Key{name}"), u32::from(name.as_bytes()[0])))
        }
        Num0 | Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 => {
            let digit = name.chars().last()?;
            Some((digit.to_string(), format!("Digit{digit}"), u32::from(digit)))
        }
        Enter => named("Enter", "Enter", 13),
        Tab => named("Tab", "Tab", 9),
        Backspace => named("Backspace", "Backspace", 8),
        Escape => named("Escape", "Escape", 27),
        Space => named(" ", "Space", 32),
        Delete => named("Delete", "Delete", 46),
        Insert => named("Insert", "Insert", 45),
        Home => named("Home", "Home", 36),
        End => named("End", "End", 35),
        PageUp => named("PageUp", "PageUp", 33),
        PageDown => named("PageDown", "PageDown", 34),
        ArrowLeft => named("ArrowLeft", "ArrowLeft", 37),
        ArrowUp => named("ArrowUp", "ArrowUp", 38),
        ArrowRight => named("ArrowRight", "ArrowRight", 39),
        ArrowDown => named("ArrowDown", "ArrowDown", 40),
        F1 => named("F1", "F1", 112),
        F2 => named("F2", "F2", 113),
        F3 => named("F3", "F3", 114),
        F4 => named("F4", "F4", 115),
        F7 => named("F7", "F7", 118),
        F8 => named("F8", "F8", 119),
        F9 => named("F9", "F9", 120),
        F10 => named("F10", "F10", 121),
        F11 => named("F11", "F11", 122),
        F12 => named("F12", "F12", 123),
        Minus => named("-", "Minus", 189),
        Equals => named("=", "Equal", 187),
        Comma => named(",", "Comma", 188),
        Period => named(".", "Period", 190),
        Slash => named("/", "Slash", 191),
        Backslash => named("\\", "Backslash", 220),
        Semicolon => named(";", "Semicolon", 186),
        Quote => named("'", "Quote", 222),
        OpenBracket => named("[", "BracketLeft", 219),
        CloseBracket => named("]", "BracketRight", 221),
        Backtick => named("`", "Backquote", 192),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(400.0, 300.0))
    }

    #[test]
    fn a_wheel_in_a_frame_of_its_own_goes_where_the_pointer_is_not_where_the_page_was_pressed() {
        let mut page = Page::default();
        let over = |pos: egui::Pos2| rect().contains(pos).then(rect);
        let press = |pressed| egui::Event::PointerButton {
            pos: egui::pos2(150.0, 70.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let wheel = || {
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
                source: egui::MouseWheelSource::Wheel,
            }]
        };
        page.route(&[press(true), press(false)], over, Some(rect()), true, None);
        // Over the page, a wheel alone scrolls it.
        let (asks, taken) = page.route(&wheel(), over, Some(rect()), true, None);
        assert_eq!((asks.len(), taken), (1, vec![0]));
        // Moved off it in one frame, the next frame's wheel is limn's.
        page.route(
            &[egui::Event::PointerMoved(egui::pos2(700.0, 400.0))],
            over,
            Some(rect()),
            true,
            None,
        );
        let (asks, taken) = page.route(&wheel(), over, Some(rect()), true, None);
        assert!(asks.is_empty() && taken.is_empty(), "{asks:?}");
    }

    /// The pointer heard apart: it gave the page the presses listed, in order.
    struct Gave(std::cell::RefCell<Vec<bool>>);

    impl Apart for Gave {
        fn gave(&self, _: egui::PointerButton, _: bool) -> bool {
            self.0.borrow_mut().remove(0)
        }
        // Until it has no more presses to tell of.
        fn moves(&self) -> bool {
            !self.0.borrow().is_empty()
        }
    }

    #[test]
    fn with_the_pointer_heard_apart_the_window_gives_the_page_only_what_that_did_not() {
        let mut page = Page::default();
        let over = |pos: egui::Pos2| rect().contains(pos).then(rect);
        let press = |button, pressed| egui::Event::PointerButton {
            pos: egui::pos2(150.0, 70.0),
            button,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let (left, right) = (egui::PointerButton::Primary, egui::PointerButton::Secondary);
        let events = vec![
            egui::Event::PointerMoved(egui::pos2(150.0, 70.0)),
            press(left, true),
            press(left, false),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
                source: egui::MouseWheelSource::Wheel,
            },
            press(right, true),
            press(right, false),
            // One it was not given (a touch, which only the window hears): the window's to give.
            press(left, true),
            egui::Event::PointerGone,
        ];
        let gave = Gave(vec![true, true, true, true, false].into());
        let (asks, taken) = page.route(&events, over, Some(rect()), false, Some(&gave));
        // The moves, the wheel and the presses it gave are not given again. The wheel is still
        // kept from limn, and a right press still takes the focus and is kept from limn's menu.
        assert_eq!(
            asks,
            [
                Ask::Focus,
                Ask::Input(BrowserInput::Down {
                    x: 50.0,
                    y: 20.0,
                    button: PointerButton::Left,
                    clicks: 1,
                    keys: Modifiers::default()
                }),
            ]
        );
        assert_eq!(taken, [3, 4]);
        assert!(
            gave.0.borrow().is_empty(),
            "each press and release was asked about"
        );
        // Given a press the window finds something else under: the page has it, limn does not.
        let gave = Gave(vec![true].into());
        let nothing = |_: egui::Pos2| None;
        let (asks, taken) = page.route(
            &[press(left, true)],
            nothing,
            Some(rect()),
            false,
            Some(&gave),
        );
        assert!(asks.is_empty());
        assert_eq!(taken, [0]);
        // While it does not give the moves (it and the window do not agree on what is the
        // page's), they are the window's to give, as without it.
        let moved = [egui::Event::PointerMoved(egui::pos2(150.0, 70.0))];
        let (asks, _) = page.route(&moved, over, Some(rect()), false, Some(&gave));
        assert_eq!(asks, [Ask::Input(BrowserInput::Move { x: 50.0, y: 20.0 })]);
    }

    #[test]
    fn the_page_takes_the_pointer_where_it_is_and_keys_while_focused() {
        let mut page = Page::default();
        let over = |pos: egui::Pos2| rect().contains(pos).then(rect);
        let press = |pressed| egui::Event::PointerButton {
            pos: egui::pos2(150.0, 70.0),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let events = vec![
            egui::Event::PointerMoved(egui::pos2(150.0, 70.0)),
            press(true),
            // A drag that leaves the page still moves in it.
            egui::Event::PointerMoved(egui::pos2(600.0, 70.0)),
            press(false),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
                source: egui::MouseWheelSource::Wheel,
            },
            egui::Event::Text("hi".into()),
        ];
        let (asks, taken) = page.route(&events, over, Some(rect()), false, None);
        assert_eq!(
            asks[..4],
            [
                Ask::Input(BrowserInput::Move { x: 50.0, y: 20.0 }),
                Ask::Input(BrowserInput::Down {
                    x: 50.0,
                    y: 20.0,
                    button: PointerButton::Left,
                    clicks: 1,
                    keys: Modifiers::default()
                }),
                Ask::Input(BrowserInput::Move { x: 500.0, y: 20.0 }),
                Ask::Input(BrowserInput::Up {
                    x: 50.0,
                    y: 20.0,
                    button: PointerButton::Left,
                    keys: Modifiers::default()
                }),
            ]
        );
        // The wheel scrolls the page down a notch and limn does not scroll as well; text is
        // limn's while the page has no focus.
        assert_eq!(
            asks[4],
            Ask::Input(BrowserInput::Wheel {
                x: 50.0,
                y: 20.0,
                dx: 0.0,
                dy: 100.0,
                precise: false
            })
        );
        assert_eq!(taken, [4]);
        let keys = vec![
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            },
            egui::Event::Text("\n".into()),
            egui::Event::Key {
                key: egui::Key::L,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            },
            egui::Event::Copy,
        ];
        let (asks, taken) = page.route(&keys, over, Some(rect()), true, None);
        assert!(
            matches!(&asks[0], Ask::Input(BrowserInput::Key { key, text, .. }) if key == "Enter" && text == "\r")
        );
        assert_eq!(asks[1..], [Ask::FocusAddress, Ask::Copy { cut: false }]);
        assert_eq!(taken, [0, 1, 2, 3]);
    }
}

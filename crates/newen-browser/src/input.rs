//! The person's pointer and keys, as Chromium's `Input` domain takes them.

use newen_model::{BrowserInput, Modifiers, PointerButton};
use serde_json::{json, Value};

/// CDP's modifier bits: Alt 1, Ctrl 2, Meta 4, Shift 8.
pub fn modifiers(keys: Modifiers) -> u8 {
    u8::from(keys.alt) | u8::from(keys.ctrl) << 1 | u8::from(keys.shift) << 3
}

fn button(button: PointerButton) -> (&'static str, u8) {
    match button {
        PointerButton::Left => ("left", 1),
        PointerButton::Right => ("right", 2),
        PointerButton::Middle => ("middle", 4),
    }
}

/// The commands one input becomes, in order. `held` is the button down since a press, so a
/// move while it is held drags (a text selection, a slider).
pub fn commands(
    input: &BrowserInput,
    held: &mut Option<PointerButton>,
) -> Vec<(&'static str, Value)> {
    let mouse = |kind: &str, x: f32, y: f32, extra: Value| {
        let mut event = json!({ "type": kind, "x": x, "y": y });
        if let (Value::Object(event), Value::Object(extra)) = (&mut event, extra) {
            event.extend(extra);
        }
        ("Input.dispatchMouseEvent", event)
    };
    match input {
        BrowserInput::Move { x, y } => {
            let (name, bits) = held.map(button).unwrap_or(("none", 0));
            vec![mouse(
                "mouseMoved",
                *x,
                *y,
                json!({ "button": name, "buttons": bits }),
            )]
        }
        BrowserInput::Leave => vec![],
        BrowserInput::Down {
            x,
            y,
            button: pressed,
            clicks,
            keys,
        } => {
            *held = Some(*pressed);
            let (name, bits) = button(*pressed);
            vec![mouse(
                "mousePressed",
                *x,
                *y,
                json!({ "button": name, "buttons": bits, "clickCount": clicks.max(&1),
                        "modifiers": modifiers(*keys) }),
            )]
        }
        BrowserInput::Up {
            x,
            y,
            button: released,
            keys,
        } => {
            *held = None;
            let (name, _) = button(*released);
            vec![mouse(
                "mouseReleased",
                *x,
                *y,
                json!({ "button": name, "buttons": 0, "clickCount": 1,
                        "modifiers": modifiers(*keys) }),
            )]
        }
        BrowserInput::Wheel { x, y, dx, dy, .. } => {
            vec![mouse(
                "mouseWheel",
                *x,
                *y,
                json!({ "deltaX": dx, "deltaY": dy }),
            )]
        }
        BrowserInput::Key {
            key,
            code,
            key_code,
            text,
            keys,
        } => {
            let bits = modifiers(*keys);
            // A key that types sends its text with the press; Ctrl or Alt chords type nothing.
            let typed = !text.is_empty() && !keys.ctrl && !keys.alt;
            let mut down = json!({
                "type": if typed { "keyDown" } else { "rawKeyDown" },
                "key": key, "code": code, "modifiers": bits,
                "windowsVirtualKeyCode": key_code, "nativeVirtualKeyCode": key_code,
            });
            if typed {
                down["text"] = json!(text);
                down["unmodifiedText"] = json!(text);
            }
            let up = json!({
                "type": "keyUp", "key": key, "code": code, "modifiers": bits,
                "windowsVirtualKeyCode": key_code, "nativeVirtualKeyCode": key_code,
            });
            vec![
                ("Input.dispatchKeyEvent", down),
                ("Input.dispatchKeyEvent", up),
            ]
        }
        BrowserInput::Text(text) => vec![("Input.insertText", json!({ "text": text }))],
        BrowserInput::Composition(text) => {
            let end = text.encode_utf16().count();
            vec![(
                "Input.imeSetComposition",
                json!({ "text": text, "selectionStart": end, "selectionEnd": end }),
            )]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_moves_with_its_button_held_and_keys_type_only_without_chords() {
        let mut held = None;
        let down = BrowserInput::Down {
            x: 10.0,
            y: 20.0,
            button: PointerButton::Left,
            clicks: 2,
            keys: Modifiers {
                shift: true,
                ..Default::default()
            },
        };
        let pressed = commands(&down, &mut held);
        assert_eq!(pressed[0].1["clickCount"], 2);
        assert_eq!(pressed[0].1["modifiers"], 8);
        let moved = commands(&BrowserInput::Move { x: 30.0, y: 20.0 }, &mut held);
        assert_eq!(
            (
                moved[0].1["button"].as_str(),
                moved[0].1["buttons"].as_u64()
            ),
            (Some("left"), Some(1))
        );
        commands(
            &BrowserInput::Up {
                x: 30.0,
                y: 20.0,
                button: PointerButton::Left,
                keys: Modifiers::default(),
            },
            &mut held,
        );
        assert_eq!(
            commands(&BrowserInput::Move { x: 31.0, y: 20.0 }, &mut held)[0].1["button"],
            "none"
        );
        let enter = |ctrl| BrowserInput::Key {
            key: "Enter".into(),
            code: "Enter".into(),
            key_code: 13,
            text: "\r".into(),
            keys: Modifiers {
                ctrl,
                ..Default::default()
            },
        };
        let typed = commands(&enter(false), &mut held);
        assert_eq!(
            (typed[0].1["type"].as_str(), typed[0].1["text"].as_str()),
            (Some("keyDown"), Some("\r"))
        );
        assert_eq!(typed[1].1["type"], "keyUp");
        let chord = commands(&enter(true), &mut held);
        assert_eq!(chord[0].1["type"], "rawKeyDown");
        assert!(chord[0].1.get("text").is_none());
    }
}

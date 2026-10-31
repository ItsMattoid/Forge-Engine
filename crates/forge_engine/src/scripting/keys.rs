//! The keys Forge reads, by the name they have in `Enum.KeyCode` (the enum itself, with all of Roblox's keys and their
//! numbers, is declared by `UserInputService`), and the mouse buttons by their `Enum.UserInputType` name.

use std::{collections::HashMap, sync::LazyLock};

use bevy::input::{keyboard::KeyCode, mouse::MouseButton};

pub static KEYS: &[(KeyCode, &str, i32)] = &[
    (KeyCode::KeyA, "A", 97), (KeyCode::KeyB, "B", 98), (KeyCode::KeyC, "C", 99), (KeyCode::KeyD, "D", 100), (KeyCode::KeyE, "E", 101),
    (KeyCode::KeyF, "F", 102), (KeyCode::KeyG, "G", 103), (KeyCode::KeyH, "H", 104), (KeyCode::KeyI, "I", 105), (KeyCode::KeyJ, "J", 106),
    (KeyCode::KeyK, "K", 107), (KeyCode::KeyL, "L", 108), (KeyCode::KeyM, "M", 109), (KeyCode::KeyN, "N", 110), (KeyCode::KeyO, "O", 111),
    (KeyCode::KeyP, "P", 112), (KeyCode::KeyQ, "Q", 113), (KeyCode::KeyR, "R", 114), (KeyCode::KeyS, "S", 115), (KeyCode::KeyT, "T", 116),
    (KeyCode::KeyU, "U", 117), (KeyCode::KeyV, "V", 118), (KeyCode::KeyW, "W", 119), (KeyCode::KeyX, "X", 120), (KeyCode::KeyY, "Y", 121),
    (KeyCode::KeyZ, "Z", 122),
    (KeyCode::Digit0, "Zero", 48), (KeyCode::Digit1, "One", 49), (KeyCode::Digit2, "Two", 50), (KeyCode::Digit3, "Three", 51),
    (KeyCode::Digit4, "Four", 52), (KeyCode::Digit5, "Five", 53), (KeyCode::Digit6, "Six", 54), (KeyCode::Digit7, "Seven", 55),
    (KeyCode::Digit8, "Eight", 56), (KeyCode::Digit9, "Nine", 57),
    (KeyCode::Space, "Space", 32), (KeyCode::Enter, "Return", 13), (KeyCode::Escape, "Escape", 27), (KeyCode::Tab, "Tab", 9),
    (KeyCode::Backspace, "Backspace", 8), (KeyCode::Delete, "Delete", 127), (KeyCode::Pause, "Pause", 19),
    (KeyCode::ShiftLeft, "LeftShift", 304), (KeyCode::ShiftRight, "RightShift", 303),
    (KeyCode::ControlLeft, "LeftControl", 306), (KeyCode::ControlRight, "RightControl", 305),
    (KeyCode::AltLeft, "LeftAlt", 308), (KeyCode::AltRight, "RightAlt", 307),
    (KeyCode::SuperLeft, "LeftSuper", 311), (KeyCode::SuperRight, "RightSuper", 312),
    (KeyCode::ArrowUp, "Up", 273), (KeyCode::ArrowDown, "Down", 274), (KeyCode::ArrowLeft, "Left", 276), (KeyCode::ArrowRight, "Right", 275),
    (KeyCode::Insert, "Insert", 277), (KeyCode::Home, "Home", 278), (KeyCode::End, "End", 279), (KeyCode::PageUp, "PageUp", 280), (KeyCode::PageDown, "PageDown", 281),
    (KeyCode::CapsLock, "CapsLock", 301), (KeyCode::NumLock, "NumLock", 300), (KeyCode::ScrollLock, "ScrollLock", 302),
    (KeyCode::Comma, "Comma", 44), (KeyCode::Period, "Period", 46), (KeyCode::Slash, "Slash", 47), (KeyCode::Semicolon, "Semicolon", 59),
    (KeyCode::Quote, "Quote", 39), (KeyCode::BracketLeft, "LeftBracket", 91), (KeyCode::BracketRight, "RightBracket", 93),
    (KeyCode::Backslash, "BackSlash", 92), (KeyCode::Minus, "Minus", 45), (KeyCode::Equal, "Equals", 61), (KeyCode::Backquote, "Backquote", 96),
    (KeyCode::Numpad0, "KeypadZero", 256), (KeyCode::Numpad1, "KeypadOne", 257), (KeyCode::Numpad2, "KeypadTwo", 258), (KeyCode::Numpad3, "KeypadThree", 259),
    (KeyCode::Numpad4, "KeypadFour", 260), (KeyCode::Numpad5, "KeypadFive", 261), (KeyCode::Numpad6, "KeypadSix", 262), (KeyCode::Numpad7, "KeypadSeven", 263),
    (KeyCode::Numpad8, "KeypadEight", 264), (KeyCode::Numpad9, "KeypadNine", 265), (KeyCode::NumpadDecimal, "KeypadPeriod", 266),
    (KeyCode::NumpadDivide, "KeypadDivide", 267), (KeyCode::NumpadMultiply, "KeypadMultiply", 268), (KeyCode::NumpadSubtract, "KeypadMinus", 269),
    (KeyCode::NumpadAdd, "KeypadPlus", 270), (KeyCode::NumpadEnter, "KeypadEnter", 271),
    (KeyCode::F1, "F1", 282), (KeyCode::F2, "F2", 283), (KeyCode::F3, "F3", 284), (KeyCode::F4, "F4", 285), (KeyCode::F5, "F5", 286),
    (KeyCode::F6, "F6", 287), (KeyCode::F7, "F7", 288), (KeyCode::F8, "F8", 289), (KeyCode::F9, "F9", 290), (KeyCode::F10, "F10", 291),
    (KeyCode::F11, "F11", 292), (KeyCode::F12, "F12", 293),
];

pub static BY_NAME: LazyLock<HashMap<&'static str, KeyCode>> = LazyLock::new(|| KEYS.iter().map(|(k, n, _)| (*n, *k)).collect());
pub static BY_CODE: LazyLock<HashMap<KeyCode, &'static str>> = LazyLock::new(|| KEYS.iter().map(|(k, n, _)| (*k, *n)).collect());

pub const MOUSE: [(&str, MouseButton); 3] =
    [("MouseButton1", MouseButton::Left), ("MouseButton2", MouseButton::Right), ("MouseButton3", MouseButton::Middle)];

pub fn mouse_by_name(name: &str) -> Option<MouseButton> {
    MOUSE.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_and_numbers_are_unique() {
        let mut names: Vec<&str> = KEYS.iter().map(|k| k.1).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total);
        let mut numbers: Vec<i32> = KEYS.iter().map(|k| k.2).collect();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), total);
        assert_eq!(BY_NAME["W"], KeyCode::KeyW);
        assert_eq!(BY_CODE[&KeyCode::Space], "Space");
    }

    /// Every key Forge reads is a key of `Enum.KeyCode`, with the number the registry gives it.
    #[test]
    fn every_key_is_in_the_enum_with_roblox_s_number() {
        let e = crate::core::registry().enums().find(|e| e.name == "KeyCode").expect("the KeyCode enum");
        for (_, name, number) in KEYS {
            let at = e.values.iter().position(|v| v == name).unwrap_or_else(|| panic!("{name} is not in Enum.KeyCode"));
            assert_eq!(e.number(at), *number, "the number of {name}");
        }
    }
}

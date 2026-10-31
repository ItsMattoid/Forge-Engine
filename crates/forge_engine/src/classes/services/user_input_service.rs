//! UserInputService: keyboard and mouse.
//!
//! The mouse position and movement are `Vector2`s, as in Roblox. Keys and buttons arrive as `InputBegan` / `InputEnded` /
//! `InputChanged` with an InputObject; `IsKeyDown` and `GetKeysPressed` ask what is held. The same service listens for
//! the clicks and key presses that `ClickDetector` and `ProximityPrompt` need (`user_input_service.luau`). There is
//! no touch screen, gamepad or gyroscope: the `...Enabled` properties say so.

use crate::core::*;

type R = Result<ScriptValue, String>;

fn is_key_down(c: &mut MethodCall) -> R {
    let key = c.string(0)?;
    Ok(ScriptValue::Bool(c.host.key_down(&key)))
}

fn is_mouse_button_pressed(c: &mut MethodCall) -> R {
    let button = c.string(0)?;
    Ok(ScriptValue::Bool(c.host.mouse_button_down(&button)))
}

fn get_mouse_delta(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Vec2(c.host.mouse_delta()))
}

fn get_mouse_location(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Vec2(c.host.mouse_position()))
}

static PROPS: &[PropMeta] = &[
    PropMeta::enumeration("MouseBehavior", "MouseBehavior", "Default").runtime().hidden().desc("LockCenter captures the mouse for looking around."),
    PropMeta::boolean("MouseIconEnabled", true).runtime().hidden(),
    PropMeta::number("MouseDeltaSensitivity", 1.0).hidden().desc("How much GetMouseDelta is scaled (kept for scripts)."),
    PropMeta::boolean("KeyboardEnabled", true).runtime().hidden().readonly(),
    PropMeta::boolean("MouseEnabled", true).runtime().hidden().readonly(),
    PropMeta::boolean("TouchEnabled", false).runtime().hidden().readonly(),
    PropMeta::boolean("GamepadEnabled", false).runtime().hidden().readonly(),
    PropMeta::boolean("AccelerometerEnabled", false).runtime().hidden().readonly(),
    PropMeta::boolean("GyroscopeEnabled", false).runtime().hidden().readonly(),
    PropMeta::boolean("VREnabled", false).runtime().hidden().readonly(),
    PropMeta::boolean("ModalEnabled", false).hidden(),
    PropMeta::boolean("OnScreenKeyboardVisible", false).runtime().hidden().readonly(),
];

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("IsKeyDown", is_key_down).desc("Whether a key (Enum.KeyCode) is held."),
    MethodDesc::new("IsMouseButtonPressed", is_mouse_button_pressed).desc("Whether a mouse button (Enum.UserInputType.MouseButton1...) is held."),
    MethodDesc::new("GetMouseDelta", get_mouse_delta).desc("How far the mouse moved this frame, as a Vector2."),
    MethodDesc::new("GetMouseLocation", get_mouse_location).desc("The mouse position in the window, as a Vector2."),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("InputBegan").desc("A key or button was pressed; passes (input, gameProcessed)."),
    SignalDesc::new("InputEnded").desc("A key or button was released."),
    SignalDesc::new("InputChanged").desc("The mouse moved or the wheel turned."),
    SignalDesc::new("JumpRequest").desc("The player pressed the jump key (Space)."),
    SignalDesc::new("LastInputTypeChanged").desc("The kind of input in use changed (never fired: there is only a keyboard and mouse)."),
    SignalDesc::new("WindowFocused").desc("The game window gained focus (never fired)."),
    SignalDesc::new("WindowFocusReleased").desc("The game window lost focus (never fired)."),
    SignalDesc::new("TextBoxFocused").desc("A text box was selected (never fired: there are none)."),
    SignalDesc::new("TextBoxFocusReleased").desc("A text box was left (never fired)."),
];

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered(
        "UserInputType",
        &["MouseButton1", "MouseButton2", "MouseButton3", "MouseWheel", "MouseMovement", "Touch", "Keyboard", "Focus", "Accelerometer", "Gyro", "Gamepad1", "Gamepad2", "Gamepad3", "Gamepad4", "TextInput", "None"],
        &[0, 1, 2, 3, 4, 7, 8, 9, 10, 11, 12, 13, 14, 15, 20, 22],
    ),
    // Roblox's keys, with its numbers (the keyboard keys of a US layout; the ones Forge can read are listed in
    // `scripting/keys.rs`).
    EnumDesc::numbered(
        "KeyCode",
        &[
            "Unknown", "Backspace", "Tab", "Clear", "Return", "Pause", "Escape", "Space", "QuotedDouble", "Hash", "Dollar", "Percent", "Ampersand", "Quote", "LeftParenthesis", "RightParenthesis", "Asterisk", "Plus",
            "Comma", "Minus", "Period", "Slash", "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Colon", "Semicolon", "LessThan", "Equals", "GreaterThan", "Question", "At",
            "LeftBracket", "BackSlash", "RightBracket", "Caret", "Underscore", "Backquote", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y",
            "Z", "LeftCurly", "Pipe", "RightCurly", "Tilde", "Delete", "KeypadZero", "KeypadOne", "KeypadTwo", "KeypadThree", "KeypadFour", "KeypadFive", "KeypadSix", "KeypadSeven", "KeypadEight",
            "KeypadNine", "KeypadPeriod", "KeypadDivide", "KeypadMultiply", "KeypadMinus", "KeypadPlus", "KeypadEnter", "KeypadEquals", "Up", "Down", "Right", "Left", "Insert", "Home", "End", "PageUp",
            "PageDown", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "NumLock", "CapsLock", "ScrollLock", "RightShift", "LeftShift", "RightControl", "LeftControl",
            "RightAlt", "LeftAlt", "RightMeta", "LeftMeta", "LeftSuper", "RightSuper", "Mode", "Compose", "Help", "Print", "SysReq", "Break", "Menu", "Power", "Euro", "Undo",
        ],
        &[
            0, 8, 9, 12, 13, 19, 27, 32, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101,
            102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 256, 257, 258, 259, 260, 261, 262, 263, 264, 265, 266, 267, 268,
            269, 270, 271, 272, 273, 274, 275, 276, 277, 278, 279, 280, 281, 282, 283, 284, 285, 286, 287, 288, 289, 290, 291, 292, 293, 294, 295, 296, 300, 301, 302, 303, 304, 305, 306, 307, 308, 309, 310,
            311, 312, 313, 314, 315, 316, 317, 318, 319, 320, 321, 322,
        ],
    ),    EnumDesc::numbered("UserInputState", &["Begin", "Change", "End", "Cancel", "None"], &[0, 1, 2, 3, 4]),
    EnumDesc::numbered("MouseBehavior", &["Default", "LockCenter", "LockCurrentPosition"], &[0, 1, 2]),
];

pub static CLASS: ClassDesc = ClassDesc::new("UserInputService", Some("Instance"))
    .describe("Keyboard and mouse input.")
    .service(11)
    .hidden_in_explorer()
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS)
    .luau(include_str!("user_input_service.luau"));
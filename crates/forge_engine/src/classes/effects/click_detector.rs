//! ClickDetector: makes the part (or the model) it is in clickable. A click of the left mouse button on it, from near
//! enough (`MaxActivationDistance` studs from the player's character), fires `MouseClick(player)`; the right button
//! fires `RightMouseClick`. (The clicking is handled by `UserInputService`.) Hover events are not fired.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::number("MaxActivationDistance", 32.0).group("Click").order(0).drag(0.5).clamp(0.0, 1.0e5).desc("How near the player's character must be, in studs."),
    PropMeta::asset("CursorIcon", "png").group("Click").order(1).hidden().desc("The cursor while pointing at the part (not used)."),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("MouseClick").desc("The part was clicked with the left button; passes the player."),
    SignalDesc::new("RightMouseClick").desc("The part was clicked with the right button; passes the player."),
    SignalDesc::new("MouseHoverEnter").desc("The cursor moved onto the part (never fired)."),
    SignalDesc::new("MouseHoverLeave").desc("The cursor moved off the part (never fired)."),
];

pub static CLASS: ClassDesc = ClassDesc::new("ClickDetector", Some("Instance"))
    .describe("Makes a part clickable.")
    .category("Effects")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "WorldObject", hint: "Select a part or a model first." })
    .groups(&["Click"])
    .props(PROPS)
    .signals(SIGNALS);
//! GuiService: facts about the on-screen interface. Forge has no GUI classes, so there is no inset, no selection and
//! no menu; the answers are those of a screen with nothing on it.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("MenuIsOpen", false).runtime().hidden().readonly(),
    PropMeta::boolean("IsWindows", true).runtime().hidden().readonly(),
    PropMeta::reference("SelectedObject", "Instance").runtime().hidden(),
    PropMeta::boolean("AutoSelectGuiEnabled", true).hidden(),
];

pub static CLASS: ClassDesc = ClassDesc::new("GuiService", Some("Instance"))
    .describe("Facts about the on-screen interface (there is none in Forge).")
    .service(25)
    .hidden_in_explorer()
    .props(PROPS)
    .luau(include_str!("gui_service.luau"));
//! StarterGui: the interface that is given to each player. Forge has no GUI classes yet, so the container only holds
//! what scripts and scenes put in it; the core-GUI switches (`SetCoreGuiEnabled`, `SetCore`) are accepted and ignored.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("ShowDevelopmentGui", true).group("StarterGui").hidden(),
    PropMeta::boolean("ScreenOrientation", false).hidden(),
];

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("CoreGuiType", &["PlayerList", "Health", "Backpack", "Chat", "All", "EmotesMenu", "Captures"], &[0, 1, 2, 3, 4, 5, 6])];

pub static CLASS: ClassDesc = ClassDesc::new("StarterGui", Some("Instance"))
    .describe("the interface that is given to each player.")
    .service(7)
    .persisted()
    .container()
    .groups(&["StarterGui"])
    .props(PROPS)
    .luau(include_str!("starter_gui.luau"));
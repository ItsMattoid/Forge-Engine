//! MaterialService: settings for how materials look. They are stored; the renderer draws each material as one of its
//! few surface kinds (see `render::material_index`).

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Use2022Materials", false).group("Materials").hidden().desc("Roblox's switch between material looks (stored, not used)."),
    PropMeta::boolean("AutomaticMaterials", true).group("Materials").hidden(),
];

pub static CLASS: ClassDesc = ClassDesc::new("MaterialService", Some("Instance")).describe("Settings for how materials look.").service(44).hidden_in_explorer().persisted().groups(&["Materials"]).props(PROPS);
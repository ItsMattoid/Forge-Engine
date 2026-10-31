//! SoundGroup: a volume control and an effects bus for the sounds that name it (`Sound.SoundGroup`). Its `Volume` multiplies
//! the sound's, its sound-effect children are applied to every sound in the group, and a group inside a group passes both
//! on to the sounds of the group it is in.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::number("Volume", 0.5).group("SoundGroup").order(0).slider(0.0, 10.0).desc("How loud the group's sounds are, relative to the sound's own Volume.")];

pub static CLASS: ClassDesc = ClassDesc::new("SoundGroup", Some("Instance"))
    .describe("A volume control and effects bus for a group of sounds.")
    .category("Audio")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("SoundService"))
    .groups(&["SoundGroup"])
    .props(PROPS);
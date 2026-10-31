//! Joint: what every constraint has - the two parts it joins and whether it is on. The simulation
//! (`physics::joints`) builds a joint for each constraint that is enabled, in the workspace and has its parts.
//!
//! A constraint joins either two parts (`Part0`, `Part1`, with the frames `C0` / `C1`) or, as Roblox's constraints do,
//! two **attachments** (`Attachment0`, `Attachment1`): then each attachment's part is the part joined and the attachment's
//! own CFrame is the frame. An attachment that is not on a part (it is in the Workspace) is a point in the world.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Constraint").order(0).flags(F_PHYS).desc("Whether the constraint holds. A disabled constraint has no effect."),
    PropMeta::reference("Part0", "BasePart").group("Constraint").order(1).flags(F_PHYS).desc("The first part. (If only one of the two parts is set, the constraint joins it to the world.)"),
    PropMeta::reference("Part1", "BasePart").group("Constraint").order(2).flags(F_PHYS).desc("The second part; empty joins Part0 to a fixed point in the world."),
    PropMeta::reference("Attachment0", "Attachment").group("Constraint").order(4).flags(F_PHYS).desc("The attachment on the first part: it says which part and where. (Used instead of Part0 and the frame when set.)"),
    PropMeta::reference("Attachment1", "Attachment").group("Constraint").order(5).flags(F_PHYS).desc("The attachment on the second part (or in the world). (Used instead of Part1 and the frame when set.)"),
    PropMeta::boolean("CollideConnected", false).group("Constraint").order(3).flags(F_PHYS).desc("Whether the two parts still collide with each other."),
];

pub static COMPONENT: ComponentDesc = ComponentDesc::new("Joint").describe("The parts a constraint joins.").props(PROPS);

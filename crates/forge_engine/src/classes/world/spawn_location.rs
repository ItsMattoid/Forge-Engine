//! SpawnLocation: a marker part for where a game's own scripts put players (find them with `CollectionService` or by class:
//! the engine moves no character itself).

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Spawn").order(0).desc("Whether scripts should use this spawn."),
];

pub static CLASS: ClassDesc = ClassDesc::new("SpawnLocation", Some("Part"))
    .describe("A marker for where scripts spawn players.")
    .category("Parts")
    .creatable()
    .spawn_at_focus()
    .insert_props(&[("Anchored", D::B(true))])
    .groups(&["Spawn"])
    .defaults(&[("Anchored", D::B(true)), ("Size", D::V(12.0, 1.0, 12.0))])
    .props(PROPS);
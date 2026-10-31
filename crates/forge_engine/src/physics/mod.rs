//! Physics, on Box3D (through `bevy_box3d`).
//!
//! `bevy_box3d`'s [`Box3dPlugin`](bevy_box3d::Box3dPlugin) owns the world and steps it at a fixed rate in Bevy's
//! `FixedUpdate`; this module is everything between that world and the engine's data model. It knows nothing about
//! specific classes: a part's physical properties are ordinary properties (see `components/rigidbody.rs` and
//! `components/collider.rs`), constraints are ordinary instances (`classes/constraints`), and components can add a
//! system to the step with `ComponentDesc::physics` (the movers do). There is no built-in character controller: a player
//! is whatever a script makes of a part (`AssemblyLinearVelocity`, `FixedRotation`, `Workspace:Raycast`).
//!
//! * [`PhysicsBridge`] ([`bridge`]) keeps the world in step with the data model and writes the results back; its
//!   module documentation says who owns what and why nothing can feed back on itself;
//! * [`groups`] - collision groups (which kinds of part collide with which);
//! * [`shapes`] - the solid shape of a part in the form Box3D takes, and the arithmetic that goes with it;
//! * [`joints`] - constraints;
//! * [`query`] - ray casts, shape casts and overlap tests against the simulated world;
//! * [`plugin`] - the Bevy plugin and the fixed-rate constants;
//! * [`collider`], [`raycast`] - the *data model's* view of a collision volume, which the editor uses to pick and
//!   to draw while nothing is simulated.
//!
//! # Units and the step
//!
//! One world unit is one stud. Box3D works in meters, so its length scale is set to [`plugin::STUDS_PER_METER`]
//! (Roblox's 0.28 m stud) before any world exists, which scales its tolerances to match. The world steps at 60 Hz
//! with 8 sub-steps each (a stack ten parts high stays within 2% of its height at the engine's gravity of 196.2).
//!
//! # What Box3D does not do, and what the engine does about it
//!
//! * Parts that do not collide but can be touched are sensors: they report the parts that overlap them, solid or not,
//!   moving or anchored. The one pair that never reports a touch is two anchored parts that both collide (Box3D has no
//!   contact events between two bodies that cannot move). Both parts of a pair need `CanTouch`.
//! * Contact events are only produced for parts something listens to (`Touched` / `TouchEnded` connected), so the
//!   cost is paid by the games that use them.
//! * An ellipsoid has no Box3D primitive: a ball with unequal sizes is a convex hull of points on it (see [`shapes`]).
//! * Parts are convex solids: concave collision (a mesh part's exact surface) is not simulated.

mod bridge;
mod collider;
pub mod groups;
pub mod joints;
mod plugin;
pub mod query;
mod raycast;
pub mod shapes;
#[cfg(test)]
mod tests;

/// What an app needs of `bevy_box3d` and `box3d` to drive the bridge, so it need not depend on them itself.
pub use bevy_box3d::{Box3dSet, Box3dStats, Box3dWorld};
pub use box3d::World as PhysicsWorld;
pub use bridge::{analytic_mass, read_spec, BodyKind, PartSpec, PhysicsBridge, StepCtx, WorldScope, WorldSlot};
/// Conversions between Bevy's math types and Box3D's, for the movers that push bodies.
pub(crate) use bridge::{bv, fv};
pub use collider::{ColliderFn, ColliderInfo, ColliderShape};
pub use plugin::{init_units, physics_config, ForgePhysicsPlugin, FIXED_HZ, MAX_SPEED, STUDS_PER_METER, SUB_STEPS};
pub use query::{CastShape, Hit as QueryHit, QueryParams};
pub use raycast::{pick_collider, pick_colliders_all, ray_collider, raycast, Hit, RayParams};

/// `box3d`'s errors have no `Display`: this is how the bridge puts one in a message.
pub(crate) fn box3d_error(e: box3d::Error) -> String {
    format!("{e:?}")
}

/// A physics system: runs before every step (see `ComponentDesc::physics`).
pub type PhysicsFn = fn(&mut StepCtx);

/// What the last physics step did (for the debug overlays; always collected, it is a handful of numbers).
#[derive(Default, Clone, Debug)]
pub struct PhysicsStats {
    /// How long the world's step took, in milliseconds.
    pub step_ms: f32,
    /// Fixed steps the last rendered frame ran (0 on a frame faster than the fixed rate, more on a slow one).
    pub substeps: u32,
    /// Anchored (static) bodies in the world.
    pub static_boxes: u32,
    /// Moving bodies by kind: `(name, simulated, awake)`.
    pub bodies: Vec<(&'static str, u32, u32)>,
    /// Pairs currently touching (only counted for parts something listens to).
    pub touch_pairs: u32,
    /// `Touched` / `TouchEnded` events produced by the last step.
    pub touch_events: u32,
    /// Bodies that moved during the last step.
    pub movers: u32,
    /// Constraints that are active.
    pub joints: u32,
}

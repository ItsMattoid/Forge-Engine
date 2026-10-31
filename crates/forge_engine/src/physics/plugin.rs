//! The Bevy side: the plugin that adds `bevy_box3d`'s world and stepping, tuned for the engine.
//!
//! The plugin only provides the world and its fixed-rate step. Keeping the world in step with the data model is the
//! job of [`PhysicsBridge`](super::PhysicsBridge), which an app drives with two systems in `FixedUpdate`, one on each
//! side of `Box3dSet::Step`: `ScriptHost::physics_pre_step` and `ScriptHost::physics_post_step` (the runtime and the
//! editor's Play mode each add them, finding the host in their own way).

use std::sync::Once;

use bevy::prelude::*;
use bevy_box3d::{Box3dConfig, Box3dPlugin};
use box3d::ContactTuning;

/// Box3D measures in meters; the engine in studs (Roblox's stud is 0.28 m).
pub const STUDS_PER_METER: f32 = 1.0 / 0.28;
/// The simulation's fixed rate.
pub const FIXED_HZ: f64 = 60.0;
/// Solver sub-steps per step. More sub-steps stiffen contacts: with 4, a ten-part stack sags by a tenth of
/// its height at the engine's gravity; with 8, by a fiftieth.
pub const SUB_STEPS: i32 = 8;

/// The fastest a part may go, in studs per second (what it is held to; Box3D's CCD takes care of what is in the way).
pub const MAX_SPEED: f32 = 10_000.0;

static UNITS: Once = Once::new();

/// Tells Box3D what a unit is. Has to happen before the first world is made, and once: [`ForgePhysicsPlugin`] and
/// anything that makes a world of its own (the tests) call it.
pub fn init_units() {
    UNITS.call_once(|| box3d::set_length_units_per_meter(STUDS_PER_METER));
}

/// The settings the engine runs Box3D with.
pub fn physics_config() -> Box3dConfig {
    init_units();
    Box3dConfig {
        gravity: Vec3::new(0.0, -196.2, 0.0),
        fixed_hz: FIXED_HZ,
        sub_steps: SUB_STEPS,
        sleeping_enabled: true,
        continuous_enabled: true,
        // (Box3D's own limit is 200 m/s, a third of what a stud-sized bullet can be asked to do.)
        maximum_linear_speed: Some(MAX_SPEED),
        // (The speed contacts are pushed apart at is in studs per second, so it scales with the units.)
        contact_tuning: Some(ContactTuning { hertz: 30.0, damping_ratio: 10.0, contact_speed: 2.0 * STUDS_PER_METER }),
        ..Box3dConfig::default()
    }
}

/// Adds the Box3D world and its fixed-rate step to an app.
pub struct ForgePhysicsPlugin;

impl Plugin for ForgePhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Box3dPlugin { config: physics_config(), single_threaded_schedules: false });
    }
}

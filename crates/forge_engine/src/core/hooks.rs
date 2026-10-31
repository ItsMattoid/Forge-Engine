//! The hooks a component can implement. This is the one place where the object
//! model (`core` + `dm`) names types owned by the subsystems that call the hooks;
//! everything else in those subsystems depends on the model, never the other way.
//!
//! | hook         | called by | when                                              |
//! |--------------|-----------|---------------------------------------------------|
//! | `render`     | `render`  | an instance changed; mirror it into Bevy          |
//! | `physics`    | `physics` | every simulation step, in `physics_order`         |
//! | `collider`   | `physics` | describe the collision volume (physics, picking)  |
//! | `on_changed` | `dm`      | right after a property was written                |
//! | `gizmo`      | editor    | draw helper shapes for the selected instance      |
//! | `audio_effect` | `audio` | describe the audio processor an instance stands for |
//! | `camera_effect` | `render` | say what an instance does to every camera (bloom, fog, the sky ...) |

use super::{meta::PropMeta, value::Id};
use crate::dm::DataModel;

pub use super::meta::GizmoFn;
pub use crate::{
    audio::EffectFn,
    physics::{ColliderFn, PhysicsFn},
    render::{AnimateFn, CameraEffectFn, RenderFn},
};

pub type OnChangedFn = fn(&mut DataModel, Id, &PropMeta);

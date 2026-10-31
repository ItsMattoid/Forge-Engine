//! CameraEffect: marks the objects that say something about how cameras see the world (screen effects, the sky, the lighting
//! settings). `render::camera_fx::collect` asks each of them, through the `camera_effect` hook of the component that goes with
//! it, what it contributes. The marker itself has nothing in it.

use crate::core::*;

pub static COMPONENT: ComponentDesc = ComponentDesc::new("CameraEffect").describe("Takes part in how cameras draw the picture (see render::camera_fx).");
//! Animated: marks the objects that move on their own, frame by frame (particles, beams, trails, highlights). The renderer
//! calls the `animate` hook of each component of such an instance every frame, with the frame's time and the camera's position
//! (see `render::effects`). The marker itself has nothing in it.

use crate::core::*;

pub static COMPONENT: ComponentDesc = ComponentDesc::new("Animated").describe("Is drawn by code that runs every frame (see render::effects).");

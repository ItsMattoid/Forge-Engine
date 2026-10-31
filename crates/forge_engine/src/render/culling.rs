//! Not drawing what cannot be seen: what the cameras are set up with so that the GPU skips meshes.
//!
//! * **View-frustum culling** is always on and needs nothing from here. Every part carries its bounds
//!   (`Aabb`, written when it is spawned, see `classes/world/parts/base_part.rs`), so Bevy can test the
//!   parts against the view (in parallel, on the CPU) without first computing their bounds from the mesh,
//!   and where the GPU builds its own draw lists (Vulkan, Metal) it culls again there, so what is outside
//!   the view costs no vertex work.
//! * **Occlusion culling** skips parts hidden behind other opaque geometry. Bevy does it on the GPU in two
//!   phases: it draws what was visible last frame to build a depth pyramid, tests the bounds of everything
//!   against that, and only then draws the rest. It needs GPU-built draw lists, which Bevy has on Vulkan and
//!   Metal but not on DirectX 12 (a `wgpu` limitation); where the hardware cannot do it nothing is added to
//!   the camera, so nothing is paid for it.
//!
//! **It is off unless a project (or the editor) asks for it, because it only pays off when the GPU is the
//! bottleneck.** Measured with `forge_player --bench` on a Radeon RX 6650 XT (Vulkan), release build, a wall
//! with a doorway and 32,768 or 2,744 cubes behind it, all of them inside the view frustum:
//!
//! | scene                                              | off     | on      |
//! |----------------------------------------------------|---------|---------|
//! | overdraw-bound: 2,744 big cubes, 1440p, no MSAA    | 2.65 ms | 1.90 ms |
//! | CPU-bound: 32,768 cubes, 720p, no MSAA, no shadows | 3.1 ms  | 3.8 ms  |
//! | CPU-bound: 32,768 cubes, 720p, 4x MSAA, shadows    | 6.2 ms  | 6.9 ms  |
//! | overdraw-bound, 1440p, 8x MSAA                     | 3.3 ms  | 9.6 ms  |
//!
//! Occlusion culling happens on the GPU, after the CPU has already visited and queued every entity in view,
//! so it cannot make a CPU-bound scene cheaper; it adds a depth prepass and a depth pyramid, which MSAA makes
//! expensive. Turn it on for scenes with heavy hidden overdraw (dense interiors, cities) played without
//! MSAA, and measure. The images it produces are identical to those without it.
//!
//! **Never on a camera that has a viewport.** Bevy 0.16 sizes a camera's depth texture to the whole render
//! target but builds the occlusion pyramid for the viewport, and tests objects against it as if the two were
//! the same rectangle. For a camera that draws into part of the window (the editor's viewport camera) the
//! objects are tested against the wrong part of the depth buffer, and visible ones are culled: with a camera
//! in an offset rectangle, every cube seen through a doorway disappeared, 7,579 pixels of a 1280x720 image
//! (a full-window camera is pixel-identical with the setting on or off). So occlusion culling works in the
//! player and in "Play in Window", not in the editor's own viewport.
//!
//! Every 3D camera is given or stripped of the components that switch occlusion culling on, according to
//! the [`Culling`] resource, so a camera the game creates later and a setting that is changed while running
//! are both handled in one place.

use bevy::{
    core_pipeline::prepass::DepthPrepass,
    prelude::*,
    render::{
        batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport},
        occlusion_culling::OcclusionCulling,
        renderer::RenderAdapterInfo,
        RenderApp,
    },
};

/// How the cameras cull.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct Culling {
    /// Whether occlusion culling is wanted (a project / editor setting; off by default, see the module docs).
    pub occlusion: bool,
    /// Whether this GPU and graphics API can do it at all (known once the renderer is up).
    pub supported: bool,
}

impl Culling {
    /// Whether the cameras should have occlusion culling right now.
    pub fn active(&self) -> bool {
        self.occlusion && self.supported
    }
}

/// Adds [`Culling`] and keeps the cameras in step with it. Part of [`super::PartMaterialPlugin`].
pub struct CullingPlugin;

impl Plugin for CullingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Culling>().add_systems(Update, apply_occlusion_culling);
    }

    /// The renderer knows what the GPU can do only once it has started, which is after every plugin was built.
    fn finish(&self, app: &mut App) {
        let Some(render) = app.get_sub_app(RenderApp) else { return };
        let world = render.world();
        let supported = world.get_resource::<GpuPreprocessingSupport>().is_some_and(|s| s.max_supported_mode == GpuPreprocessingMode::Culling);
        let backend = world.get_resource::<RenderAdapterInfo>().map(|i| format!("{:?}", i.backend)).unwrap_or_else(|| "unknown".into());
        if supported {
            info!("occlusion culling is available ({backend})");
        } else {
            info!("occlusion culling is not available on this graphics API ({backend}): it needs GPU-built draw lists (Vulkan or Metal); frustum culling still applies");
        }
        app.world_mut().resource_mut::<Culling>().supported = supported;
    }
}

/// Gives every 3D camera occlusion culling (and the depth prepass it needs), or takes it away, as [`Culling`]
/// says. A camera with a viewport (one that draws into a part of its window, like the editor's) never has it.
fn apply_occlusion_culling(culling: Res<Culling>, mut commands: Commands, cameras: Query<(Entity, &Camera, Has<OcclusionCulling>), With<Camera3d>>) {
    for (entity, camera, has) in &cameras {
        let wanted = culling.active() && camera.viewport.is_none();
        if wanted && !has {
            commands.entity(entity).insert((DepthPrepass, OcclusionCulling));
        } else if !wanted && has {
            commands.entity(entity).remove::<(DepthPrepass, OcclusionCulling)>();
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    fn run(world: &mut World) {
        world.run_system_once(apply_occlusion_culling).unwrap();
    }

    #[test]
    fn cameras_get_occlusion_culling_only_when_it_is_wanted_and_the_hardware_can_do_it() {
        let mut world = World::new();
        let camera = world.spawn(Camera3d::default()).id();
        let other = world.spawn_empty().id();
        let has = |world: &World| (world.entity(camera).contains::<OcclusionCulling>(), world.entity(camera).contains::<DepthPrepass>());

        world.insert_resource(Culling { occlusion: true, supported: false });
        run(&mut world);
        assert_eq!(has(&world), (false, false), "unsupported: no depth prepass is paid for");

        world.insert_resource(Culling { occlusion: false, supported: true });
        run(&mut world);
        assert_eq!(has(&world), (false, false), "switched off");

        world.insert_resource(Culling { occlusion: true, supported: true });
        run(&mut world);
        assert_eq!(has(&world), (true, true), "the prepass comes with it: occlusion culling is ignored without one");
        assert!(!world.entity(other).contains::<OcclusionCulling>(), "only cameras");

        let later = world.spawn(Camera3d::default()).id();
        run(&mut world);
        assert!(world.entity(later).contains::<OcclusionCulling>(), "a camera created later is covered too");

        world.resource_mut::<Culling>().occlusion = false;
        run(&mut world);
        assert_eq!(has(&world), (false, false), "and it can be switched off while running");
        assert!(!world.entity(later).contains::<DepthPrepass>());
    }

    #[test]
    fn a_camera_that_draws_into_part_of_its_window_never_gets_it() {
        use bevy::camera::Viewport;
        let mut world = World::new();
        world.insert_resource(Culling { occlusion: true, supported: true });
        let full = world.spawn(Camera3d::default()).id();
        let part = world.spawn((Camera3d::default(), Camera { viewport: Some(Viewport { physical_position: UVec2::new(300, 100), physical_size: UVec2::new(800, 500), ..default() }), ..default() })).id();
        run(&mut world);
        assert!(world.entity(full).contains::<OcclusionCulling>());
        assert!(!world.entity(part).contains::<OcclusionCulling>() && !world.entity(part).contains::<DepthPrepass>(), "Bevy culls visible objects for such a camera");

        // A camera that gets its viewport later (the editor sets it after the camera exists) loses it again.
        world.entity_mut(full).get_mut::<Camera>().unwrap().viewport = Some(Viewport::default());
        run(&mut world);
        assert!(!world.entity(full).contains::<OcclusionCulling>() && !world.entity(full).contains::<DepthPrepass>());
    }
}

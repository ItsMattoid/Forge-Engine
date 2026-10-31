//! Drawing the edges of every mesh over the scene, to see how things are built.
//!
//! This is Bevy's wireframe pass, which needs the graphics API to draw lines for polygons
//! (`POLYGON_MODE_LINE`: Vulkan, DirectX 12 and Metal have it; it is native only). The renderer turns that
//! feature on by itself wherever the adapter has it, so all that is needed here is to know whether it did,
//! and to switch Bevy's global wireframe with one flag, [`WireframeView::enabled`].
//!
//! What it draws are the triangle edges of the meshes, so a block shows the diagonal of each face as well.
//! By default the shaded scene stays underneath; [`WireframeView::hide_solid`] takes it away, leaving only
//! the edges (the parts' shader discards what it would draw, see `part.wgsl`). The depth prepass, and so
//! occlusion culling, still see the solid parts: what the wireframe pass draws is what the culling kept, and
//! with occlusion culling on, edges behind a solid part are still hidden by the depth buffer.
//!
//! While the view is off the plugin costs nothing: its systems only look at entities that carry a wireframe
//! component, and none does.

use bevy::{
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*,
    render::{render_resource::WgpuFeatures, renderer::RenderDevice, RenderApp},
};

use super::PartMaterial;

/// The colour of the edges: a saturated green, which shows on the light parts, the dark ones and the sky.
const EDGE_COLOR: Color = Color::srgb(0.0, 1.0, 0.4);

/// Whether the edges are drawn, and whether the shaded scene is drawn with them.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct WireframeView {
    /// Whether the edges are wanted (a viewport setting).
    pub enabled: bool,
    /// Whether the graphics API can draw them at all (known once the renderer is up).
    pub supported: bool,
    /// Draw only the edges, without the shaded parts. Does nothing unless the edges are drawn.
    pub hide_solid: bool,
}

impl WireframeView {
    /// Whether the edges are drawn right now.
    pub fn active(&self) -> bool {
        self.enabled && self.supported
    }

    /// Whether the shaded parts are left out right now (never when there would be no edges to look at).
    pub fn hides_solid(&self) -> bool {
        self.hide_solid && self.active()
    }
}

/// Adds [`WireframeView`] and keeps Bevy's wireframe in step with it. Part of [`super::PartMaterialPlugin`].
pub struct WireframeViewPlugin;

impl Plugin for WireframeViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WireframePlugin::default())
            .init_resource::<WireframeView>()
            .insert_resource(WireframeConfig { global: false, default_color: EDGE_COLOR, ..default() })
            // (Also when materials were added: a surface kind used for the first time gets the current state.)
            .add_systems(Update, apply_wireframe.run_if(resource_changed::<WireframeView>.or_else(resource_changed::<Assets<PartMaterial>>)));
    }

    /// The renderer knows what the GPU can do only once it has started, which is after every plugin was built.
    fn finish(&self, app: &mut App) {
        let Some(render) = app.get_sub_app(RenderApp) else { return };
        let supported = render.world().get_resource::<RenderDevice>().is_some_and(|d| d.features().contains(WgpuFeatures::POLYGON_MODE_LINE));
        if !supported {
            info!("wireframe view is not available on this graphics adapter (no polygon line mode)");
        }
        app.world_mut().resource_mut::<WireframeView>().supported = supported;
    }
}

/// Switches Bevy's global wireframe, and whether the parts draw themselves, to what [`WireframeView`] says.
fn apply_wireframe(view: Res<WireframeView>, mut config: ResMut<WireframeConfig>, mut materials: ResMut<Assets<PartMaterial>>) {
    if config.global != view.active() {
        config.global = view.active();
    }
    let solid = if view.hides_solid() { 0.0 } else { 1.0 };
    // (Only the materials that differ are touched: writing to an asset makes the renderer prepare it again.)
    let stale: Vec<_> = materials.iter().filter(|(_, m)| m.extension.solid != solid).map(|(id, _)| id).collect();
    for id in stale {
        if let Some(mut material) = materials.get_mut(id) {
            material.extension.solid = solid;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;
    use crate::{assets::material::MaterialDef, render::part_material::make_material};

    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(WireframeConfig::default());
        world.init_resource::<Assets<PartMaterial>>();
        world
    }

    #[test]
    fn the_edges_are_drawn_only_when_wanted_and_the_graphics_api_can_draw_them() {
        let mut world = world();
        for (enabled, supported, drawn) in [(false, false, false), (true, false, false), (false, true, false), (true, true, true), (false, true, false)] {
            world.insert_resource(WireframeView { enabled, supported, hide_solid: false });
            world.run_system_once(apply_wireframe).unwrap();
            assert_eq!(world.resource::<WireframeConfig>().global, drawn, "enabled {enabled}, supported {supported}");
        }
    }

    #[test]
    fn the_shaded_parts_are_left_out_only_while_there_are_edges_to_look_at() {
        let mut world = world();
        let (plain, glass) = {
            let mut materials = world.resource_mut::<Assets<PartMaterial>>();
            (materials.add(make_material(&MaterialDef::builtin("Plastic"), &Default::default(), false, true)), materials.add(make_material(&MaterialDef::builtin("Glass"), &Default::default(), true, true)))
        };
        let solid = |world: &World, h: &Handle<PartMaterial>| world.resource::<Assets<PartMaterial>>().get(h).unwrap().extension.solid;
        for (enabled, supported, hide_solid, parts_drawn) in [(false, true, true, true), (true, false, true, true), (true, true, false, true), (true, true, true, false), (true, true, false, true)] {
            world.insert_resource(WireframeView { enabled, supported, hide_solid });
            world.run_system_once(apply_wireframe).unwrap();
            assert_eq!((solid(&world, &plain) == 1.0, solid(&world, &glass) == 1.0), (parts_drawn, parts_drawn), "enabled {enabled}, supported {supported}, hide_solid {hide_solid}");
        }
    }
}

//! The material every part is drawn with, built so that parts are instanced.
//!
//! Bevy draws entities that have the *same mesh and the same material* in one instanced draw call,
//! and as many such calls as there are distinct pairs otherwise. A material per colour (what the
//! renderer used to make) therefore costs one draw call per colour. Here the colour is not part of
//! the material but of the *instance*: it travels in the entity's `MeshTag` and the shader
//! (`part.wgsl`) multiplies it into the material's own colour (and colour map). What is left in the
//! material is what really changes the look of a surface and is the same for every part that uses it:
//! a [`MaterialDef`] - the project's `.mat` asset - turned into a Bevy `StandardMaterial` plus the few
//! extra inputs of the part shader ([`PartLook`]).
//!
//! So a scene of any number of parts, in any number of colours, costs one draw call per (shape, material)
//! pair, and a part created at run time joins the batch it belongs to without any new GPU resources.
//!
//! See-through parts (`Transparency` above 0, or a see-through material) are drawn in Bevy's sorted
//! transparent pass: that pass cannot merge things that interleave in depth, so they stay one call each, as
//! blending needs.

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    math::{Affine2, Vec2},
    pbr::{ExtendedMaterial, MaterialExtension, MaterialPlugin, StandardMaterial},
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_resource::{AsBindGroup, AsBindGroupShaderType, ShaderType},
        texture::GpuImage,
    },
    shader::ShaderRef,
};

use crate::assets::material::{AlphaKind, MaterialDef, MaterialKind, UvMode};

/// The part shader (`part.wgsl`), compiled into the engine.
pub const PART_SHADER: Handle<Shader> = uuid_handle!("6c3f1b8e-2d4a-4f57-9a10-5b7e8c21d4f3");

/// The extra per-material data of the part shader (one uniform buffer, binding 100).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, PartialEq)]
#[uniform(100, PartLookUniform)]
pub struct PartLook {
    /// 1 draws the part, 0 draws nothing (the wireframe-only view, see `render::wireframe`). Only the
    /// shaded pass looks at it: the depth prepass and the shadows are unchanged.
    pub solid: f32,
    /// The material's own opacity (multiplied into the part's and the colour map's).
    pub opacity: f32,
    /// Glass: how much more opaque the surface is at grazing angles.
    pub edge_opacity: f32,
    /// The emission is also multiplied by the part's colour (Neon parts glow in their own colour).
    pub emission_from_color: bool,
    /// Texture coordinates are projected along the part's axes in studs, not read from the mesh.
    pub studs_uv: bool,
    /// How deep normal-map bumps look (1 as authored).
    pub normal_strength: f32,
    /// How much the occlusion map darkens.
    pub occlusion_strength: f32,
    /// In `studs_uv` mode: studs per repeat of the maps.
    pub studs_per_tile: Vec2,
}

impl Default for PartLook {
    fn default() -> Self {
        PartLook { solid: 1.0, opacity: 1.0, edge_opacity: 0.0, emission_from_color: false, studs_uv: false, normal_strength: 1.0, occlusion_strength: 1.0, studs_per_tile: Vec2::splat(4.0) }
    }
}

/// What the shader reads (`struct PartLook` in `part.wgsl`: keep the two in step).
#[derive(Clone, Default, ShaderType)]
pub struct PartLookUniform {
    pub solid: f32,
    pub opacity: f32,
    pub edge_opacity: f32,
    pub emission_from_color: f32,
    pub studs_uv: f32,
    pub normal_strength: f32,
    pub occlusion_strength: f32,
    pub padding: f32,
    pub studs_per_tile: Vec2,
    pub padding2: Vec2,
}

impl AsBindGroupShaderType<PartLookUniform> for PartLook {
    fn as_bind_group_shader_type(&self, _images: &RenderAssets<GpuImage>) -> PartLookUniform {
        PartLookUniform {
            solid: self.solid,
            opacity: self.opacity,
            edge_opacity: self.edge_opacity,
            emission_from_color: f32::from(u8::from(self.emission_from_color)),
            studs_uv: f32::from(u8::from(self.studs_uv)),
            normal_strength: self.normal_strength,
            occlusion_strength: self.occlusion_strength,
            padding: 0.0,
            studs_per_tile: self.studs_per_tile,
            padding2: Vec2::ZERO,
        }
    }
}

impl MaterialExtension for PartLook {
    fn fragment_shader() -> ShaderRef {
        PART_SHADER.into()
    }
}

/// What a part is drawn with.
pub type PartMaterial = ExtendedMaterial<StandardMaterial, PartLook>;

/// Registers the part shader and material, the culling the cameras get (see [`super::culling`]) and the
/// wireframe view (see [`super::wireframe`]). Add it to every app that renders parts (the editor and the game).
pub struct PartMaterialPlugin;

impl Plugin for PartMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, PART_SHADER, "part.wgsl", Shader::from_wgsl);
        app.add_plugins((MaterialPlugin::<PartMaterial>::default(), MaterialPlugin::<super::HighlightMaterial>::default(), super::CullingPlugin, super::WireframeViewPlugin));
        // Bevy 0.19 clusters lights on the GPU by default: extra compute passes and bind groups every frame (about 0.4 ms of
        // CPU on the render thread for a small scene) that only pay off with hundreds of lights. The CPU clustering the older
        // Bevy used costs nothing noticeable for the handful of lights a game here has, so it is used instead.
        app.add_systems(First, use_cpu_light_clustering);
        // What the scene says about the cameras (screen effects, the sky, the light of the sky) is put on every 3D camera.
        app.add_systems(PostUpdate, (super::camera_fx::apply_camera_look, super::camera_fx::sync_sun_rays));
    }
}

/// Turns GPU light clustering off (see [`PartMaterialPlugin`]). Checked every frame: Bevy sets the resource up when the
/// render plugin finishes, and a camera or window change can set it up again.
fn use_cpu_light_clustering(settings: Option<ResMut<bevy::light::cluster::GlobalClusterSettings>>) {
    if let Some(mut settings) = settings {
        if settings.gpu_clustering.is_some() {
            settings.gpu_clustering = None;
        }
    }
}

/// A part's colour and opacity as the four bytes of its `MeshTag` (red in the lowest byte).
pub fn pack_tag(color: Vec3, alpha: f32) -> u32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    byte(color.x) | byte(color.y) << 8 | byte(color.z) << 16 | byte(alpha) << 24
}

/// The inverse of [`pack_tag`] (to the nearest byte).
pub fn unpack_tag(tag: u32) -> (Vec3, f32) {
    let channel = |shift: u32| ((tag >> shift) & 255) as f32 / 255.0;
    (Vec3::new(channel(0), channel(8), channel(16)), channel(24))
}


/// The pictures of a material, already loaded (see `AssetCache::material`).
#[derive(Clone, Default, Debug)]
pub struct MaterialImages {
    pub base_color: Option<Handle<Image>>,
    pub normal: Option<Handle<Image>>,
    /// Roughness (green) and metallic (blue) in one picture.
    pub metallic_roughness: Option<Handle<Image>>,
    pub occlusion: Option<Handle<Image>>,
    pub emission: Option<Handle<Image>>,
    pub has_roughness_map: bool,
    pub has_metallic_map: bool,
}

/// The shared Bevy material for a material asset.
///
/// * `see_through`: the variant that blends (a part with `Transparency`, or a see-through material).
/// * `studs_uv`: the maps are projected in studs (parts made from the engine's shapes) instead of using the mesh's own
///   coordinates ([`UvMode::Auto`] decides by the kind of part; the other modes override it).
pub fn make_material(def: &MaterialDef, images: &MaterialImages, see_through: bool, studs_uv: bool) -> PartMaterial {
    let studs = match def.uv.mode {
        // (A cut-out's alpha is also tested by Bevy's own depth and shadow passes, which only know the mesh's coordinates.)
        UvMode::Auto => studs_uv && def.alpha_mode != AlphaKind::Mask,
        UvMode::Mesh => false,
        UvMode::Studs => true,
    };
    let (sin, cos) = def.uv.rotation.to_radians().sin_cos();
    let uv_transform = Affine2::from_cols(Vec2::new(def.uv.tiling[0] * cos, def.uv.tiling[0] * sin), Vec2::new(-def.uv.tiling[1] * sin, def.uv.tiling[1] * cos), Vec2::new(def.uv.offset[0], def.uv.offset[1]));
    let emission = {
        let c = Color::srgb(def.emission_color[0], def.emission_color[1], def.emission_color[2]).to_linear();
        LinearRgba::new(c.red * def.emission_strength, c.green * def.emission_strength, c.blue * def.emission_strength, 1.0)
    };
    let alpha_mode = if def.alpha_mode == AlphaKind::Mask {
        AlphaMode::Mask(def.alpha_cutoff)
    } else if see_through || def.alpha_mode == AlphaKind::Blend {
        AlphaMode::Blend
    } else {
        AlphaMode::Opaque
    };
    let transmissive = def.transmission > 0.0;
    let mut base = StandardMaterial {
            // (The part's colour comes from the instance and is multiplied in by the shader.)
            base_color: Color::srgb(def.base_color[0], def.base_color[1], def.base_color[2]),
            base_color_texture: images.base_color.clone(),
            normal_map_texture: images.normal.clone(),
            flip_normal_map_y: def.flip_normal_y,
            metallic_roughness_texture: images.metallic_roughness.clone(),
            // With a map the number scales it; a map for one of the two leaves the other as the number says.
            perceptual_roughness: def.roughness,
            metallic: def.metallic,
            reflectance: def.reflectance,
            occlusion_texture: images.occlusion.clone(),
            emissive: emission,
            emissive_texture: images.emission.clone(),
            specular_transmission: def.transmission,
            ior: def.ior,
            thickness: if transmissive { 0.5 } else { 0.0 },
            alpha_mode,
            double_sided: def.double_sided,
            unlit: def.kind == MaterialKind::Unlit,
            uv_transform,
            ..default()
        };
    if def.double_sided {
        base.cull_mode = None;
    }
    PartMaterial {
        base,
        extension: PartLook {
            solid: 1.0,
            opacity: def.opacity,
            edge_opacity: if def.kind == MaterialKind::Glass { def.edge_opacity } else { 0.0 },
            emission_from_color: def.emission_from_color,
            studs_uv: studs,
            normal_strength: def.normal_strength,
            occlusion_strength: def.occlusion_strength,
            studs_per_tile: Vec2::new(def.uv.studs_per_tile[0].max(0.001), def.uv.studs_per_tile[1].max(0.001)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::material::MaterialDef;

    /// Bevy moves the material's bind group between releases (it was group 2, it is group 3 in 0.19), which no Rust test
    /// notices - the pipeline just fails to build when the window opens. The shader has to name the group the way
    /// Bevy's own material shaders do, so it follows the move.
    #[test]
    fn the_shader_binds_its_data_in_whatever_group_bevy_gives_materials() {
        let shader = include_str!("part.wgsl");
        let line = shader.lines().find(|l| l.contains("@binding(100)")).unwrap_or_else(|| panic!("no @binding(100) in part.wgsl"));
        assert!(line.contains("@group(#{MATERIAL_BIND_GROUP})"), "'{line}' must use #{{MATERIAL_BIND_GROUP}}, not a fixed group number");
        assert!(!shader.contains("@group(2)"), "a fixed material group number breaks on the next Bevy upgrade");
    }

    /// The Rust uniform and the WGSL struct must list the same fields in the same order, or the shader reads the wrong
    /// numbers without any error.
    #[test]
    fn the_uniform_has_the_fields_the_shader_reads_in_the_same_order() {
        let shader = include_str!("part.wgsl");
        let start = shader.find("struct PartLook {").expect("struct PartLook in part.wgsl");
        let body = &shader[start..shader[start..].find('}').unwrap() + start];
        let wgsl: Vec<&str> = body.lines().filter_map(|l| l.trim().split(':').next()).filter(|n| !n.is_empty() && !n.starts_with("struct") && !n.starts_with("//")).collect();
        assert_eq!(wgsl, ["solid", "opacity", "edge_opacity", "emission_from_color", "studs_uv", "normal_strength", "occlusion_strength", "padding", "studs_per_tile", "padding2"]);
    }

    #[test]
    fn colours_survive_the_trip_through_the_tag_to_the_nearest_byte() {
        for color in [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0), Vec3::new(0.63, 0.2, 0.9), Vec3::new(0.01, 0.99, 0.5)] {
            for alpha in [0.0, 0.12, 0.5, 1.0] {
                let (c, a) = unpack_tag(pack_tag(color, alpha));
                assert!((c - color).abs().max_element() <= 0.5 / 255.0 + 1e-6, "{color:?} -> {c:?}");
                assert!((a - alpha).abs() <= 0.5 / 255.0 + 1e-6);
            }
        }
        assert_eq!(unpack_tag(pack_tag(Vec3::new(2.0, -1.0, f32::NAN), 7.0)), (Vec3::new(1.0, 0.0, 0.0), 1.0), "out-of-range values are clamped, not wrapped into other channels");
    }

    #[test]
    fn the_tag_keeps_each_channel_in_its_own_byte() {
        assert_eq!(pack_tag(Vec3::new(1.0, 0.0, 0.0), 0.0), 0x0000_00FF);
        assert_eq!(pack_tag(Vec3::new(0.0, 1.0, 0.0), 0.0), 0x0000_FF00);
        assert_eq!(pack_tag(Vec3::new(0.0, 0.0, 1.0), 0.0), 0x00FF_0000);
        assert_eq!(pack_tag(Vec3::ZERO, 1.0), 0xFF00_0000);
    }

    #[test]
    fn a_material_asset_becomes_the_bevy_material_its_numbers_describe() {
        let none = MaterialImages::default();
        let plastic = make_material(&MaterialDef::builtin("Plastic"), &none, false, true);
        let metal = make_material(&MaterialDef::builtin("Metal"), &none, false, true);
        let neon = make_material(&MaterialDef::builtin("Neon"), &none, false, true);
        let glass = make_material(&MaterialDef::builtin("Glass"), &none, true, true);
        assert_eq!((plastic.base.metallic, metal.base.metallic), (0.0, 1.0));
        assert!(neon.base.emissive.red > 0.0 && plastic.base.emissive.red == 0.0, "neon gives off light, plastic does not");
        assert!(neon.extension.emission_from_color, "in the part's own colour");
        assert!(matches!(glass.base.alpha_mode, AlphaMode::Blend) && glass.extension.opacity < 0.5 && glass.extension.edge_opacity > 0.0);
        assert!(matches!(plastic.base.alpha_mode, AlphaMode::Opaque));
        assert!(matches!(make_material(&MaterialDef::builtin("Plastic"), &none, true, true).base.alpha_mode, AlphaMode::Blend), "a part with Transparency blends");
    }

    #[test]
    fn every_number_a_material_has_reaches_the_shader_or_the_standard_material() {
        let mut def = MaterialDef::default();
        def.roughness = 0.31;
        def.reflectance = 0.7;
        def.normal_strength = 0.5;
        def.occlusion_strength = 0.25;
        def.flip_normal_y = true;
        def.double_sided = true;
        def.uv.tiling = [2.0, 3.0];
        def.uv.offset = [0.5, 0.25];
        def.uv.studs_per_tile = [8.0, 2.0];
        def.alpha_mode = AlphaKind::Mask;
        def.alpha_cutoff = 0.3;
        def.transmission = 0.6;
        let m = make_material(&def, &MaterialImages::default(), false, false);
        assert_eq!((m.base.perceptual_roughness, m.base.reflectance, m.base.flip_normal_map_y, m.base.double_sided), (0.31, 0.7, true, true));
        assert!(m.base.cull_mode.is_none());
        assert!(matches!(m.base.alpha_mode, AlphaMode::Mask(c) if (c - 0.3).abs() < 1e-6));
        assert_eq!(m.base.specular_transmission, 0.6);
        assert_eq!((m.base.uv_transform.matrix2.x_axis.x, m.base.uv_transform.matrix2.y_axis.y, m.base.uv_transform.translation), (2.0, 3.0, Vec2::new(0.5, 0.25)));
        assert_eq!((m.extension.normal_strength, m.extension.occlusion_strength, m.extension.studs_per_tile), (0.5, 0.25, Vec2::new(8.0, 2.0)));
    }

    #[test]
    fn texture_coordinates_come_from_the_studs_on_shapes_and_from_the_mesh_on_models_unless_the_material_says_otherwise() {
        let none = MaterialImages::default();
        let mut def = MaterialDef::default();
        assert!(make_material(&def, &none, false, true).extension.studs_uv, "a block");
        assert!(!make_material(&def, &none, false, false).extension.studs_uv, "an imported model");
        def.uv.mode = UvMode::Mesh;
        assert!(!make_material(&def, &none, false, true).extension.studs_uv);
        def.uv.mode = UvMode::Studs;
        assert!(make_material(&def, &none, false, false).extension.studs_uv);
    }

    #[test]
    fn rotation_turns_the_uv_transform() {
        let mut def = MaterialDef::default();
        def.uv.rotation = 90.0;
        let m = make_material(&def, &MaterialImages::default(), false, false);
        let moved = m.base.uv_transform.transform_vector2(Vec2::X);
        assert!((moved - Vec2::Y).length() < 1e-5, "{moved:?}");
    }
}

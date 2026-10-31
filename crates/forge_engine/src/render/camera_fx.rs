//! What the cameras do to the picture: the screen effects and the sky.
//!
//! The things that affect *every* camera - bloom, colour correction, depth of field, fog, the sky, and the light the sky
//! gives to surfaces (so metal and glass reflect it) - are described by objects in the scene (the `Lighting` service's
//! settings and the effects inside it). Each of those classes says what it contributes with the `camera_effect` hook of its
//! component; [`collect`] gathers them into one [`CameraEffects`] (plain numbers and file paths, no Bevy), and
//! `render::sync` turns it into a [`CameraLook`] (Bevy components, with the sky's pictures loaded) whenever it changed.
//! [`apply_camera_look`] puts that on every 3D camera, the editor's and the game's alike, so the viewport shows what the
//! game will.
//!
//! What the effects map to in the renderer, and what they do not:
//!
//! * `BloomEffect`: Bevy's bloom (needs a high-dynamic-range picture, which the camera then gets) - Neon and any emissive
//!   material glow through it.
//! * `ColorCorrectionEffect`: Bevy's colour grading (brightness as exposure, contrast, saturation, tint as colour temperature).
//! * `DepthOfFieldEffect`: Bevy's depth of field. `BlurEffect`: the same effect with everything out of focus.
//! * `Atmosphere` and `Lighting.FogColor / FogStart / FogEnd`: distance fog (the atmosphere's colours are the fog's scattering).
//! * `Sky`: a cube-mapped sky from the six pictures, lighting the scene too. The sun, moon and stars Roblox draws are not.
//! * `SunRaysEffect`: volumetric light shafts from the sun (Bevy's volumetric fog).
//! * `Lighting` itself: the hemisphere of sky and ground colour that lights shadowed surfaces and shows in reflections.

use bevy::{
    asset::RenderAssetUsages,
    camera::Hdr,
    core_pipeline::{prepass::DepthPrepass, tonemapping::Tonemapping},
    light::{EnvironmentMapLight, FogVolume, GeneratedEnvironmentMapLight, Skybox, VolumetricFog},
    math::Quat,
    pbr::{DistanceFog, FogFalloff},
    post_process::{
        bloom::{Bloom, BloomCompositeMode, BloomPrefilter},
        dof::{DepthOfField, DepthOfFieldMode},
    },
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension},
        view::{ColorGrading, ColorGradingGlobal, ColorGradingSection},
    },
};

use super::SpawnParams;
use crate::{assets::{texture, GameSource}, core::Id, dm::DataModel};

/// What an effect class says (see the module docs).
pub type CameraEffectFn = fn(&DataModel, Id, &mut CameraEffects);

#[derive(Clone, Debug, PartialEq)]
pub struct BloomFx {
    pub intensity: f32,
    pub size: f32,
    pub threshold: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorFx {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub tint: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct DofFx {
    pub focus_distance: f32,
    pub in_focus_radius: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FogFx {
    pub color: [f32; 3],
    pub kind: FogKind,
    /// How much of the haze is lit up around the sun (0 none).
    pub glare: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FogKind {
    Linear { start: f32, end: f32 },
    /// Haze with colours: seen through this much air the scene is `visibility` studs clear.
    Atmospheric { visibility: f32, extinction: [f32; 3] },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyFx {
    /// Right, left, top, bottom, back, front: the project paths of the six pictures (empty for a missing one).
    pub faces: [String; 6],
    /// Degrees about each axis.
    pub orientation: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SunRaysFx {
    pub intensity: f32,
    pub spread: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvFx {
    pub sky_color: [f32; 3],
    pub ambient: [f32; 3],
    pub ambient_brightness: f32,
    /// 0 at night to 1 by day.
    pub day: f32,
    /// Lighting.EnvironmentDiffuseScale / EnvironmentSpecularScale: how much of the sky lights and shines.
    pub diffuse_scale: f32,
    pub specular_scale: f32,
    /// Lighting.ExposureCompensation, in stops.
    pub exposure: f32,
}

impl Default for EnvFx {
    fn default() -> Self {
        EnvFx { sky_color: [0.53, 0.72, 0.95], ambient: [0.7, 0.75, 0.85], ambient_brightness: 350.0, day: 1.0, diffuse_scale: 1.0, specular_scale: 1.0, exposure: 0.0 }
    }
}

/// Everything the scene says about how cameras treat the picture.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CameraEffects {
    pub env: EnvFx,
    pub bloom: Option<BloomFx>,
    pub color: Option<ColorFx>,
    pub dof: Option<DofFx>,
    pub blur: Option<f32>,
    /// `Lighting.FogColor / FogStart / FogEnd`.
    pub lighting_fog: Option<FogFx>,
    /// The `Atmosphere` object; it wins over the lighting fog.
    pub atmosphere: Option<FogFx>,
    pub sky: Option<SkyFx>,
    pub sun_rays: Option<SunRaysFx>,
}

impl CameraEffects {
    pub fn fog(&self) -> Option<&FogFx> {
        self.atmosphere.as_ref().or(self.lighting_fog.as_ref())
    }
}

/// The scene's effects, from the `camera_effect` hooks of every instance that has one.
pub fn collect(dm: &DataModel) -> CameraEffects {
    let mut fx = CameraEffects::default();
    for id in dm.with_component("CameraEffect") {
        let Some(class) = dm.get(id).map(|i| i.class) else { continue };
        // Effects work in Lighting (and the Lighting service itself) or in the Workspace, as in Roblox.
        let lighting = dm.service("Lighting");
        if lighting != Some(id) && !dm.in_workspace(id) && !lighting.is_some_and(|l| dm.is_descendant_of(id, l)) {
            continue;
        }
        for comp in &class.components {
            if let Some(hook) = comp.camera_effect {
                hook(dm, id, &mut fx);
            }
        }
    }
    fx
}

// ---- mappings (pure) ---------------------------------------------------------------------------------

/// Bevy's colour grading for a colour correction effect.
pub fn grading_of(c: &ColorFx) -> ColorGrading {
    let section = ColorGradingSection { contrast: (1.0 + c.contrast).clamp(0.0, 3.0), ..ColorGradingSection::default() };
    // Brightness is exposure in stops (a full +1 is two stops); a tint is a shift of the colour temperature and tint.
    let temperature = (c.tint[0] - c.tint[2]) * 0.6;
    let tint = (c.tint[1] - (c.tint[0] + c.tint[2]) * 0.5) * -0.6;
    ColorGrading {
        global: ColorGradingGlobal { exposure: c.brightness * 2.0, temperature, tint, post_saturation: (1.0 + c.saturation).clamp(0.0, 3.0), ..ColorGradingGlobal::default() },
        shadows: section,
        midtones: section,
        highlights: section,
    }
}

/// Bevy's bloom for a bloom effect: Roblox's Intensity scales the glow, Size how far it spreads, Threshold how bright a
/// surface must be before it glows.
pub fn bloom_of(b: &BloomFx) -> Bloom {
    Bloom {
        intensity: (0.12 * b.intensity).clamp(0.0, 1.0),
        prefilter: BloomPrefilter { threshold: (b.threshold * 0.75).max(0.0), threshold_softness: 0.6 },
        composite_mode: BloomCompositeMode::Additive,
        max_mip_dimension: if b.size > 40.0 { 1024 } else { 512 },
        scale: Vec2::splat((b.size / 24.0).clamp(0.25, 3.0)),
        ..Bloom::NATURAL
    }
}

/// Bevy's depth of field. `blur` makes everything out of focus (a `BlurEffect`).
pub fn dof_of(d: Option<&DofFx>, blur: Option<f32>) -> Option<DepthOfField> {
    match (d, blur) {
        (Some(d), _) => {
            // More blur intensity is a wider aperture (a smaller f-number); the in-focus radius is what the lens keeps sharp.
            let strength = d.near.max(d.far).clamp(0.0, 1.0);
            Some(DepthOfField {
                mode: DepthOfFieldMode::Gaussian,
                focal_distance: d.focus_distance.max(0.1),
                aperture_f_stops: 9.0 - 8.4 * strength + d.in_focus_radius.clamp(0.0, 1000.0) * 0.01,
                max_circle_of_confusion_diameter: 64.0,
                max_depth: 4000.0,
                ..DepthOfField::default()
            })
        }
        (None, Some(size)) if size > 0.0 => Some(DepthOfField { mode: DepthOfFieldMode::Gaussian, focal_distance: 0.05, aperture_f_stops: 6.0 - (size / 56.0).clamp(0.0, 1.0) * 5.4, max_circle_of_confusion_diameter: size.clamp(1.0, 64.0), max_depth: 4000.0, ..DepthOfField::default() }),
        _ => None,
    }
}

/// Bevy's distance fog.
pub fn fog_of(f: &FogFx) -> DistanceFog {
    let color = Color::srgb(f.color[0], f.color[1], f.color[2]);
    let falloff = match &f.kind {
        FogKind::Linear { start, end } => FogFalloff::Linear { start: *start, end: end.max(*start + 1.0) },
        FogKind::Atmospheric { visibility, extinction } => {
            FogFalloff::from_visibility_colors(visibility.max(10.0), Color::srgb(extinction[0], extinction[1], extinction[2]), color)
        }
    };
    let (glow, exponent) = if f.glare > 0.0 { (Color::srgba(1.0, 0.95, 0.8, (f.glare / 10.0).clamp(0.0, 1.0)), 30.0) } else { (Color::NONE, 8.0) };
    DistanceFog { color, falloff, directional_light_color: glow, directional_light_exponent: exponent }
}

/// The colours of the sky hemisphere, as `(top, middle, bottom)` linear, for the light the sky gives and the reflections.
pub fn hemisphere(env: &EnvFx) -> [Vec3; 3] {
    let day = env.day.clamp(0.0, 1.0);
    let lin = |c: [f32; 3]| Color::srgb(c[0], c[1], c[2]).to_linear();
    let (sky, amb) = (lin(env.sky_color), lin(env.ambient));
    let to = |c: LinearRgba, k: f32| Vec3::new(c.red, c.green, c.blue) * k;
    let level = 0.04 + 0.96 * day;
    [to(sky, level), to(sky, level * 0.55) + to(amb, level * 0.45), to(amb, level * 0.35)]
}

// ---- the Bevy side -----------------------------------------------------------------------------------

/// How cameras look, ready to put on them (see [`apply_camera_look`]).
#[derive(Resource, Clone)]
pub struct CameraLook {
    pub bloom: Option<Bloom>,
    pub grading: Option<ColorGrading>,
    pub dof: Option<DepthOfField>,
    pub fog: Option<DistanceFog>,
    pub sky: Option<Skybox>,
    pub env: Environment,
    pub sun_rays: Option<SunRaysFx>,
    /// Something needs the depth of the scene before the main pass (depth of field, decals).
    pub depth_prepass: bool,
}

#[derive(Clone)]
pub enum Environment {
    /// The light of a plain hemisphere (no Sky object).
    Gradient(EnvironmentMapLight),
    /// The light of the Sky's pictures, filtered on the GPU.
    Sky(GeneratedEnvironmentMapLight),
}

/// How bright the environment's colours are, in the units Bevy lights with (the same scale `AmbientBrightness` has).
const ENV_UNITS_PER_BRIGHTNESS: f32 = 3.0;

impl SpawnParams<'_> {
    /// A cube map from six pictures (in Bevy's order: +X, -X, +Y, -Y, +Z, -Z), all brought to one power-of-two size.
    pub fn cubemap(&mut self, faces: &[String; 6], source: &GameSource) -> Option<Handle<Image>> {
        let mut decoded: Vec<Option<texture::Decoded>> = Vec::with_capacity(6);
        for path in faces {
            decoded.push(if path.is_empty() { None } else { source.read(path).ok().and_then(|b| texture::decode(&b, path.rsplit('.').next().unwrap_or("")).ok()) });
        }
        let edge = decoded.iter().flatten().map(|d| d.width.max(d.height)).max()?.clamp(16, 1024).next_power_of_two().min(1024);
        let mut data: Vec<u8> = Vec::with_capacity((edge * edge * 4 * 6) as usize);
        for d in &decoded {
            match d {
                Some(d) => data.extend_from_slice(&texture::resized(d, edge, edge).rgba),
                None => data.extend(std::iter::repeat([0u8, 0, 0, 255]).take((edge * edge) as usize).flatten()),
            }
        }
        let mut image = Image::new(Extent3d { width: edge, height: edge, depth_or_array_layers: 6 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
        image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
        Some(self.images.add(image))
    }
}

/// Turns what the scene says into what cameras get. Loads the sky's pictures.
pub fn resolve(fx: &CameraEffects, assets: &mut SpawnParams, source: &GameSource) -> CameraLook {
    let dof = dof_of(fx.dof.as_ref(), fx.blur);
    let [top, mid, bottom] = hemisphere(&fx.env);
    let intensity = fx.env.ambient_brightness * ENV_UNITS_PER_BRIGHTNESS;
    let color = |v: Vec3| Color::linear_rgb(v.x, v.y, v.z);
    let day = fx.env.day.clamp(0.0, 1.0);

    let sky_image = fx.sky.as_ref().and_then(|s| assets.cubemap(&s.faces, source));
    let (sky, env) = match (&fx.sky, sky_image) {
        (Some(s), Some(image)) => {
            let rotation = Quat::from_euler(EulerRot::YXZ, s.orientation[1].to_radians(), s.orientation[0].to_radians(), s.orientation[2].to_radians());
            (
                Some(Skybox { image: Some(image.clone()), brightness: 1000.0 * (0.05 + 0.95 * day), rotation }),
                Environment::Sky(GeneratedEnvironmentMapLight { environment_map: image, intensity: intensity * (0.05 + 0.95 * day) / 3.0 * fx.env.diffuse_scale.max(fx.env.specular_scale).clamp(0.0, 1.0), rotation, ..default() }),
            )
        }
        _ => {
            // (One picture for the light and one for the shine, so each can be turned down on its own.)
            let (ds, ss) = (fx.env.diffuse_scale.clamp(0.0, 1.0), fx.env.specular_scale.clamp(0.0, 1.0));
            let diffuse = EnvironmentMapLight::hemispherical_gradient(&mut assets.images, color(top * ds), color(mid * ds), color(bottom * ds));
            let specular = if (ds - ss).abs() < 1e-4 { diffuse.clone() } else { EnvironmentMapLight::hemispherical_gradient(&mut assets.images, color(top * ss), color(mid * ss), color(bottom * ss)) };
            (None, Environment::Gradient(EnvironmentMapLight { diffuse_map: diffuse.diffuse_map, specular_map: specular.specular_map, intensity, ..default() }))
        }
    };
    // Exposure compensation is part of the colour grading, together with a colour correction effect.
    let grading = match (&fx.color, fx.env.exposure != 0.0) {
        (None, false) => None,
        (color, _) => {
            let mut g = color.as_ref().map(grading_of).unwrap_or_default();
            g.global.exposure += fx.env.exposure;
            Some(g)
        }
    };
    CameraLook {
        bloom: fx.bloom.as_ref().map(bloom_of),
        grading,
        depth_prepass: dof.is_some(),
        dof,
        fog: fx.fog().map(fog_of),
        sky,
        env,
        sun_rays: fx.sun_rays.clone(),
    }
}

/// Puts [`CameraLook`] on every 3D camera: when it changed, and on cameras that have just appeared.
pub fn apply_camera_look(look: Option<Res<CameraLook>>, mut commands: Commands, cameras: Query<Entity, With<Camera3d>>, fresh: Query<Entity, Added<Camera3d>>, decals: Query<(), With<bevy::pbr::decal::ForwardDecal>>) {
    let Some(look) = look else { return };
    if !look.is_changed() && fresh.is_empty() {
        return;
    }
    for e in &cameras {
        let mut cam = commands.entity(e);
        match &look.bloom {
            Some(b) => {
                cam.insert((b.clone(), Tonemapping::TonyMcMapface));
            }
            None => {
                cam.remove::<(Bloom, Hdr)>();
            }
        }
        match &look.grading {
            Some(g) => cam.insert(g.clone()),
            None => cam.insert(ColorGrading::default()),
        };
        match &look.dof {
            Some(d) => cam.insert(*d),
            None => cam.remove::<DepthOfField>(),
        };
        match &look.fog {
            Some(f) => cam.insert(f.clone()),
            None => cam.remove::<DistanceFog>(),
        };
        match &look.sky {
            Some(s) => cam.insert(s.clone()),
            None => cam.remove::<Skybox>(),
        };
        match &look.env {
            Environment::Gradient(l) => {
                cam.remove::<GeneratedEnvironmentMapLight>();
                cam.insert(l.clone())
            }
            Environment::Sky(l) => {
                cam.remove::<EnvironmentMapLight>();
                cam.insert(l.clone())
            }
        };
        if look.depth_prepass || !decals.is_empty() {
            cam.insert(DepthPrepass);
        }
        match &look.sun_rays {
            Some(_) => cam.insert(VolumetricFog { ambient_intensity: 0.0, ..default() }),
            None => cam.remove::<VolumetricFog>(),
        };
    }
}

/// The fog volume that makes the sun's light shafts (one box around the world, made when the effect is on).
#[derive(Component)]
pub struct SunRaysVolume;

/// Keeps a single big fog volume while `SunRaysEffect` is on, and removes it when it is not.
pub fn sync_sun_rays(look: Option<Res<CameraLook>>, mut commands: Commands, volumes: Query<Entity, With<SunRaysVolume>>) {
    let Some(look) = look else { return };
    if !look.is_changed() {
        return;
    }
    match (&look.sun_rays, volumes.single()) {
        (Some(rays), Ok(e)) => {
            commands.entity(e).insert(FogVolume { density_factor: 0.004 + 0.03 * rays.intensity, scattering_asymmetry: 0.3 + 0.6 * rays.spread, ..default() });
        }
        (Some(rays), Err(_)) => {
            commands.spawn((Name::new("Sun rays"), SunRaysVolume, FogVolume { density_factor: 0.004 + 0.03 * rays.intensity, scattering_asymmetry: 0.3 + 0.6 * rays.spread, ..default() }, Transform::from_scale(Vec3::splat(2000.0))));
        }
        (None, Ok(e)) => commands.entity(e).despawn(),
        (None, Err(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_correction_maps_to_grading_numbers() {
        let g = grading_of(&ColorFx { brightness: 0.5, contrast: 0.25, saturation: -1.0, tint: [1.0, 1.0, 1.0] });
        assert_eq!(g.global.exposure, 1.0);
        assert_eq!(g.global.post_saturation, 0.0, "saturation -1 is grey");
        assert_eq!((g.midtones.contrast, g.shadows.contrast, g.highlights.contrast), (1.25, 1.25, 1.25));
        assert!(g.global.temperature.abs() < 1e-6 && g.global.tint.abs() < 1e-6, "a white tint changes nothing");
        let warm = grading_of(&ColorFx { brightness: 0.0, contrast: 0.0, saturation: 0.0, tint: [1.0, 0.9, 0.6] });
        assert!(warm.global.temperature > 0.0, "a warm tint makes the picture redder");
    }

    #[test]
    fn bloom_follows_its_three_numbers() {
        let weak = bloom_of(&BloomFx { intensity: 0.5, size: 24.0, threshold: 1.0 });
        let strong = bloom_of(&BloomFx { intensity: 2.0, size: 56.0, threshold: 4.0 });
        assert!(strong.intensity > weak.intensity && strong.prefilter.threshold > weak.prefilter.threshold);
        assert!(strong.scale.x > weak.scale.x && strong.max_mip_dimension >= weak.max_mip_dimension);
        assert_eq!(bloom_of(&BloomFx { intensity: 0.0, size: 24.0, threshold: 0.0 }).intensity, 0.0);
    }

    #[test]
    fn depth_of_field_and_blur_are_one_effect_with_different_focus() {
        let dof = dof_of(Some(&DofFx { focus_distance: 30.0, in_focus_radius: 10.0, near: 1.0, far: 1.0 }), None).unwrap();
        assert_eq!(dof.focal_distance, 30.0);
        let soft = dof_of(Some(&DofFx { focus_distance: 30.0, in_focus_radius: 10.0, near: 0.1, far: 0.1 }), None).unwrap();
        assert!(soft.aperture_f_stops > dof.aperture_f_stops, "less intensity is a smaller aperture, so less blur");
        let blur = dof_of(None, Some(30.0)).unwrap();
        assert!(blur.focal_distance < 1.0, "everything is out of focus");
        assert!(dof_of(None, Some(0.0)).is_none() && dof_of(None, None).is_none());
    }

    #[test]
    fn fog_is_linear_or_atmospheric() {
        let linear = fog_of(&FogFx { color: [0.5, 0.5, 0.5], kind: FogKind::Linear { start: 10.0, end: 200.0 }, glare: 0.0 });
        assert!(matches!(linear.falloff, FogFalloff::Linear { start, end } if start == 10.0 && end == 200.0));
        let haze = fog_of(&FogFx { color: [0.8, 0.7, 0.4], kind: FogKind::Atmospheric { visibility: 900.0, extinction: [0.4, 0.3, 0.1] }, glare: 4.0 });
        assert!(matches!(haze.falloff, FogFalloff::Atmospheric { .. }));
        assert!(haze.directional_light_color.alpha() > 0.0 && linear.directional_light_color.alpha() == 0.0, "glare lights the haze around the sun");
        let bad = fog_of(&FogFx { color: [0.0; 3], kind: FogKind::Linear { start: 50.0, end: 10.0 }, glare: 0.0 });
        assert!(matches!(bad.falloff, FogFalloff::Linear { start, end } if end > start), "an end before the start is repaired");
    }

    #[test]
    fn the_hemisphere_darkens_at_night_and_is_lightest_at_the_top() {
        let day = hemisphere(&EnvFx::default());
        let night = hemisphere(&EnvFx { day: 0.0, ..EnvFx::default() });
        assert!(day[0].length() > night[0].length() * 5.0);
        assert!(day[0].length() > day[2].length(), "the sky is brighter than the ground");
    }

    #[test]
    fn the_effects_of_a_scene_are_collected_from_the_objects_that_say_them() {
        let mut dm = DataModel::new();
        let lighting = dm.service("Lighting").unwrap();
        let fx = collect(&dm);
        assert!(fx.bloom.is_none() && fx.sky.is_none() && fx.fog().is_none(), "a new scene has no effects");
        assert_eq!(fx.env.ambient_brightness, 350.0, "but it has its lighting");
        for class in ["BloomEffect", "ColorCorrectionEffect", "DepthOfFieldEffect", "BlurEffect", "Atmosphere", "Sky", "SunRaysEffect"] {
            let id = dm.create(class).unwrap();
            dm.set_parent(id, Some(lighting)).unwrap();
        }
        let fx = collect(&dm);
        assert!(fx.bloom.is_some() && fx.color.is_some() && fx.dof.is_some() && fx.blur.is_some() && fx.atmosphere.is_some() && fx.sun_rays.is_some());
        // The sky has no pictures yet: it is not a sky.
        assert!(fx.sky.is_none());
        // Switching an effect off removes it.
        let bloom = dm.find_by_class(lighting, "BloomEffect", false, true).unwrap();
        dm.set_prop(bloom, "Enabled", crate::core::Value::Bool(false)).unwrap();
        assert!(collect(&dm).bloom.is_none());
        // Lighting's own fog takes effect once its end is brought in; the atmosphere wins over it.
        dm.set_prop(lighting, "FogEnd", crate::core::Value::Num(500.0)).unwrap();
        assert!(matches!(collect(&dm).lighting_fog, Some(FogFx { kind: FogKind::Linear { end, .. }, .. }) if end == 500.0));
        assert!(matches!(collect(&dm).fog().unwrap().kind, FogKind::Atmospheric { .. }));
    }
}

//! Lighting: the sun (driven by the time of day), the light of the sky and ground, fog and the sky colour.
//!
//! `Environment` mirrors the service into Bevy: one directional light for the sun and the clear colour. The sky's light (the
//! hemisphere of SkyColor above and Ambient below that lights shadowed surfaces and shows in reflections) and the fog are
//! given to every camera through `render::camera_fx`, as are the effects (bloom, colour correction ...) and the Sky and
//! Atmosphere objects inside this service.

use bevy::{light::CascadeShadowConfigBuilder, prelude::*};

use crate::{
    compat,
    core::{ClassDesc, ComponentDesc, EnumDesc, MethodCall, MethodDesc, PropMeta, ScriptValue, Value, F_VISUAL},
    dm::{DataModel, Id},
    render::{
        camera_fx::{CameraEffects, EnvFx, FogFx, FogKind},
        entity_of, set_entity, srgb, RenderCtx,
    },
};

type R = Result<ScriptValue, String>;

// ---- Roblox's view of the time ----------------------------------------------------------------------

/// `TimeOfDay` is the clock as "HH:MM:SS"; writing it sets `ClockTime`.
fn get_time_of_day(dm: &DataModel, id: Id) -> Option<Value> {
    let seconds = (dm.num(id, "ClockTime", 14.0).rem_euclid(24.0) * 3600.0).round() as u32 % 86_400;
    Some(Value::Str(format!("{:02}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)))
}

fn set_time_of_day(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Str(text) = v else { return Err("TimeOfDay must be a string like \"14:30:00\"".into()) };
    let parts: Vec<f32> = text.split(':').map(|p| p.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|_| format!("'{text}' is not a time of day (use HH:MM:SS)"))?;
    let [h, m, s] = match parts.as_slice() {
        [h] => [*h, 0.0, 0.0],
        [h, m] => [*h, *m, 0.0],
        [h, m, s] => [*h, *m, *s],
        _ => return Err(format!("'{text}' is not a time of day (use HH:MM:SS)")),
    };
    dm.set_prop(id, "ClockTime", Value::Num((h + m / 60.0 + s / 3600.0).rem_euclid(24.0)))
}

/// Where the sun is, as a unit vector from the world toward it (the renderer lights the world from here).
pub fn sun_direction(clock: f32) -> bevy::math::Vec3 {
    let angle = (clock - 6.0) / 12.0 * std::f32::consts::PI;
    bevy::math::Vec3::new(angle.cos(), angle.sin(), 0.3).normalize()
}

fn get_minutes(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Num(f64::from(c.dm.num(c.this, "ClockTime", 14.0) * 60.0)))
}

fn set_minutes(c: &mut MethodCall) -> R {
    let minutes = c.number(0)? as f32;
    c.dm.set_prop(c.this, "ClockTime", Value::Num((minutes / 60.0).rem_euclid(24.0)))?;
    Ok(ScriptValue::Nil)
}

fn get_sun_direction(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Vec3(sun_direction(c.dm.num(c.this, "ClockTime", 14.0))))
}

fn get_moon_direction(c: &mut MethodCall) -> R {
    Ok(ScriptValue::Vec3(-sun_direction(c.dm.num(c.this, "ClockTime", 14.0))))
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("GetMinutesAfterMidnight", get_minutes).desc("The time of day in minutes."),
    MethodDesc::new("SetMinutesAfterMidnight", set_minutes).desc("Sets the time of day in minutes."),
    MethodDesc::new("GetSunDirection", get_sun_direction).desc("A unit vector pointing from the world toward the sun."),
    MethodDesc::new("GetMoonDirection", get_moon_direction).desc("A unit vector pointing from the world toward the moon."),
];

pub static ENUMS: &[EnumDesc] = &[
    EnumDesc::numbered("Technology", &["Legacy", "Voxel", "Compatibility", "ShadowMap", "Future"], &[0, 1, 2, 3, 4]),
    EnumDesc::new("LightingStyle", &["Realistic", "Soft"]),
    EnumDesc::new("EnvironmentalSimulation", &["Off", "On"]),
];

static PROPS: &[PropMeta] = &[
    PropMeta::number("ClockTime", 14.0).group("Lighting").order(0).flags(F_VISUAL).slider(0.0, 24.0).desc("Time of day in hours: 6 sunrise, 12 noon, 18 sunset."),
    PropMeta::number("Brightness", 2.0).group("Lighting").order(1).flags(F_VISUAL).drag(0.05).clamp(0.0, 100.0).desc("Brightness of the sun."),
    PropMeta::boolean("GlobalShadows", true).group("Lighting").order(2).flags(F_VISUAL).desc("Whether the sun casts shadows."),
    PropMeta::color3("SkyColor", 0.53, 0.72, 0.95).group("Sky").order(0).flags(F_VISUAL).desc("Colour of the sky at midday (it darkens at night)."),
    PropMeta::color3("Ambient", 0.7, 0.75, 0.85).group("Sky").order(1).flags(F_VISUAL).desc("Colour of the light that reaches shadowed areas."),
    PropMeta::number("AmbientBrightness", 350.0).group("Sky").order(2).flags(F_VISUAL).drag(5.0).clamp(0.0, 100_000.0).desc("Strength of the ambient light."),
    PropMeta::string("TimeOfDay", "14:00:00").group("Lighting").order(3).no_save().desc("ClockTime as hours:minutes:seconds.").computed(get_time_of_day, set_time_of_day),
    PropMeta::color3("OutdoorAmbient", 0.5, 0.5, 0.5).group("Sky").order(3).hidden().desc("Roblox's ambient light outdoors (kept for scripts; the renderer uses Ambient)."),
    PropMeta::color3("ColorShift_Top", 0.0, 0.0, 0.0).group("Sky").order(4).hidden().desc("Tints lit surfaces (kept for scripts; not drawn)."),
    PropMeta::color3("ColorShift_Bottom", 0.0, 0.0, 0.0).group("Sky").order(5).hidden().desc("Tints shadowed surfaces (kept for scripts; not drawn)."),
    PropMeta::color3("FogColor", 0.75, 0.75, 0.75).group("Fog").order(0).flags(F_VISUAL).desc("The colour of the fog."),
    PropMeta::number("FogStart", 0.0).group("Fog").order(1).flags(F_VISUAL).drag(10.0).clamp(0.0, 1.0e7).desc("Where the fog begins."),
    PropMeta::number("FogEnd", 100_000.0).group("Fog").order(2).flags(F_VISUAL).drag(10.0).clamp(0.0, 1.0e7).desc("Where the fog is thickest. At 100000 or more there is no fog; an Atmosphere in Lighting replaces this fog."),
    PropMeta::number("GeographicLatitude", 41.733).group("Lighting").order(4).drag(0.5).clamp(-90.0, 90.0).hidden().desc("Where on Earth the game is, for the sun's path (kept for scripts)."),
    PropMeta::number("ExposureCompensation", 0.0).group("Lighting").order(5).flags(F_VISUAL).drag(0.05).clamp(-5.0, 5.0).desc("Makes the picture brighter or darker, in stops of light."),
    PropMeta::number("EnvironmentDiffuseScale", 1.0).group("Lighting").order(6).flags(F_VISUAL).slider(0.0, 1.0).desc("How much the sky lights the surfaces (the light of the Ambient and sky colours; 0 turns it off)."),
    PropMeta::number("EnvironmentSpecularScale", 1.0).group("Lighting").order(7).flags(F_VISUAL).slider(0.0, 1.0).desc("How much the sky shines off shiny surfaces (reflections; 0 turns them off)."),
    PropMeta::number("ShadowSoftness", 0.2).group("Lighting").order(8).slider(0.0, 1.0).hidden().desc("How soft the edges of shadows are (kept for scripts)."),
    PropMeta::enumeration("Technology", "Technology", "ShadowMap").group("Lighting").order(9).hidden().desc("Which lighting engine Roblox would use (informational: Forge has one)."),
    PropMeta::boolean("PrioritizeLightingQuality", false).group("Lighting").order(10).hidden(),
];

// ---- rendering ----------------------------------------------------------------------------------

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, _bits: u8) {
    let clock = dm.num(id, "ClockTime", 14.0);
    let brightness = dm.num(id, "Brightness", 2.0);
    let shadows = dm.flag(id, "GlobalShadows");
    let sky = dm.vec3(id, "SkyColor");
    let ambient = dm.vec3(id, "Ambient");
    let ambient_brightness = dm.num(id, "AmbientBrightness", 350.0);

    // 6:00 sunrise, 12:00 overhead, 18:00 sunset.
    let angle = (clock - 6.0) / 12.0 * std::f32::consts::PI;
    let height = angle.sin();
    let t = (height / 0.25).clamp(0.0, 1.0);
    let day = t * t * (3.0 - 2.0 * t);
    let to_sun = sun_direction(clock);

    let sun = DirectionalLight { color: Color::srgb(1.0, 0.96, 0.9), illuminance: brightness * 5000.0 * day, shadow_maps_enabled: shadows && day > 0.0, ..default() };
    let transform = Transform::from_rotation(Quat::from_rotation_arc(Vec3::NEG_Z, -to_sun));
    match entity_of(dm, id) {
        Some(e) => compat::insert(ctx.commands, e, (sun, transform)),
        None => {
            let cascade = CascadeShadowConfigBuilder { maximum_distance: 600.0, first_cascade_far_bound: 40.0, num_cascades: 3, ..default() }.build();
            let e = ctx.commands.spawn((Name::new("Sun"), sun, transform, cascade)).id();
            set_entity(dm, id, Some(e));
        }
    }
    ctx.commands.insert_resource(ClearColor(srgb(sky * (0.03 + 0.97 * day))));
    // The light of the sky and the ground that reaches shadowed surfaces comes from the cameras' environment light (see
    // `render::camera_fx`), so the global ambient light is off: it would count the same light twice.
    let _ = (ambient, ambient_brightness);
    ctx.commands.insert_resource(GlobalAmbientLight { color: srgb(ambient), brightness: 0.0, ..default() });
}

/// The sky and ground colours that light shadowed surfaces, and the fog of Fog*: what the cameras need from this service.
fn camera_effect(dm: &DataModel, id: Id, fx: &mut CameraEffects) {
    let clock = dm.num(id, "ClockTime", 14.0);
    let t = ((clock - 6.0) / 12.0 * std::f32::consts::PI).sin() / 0.25;
    let t = t.clamp(0.0, 1.0);
    fx.env = EnvFx {
        sky_color: dm.vec3(id, "SkyColor").to_array(),
        ambient: dm.vec3(id, "Ambient").to_array(),
        ambient_brightness: dm.num(id, "AmbientBrightness", 350.0),
        day: t * t * (3.0 - 2.0 * t),
        diffuse_scale: dm.num(id, "EnvironmentDiffuseScale", 1.0),
        specular_scale: dm.num(id, "EnvironmentSpecularScale", 1.0),
        exposure: dm.num(id, "ExposureCompensation", 0.0),
    };
    let end = dm.num(id, "FogEnd", 100_000.0);
    if end < 99_999.0 {
        fx.lighting_fog = Some(FogFx { color: dm.vec3(id, "FogColor").to_array(), kind: FogKind::Linear { start: dm.num(id, "FogStart", 0.0), end }, glare: 0.0 });
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("Environment").describe("Sun, ambient light and sky colour.").render(render).camera_effect(camera_effect);

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("Lighting", Some("Instance"))
    .describe("Sun, ambient light and sky.")
    .service(2)
    .persisted()
    .groups(&["Lighting", "Sky", "Fog"])
    .components(&["Environment", "CameraEffect"])
    .props(PROPS)
    .methods(METHODS);

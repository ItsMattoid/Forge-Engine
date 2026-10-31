//! Appearance: how a surface looks. The renderers (primitive / mesh) read these.
//!
//! `BrickColor` is the same colour as `Color`, snapped to Roblox's palette: setting it sets `Color`, reading it gives the
//! palette entry nearest to `Color`.

use crate::{core::*, dm::DataModel};

fn get_brick_color(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Data("BrickColor", vec![f32::from(brickcolor::nearest(dm.vec3(id, "Color")).number)]))
}

fn set_brick_color(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Data(_, numbers) = v else { return Err("BrickColor must be a BrickColor".into()) };
    let number = numbers.first().copied().unwrap_or(0.0) as u16;
    let entry = brickcolor::by_number(number).ok_or_else(|| format!("{number} is not a BrickColor"))?;
    dm.set_prop(id, "Color", Value::Color(entry.color()))
}

static PROPS: &[PropMeta] = &[
    PropMeta::color3("Color", 0.64, 0.64, 0.64).group("Appearance").order(0).flags(F_VISUAL).desc("Surface colour."),
    PropMeta::data("BrickColor", "BrickColor", &[194.0]).group("Appearance").order(1).no_save().hidden().desc("Color as the nearest BrickColor.").computed(get_brick_color, set_brick_color),
    PropMeta::number("Transparency", 0.0).group("Appearance").order(2).flags(F_VISUAL).slider(0.0, 1.0).desc("0 is solid, 1 is invisible."),
    PropMeta::number("Reflectance", 0.0).group("Appearance").order(3).flags(F_VISUAL).slider(0.0, 1.0).desc("How mirror-like the surface is: the part becomes more metallic and smoother, so it reflects the sky."),
    PropMeta::enumeration("Material", "Material", "Plastic").group("Appearance").order(4).flags(F_VISUAL).desc("The kind of surface. Each item is the material file of the same name in the project (Plastic.mat ...), or the engine's look for it when the project has none; edit them in the Material Editor."),
    PropMeta::asset("MaterialVariant", "mat")
        .label("Material Asset")
        .group("Appearance")
        .order(5)
        .flags(F_VISUAL)
        .desc("A material file (.mat) to draw the part with instead of the one its Material names: drop one here, pick one, or type a material's name. Edit it in the Material Editor."),
    PropMeta::asset("ColorMap", "png")
        .label("Texture")
        .group("Appearance")
        .order(6)
        .flags(F_VISUAL)
        .aliases(&["TextureID"])
        .desc("A picture (png, jpg ...) laid over the part, multiplied with its Color: drop one here or pick one. It replaces the material's own colour map."),
    PropMeta::boolean("CastShadow", true).group("Appearance").order(7).flags(F_VISUAL).desc("Whether the object casts shadows."),
    PropMeta::number("LocalTransparencyModifier", 0.0).group("Appearance").order(8).runtime().hidden().desc("Extra transparency for this player only (kept for scripts; not drawn)."),
];

pub static COMPONENT: ComponentDesc = ComponentDesc::new("Appearance").describe("Colour, material and transparency.").props(PROPS);

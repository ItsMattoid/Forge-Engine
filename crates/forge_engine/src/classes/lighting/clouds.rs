//! Clouds: a layer of clouds drawn as one very large sheet high over the camera (`render::effects`), fading out toward the
//! horizon and drifting with the wind. `Cover` is how much of the sky is covered, `Density` how thick the clouds are and `Color`
//! their colour. One layer, at a fixed height; the clouds are not lit by the sun and cast no shadows.

use crate::{
    core::*,
    dm::DataModel,
    render::{
        effects::{self, CloudSpec},
        RenderCtx,
    },
};

static PROPS: &[PropMeta] = &[
    PropMeta::number("Cover", 0.5).group("Clouds").order(0).flags(F_VISUAL).slider(0.0, 1.0).desc("How much of the sky is covered."),
    PropMeta::number("Density", 0.7).group("Clouds").order(1).flags(F_VISUAL).slider(0.0, 1.0).desc("How thick the clouds are."),
    PropMeta::color3("Color", 1.0, 1.0, 1.0).group("Clouds").order(2).flags(F_VISUAL).desc("The colour of the clouds."),
    PropMeta::boolean("Enabled", true).group("Clouds").order(3).flags(F_VISUAL).desc("Whether the clouds are shown."),
];

/// Whether the object is somewhere the scene looks at (the world, or the lighting settings).
fn in_the_scene(dm: &DataModel, id: Id) -> bool {
    dm.in_workspace(id) || dm.service("Lighting").is_some_and(|l| dm.is_descendant_of(id, l))
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    if !dm.flag(id, "Enabled") || !in_the_scene(dm, id) {
        return ctx.fx_clear(id);
    }
    let spec = CloudSpec { cover: dm.num(id, "Cover", 0.5), density: dm.num(id, "Density", 0.7), color: dm.vec3(id, "Color") };
    let name = dm.name(id);
    effects::run_clouds(ctx, id, &name, &spec);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("CloudsFx").describe("Draws the sheet of clouds (render::effects).").animate(animate);

pub static CLASS: ClassDesc = ClassDesc::new("Clouds", Some("Instance"))
    .describe("A layer of clouds.")
    .category("Lighting")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("Lighting"))
    .groups(&["Clouds"])
    .components(&["Animated", "CloudsFx"])
    .props(PROPS);

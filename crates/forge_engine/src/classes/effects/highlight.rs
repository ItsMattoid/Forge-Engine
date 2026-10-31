//! Highlight: a tint over an object and an outline around it. The adornee is a part, or a model (every part in it); with none, the
//! highlight's parent. The tint is a copy of each part's mesh drawn over it in `FillColor`; the outline is a slightly bigger
//! copy drawn behind it in `OutlineColor`, which shows as a band around the silhouette about as wide on the screen whatever
//! the distance. With `DepthMode` `AlwaysOnTop` the tint shows through whatever is in front of the object; the outline is
//! hidden by what is in front, as it is in `Occluded`. A highlight does not light anything and is not shaded.

use std::collections::{HashMap, HashSet};

use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    pbr::StandardMaterial,
    prelude::*,
};

use crate::{
    core::*,
    dm::DataModel,
    render::{drawn, HighlightMaterial, OnTop, RenderCtx},
};

pub static ENUMS: &[EnumDesc] = &[EnumDesc::new("HighlightDepthMode", &["AlwaysOnTop", "Occluded"])];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Highlight").order(0).flags(F_VISUAL).desc("Whether the highlight is shown."),
    PropMeta::reference("Adornee", "Instance").group("Highlight").order(1).flags(F_VISUAL).desc("What is highlighted: a part, or a model with all its parts (the highlight's parent when empty)."),
    PropMeta::color3("FillColor", 1.0, 0.0, 0.0).group("Highlight").order(2).flags(F_VISUAL).desc("The tint of the object."),
    PropMeta::number("FillTransparency", 0.5).group("Highlight").order(3).flags(F_VISUAL).slider(0.0, 1.0).desc("How see-through the tint is."),
    PropMeta::color3("OutlineColor", 1.0, 1.0, 1.0).group("Highlight").order(4).flags(F_VISUAL).desc("The colour of the outline."),
    PropMeta::number("OutlineTransparency", 0.0).group("Highlight").order(5).flags(F_VISUAL).slider(0.0, 1.0).desc("How see-through the outline is."),
    PropMeta::enumeration("DepthMode", "HighlightDepthMode", "AlwaysOnTop")
        .group("Highlight")
        .order(6)
        .desc("Whether the tint shows through whatever is in front of the object (the outline never does)."),
];

/// The parts a highlight marks.
pub fn adornee_parts(dm: &DataModel, id: Id) -> Vec<Id> {
    let target = dm.reference(id, "Adornee").or_else(|| dm.parent(id));
    let Some(target) = target else { return Vec::new() };
    if dm.is_a(target, "BasePart") {
        return vec![target];
    }
    if dm.is_a(target, "Model") {
        return dm.subtree(target).into_iter().filter(|p| dm.is_a(*p, "BasePart")).collect();
    }
    Vec::new()
}

/// What the highlight's two materials were last made from.
#[derive(Clone, Copy, PartialEq, Default)]
struct Look {
    fill: [f32; 4],
    outline: [f32; 4],
    on_top: bool,
}

/// What is drawn for one part.
struct Shown {
    mesh: AssetId<Mesh>,
    fill: Vec<Entity>,
    outline: Vec<Entity>,
}

#[derive(Default)]
struct State {
    parts: HashMap<Id, Shown>,
    fill: Option<Handle<HighlightMaterial>>,
    outline: Option<Handle<HighlightMaterial>>,
    look: Option<Look>,
}

fn material(color: [f32; 4], on_top: bool, cull: Option<bevy::render::render_resource::Face>) -> HighlightMaterial {
    HighlightMaterial {
        base: StandardMaterial { base_color: Color::srgba(color[0], color[1], color[2], color[3]), unlit: true, alpha_mode: AlphaMode::Blend, cull_mode: cull, double_sided: false, ..default() },
        extension: OnTop { always: on_top },
    }
}

/// How far the outline reaches out from the part, in studs: about the same on the screen at any distance.
pub fn outline_width(distance: f32) -> f32 {
    (distance * 0.008).clamp(0.03, 1.5)
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    let parts = if dm.flag(id, "Enabled") && dm.in_workspace(id) { adornee_parts(dm, id) } else { Vec::new() };
    let (fill_alpha, outline_alpha) = (1.0 - dm.num(id, "FillTransparency", 0.5), 1.0 - dm.num(id, "OutlineTransparency", 0.0));
    let (want_fill, want_outline) = (fill_alpha > 0.0, outline_alpha > 0.0);
    if parts.is_empty() || !(want_fill || want_outline) {
        return ctx.fx_clear(id);
    }
    let (fill, outline) = (dm.vec3(id, "FillColor"), dm.vec3(id, "OutlineColor"));
    let look = Look { fill: [fill.x, fill.y, fill.z, fill_alpha.min(1.0)], outline: [outline.x, outline.y, outline.z, outline_alpha.min(1.0)], on_top: dm.str_ref(id, "DepthMode") != "Occluded" };

    let mut st: State = std::mem::take(ctx.fx_state(id));
    if st.look != Some(look) {
        let (fill_material, outline_material) = (material(look.fill, look.on_top, None), material(look.outline, false, Some(bevy::render::render_resource::Face::Front)));
        match (&st.fill, &st.outline) {
            (Some(f), Some(o)) => {
                if let Some(mut m) = ctx.assets.highlight_materials.get_mut(f) {
                    *m = fill_material;
                }
                if let Some(mut m) = ctx.assets.highlight_materials.get_mut(o) {
                    *m = outline_material;
                }
            }
            _ => {
                st.fill = Some(ctx.assets.highlight_materials.add(fill_material));
                st.outline = Some(ctx.assets.highlight_materials.add(outline_material));
            }
        }
        st.look = Some(look);
    }
    let (Some(fill_material), Some(outline_material)) = (st.fill.clone(), st.outline.clone()) else { return };

    let mut live: HashSet<Id> = HashSet::new();
    for part in parts {
        // A part that is not drawn (hidden, or not in the world) has nothing to highlight.
        let Some(d) = drawn(dm, part) else { continue };
        let Some((position, _)) = dm.world_cframe(part) else { continue };
        live.insert(part);
        let stale = st.parts.get(&part).is_none_or(|s| s.mesh != d.mesh || s.fill.is_empty() == want_fill || s.outline.is_empty() == want_outline);
        if stale {
            if let Some(old) = st.parts.remove(&part) {
                for e in old.fill.into_iter().chain(old.outline) {
                    ctx.fx_release(id, e);
                }
            }
            let model = if dm.is_a(part, "MeshPart") { ctx.assets.file_model(dm.str_ref(part, "MeshId"), ctx.source) } else { ctx.assets.shape_model(dm.str_ref(part, "Shape")) };
            let mut shown = Shown { mesh: d.mesh, fill: Vec::new(), outline: Vec::new() };
            for piece in &model.pieces {
                for (list, material, wanted, name) in [(&mut shown.fill, &fill_material, want_fill, "Highlight fill"), (&mut shown.outline, &outline_material, want_outline, "Highlight outline")] {
                    if !wanted {
                        continue;
                    }
                    let e =
                        ctx.commands.spawn((Name::new(name), Mesh3d(piece.mesh.clone()), MeshMaterial3d(material.clone()), d.transform, Visibility::Visible, NotShadowCaster, NotShadowReceiver)).id();
                    ctx.fx_own(id, e);
                    list.push(e);
                }
            }
            st.parts.insert(part, shown);
        }
        let Some(shown) = st.parts.get(&part) else { continue };
        let native = d.native.max(Vec3::splat(1e-4));
        let reach = outline_width(ctx.opts.camera.map_or(30.0, |c| c.distance(position)));
        // The tint a hair bigger than the part so it does not flicker against its surface; the outline as far out as it reaches.
        let (fill_at, outline_at) =
            (Transform { scale: d.transform.scale + Vec3::splat(0.02) / native, ..d.transform }, Transform { scale: d.transform.scale + Vec3::splat(2.0 * reach) / native, ..d.transform });
        for e in &shown.fill {
            ctx.set_transform(*e, fill_at);
        }
        for e in &shown.outline {
            ctx.set_transform(*e, outline_at);
        }
    }
    // Parts that stopped being highlighted (taken out of the model, hidden, no longer in the world).
    let gone: Vec<Id> = st.parts.keys().filter(|p| !live.contains(*p)).copied().collect();
    for part in gone {
        if let Some(old) = st.parts.remove(&part) {
            for e in old.fill.into_iter().chain(old.outline) {
                ctx.fx_release(id, e);
            }
        }
    }
    *ctx.fx_state(id) = st;
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("HighlightFx").describe("Draws the tint and the outline of the highlight (render::effects).").animate(animate);

pub static CLASS: ClassDesc = ClassDesc::new("Highlight", Some("Instance"))
    .describe("An outline and tint around an object.")
    .category("Effects")
    .creatable()
    .groups(&["Highlight"])
    .components(&["Animated", "HighlightFx"])
    .props(PROPS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outline_is_wider_far_away_but_never_vanishes_or_swallows_the_part() {
        assert_eq!(outline_width(0.0), 0.03);
        assert!(outline_width(100.0) > outline_width(10.0));
        assert_eq!(outline_width(1.0e6), 1.5);
    }
}

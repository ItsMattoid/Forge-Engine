//! Beam: a band between two attachments, drawn as a ribbon (`render::effects`). It runs from `Attachment0` to `Attachment1`,
//! leaving each along the attachment's X axis as far as `CurveSize0` and `CurveSize1` bend it, `Segments` pieces long, as wide as
//! `Width0` at the start and `Width1` at the end, with `Color` and `Transparency` changing along its length. `Texture` is a
//! project picture drawn along it (`TextureMode` says how it fits, `TextureLength` how long one copy is, `TextureSpeed` how
//! fast it slides). A `LightEmission` of half or more adds the beam's light to what is behind it. `FaceCamera` keeps the band
//! turned to the camera; otherwise it is as wide along the attachments' Y axes. Beams are not shaded by the world's lights.

use bevy::math::{Quat, Vec3};

use crate::{
    core::*,
    dm::DataModel,
    render::{
        effects::{self, BeamSpec, ColorSeq, Look, NumberSeq, Procedural, TextureMode as Mode},
        RenderCtx,
    },
};

pub static ENUMS: &[EnumDesc] = &[EnumDesc::new("TextureMode", &["Stretch", "Wrap", "Static"])];

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Beam").order(0).flags(F_VISUAL).desc("Whether the beam is shown."),
    PropMeta::reference("Attachment0", "Attachment").group("Beam").order(1).flags(F_VISUAL).desc("Where the beam starts."),
    PropMeta::reference("Attachment1", "Attachment").group("Beam").order(2).flags(F_VISUAL).desc("Where the beam ends."),
    PropMeta::data("Color", "ColorSequence", &[0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]).group("Beam").order(3).desc("The colour along the beam."),
    PropMeta::data("Transparency", "NumberSequence", &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]).group("Beam").order(4).desc("How see-through the beam is along its length."),
    PropMeta::number("Width0", 1.0).group("Beam").order(5).drag(0.1).clamp(0.0, 1.0e4).desc("The width at the start."),
    PropMeta::number("Width1", 1.0).group("Beam").order(6).drag(0.1).clamp(0.0, 1.0e4).desc("The width at the end."),
    PropMeta::integer("Segments", 10).group("Beam").order(7).clamp(1.0, 300.0).desc("How many pieces the beam is made of (more is smoother)."),
    PropMeta::number("CurveSize0", 0.0).group("Beam").order(8).drag(0.1).desc("How far the start curves away from the line, along the attachment's X axis."),
    PropMeta::number("CurveSize1", 0.0).group("Beam").order(9).drag(0.1).desc("How far the end curves away from the line, along the attachment's X axis."),
    PropMeta::boolean("FaceCamera", false).group("Beam").order(10).desc("Whether the beam always faces the camera."),
    PropMeta::number("LightEmission", 0.0).group("Beam").order(11).slider(0.0, 1.0).desc("How much the beam glows: from half, its light is added to what is behind it."),
    PropMeta::number("LightInfluence", 0.0).group("Beam").order(12).slider(0.0, 1.0).hidden().desc("Kept for scripts: beams are not shaded by the world's lights."),
    PropMeta::asset("Texture", "png").group("Beam").order(13).desc("A picture drawn along the beam."),
    PropMeta::number("TextureLength", 1.0).group("Beam").order(14).drag(0.1).clamp(0.0, 1.0e4).desc("How long one copy of the picture is."),
    PropMeta::enumeration("TextureMode", "TextureMode", "Stretch").group("Beam").order(15).desc("How the picture fits the beam."),
    PropMeta::number("TextureSpeed", 1.0).group("Beam").order(16).drag(0.1).desc("How fast the picture slides along the beam."),
    PropMeta::number("ZOffset", 0.0).group("Beam").order(17).drag(0.1).desc("Moves the beam toward or away from the camera."),
];

fn end_of(dm: &DataModel, id: Id, name: &str) -> Option<(Vec3, Quat)> {
    let at = dm.reference(id, name)?;
    dm.in_workspace(at).then(|| dm.world_cframe(at)).flatten()
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    let ends = end_of(dm, id, "Attachment0").zip(end_of(dm, id, "Attachment1"));
    let (Some((start, end)), true) = (ends, dm.flag(id, "Enabled") && dm.in_workspace(id)) else { return ctx.fx_clear(id) };
    let texture = dm.string(id, "Texture");
    let spec = BeamSpec {
        start,
        end,
        width: (dm.num(id, "Width0", 1.0), dm.num(id, "Width1", 1.0)),
        segments: dm.num(id, "Segments", 10.0).round().max(1.0) as usize,
        curve: (dm.num(id, "CurveSize0", 0.0), dm.num(id, "CurveSize1", 0.0)),
        color: ColorSeq::read(dm, id, "Color"),
        transparency: NumberSeq::read(dm, id, "Transparency", 0.0),
        face_camera: dm.flag(id, "FaceCamera"),
        z_offset: dm.num(id, "ZOffset", 0.0),
        texture_length: dm.num(id, "TextureLength", 1.0),
        texture_mode: Mode::from_name(dm.str_ref(id, "TextureMode")),
        texture_speed: if texture.is_empty() { 0.0 } else { dm.num(id, "TextureSpeed", 1.0) },
        look: Look::new(&texture, Procedural::Plain, dm.num(id, "LightEmission", 0.0)),
    };
    let name = dm.name(id);
    effects::run_beam(ctx, id, &name, &spec);
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("BeamFx").describe("Draws the band of the beam (render::effects).").animate(animate);

pub static CLASS: ClassDesc =
    ClassDesc::new("Beam", Some("Instance")).describe("A band between two attachments.").category("Effects").creatable().groups(&["Beam"]).components(&["Animated", "BeamFx"]).props(PROPS);

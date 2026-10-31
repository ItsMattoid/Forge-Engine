//! Trail: a streak behind a moving pair of attachments, drawn as a ribbon (`render::effects`). While `Enabled`, a piece is
//! added every `MinLength` studs the attachments move, and lasts `Lifetime` seconds; `MaxLength` limits how long the whole
//! trail may be (0: no limit). `Color`, `Transparency` and `WidthScale` change along it from the live end (0) to the old end
//! (1). `Texture` is a project picture stretched along it. A `LightEmission` of half or more adds the trail's light to what is
//! behind it. Trails are not shaded by the world's lights.

use crate::{
    core::*,
    dm::DataModel,
    render::{
        effects::{self, ColorSeq, Look, NumberSeq, Procedural, TrailSpec},
        RenderCtx,
    },
};

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Trail").order(0).flags(F_VISUAL).desc("Whether the trail is drawn."),
    PropMeta::reference("Attachment0", "Attachment").group("Trail").order(1).flags(F_VISUAL).desc("One edge of the trail."),
    PropMeta::reference("Attachment1", "Attachment").group("Trail").order(2).flags(F_VISUAL).desc("The other edge of the trail."),
    PropMeta::number("Lifetime", 2.0).group("Trail").order(3).drag(0.1).clamp(0.0, 20.0).desc("How long a piece of the trail lasts, in seconds."),
    PropMeta::data("Color", "ColorSequence", &[0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]).group("Trail").order(4).desc("The colour along the trail."),
    PropMeta::data("Transparency", "NumberSequence", &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]).group("Trail").order(5).desc("How see-through the trail is along its length."),
    PropMeta::data("WidthScale", "NumberSequence", &[0.0, 1.0, 0.0, 1.0, 1.0, 0.0]).group("Trail").order(6).desc("How wide the trail is along its length."),
    PropMeta::number("MinLength", 0.1).group("Trail").order(7).drag(0.01).clamp(0.0, 1.0e4).desc("The shortest piece the trail is made of."),
    PropMeta::number("MaxLength", 0.0).group("Trail").order(8).drag(0.1).clamp(0.0, 1.0e4).desc("The longest the trail may be (0: no limit)."),
    PropMeta::boolean("FaceCamera", false).group("Trail").order(9).desc("Whether the trail always faces the camera."),
    PropMeta::number("LightEmission", 0.0).group("Trail").order(10).slider(0.0, 1.0).desc("How much the trail glows: from half, its light is added to what is behind it."),
    PropMeta::asset("Texture", "png").group("Trail").order(11).desc("A picture drawn along the trail."),
    PropMeta::number("ClearCount", 0.0).group("Trail").order(12).hidden().runtime().desc("How often scripts have called Clear (the renderer removes the trail)."),
];

fn clear(c: &mut MethodCall) -> Result<ScriptValue, String> {
    let so_far = c.dm.num(c.this, "ClearCount", 0.0);
    c.dm.set_prop(c.this, "ClearCount", Value::Num(so_far + 1.0))?;
    Ok(ScriptValue::Nil)
}

static METHODS: &[MethodDesc] = &[MethodDesc::new("Clear", clear).desc("Removes the trail drawn so far.")];

fn edge(dm: &DataModel, id: Id, name: &str) -> Option<bevy::math::Vec3> {
    let at = dm.reference(id, name)?;
    dm.in_workspace(at).then(|| dm.world_cframe(at).map(|(p, _)| p)).flatten()
}

fn animate(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    let Some(edges) = edge(dm, id, "Attachment0").zip(edge(dm, id, "Attachment1")).filter(|_| dm.in_workspace(id)) else { return ctx.fx_clear(id) };
    let texture = dm.string(id, "Texture");
    let spec = TrailSpec {
        edges,
        lifetime: dm.num(id, "Lifetime", 2.0),
        color: ColorSeq::read(dm, id, "Color"),
        transparency: NumberSeq::read(dm, id, "Transparency", 0.0),
        width_scale: NumberSeq::read(dm, id, "WidthScale", 1.0),
        min_length: dm.num(id, "MinLength", 0.1),
        max_length: dm.num(id, "MaxLength", 0.0),
        face_camera: dm.flag(id, "FaceCamera"),
        look: Look::new(&texture, Procedural::Plain, dm.num(id, "LightEmission", 0.0)),
    };
    let name = dm.name(id);
    effects::run_trail(ctx, id, &name, &spec, dm.flag(id, "Enabled"), dm.num(id, "ClearCount", 0.0));
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("TrailFx").describe("Draws the streak of the trail (render::effects).").animate(animate);

pub static CLASS: ClassDesc = ClassDesc::new("Trail", Some("Instance"))
    .describe("A streak behind a moving object.")
    .category("Effects")
    .creatable()
    .groups(&["Trail"])
    .components(&["Animated", "TrailFx"])
    .props(PROPS)
    .methods(METHODS);

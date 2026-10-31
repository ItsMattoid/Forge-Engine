//! Decal: a picture on one face of a part. It is drawn with Bevy's forward decals: a flat box laid on the face that paints
//! whatever surface is inside it, so the picture follows the part when it moves or is resized, and shows on any shape (a decal
//! on a ball paints the part of the ball that is under the face's square).
//!
//! `Texture` is a project picture (png, jpg ...); drop one on the property or on the decal, or on a part to make a decal for it.
//! `Color3` tints the picture and `Transparency` fades it. `ZIndex` is kept for scripts: decals on the same face are drawn in the
//! order they were made. A `Texture` object (the class below) repeats the picture every `StudsPerTileU` x `StudsPerTileV` studs
//! instead of stretching it over the face.

use bevy::{
    pbr::{decal::ForwardDecal, ExtendedMaterial, StandardMaterial},
    prelude::*,
};

use crate::{
    assets::material::SamplerSettings,
    compat,
    core::*,
    dm::{DataModel, Id},
    render::{despawn_entity, entity_of, set_entity, RenderCtx},
};

static PROPS: &[PropMeta] = &[
    PropMeta::asset("Texture", "png").group("Decal").order(1).flags(F_VISUAL).desc("The picture (a project file: png, jpg ...)."),
    PropMeta::number("Transparency", 0.0).group("Decal").order(2).flags(F_VISUAL).slider(0.0, 1.0).desc("0 is solid, 1 is invisible."),
    PropMeta::color3("Color3", 1.0, 1.0, 1.0).group("Decal").order(3).flags(F_VISUAL).desc("Tints the picture."),
    PropMeta::integer("ZIndex", 1).group("Decal").order(4).clamp(-2_147_483_648.0, 2_147_483_647.0).desc("Which decal is on top when several are on the same face (kept for scripts: they are drawn in the order they were made)."),
];

/// The face of a part as `(outward normal, picture right, picture up)` in the part's own axes.
pub fn face_axes(face: &str) -> (Vec3, Vec3, Vec3) {
    match face {
        "Right" => (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        "Left" => (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        "Top" => (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        "Bottom" => (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        "Back" => (Vec3::Z, Vec3::X, Vec3::Y),
        // Front: the side the part looks out of (-Z).
        _ => (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    }
}

/// Where the decal's box is: its transform in the world, and the size of the face it lies on.
pub fn decal_transform(part_pos: Vec3, part_rot: Quat, part_size: Vec3, face: &str) -> (Transform, Vec2) {
    let (normal, right, up) = face_axes(face);
    let half = (normal.abs() * part_size).length() * 0.5;
    let (w, h) = ((right.abs() * part_size).length(), (up.abs() * part_size).length());
    // The decal quad lies in its local XZ plane, with its local Y pointing out of the face: turn Y onto the normal, and the
    // quad's X onto the picture's right (the quad's -Z is the picture's up).
    let basis = Mat3::from_cols(right, normal, -up);
    let local = Quat::from_mat3(&basis);
    let depth = (part_size.min_element() * 0.5).clamp(0.05, 0.4);
    (Transform { translation: part_pos + part_rot * (normal * half), rotation: part_rot * local, scale: Vec3::new(w, depth, h) }, Vec2::new(w, h))
}

fn render(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id, bits: u8) {
    let _ = bits;
    let part = dm.parent(id).filter(|p| dm.is_a(*p, "BasePart") && dm.in_workspace(*p));
    let path = dm.string(id, "Texture");
    let transparency = dm.num(id, "Transparency", 0.0);
    let Some(part) = part.filter(|_| !path.is_empty() && transparency < 1.0) else {
        despawn_entity(ctx, dm, id);
        return;
    };
    let Some((pos, rot)) = dm.world_cframe(part) else { return };
    let (transform, face_size) = decal_transform(pos, rot, dm.size(part), dm.str_ref(id, "Face"));
    // A `Texture` repeats every so many studs; a `Decal` covers the face once.
    let (tile, offset) = if dm.is_a(id, "Texture") {
        let (su, sv) = (dm.num(id, "StudsPerTileU", 2.0).max(0.001), dm.num(id, "StudsPerTileV", 2.0).max(0.001));
        (Vec2::new(face_size.x / su, face_size.y / sv), Vec2::new(dm.num(id, "OffsetStudsU", 0.0) / su, dm.num(id, "OffsetStudsV", 0.0) / sv))
    } else {
        (Vec2::ONE, Vec2::ZERO)
    };
    let tint = dm.vec3(id, "Color3");
    let image = ctx.assets.texture(&path, true, &SamplerSettings::default(), ctx.source);
    let material = ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(tint.x, tint.y, tint.z, 1.0 - transparency),
            base_color_texture: image,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.9,
            uv_transform: bevy::math::Affine2::from_scale_angle_translation(tile, 0.0, offset),
            ..default()
        },
        extension: Default::default(),
    };
    let handle = ctx.assets.decal_materials.add(material);
    match entity_of(dm, id) {
        Some(e) => compat::insert(ctx.commands, e, (transform, MeshMaterial3d(handle))),
        None => {
            let e = ctx.commands.spawn((Name::new(dm.name(id)), ForwardDecal, MeshMaterial3d(handle), transform)).id();
            set_entity(dm, id, Some(e));
        }
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("DecalRenderer").describe("Paints a picture on a face of the part it is in.").render(render);

pub static CLASS: ClassDesc = ClassDesc::new("Decal", Some("FaceInstance"))
    .describe("A picture on one face of a part.")
    .category("Parts")
    .creatable()
    .parent_rule(ParentRule::Selected { class: "BasePart", hint: "Select a part first: a decal belongs to a part." })
    .groups(&["Decal"])
    .components(&["DecalRenderer"])
    .props(PROPS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decal_lies_on_the_face_it_names_with_the_size_of_that_face() {
        let size = Vec3::new(4.0, 2.0, 6.0);
        for (face, centre, wh) in [
            ("Top", Vec3::new(0.0, 1.0, 0.0), (4.0, 6.0)),
            ("Bottom", Vec3::new(0.0, -1.0, 0.0), (4.0, 6.0)),
            ("Right", Vec3::new(2.0, 0.0, 0.0), (6.0, 2.0)),
            ("Left", Vec3::new(-2.0, 0.0, 0.0), (6.0, 2.0)),
            ("Back", Vec3::new(0.0, 0.0, 3.0), (4.0, 2.0)),
            ("Front", Vec3::new(0.0, 0.0, -3.0), (4.0, 2.0)),
        ] {
            let (t, face_size) = decal_transform(Vec3::ZERO, Quat::IDENTITY, size, face);
            assert!((t.translation - centre).length() < 1e-5, "{face}: {:?}", t.translation);
            assert_eq!((face_size.x, face_size.y), wh, "{face}");
            assert_eq!((t.scale.x, t.scale.z), wh, "{face}: the box is as big as the face");
            // The box's own Y points out of the face.
            let (normal, right, up) = face_axes(face);
            assert!((t.rotation * Vec3::Y - normal).length() < 1e-5, "{face}");
            assert!((t.rotation * Vec3::X - right).length() < 1e-5, "{face}");
            assert!((t.rotation * Vec3::NEG_Z - up).length() < 1e-5, "{face}");
        }
    }

    #[test]
    fn a_decal_turns_and_moves_with_its_part() {
        let rot = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let (t, _) = decal_transform(Vec3::new(10.0, 0.0, 0.0), rot, Vec3::new(2.0, 2.0, 2.0), "Front");
        // The part's front (-Z) now points along -X... turned a quarter about Y: -Z becomes -X.
        assert!((t.translation - Vec3::new(9.0, 0.0, 0.0)).length() < 1e-4, "{:?}", t.translation);
    }
}
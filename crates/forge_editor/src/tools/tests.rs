use forge_engine::dm::DataModel;

use super::*;

fn dm_with_part() -> (DataModel, Id) {
    let mut dm = DataModel::new();
    let ws = dm.workspace;
    let part = dm.create("Part").unwrap();
    dm.set_prop(part, "Size", Value::V3(Vec3::new(4.0, 2.0, 6.0))).unwrap();
    dm.set_prop(part, "Position", Value::V3(Vec3::new(10.0, 5.0, 0.0))).unwrap();
    dm.set_parent(part, Some(ws)).unwrap();
    (dm, part)
}

fn add_part(dm: &mut DataModel, pos: Vec3, size: Vec3) -> Id {
    let ws = dm.workspace;
    let p = dm.create("Part").unwrap();
    dm.set_prop(p, "Size", Value::V3(size)).unwrap();
    dm.set_prop(p, "Position", Value::V3(pos)).unwrap();
    dm.set_parent(p, Some(ws)).unwrap();
    p
}

fn ray(from: Vec3, to: Vec3) -> (Vec3, Vec3) {
    (from, (to - from).normalize())
}

/// A view from `cam` looking at `target`, over a 1000 x 700 viewport.
fn view_of(cam: Vec3, target: Vec3) -> View {
    let t = Transform::from_translation(cam).looking_at(target, Vec3::Y);
    View { pos: cam, rot: t.rotation, fov: 70f32.to_radians(), min: Vec2::ZERO, size: Vec2::new(1000.0, 700.0) }
}

fn frame(dm: &DataModel, tool: Tool, id: Id, camera: Vec3) -> Frame {
    frame_for(dm, tool, Space::World, id, &[id], camera).unwrap()
}

/// The handle under the mouse when it points at `world`, as seen from `cam` looking at the gizmo.
fn hit_at(tool: Tool, f: &Frame, cam: Vec3, world: Vec3) -> Option<Handle> {
    let view = view_of(cam, f.origin);
    hit(tool, f, &view, view.project(world)?)
}

fn free() -> Snap {
    Snap::steps(0.0, 0.0)
}

#[test]
fn scale_handles_sit_on_the_real_bounding_box() {
    let (mut dm, part) = dm_with_part();
    let (b, own) = scale_bounds(&dm, &[part]).unwrap();
    assert!(own);
    assert_eq!((b.center, b.half), (Vec3::new(10.0, 5.0, 0.0), Vec3::new(2.0, 1.0, 3.0)));
    // A resized object has new bounds straight away.
    dm.set_prop(part, "Size", Value::V3(Vec3::new(8.0, 2.0, 6.0))).unwrap();
    assert_eq!(scale_bounds(&dm, &[part]).unwrap().0.half.x, 4.0);
    // A pivot offset does not move the box.
    dm.set_prop(part, "PivotOffset", Value::V3(Vec3::new(3.0, 0.0, 0.0))).unwrap();
    assert_eq!(scale_bounds(&dm, &[part]).unwrap().0.center, Vec3::new(10.0, 5.0, 0.0));
}

#[test]
fn a_model_or_several_objects_get_the_box_around_everything() {
    let (mut dm, part) = dm_with_part();
    let ws = dm.workspace;
    let other = add_part(&mut dm, Vec3::new(-10.0, 0.0, 0.0), Vec3::splat(2.0));
    let model = dm.create("Model").unwrap();
    dm.set_parent(model, Some(ws)).unwrap();
    dm.set_parent(part, Some(model)).unwrap();
    dm.set_parent(other, Some(model)).unwrap();
    let (b, own) = scale_bounds(&dm, &[model]).unwrap();
    assert!(!own);
    assert_eq!(b.rot, Quat::IDENTITY);
    assert!((b.center.x - 0.5).abs() < 1e-4 && (b.half.x - 11.5).abs() < 1e-4, "spans x from -11 to 12: {b:?}");
    // The same box when the two parts are just selected side by side.
    let (b2, own2) = scale_bounds(&dm, &[part, other]).unwrap();
    assert!(!own2 && b2 == b);
    let empty = dm.create("Model").unwrap();
    dm.set_parent(empty, Some(ws)).unwrap();
    assert!(scale_bounds(&dm, &[empty]).is_none(), "an empty model has no bounds, so no handles");
}

#[test]
fn with_several_objects_selected_the_gizmo_sits_at_the_middle_of_their_combined_bounds() {
    let (mut dm, a) = dm_with_part(); // x 8..12, z -3..3
    let b = add_part(&mut dm, Vec3::new(-20.0, 5.0, 8.0), Vec3::new(2.0, 2.0, 2.0)); // x -21..-19, z 7..9
    let cam = Vec3::new(0.0, 30.0, 60.0);
    let alone = frame_for(&dm, Tool::Move, Space::World, a, &[a], cam).unwrap();
    assert_eq!(alone.origin, Vec3::new(10.0, 5.0, 0.0), "one object: its own pivot");
    let both = frame_for(&dm, Tool::Move, Space::World, a, &[a, b], cam).unwrap();
    let (cx, cz) = ((12.0 + -21.0) / 2.0, (-3.0 + 9.0) / 2.0);
    assert!((both.origin - Vec3::new(cx, 5.0, cz)).length() < 1e-4, "the middle of the union box, got {:?}", both.origin);
    let rotate = frame_for(&dm, Tool::Rotate, Space::World, a, &[a, b], cam).unwrap();
    assert!((rotate.origin - both.origin).length() < 1e-5, "the rotate gizmo is there too");
    // Scale works on the same combined box.
    let scale = frame_for(&dm, Tool::Scale, Space::World, a, &[a, b], cam).unwrap();
    assert!((scale.origin - both.origin).length() < 1e-4 && !scale.own_box);
    // The pivot tool is always about the active object.
    let pivot = frame_for(&dm, Tool::Pivot, Space::World, a, &[a, b], cam).unwrap();
    assert_eq!(pivot.origin, Vec3::new(10.0, 5.0, 0.0));
}

// ---- move: arrows ---------------------------------------------------------------------------------------------

#[test]
fn dragging_a_move_arrow_moves_along_the_axis_and_measures_it() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame(&dm, Tool::Move, part, cam);
    let grab = f.origin + Vec3::X * (f.len * 0.5);
    let handle = hit_at(Tool::Move, &f, cam, grab).expect("an arrow");
    assert_eq!(handle, Handle::Move(MoveHandle::Arrow(0, 1.0)));
    let mut drag = begin_drag(&dm, handle, f, ray(cam, grab), &[part], part).unwrap();
    let snap = Snap::steps(1.0, 15.0);
    // The mouse moves to a point 7.2 further along X: snapped to 7.
    let to = grab + Vec3::X * 7.2;
    assert!(update_drag(&mut dm, &mut drag, ray(cam, to), &snap));
    assert!((dm.vec3(part, "Position").x - 17.0).abs() < 1e-3);
    assert_eq!(dm.vec3(part, "Position").y, 5.0, "other axes are untouched");
    assert!((drag.done.moved[0] - 7.0).abs() < 1e-3 && drag.done.moved[1] == 0.0);
    // Back past the start: the measurement goes negative, from the same starting point.
    let back = grab - Vec3::X * 3.0;
    update_drag(&mut dm, &mut drag, ray(cam, back), &snap);
    assert!((drag.done.moved[0] + 3.0).abs() < 1e-3);
    assert!((dm.vec3(part, "Position").x - 7.0).abs() < 1e-3);
}

#[test]
fn the_negative_arrow_moves_along_the_same_axis() {
    let (dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame(&dm, Tool::Move, part, cam);
    let grab = f.origin - Vec3::Y * (f.len * 0.5);
    assert_eq!(hit_at(Tool::Move, &f, cam, grab), Some(Handle::Move(MoveHandle::Arrow(1, -1.0))));
}

// ---- move: plane handles --------------------------------------------------------------------------------------

#[test]
fn a_plane_handle_moves_two_axes_and_never_the_third() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(30.0, 25.0, 40.0);
    let f = frame(&dm, Tool::Move, part, cam);
    let signs = gizmo::facing_signs(cam, f.origin, &f.dirs);
    let c = gizmo::plane_corners(f.origin, &f.dirs, &signs, 1, f.len); // the XZ plane (normal Y)
    let grab = (c[0] + c[2]) * 0.5;
    let handle = hit_at(Tool::Move, &f, cam, grab).unwrap();
    assert_eq!(handle, Handle::Move(MoveHandle::Plane(1)));
    let mut drag = begin_drag(&dm, handle, f, ray(cam, grab), &[part], part).unwrap();
    // Aim at a spot 3 along X and -4 along Z from the grab point, on the same plane.
    let to = grab + Vec3::new(3.0, 0.0, -4.0);
    update_drag(&mut dm, &mut drag, ray(cam, to), &free());
    let p = dm.vec3(part, "Position");
    assert!((p - Vec3::new(13.0, 5.0, -4.0)).length() < 1e-3, "{p:?}");
    assert!(drag.done.moved[1] == 0.0 && (drag.done.moved[0] - 3.0).abs() < 1e-3 && (drag.done.moved[2] + 4.0).abs() < 1e-3);
}

#[test]
fn plane_handles_are_drawn_small_but_picked_from_a_much_bigger_area() {
    let (dm, part) = dm_with_part();
    for (cam, plane) in [(Vec3::new(30.0, 25.0, 40.0), 1), (Vec3::new(-20.0, 30.0, 35.0), 2), (Vec3::new(45.0, 12.0, -25.0), 0)] {
        let f = frame(&dm, Tool::Move, part, cam);
        let view = view_of(cam, f.origin);
        let signs = gizmo::facing_signs(cam, f.origin, &f.dirs);
        let corners: Vec<Vec2> = gizmo::plane_corners(f.origin, &f.dirs, &signs, plane, f.len).iter().map(|c| view.project(*c).unwrap()).collect();
        let centre = corners.iter().copied().sum::<Vec2>() / 4.0;
        // Drawn small: the square is well under a fifth of the handle length.
        let len_on_screen = f.len * view.pixels_per_unit(f.origin);
        let side = (corners[1] - corners[0]).length().max((corners[3] - corners[0]).length());
        assert!(side < len_on_screen * 0.22, "{side} points of a {len_on_screen}-point gizmo");
        assert_eq!(hit(Tool::Move, &f, &view, centre), Some(Handle::Move(MoveHandle::Plane(plane))), "its middle");
        // Picked from well outside what is drawn, on every side (except where that is on an arrow, which wins).
        let pad = gizmo::plane_pick_pad(len_on_screen);
        assert!(pad >= 6.0);
        let tolerance = gizmo::arrow_pick_tolerance(len_on_screen);
        let mut grabbed_outside = 0;
        for step in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            // (from the square's own edge outwards)
            let edge = corners.iter().map(|c| (*c - centre).dot(step)).fold(f32::MIN, f32::max);
            let out = centre + step * (edge + pad * 0.8);
            let got = hit(Tool::Move, &f, &view, out);
            let on_an_arrow = (0..3).any(|axis| {
                [1.0f32, -1.0].into_iter().any(|sign| match (view.project(f.origin + f.dirs[axis] * (sign * f.len * gizmo::ARROW_GAP)), view.project(f.origin + f.dirs[axis] * (sign * f.len))) {
                    (Some(a), Some(b)) => gizmo::point_segment_distance(out, a, b) <= tolerance,
                    _ => true,
                })
            });
            if on_an_arrow {
                continue;
            }
            // (Another plane's margin may overlap this one's: then the closer square is picked, still a plane.)
            assert!(matches!(got, Some(Handle::Move(MoveHandle::Plane(_)))), "{:.0} points outside the square, towards {step:?}: {got:?}", pad * 0.8);
            grabbed_outside += 1;
        }
        assert!(grabbed_outside >= 2, "the area round the square is for grabbing: only {grabbed_outside} of 4 sides were");
        // ... but not from far away.
        assert_eq!(hit(Tool::Move, &f, &view, centre + Vec2::new(len_on_screen * 1.5, 0.0)), None);
    }
}

#[test]
fn plane_handles_stay_pickable_at_every_distance_and_angle_and_keep_clear_of_the_arrows() {
    let (dm, part) = dm_with_part();
    let origin = Vec3::new(10.0, 5.0, 0.0);
    for dist in [6.0f32, 20.0, 80.0, 400.0] {
        for dir in [Vec3::new(1.0, 1.0, 1.0), Vec3::new(-1.0, 1.3, 0.8), Vec3::new(0.6, 2.0, -1.0), Vec3::new(-0.2, 0.7, 2.0), Vec3::new(2.0, 0.5, 0.3)] {
            let cam = origin + dir.normalize() * dist;
            let f = frame(&dm, Tool::Move, part, cam);
            let view = view_of(cam, f.origin);
            let signs = gizmo::facing_signs(cam, f.origin, &f.dirs);
            for normal in 0..3 {
                let c = gizmo::plane_corners(f.origin, &f.dirs, &signs, normal, f.len);
                let middle = view.project((c[0] + c[2]) * 0.5).unwrap();
                let facing = ((f.origin - cam).normalize().dot(f.dirs[normal])).abs();
                let got = hit(Tool::Move, &f, &view, middle);
                // Seen along an axis, an arrow can lie right over a plane's square: then the arrow, which wins, is
                // what is picked. Everywhere else the plane must be.
                let near_an_arrow = (0..3).any(|axis| {
                    [1.0f32, -1.0].into_iter().any(|sign| {
                        let (a, b) = (f.origin + f.dirs[axis] * (sign * f.len * gizmo::ARROW_GAP), f.origin + f.dirs[axis] * (sign * f.len));
                        match (view.project(a), view.project(b)) {
                            (Some(a), Some(b)) => gizmo::point_segment_distance(middle, a, b) < 16.0,
                            _ => true,
                        }
                    })
                });
                if facing >= gizmo::MIN_PLANE_FACING * 1.5 && !near_an_arrow {
                    assert!(matches!(got, Some(Handle::Move(MoveHandle::Plane(_)))), "plane {normal} from {dir:?} at {dist}: {got:?}");
                }
            }
            // The arrows keep their own area: half way along each one is an arrow, never swallowed by a plane's bigger area.
            for axis in 0..3 {
                for sign in [1.0f32, -1.0] {
                    let on = f.origin + f.dirs[axis] * (sign * f.len * 0.6);
                    if let Some(p) = view.project(on) {
                        let got = hit(Tool::Move, &f, &view, p);
                        assert!(matches!(got, Some(Handle::Move(MoveHandle::Arrow(..)))), "arrow {axis}{sign:+} from {dir:?} at {dist}: {got:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn a_plane_seen_edge_on_cannot_be_grabbed_and_cannot_throw_the_object_away() {
    let (mut dm, part) = dm_with_part();
    // The camera is level with the object: its XZ plane (normal Y) is seen exactly edge-on.
    let cam = Vec3::new(10.0, 5.0, 40.0);
    let f = frame(&dm, Tool::Move, part, cam);
    let view = view_of(cam, f.origin);
    let signs = gizmo::facing_signs(cam, f.origin, &f.dirs);
    let c = gizmo::plane_corners(f.origin, &f.dirs, &signs, 1, f.len);
    let got = hit(Tool::Move, &f, &view, view.project((c[0] + c[2]) * 0.5).unwrap());
    assert_ne!(got, Some(Handle::Move(MoveHandle::Plane(1))));

    // A drag that was started from a good angle holds still when the mouse ray runs along the plane.
    let high = Vec3::new(10.0, 40.0, 40.0);
    let f = frame(&dm, Tool::Move, part, high);
    let mid = gizmo::plane_corners(f.origin, &f.dirs, &gizmo::facing_signs(high, f.origin, &f.dirs), 1, f.len);
    let grab = (mid[0] + mid[2]) * 0.5;
    let mut drag = begin_drag(&dm, Handle::Move(MoveHandle::Plane(1)), f, ray(high, grab), &[part], part).unwrap();
    let before = dm.vec3(part, "Position");
    let along_the_plane = (high, Vec3::new(0.0, 0.0, -1.0).with_y(0.001).normalize());
    assert!(!update_drag(&mut dm, &mut drag, along_the_plane, &free()));
    assert_eq!(dm.vec3(part, "Position"), before);
}

#[test]
fn moving_several_objects_keeps_their_layout_and_the_gizmo_is_between_them() {
    let (mut dm, a) = dm_with_part();
    let b = add_part(&mut dm, Vec3::new(0.0, 0.0, 20.0), Vec3::ONE);
    let cam = Vec3::new(10.0, 8.0, 60.0);
    let f = frame_for(&dm, Tool::Move, Space::World, a, &[a, b], cam).unwrap();
    assert!(f.origin.x > 0.5 && f.origin.x < 9.5 && f.origin.z > 5.0, "between the two, not on either: {:?}", f.origin);
    let grab = f.origin + Vec3::X * (f.len * 0.5);
    let mut drag = begin_drag(&dm, Handle::Move(MoveHandle::Arrow(0, 1.0)), f, ray(cam, grab), &[a, b], a).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 5.0), &Snap::steps(1.0, 0.0));
    assert!((dm.vec3(a, "Position").x - 15.0).abs() < 1e-3 && (dm.vec3(b, "Position").x - 5.0).abs() < 1e-3, "both moved by the same 5");
    assert_eq!(dm.vec3(b, "Position").z, 20.0);
}

#[test]
fn the_ruler_is_laid_out_without_one_line_over_another() {
    for moved in [7.3f32, -4.0, 0.0, 0.2, -123.0] {
        let spans = gizmo::ruler_spans(moved, 5.0);
        // The travelled stretch and the two stretches of guide cover the whole line and never overlap.
        let mut all: Vec<(f32, f32)> = spans.guide.iter().copied().chain(std::iter::once((spans.travelled.0.min(spans.travelled.1), spans.travelled.0.max(spans.travelled.1)))).filter(|(a, b)| b > a).collect();
        all.sort_by(|a, b| a.0.total_cmp(&b.0));
        for w in all.windows(2) {
            assert!(w[1].0 >= w[0].1 - 1e-6, "{moved}: {:?} overlaps {:?}", w[0], w[1]);
        }
        let (lo, hi) = (moved.min(0.0) - 5.0, moved.max(0.0) + 5.0);
        assert!((all.first().unwrap().0 - lo).abs() < 1e-5 && (all.last().unwrap().1 - hi).abs() < 1e-5, "{moved}: the whole reach is covered");
    }
}

// ---- scale ----------------------------------------------------------------------------------------------------

/// Takes hold of the +X scale sphere of the 4 x 2 x 6 part (its +X face is at x = 12).
fn grab_plus_x(dm: &DataModel, part: Id) -> (Drag, Vec3, Vec3) {
    let cam = Vec3::new(40.0, 20.0, 25.0);
    let f = frame(dm, Tool::Scale, part, cam);
    let grab = f.bounds.unwrap().handle(cam, 0, 1.0);
    assert_eq!(hit_at(Tool::Scale, &f, cam, grab), Some(Handle::Scale(0, 1.0)));
    (begin_drag(dm, Handle::Scale(0, 1.0), f, ray(cam, grab), &[part], part).unwrap(), cam, grab)
}

fn corner_x(dm: &DataModel, part: Id) -> (f32, f32) {
    let (x, w) = (dm.vec3(part, "Position").x, dm.size(part).x);
    (x - w / 2.0, x + w / 2.0)
}

#[test]
fn scaling_a_part_grows_one_face_and_keeps_the_opposite_one_fixed() {
    let (mut dm, part) = dm_with_part();
    let (mut drag, cam, grab) = grab_plus_x(&dm, part);
    let snap = Snap::steps(0.5, 0.0);
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 3.0), &snap);
    assert_eq!(dm.size(part).x, 7.0, "4 + 3");
    assert!((corner_x(&dm, part).0 - 8.0).abs() < 1e-3, "the -X face stayed at x = 8");
    assert!((drag.done.pull - 3.0).abs() < 1e-3 && (drag.done.factor - 1.75).abs() < 1e-3 && (drag.done.extent - 7.0).abs() < 1e-3);
    assert_eq!(dm.size(part).y, 2.0, "the other sizes are not touched");
    // Pulling the face inwards shrinks, and the sign says so.
    update_drag(&mut dm, &mut drag, ray(cam, grab - Vec3::X * 1.0), &snap);
    assert!((dm.size(part).x - 3.0).abs() < 1e-3 && drag.done.pull < 0.0);
    // It never collapses to nothing.
    update_drag(&mut dm, &mut drag, ray(cam, grab - Vec3::X * 30.0), &snap);
    assert!(dm.size(part).x >= 0.05);
}

#[test]
fn scaling_the_negative_side_grows_the_other_way() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(-40.0, 20.0, 25.0);
    let f = frame(&dm, Tool::Scale, part, cam);
    let grab = f.bounds.unwrap().handle(cam, 0, -1.0);
    let mut drag = begin_drag(&dm, Handle::Scale(0, -1.0), f, ray(cam, grab), &[part], part).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, grab - Vec3::X * 2.0), &free());
    assert!((dm.size(part).x - 6.0).abs() < 1e-3);
    assert!((corner_x(&dm, part).1 - 12.0).abs() < 1e-3, "the +X face stayed where it was");
}

#[test]
fn shift_scales_all_three_sizes_by_the_same_factor_from_whichever_handle() {
    // Grabbing the +X sphere: 4 -> 6 is a factor of 1.5, so 4 x 2 x 6 becomes 6 x 3 x 9, anchored at the -X face.
    let (mut dm, part) = dm_with_part();
    let (mut drag, cam, grab) = grab_plus_x(&dm, part);
    assert!(update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 2.0), &Snap { proportional: true, ..free() }));
    let s = dm.size(part);
    assert!((s - Vec3::new(6.0, 3.0, 9.0)).length() < 1e-3, "{s:?}");
    assert!((corner_x(&dm, part).0 - 8.0).abs() < 1e-3, "the opposite face did not move");
    let p = dm.vec3(part, "Position");
    assert!((p.y - 5.0).abs() < 1e-3 && p.z.abs() < 1e-3, "centred on the axis through the face the others stay centred: {p:?}");
    assert!(drag.done.proportional && (drag.done.factor - 1.5).abs() < 1e-4);

    // Another handle gives the same proportions: the -Y one, 2 -> 3 is a factor of 1.5 as well.
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, -30.0, 25.0);
    let f = frame(&dm, Tool::Scale, part, cam);
    let b = f.bounds.unwrap();
    let grab = b.handle(cam, 1, -1.0);
    let mut drag = begin_drag(&dm, Handle::Scale(1, -1.0), f, ray(cam, grab), &[part], part).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, grab - Vec3::Y * 1.0), &Snap { proportional: true, ..free() });
    assert!((dm.size(part) - Vec3::new(6.0, 3.0, 9.0)).length() < 1e-3, "{:?}", dm.size(part));
    assert!((dm.vec3(part, "Position") - Vec3::new(10.0, 4.5, 0.0)).length() < 1e-3, "the +Y face stays at y = 6 and the others stay centred on it: {:?}", dm.vec3(part, "Position"));
    assert!((dm.vec3(part, "Position").y + dm.size(part).y / 2.0 - 6.0).abs() < 1e-3);
}

#[test]
fn ctrl_scales_both_sides_about_the_centre() {
    let (mut dm, part) = dm_with_part();
    let (mut drag, cam, grab) = grab_plus_x(&dm, part);
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 2.0), &Snap { from_center: true, ..free() });
    assert!((dm.size(part) - Vec3::new(8.0, 2.0, 6.0)).length() < 1e-3, "2 on each side: {:?}", dm.size(part));
    assert!((dm.vec3(part, "Position") - Vec3::new(10.0, 5.0, 0.0)).length() < 1e-4, "the centre stays put");
    assert!((corner_x(&dm, part).0 - 6.0).abs() < 1e-3 && (corner_x(&dm, part).1 - 14.0).abs() < 1e-3, "both faces moved by 2");
    assert!((drag.done.pull - 2.0).abs() < 1e-4 && (drag.done.extent - 8.0).abs() < 1e-4, "the grabbed face moved 2: it is where the mouse is");
    // Pulling in shrinks from both sides, and keeps the minimum.
    update_drag(&mut dm, &mut drag, ray(cam, grab - Vec3::X * 20.0), &Snap { from_center: true, ..free() });
    assert!(dm.size(part).x >= 0.05 && (dm.vec3(part, "Position") - Vec3::new(10.0, 5.0, 0.0)).length() < 1e-4);
}

#[test]
fn ctrl_and_shift_scale_in_proportion_about_the_centre() {
    let (mut dm, part) = dm_with_part();
    let (mut drag, cam, grab) = grab_plus_x(&dm, part);
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 2.0), &Snap { proportional: true, from_center: true, ..free() });
    assert!((dm.size(part) - Vec3::new(8.0, 4.0, 12.0)).length() < 1e-3, "doubled: {:?}", dm.size(part));
    assert!((dm.vec3(part, "Position") - Vec3::new(10.0, 5.0, 0.0)).length() < 1e-4, "about the centre");
    assert!((drag.done.factor - 2.0).abs() < 1e-4);
}

#[test]
fn changing_modifiers_mid_drag_never_moves_the_handle_being_held_and_adds_up_no_error() {
    let (mut dm, part) = dm_with_part();
    let (mut drag, cam, grab) = grab_plus_x(&dm, part);
    let to = ray(cam, grab + Vec3::X * 2.0);
    // The mouse stays where it is; the keys come and go. The grabbed (+X) face is at x = 14 whatever they are.
    for (shift, ctrl) in [(false, false), (false, true), (true, true), (true, false), (false, false), (true, true), (false, true)] {
        update_drag(&mut dm, &mut drag, to, &Snap { proportional: shift, from_center: ctrl, ..free() });
        assert!((corner_x(&dm, part).1 - 14.0).abs() < 1e-3, "shift {shift} ctrl {ctrl}: the face under the mouse is at {}", corner_x(&dm, part).1);
    }
    // Move about, with the keys changing, and come back to the start: the part is exactly as it was.
    let wander = [(1.0, true, false), (-1.5, false, true), (4.0, true, true), (0.3, false, false), (-0.9, true, false)];
    for (x, shift, ctrl) in wander {
        update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * x), &Snap { proportional: shift, from_center: ctrl, ..free() });
    }
    update_drag(&mut dm, &mut drag, ray(cam, grab), &free());
    assert!((dm.size(part) - Vec3::new(4.0, 2.0, 6.0)).length() < 1e-4, "{:?}", dm.size(part));
    assert!((dm.vec3(part, "Position") - Vec3::new(10.0, 5.0, 0.0)).length() < 1e-4, "{:?}", dm.vec3(part, "Position"));
}

#[test]
fn shift_scales_a_model_in_proportion_including_parts_that_are_turned() {
    let (mut dm, part) = dm_with_part(); // x 8..12
    let ws = dm.workspace;
    let turned = add_part(&mut dm, Vec3::new(0.0, 5.0, 0.0), Vec3::splat(2.0));
    dm.set_prop(turned, "Orientation", Value::V3(Vec3::new(0.0, 45.0, 0.0))).unwrap();
    let model = dm.create("Model").unwrap();
    dm.set_parent(model, Some(ws)).unwrap();
    dm.set_parent(part, Some(model)).unwrap();
    dm.set_parent(turned, Some(model)).unwrap();
    let cam = Vec3::new(40.0, 20.0, 25.0);
    let f = frame(&dm, Tool::Scale, model, cam);
    let b = f.bounds.unwrap();
    let extent0 = b.half.x * 2.0;
    let grab = b.handle(cam, 0, 1.0);
    let anchor = b.face(0, -1.0);
    let mut drag = begin_drag(&dm, Handle::Scale(0, 1.0), f, ray(cam, grab), &[model], model).unwrap();
    // Grow the box by its own width: every part doubles.
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * extent0), &Snap { proportional: true, ..free() });
    assert!((dm.size(part) - Vec3::new(8.0, 4.0, 12.0)).length() < 1e-3, "{:?}", dm.size(part));
    assert!((dm.size(turned) - Vec3::splat(4.0)).length() < 1e-3, "a part turned 45 degrees is scaled too: {:?}", dm.size(turned));
    assert!((dm.vec3(part, "Position").x - (anchor.x + (10.0 - anchor.x) * 2.0)).abs() < 1e-3, "positions are stretched away from the anchored face");
    // Without Shift the turned part cannot be stretched along one axis without shearing, so only its place follows.
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * extent0), &free());
    assert!((dm.size(turned) - Vec3::splat(2.0)).length() < 1e-3);
}

#[test]
fn scaling_a_model_stretches_what_is_inside_it() {
    let (mut dm, part) = dm_with_part();
    let ws = dm.workspace;
    let other = add_part(&mut dm, Vec3::new(0.0, 5.0, 0.0), Vec3::splat(2.0));
    let model = dm.create("Model").unwrap();
    dm.set_parent(model, Some(ws)).unwrap();
    dm.set_parent(part, Some(model)).unwrap();
    dm.set_parent(other, Some(model)).unwrap();
    // The box spans x from -1 to 12 (13 wide).
    let cam = Vec3::new(40.0, 20.0, 25.0);
    let f = frame(&dm, Tool::Scale, model, cam);
    let b = f.bounds.unwrap();
    let grab = b.handle(cam, 0, 1.0);
    let mut drag = begin_drag(&dm, Handle::Scale(0, 1.0), f, ray(cam, grab), &[model], model).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 13.0), &free());
    // The whole box doubled: the anchored -X face stays at x = -1, everything else is stretched away from it.
    assert!((drag.done.factor - 2.0).abs() < 1e-3);
    assert!((dm.size(part).x - 8.0).abs() < 1e-3, "a part lined up with the axis is stretched too");
    assert!((dm.vec3(part, "Position").x - 21.0).abs() < 1e-3, "-1 + (10 + 1) * 2");
    assert!((dm.vec3(other, "Position").x - 1.0).abs() < 1e-3, "-1 + 1 * 2");
    assert!((dm.size(other).x - 4.0).abs() < 1e-3);
    assert_eq!(dm.vec3(part, "Position").y, 5.0);
    // About the centre instead (Ctrl): the box doubles around x = 5.5.
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 6.5), &Snap { from_center: true, ..free() });
    assert!((dm.vec3(other, "Position").x - (5.5 + (0.0 - 5.5) * 2.0)).abs() < 1e-3);
    assert!((dm.vec3(part, "Position").x - (5.5 + (10.0 - 5.5) * 2.0)).abs() < 1e-3);
}

#[test]
fn scaling_several_selected_objects_stretches_them_together() {
    let (mut dm, a) = dm_with_part(); // x 8..12
    let b = add_part(&mut dm, Vec3::new(0.0, 5.0, 0.0), Vec3::splat(2.0)); // x -1..1
    let cam = Vec3::new(40.0, 20.0, 25.0);
    let f = frame_for(&dm, Tool::Scale, Space::World, a, &[a, b], cam).unwrap();
    assert!(!f.own_box);
    let grab = f.bounds.unwrap().handle(cam, 0, 1.0);
    let mut drag = begin_drag(&dm, Handle::Scale(0, 1.0), f, ray(cam, grab), &[a, b], a).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 13.0), &free());
    assert!((dm.size(a).x - 8.0).abs() < 1e-3 && (dm.size(b).x - 4.0).abs() < 1e-3, "both were stretched by the same factor");
    assert!((dm.vec3(a, "Position").x - 21.0).abs() < 1e-3 && (dm.vec3(b, "Position").x - 1.0).abs() < 1e-3);
}

#[test]
fn scale_text_is_one_short_value() {
    let one_axis = Done { extent: 7.0, factor: 1.75, ..Done::default() };
    assert_eq!(scale_text(&one_axis, 0), "X 7.00");
    assert_eq!(scale_text(&one_axis, 2), "Z 7.00");
    assert_eq!(scale_text(&Done { proportional: true, ..one_axis }, 1), "\u{d7}1.75", "with Shift all three change, so the factor is what matters");
}

// ---- rotate ----------------------------------------------------------------------------------------------------

#[test]
fn a_ring_drag_turns_around_its_axis_in_snapped_steps() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 5.0, 40.0);
    let f = frame(&dm, Tool::Rotate, part, cam);
    let r = f.len * 0.9;
    // Take hold of the Z ring (a circle in the XY plane) at its right-hand side.
    let grab = f.origin + Vec3::X * r;
    let handle = hit_at(Tool::Rotate, &f, cam, grab).unwrap();
    assert_eq!(handle, Handle::Ring(2));
    let mut drag = begin_drag(&dm, handle, f, ray(cam, grab), &[part], part).unwrap();
    let snap = Snap::steps(1.0, 15.0);
    let a = 38.0f32.to_radians();
    update_drag(&mut dm, &mut drag, ray(cam, f.origin + Vec3::new(a.cos(), a.sin(), 0.0) * r), &snap);
    assert!((drag.done.angle - 45.0).abs() < 1e-3 || (drag.done.angle - 30.0).abs() < 1e-3, "got {}", drag.done.angle);
    assert!(drag.done.angle % 15.0 == 0.0);
    let (_, q) = dm.get_pivot(part).unwrap();
    let (axis, angle) = q.to_axis_angle();
    assert!(axis.abs_diff_eq(Vec3::Z, 1e-3) || axis.abs_diff_eq(Vec3::NEG_Z, 1e-3));
    assert!((angle.to_degrees() - drag.done.angle.abs()).abs() < 1e-2);
    assert_eq!(dm.vec3(part, "Position"), Vec3::new(10.0, 5.0, 0.0), "turning a single object leaves its position alone");
}

#[test]
fn a_ring_keeps_turning_past_half_a_turn_and_a_whole_one_in_either_direction() {
    for (direction, steps) in [(1.0f32, 20), (-1.0, 20)] {
        let (mut dm, part) = dm_with_part();
        let cam = Vec3::new(10.0, 5.0, 40.0);
        let f = frame(&dm, Tool::Rotate, part, cam);
        let r = f.len * 0.9;
        let mut drag = begin_drag(&dm, Handle::Ring(2), f, ray(cam, f.origin + Vec3::X * r), &[part], part).unwrap();
        let mut last = 0.0f32;
        // The mouse goes round the ring 20 degrees at a time: 400 degrees (or -400) in all.
        for k in 1..=steps {
            let degrees = direction * 20.0 * k as f32;
            let a = degrees.to_radians();
            assert!(update_drag(&mut dm, &mut drag, ray(cam, f.origin + Vec3::new(a.cos(), a.sin(), 0.0) * r), &free()), "step {k}");
            assert!((drag.done.angle - degrees).abs() < 1e-2, "step {k}: the total is {} and not {degrees}", drag.done.angle);
            // Never a jump: each frame is 20 degrees further on, across 180 and across 360.
            assert!((drag.done.angle - last - direction * 20.0).abs() < 1e-2, "step {k} jumped from {last} to {}", drag.done.angle);
            last = drag.done.angle;
        }
        let (_, q) = dm.get_pivot(part).unwrap();
        let turned = q * Vec3::X;
        let a = (direction * 400.0f32).to_radians();
        assert!((turned - Vec3::new(a.cos(), a.sin(), 0.0)).length() < 1e-3, "the object is where 400 degrees puts it: {turned:?}");
        // And back: the mouse is at 40 degrees round the ring now (400 is a turn and 40); 20 degrees round is 20 degrees
        // less, not a turn less.
        let back = (direction * 20.0f32).to_radians();
        update_drag(&mut dm, &mut drag, ray(cam, f.origin + Vec3::new(back.cos(), back.sin(), 0.0) * r), &free());
        assert!((drag.done.angle - direction * 380.0).abs() < 1e-2, "unwinding 20 degrees from 400: {}", drag.done.angle);
    }
}

#[test]
fn rotating_several_objects_swings_them_around_the_middle_of_the_selection() {
    let (mut dm, a) = dm_with_part(); // centre x = 10
    let b = add_part(&mut dm, Vec3::new(20.0, 5.0, 0.0), Vec3::new(4.0, 2.0, 6.0));
    let cam = Vec3::new(15.0, 5.0, 60.0);
    let f = frame_for(&dm, Tool::Rotate, Space::World, a, &[a, b], cam).unwrap();
    assert!((f.origin - Vec3::new(15.0, 5.0, 0.0)).length() < 1e-3, "the middle of both: {:?}", f.origin);
    let drag = begin_drag(&dm, Handle::Ring(2), f, ray(cam, f.origin + Vec3::X * (f.len * 0.9)), &[a, b], a).unwrap();
    let turn = Quat::from_axis_angle(f.dirs[2], std::f32::consts::FRAC_PI_2);
    assert!(turn_all(&mut dm, &drag, turn));
    let (pa, pb) = (dm.vec3(a, "Position"), dm.vec3(b, "Position"));
    assert!((pa - (f.origin + turn * (Vec3::new(10.0, 5.0, 0.0) - f.origin))).length() < 1e-3, "each orbits the gizmo: {pa:?}");
    assert!((pb - (f.origin + turn * (Vec3::new(20.0, 5.0, 0.0) - f.origin))).length() < 1e-3, "{pb:?}");
    assert!(((pa + pb) * 0.5 - f.origin).length() < 1e-3, "the middle between them did not move");
}

#[test]
fn the_trackball_only_starts_where_no_ring_is_under_the_mouse() {
    let (dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 5.0, 40.0);
    let f = frame(&dm, Tool::Rotate, part, cam);
    let r = f.len * 0.9;
    let middle = f.origin + Vec3::new(r * 0.35, r * 0.25, 0.0);
    assert_eq!(hit_at(Tool::Rotate, &f, cam, middle), Some(Handle::Trackball));
    let on_ring = f.origin + Vec3::X * r;
    assert!(matches!(hit_at(Tool::Rotate, &f, cam, on_ring), Some(Handle::Ring(_))), "a ring beats the ball");
    assert_eq!(hit_at(Tool::Rotate, &f, cam, f.origin + Vec3::X * (r * 1.8)), None, "outside everything");
}

#[test]
fn dragging_the_trackball_turns_freely() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 5.0, 40.0);
    let f = frame(&dm, Tool::Rotate, part, cam);
    let r = f.len * 0.9;
    let mut drag = begin_drag(&dm, Handle::Trackball, f, ray(cam, f.origin), &[part], part).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, f.origin + Vec3::new(r * 0.6, 0.0, 0.0)), &Snap::steps(1.0, 15.0));
    let (_, q) = dm.get_pivot(part).unwrap();
    assert!(q.angle_between(Quat::IDENTITY) > 0.3, "it turned");
    assert!((q * Vec3::Z).x > 0.3, "the front turned towards the mouse's side");
    assert!(drag.done.angle > 10.0 && drag.done.angle % 15.0 != 0.0, "the trackball is not snapped to the rotation increment");
}

#[test]
fn the_trackball_has_no_limit_circling_the_mouse_keeps_turning_the_object_smoothly() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 5.0, 40.0);
    let f = frame(&dm, Tool::Rotate, part, cam);
    let r = f.len * 0.9;
    let at = |phi: f32| f.origin + Vec3::new(phi.cos(), phi.sin(), 0.0) * (r * 0.85);
    let mut drag = begin_drag(&dm, Handle::Trackball, f, ray(cam, at(0.0)), &[part], part).unwrap();
    let (mut previous, mut total, mut biggest) = (Quat::IDENTITY, 0.0f32, 0.0f32);
    let frames_per_circle = 120;
    for k in 1..=(3 * frames_per_circle) {
        let phi = k as f32 / frames_per_circle as f32 * std::f32::consts::TAU;
        update_drag(&mut dm, &mut drag, ray(cam, at(phi)), &free());
        let (_, now) = dm.get_pivot(part).unwrap();
        let step = now.angle_between(previous);
        total += step;
        biggest = biggest.max(step);
        previous = now;
        if k % frames_per_circle == 0 {
            // Back where the mouse started: a drag measured from the start would be back at "no rotation".
            assert!(now.angle_between(Quat::IDENTITY) > 0.05, "after {} turns of the mouse the object is not back where it began: it kept turning", k / frames_per_circle);
        }
    }
    assert!(total.to_degrees() > 540.0, "three circles turned it through {} degrees in all", total.to_degrees());
    assert!(biggest.to_degrees() < 20.0, "no frame jumps: the biggest step was {} degrees", biggest.to_degrees());
}

#[test]
fn the_trackball_is_smooth_across_its_centre_and_past_its_edge() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 5.0, 40.0);
    let f = frame(&dm, Tool::Rotate, part, cam);
    let r = f.len * 0.9;
    let mut drag = begin_drag(&dm, Handle::Trackball, f, ray(cam, f.origin + Vec3::new(-r * 0.7, 0.0, 0.0)), &[part], part).unwrap();
    let mut previous = Quat::IDENTITY;
    // Left across the middle, out past the right edge and a long way beyond it, then back across.
    let path: Vec<f32> = (0..=40).map(|k| -0.7 + k as f32 * 0.06).chain((0..=40).map(|k| 1.7 - k as f32 * 0.06)).collect();
    for x in path {
        update_drag(&mut dm, &mut drag, ray(cam, f.origin + Vec3::new(x * r, 0.0, 0.0)), &free());
        let (_, now) = dm.get_pivot(part).unwrap();
        assert!(now.angle_between(previous).to_degrees() < 20.0, "at x = {x} the rotation jumped by {}", now.angle_between(previous).to_degrees());
        assert!(now.is_finite() && (now.length() - 1.0).abs() < 1e-4);
        previous = now;
    }
}

// ---- pivot -----------------------------------------------------------------------------------------------------

#[test]
fn the_pivot_tool_moves_the_pivot_and_leaves_the_object_where_it_is() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame(&dm, Tool::Pivot, part, cam);
    assert_eq!(f.origin, Vec3::new(10.0, 5.0, 0.0), "starts at the centre");
    let grab = f.origin + Vec3::X * (f.len * 0.5);
    let handle = hit_at(Tool::Pivot, &f, cam, grab).unwrap();
    let mut drag = begin_drag(&dm, handle, f, ray(cam, grab), &[part], part).unwrap();
    let snap = Snap::steps(0.5, 15.0);
    assert!(update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 1.9), &snap));
    assert_eq!(dm.vec3(part, "Position"), Vec3::new(10.0, 5.0, 0.0), "the object did not move");
    assert!((dm.vec3(part, "PivotOffset") - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-3, "the pivot is 2 studs along its X, got {:?}", dm.vec3(part, "PivotOffset"));
    assert!((drag.done.moved[0] - 2.0).abs() < 1e-3);

    // From then on, the move tool's gizmo sits at the pivot, and turning swings the object about it.
    let move_frame = frame(&dm, Tool::Move, part, cam);
    assert!((move_frame.origin - Vec3::new(12.0, 5.0, 0.0)).length() < 1e-3);
    let rot_frame = frame(&dm, Tool::Rotate, part, cam);
    let rdrag = begin_drag(&dm, Handle::Ring(2), rot_frame, ray(cam, rot_frame.origin + Vec3::X * (rot_frame.len * 0.9)), &[part], part).unwrap();
    assert!(turn_all(&mut dm, &rdrag, Quat::from_axis_angle(Vec3::Z, std::f32::consts::FRAC_PI_2)));
    assert!((dm.get_pivot(part).unwrap().0 - Vec3::new(12.0, 5.0, 0.0)).length() < 1e-3, "the pivot did not move");
    assert!((dm.vec3(part, "Position") - Vec3::new(12.0, 3.0, 0.0)).length() < 1e-3, "the centre swung round it, got {:?}", dm.vec3(part, "Position"));
}

// ---- the Select tool picks things up ------------------------------------------------------------------------------

/// Presses on the part's top face from above (a steep view) and returns the drag and the camera.
fn pick_up_from_above(dm: &DataModel, part: Id) -> (Drag, Vec3, Vec3) {
    let cam = Vec3::new(10.0, 40.0, 25.0);
    let point = Vec3::new(11.0, 6.0, 1.0); // on the top face (y = 6) of the 4 x 2 x 6 part
    let r = ray(cam, point);
    let begun = begin_drag(dm, Handle::Surface, surface_frame(point), r, &[part], part).unwrap();
    (begun, cam, point)
}

#[test]
fn a_picked_up_object_keeps_its_offset_from_the_cursor_and_slides_on_the_level_through_the_grab() {
    let (mut dm, part) = dm_with_part();
    let (mut drag, cam, point) = pick_up_from_above(&dm, part);
    // Not moved yet: the first frame leaves it exactly where it is (no snap to the cursor).
    assert!(!update_drag(&mut dm, &mut drag, ray(cam, point), &free()));
    assert_eq!(dm.vec3(part, "Position"), Vec3::new(10.0, 5.0, 0.0));
    // The cursor goes to another point of the same level: the object moves by exactly the same amount.
    let there = Vec3::new(14.0, 6.0, -2.5);
    assert!(update_drag(&mut dm, &mut drag, ray(cam, there), &free()));
    assert!((dm.vec3(part, "Position") - Vec3::new(13.0, 5.0, -3.5)).length() < 1e-3, "{:?}", dm.vec3(part, "Position"));
    // Whatever the cursor is aimed at above or below the level, the height does not change.
    assert_eq!(dm.vec3(part, "Position").y, 5.0);
    // Snapped to the move increment like the Move tool.
    update_drag(&mut dm, &mut drag, ray(cam, Vec3::new(14.4, 6.0, -2.4)), &Snap::steps(1.0, 0.0));
    assert!((dm.vec3(part, "Position") - Vec3::new(13.0, 5.0, -3.0)).length() < 1e-3, "{:?}", dm.vec3(part, "Position"));
}

#[test]
fn picking_up_works_from_any_camera_angle() {
    for cam in [Vec3::new(10.0, 40.0, 25.0), Vec3::new(-30.0, 20.0, -10.0), Vec3::new(10.0, 12.0, 60.0), Vec3::new(10.0, 6.4, 60.0), Vec3::new(10.0, 6.0, 60.0)] {
        let (mut dm, part) = dm_with_part();
        let point = Vec3::new(11.0, 6.0, 1.0);
        let r = ray(cam, point);
        let mut drag = begin_drag(&dm, Handle::Surface, surface_frame(point), r, &[part], part).unwrap();
        // The cursor moves a little to the side on the screen: the object follows by about the same distance, and never flies off.
        let view = view_of(cam, point);
        let from = view.project(point).unwrap();
        let moved = view.ray(from + Vec2::new(30.0, 0.0)).unwrap();
        update_drag(&mut dm, &mut drag, moved, &free());
        let shift = (dm.vec3(part, "Position") - Vec3::new(10.0, 5.0, 0.0)).length();
        assert!(shift.is_finite() && shift < 30.0, "from {cam:?} a 30-point move slid it {shift} studs");
        if drag.plane == Vec3::Y {
            assert!((dm.vec3(part, "Position").y - 5.0).abs() < 1e-4, "from {cam:?}: the height is kept");
        } else {
            // A level view: the object slides across the plane facing the camera, at the same depth.
            let depth = |p: Vec3| (p - cam).dot(view.forward());
            assert!((depth(dm.vec3(part, "Position")) - depth(Vec3::new(10.0, 5.0, 0.0))).abs() < 1e-2, "from {cam:?}: depth kept");
        }
    }
}

#[test]
fn picking_up_a_selection_of_several_moves_all_of_them_together() {
    let (mut dm, a) = dm_with_part();
    let b = add_part(&mut dm, Vec3::new(-6.0, 1.0, 4.0), Vec3::splat(2.0));
    let cam = Vec3::new(10.0, 40.0, 25.0);
    let point = Vec3::new(11.0, 6.0, 1.0);
    let mut drag = begin_drag(&dm, Handle::Surface, surface_frame(point), ray(cam, point), &[a, b], a).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, point + Vec3::new(2.0, 0.0, 3.0)), &free());
    assert!((dm.vec3(a, "Position") - Vec3::new(12.0, 5.0, 3.0)).length() < 1e-3);
    assert!((dm.vec3(b, "Position") - Vec3::new(-4.0, 1.0, 7.0)).length() < 1e-3, "their layout is kept");
}

#[test]
fn a_level_view_slides_on_the_plane_facing_the_camera_and_a_steep_one_on_the_level() {
    assert_eq!(gizmo::surface_drag_normal(Vec3::new(0.0, -0.9, -0.4).normalize()), Vec3::Y);
    assert_eq!(gizmo::surface_drag_normal(Vec3::new(0.0, 0.5, -1.0).normalize()), Vec3::Y, "looking up at something: the level through it still works");
    let level = Vec3::new(0.3, 0.02, -1.0).normalize();
    assert!(gizmo::surface_drag_normal(level).abs_diff_eq(-level, 1e-5));
}

// ---- the Select tool sets what it carries down on the surface under the mouse ----------------------------------------

/// A 4 x 2 x 6 part floating at (10, 5, 0) over a ground whose top is y = 0, a 6 x 4 x 6 block (top y = 4) at
/// x = -20, and a wall whose -X face is x = 29.
fn scene_to_set_down_on() -> (DataModel, Id, Id, Id, Id) {
    let (mut dm, part) = dm_with_part();
    let ground = add_part(&mut dm, Vec3::new(0.0, -1.0, 0.0), Vec3::new(200.0, 2.0, 200.0));
    let block = add_part(&mut dm, Vec3::new(-20.0, 2.0, 0.0), Vec3::new(6.0, 4.0, 6.0));
    let wall = add_part(&mut dm, Vec3::new(30.0, 5.0, 0.0), Vec3::new(2.0, 10.0, 20.0));
    (dm, part, ground, block, wall)
}

/// Picks `part` up by the point on its top face that the ray from `cam` to `via` goes through.
fn carry(dm: &DataModel, part: Id, cam: Vec3, via: Vec3) -> Drag {
    begin_drag(dm, Handle::Surface, surface_frame(via), ray(cam, via), &[part], part).unwrap()
}

/// How far the part's box is from the surface it is meant to touch, along the surface's normal (0 = touching).
fn gap_to(dm: &DataModel, part: Id, point: Vec3, normal: Vec3) -> f32 {
    let (min, max) = dm.collider(part).unwrap().aabb();
    let (center, half) = ((min + max) * 0.5, (max - min) * 0.5);
    (center - point).dot(normal) - normal.abs().dot(half)
}

#[test]
fn a_carried_object_is_set_down_on_the_ground_under_the_mouse() {
    let (mut dm, part, _, _, _) = scene_to_set_down_on();
    let cam = Vec3::new(10.0, 40.0, 25.0);
    let mut drag = carry(&dm, part, cam, Vec3::new(11.0, 6.0, 1.0));
    for target in [Vec3::new(14.0, 0.0, -5.0), Vec3::new(-8.0, 0.0, 12.0), Vec3::new(60.0, 0.0, 20.0)] {
        update_drag(&mut dm, &mut drag, ray(cam, target), &free());
        let p = dm.vec3(part, "Position");
        assert!((p.y - 1.0).abs() < 1e-4, "resting on the ground: the 2-tall box is centred 1 above it, got {}", p.y);
        assert!(gap_to(&dm, part, target, Vec3::Y).abs() < 1e-4, "touching, neither sunk in nor hovering");
    }
    // It is centred on the point the mouse is at (not offset from it by where it happened to be grabbed).
    for at in [Vec3::new(14.0, 0.0, -5.0), Vec3::new(-33.0, 0.0, 27.5), Vec3::new(0.25, 0.0, 0.0)] {
        update_drag(&mut dm, &mut drag, ray(cam, at), &free());
        let p = dm.vec3(part, "Position");
        assert!((p.x - at.x).abs() < 1e-3 && (p.z - at.z).abs() < 1e-3, "the mouse is at {at:?} and the object is at {p:?}");
    }
}

#[test]
fn a_carried_object_is_under_the_mouse_from_the_first_frame_even_if_it_was_grabbed_by_its_edge() {
    let (mut dm, part, ..) = scene_to_set_down_on();
    dm.set_prop(part, "Position", Value::V3(Vec3::new(10.0, 1.0, 0.0))).unwrap(); // resting on the ground
    let cam = Vec3::new(10.0, 40.0, 25.0);
    // Grabbed by a corner of its top face, well away from its middle.
    let mut drag = carry(&dm, part, cam, Vec3::new(11.8, 2.0, 2.9));
    // The mouse is over the ground a little way off: the object is centred there, resting on the ground like before.
    let at = Vec3::new(13.0, 0.0, -2.0);
    assert!(update_drag(&mut dm, &mut drag, ray(cam, at), &free()));
    let p = dm.vec3(part, "Position");
    assert!((p - Vec3::new(13.0, 1.0, -2.0)).length() < 1e-3, "{p:?}");
}
#[test]
fn a_carried_object_comes_to_rest_on_top_of_the_object_it_is_dragged_over() {
    let (mut dm, part, _, block, _) = scene_to_set_down_on();
    let cam = Vec3::new(-14.0, 40.0, 25.0);
    let mut drag = carry(&dm, part, cam, Vec3::new(11.0, 6.0, 1.0));
    // The mouse is over the block's top (y = 4): the part is set down on it, 2 + 2 = 4 + 1 above the ground.
    let on_block = Vec3::new(-20.0, 4.0, 0.0);
    update_drag(&mut dm, &mut drag, ray(cam, on_block), &free());
    assert!((dm.vec3(part, "Position").y - 5.0).abs() < 1e-4, "{:?}", dm.vec3(part, "Position"));
    assert!(gap_to(&dm, part, on_block, Vec3::Y).abs() < 1e-4);
    assert!(drag.surfaces.len() >= 3 && dm.exists(block));
    // Past the block's edge, onto the ground again.
    update_drag(&mut dm, &mut drag, ray(cam, Vec3::new(-30.0, 0.0, 0.0)), &free());
    assert!((dm.vec3(part, "Position").y - 1.0).abs() < 1e-4);
}

#[test]
fn a_carried_object_is_set_against_a_wall_with_its_nearest_side_touching_it() {
    let (mut dm, part, _, _, _) = scene_to_set_down_on();
    // From the side, along the ground: the mouse is on the wall's -X face (x = 29).
    let cam = Vec3::new(0.0, 5.0, 4.0);
    let mut drag = carry(&dm, part, Vec3::new(10.0, 20.0, 30.0), Vec3::new(11.0, 6.0, 1.0));
    let on_wall = Vec3::new(29.0, 6.0, 3.0);
    update_drag(&mut dm, &mut drag, ray(cam, on_wall), &free());
    let (min, max) = dm.collider(part).unwrap().aabb();
    assert!((max.x - 29.0).abs() < 1e-3, "the part's +X side lies on the wall's face: it reaches x = {}", max.x);
    assert!(min.x < 29.0 - 3.9, "and the part is on the near side of the wall");
    assert!(gap_to(&dm, part, on_wall, Vec3::NEG_X).abs() < 1e-4);
    let p = dm.vec3(part, "Position");
    assert!((p.y - 6.0).abs() < 1e-3 && (p.z - 3.0).abs() < 1e-3, "centred on the mouse's point on the wall: {p:?}");
}

#[test]
fn what_is_carried_is_set_down_by_its_whole_box_whatever_it_is_made_of() {
    let (mut dm, a, ..) = scene_to_set_down_on();
    // Two parts, one tall: the box that is set down is round both.
    let tall = add_part(&mut dm, Vec3::new(16.0, 9.0, 0.0), Vec3::new(2.0, 8.0, 2.0)); // y 5..13
    let cam = Vec3::new(10.0, 60.0, 30.0);
    let point = Vec3::new(11.0, 6.0, 1.0);
    let mut drag = begin_drag(&dm, Handle::Surface, surface_frame(point), ray(cam, point), &[a, tall], a).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, Vec3::new(0.0, 0.0, 10.0)), &free());
    // Their lowest point (the bottom of the part at y = 4 .. 6, which is 4 and the tall one 5) is now on the ground.
    let lowest = [a, tall].iter().map(|id| dm.collider(*id).unwrap().aabb().0.y).fold(f32::MAX, f32::min);
    assert!(lowest.abs() < 1e-4, "the box of both rests on the ground: lowest point {lowest}");
    assert!((dm.vec3(tall, "Position").y - dm.vec3(a, "Position").y - 4.0).abs() < 1e-4, "and they kept their layout");
    // The middle of the box round both is under the mouse (the ground point 0, 10).
    let (min_a, max_a) = dm.collider(a).unwrap().aabb();
    let (min_t, max_t) = dm.collider(tall).unwrap().aabb();
    let middle = (min_a.min(min_t) + max_a.max(max_t)) * 0.5;
    assert!(middle.x.abs() < 1e-3 && (middle.z - 10.0).abs() < 1e-3, "{middle:?}");
}

#[test]
fn along_a_level_surface_the_move_increment_applies_and_across_it_never() {
    let (mut dm, part, ..) = scene_to_set_down_on();
    let cam = Vec3::new(10.0, 40.0, 25.0);
    let mut drag = carry(&dm, part, cam, Vec3::new(11.0, 6.0, 1.0));
    update_drag(&mut dm, &mut drag, ray(cam, Vec3::new(14.3, 0.0, -5.4)), &Snap::steps(1.0, 0.0));
    let p = dm.vec3(part, "Position");
    assert!((p.x - 14.0).abs() < 1e-4 && (p.z + 5.0).abs() < 1e-4, "on whole studs of the world, next to the mouse: {p:?}");
    assert!((dm.vec3(part, "Position").y - 1.0).abs() < 1e-4, "but it still touches the ground exactly");
}

#[test]
fn over_empty_sky_the_carried_object_slides_on_its_plane_as_before() {
    let (mut dm, part, ..) = scene_to_set_down_on();
    let cam = Vec3::new(10.0, 40.0, 25.0);
    let point = Vec3::new(11.0, 6.0, 1.0);
    let mut drag = carry(&dm, part, cam, point);
    // A ray that meets nothing at all (up into the sky): the level through the grab point is followed, which a ray
    // that goes up cannot meet, so it holds still instead of leaping away.
    let sky = (cam, Vec3::new(0.3, 0.4, -1.0).normalize());
    assert!(nearest_surface(&drag.surfaces, sky).is_none());
    assert!(!update_drag(&mut dm, &mut drag, sky, &free()));
    assert_eq!(dm.vec3(part, "Position"), Vec3::new(10.0, 5.0, 0.0));
}

#[test]
fn set_down_places_a_box_against_any_normal() {
    let b = SnapBox { center: Vec3::new(0.0, 5.0, 0.0), half: Vec3::new(2.0, 1.0, 3.0) };
    // On the floor: centred on the point, one half-height above it.
    let d = set_down(&b, Vec3::new(10.0, 0.0, 10.0), Vec3::Y, 0.0);
    assert!((b.center + d - Vec3::new(10.0, 1.0, 10.0)).length() < 1e-5);
    // Against a wall facing -X: the box's +X half-width (2) from the wall.
    let d = set_down(&b, Vec3::new(30.0, 0.0, 0.0), Vec3::NEG_X, 0.0); // (the wall's face is x = 30)
    assert!(((b.center + d).x - 28.0).abs() < 1e-5);
    // A slanted surface: the box touches it at the corner nearest it (support = |n| . half).
    let n = Vec3::new(1.0, 1.0, 0.0).normalize();
    let d = set_down(&b, Vec3::ZERO, n, 0.0);
    let centre = b.center + d;
    assert!(((centre - Vec3::ZERO).dot(n) - n.abs().dot(b.half)).abs() < 1e-5, "touching along the normal");
}

// ---- quarter turns (Rotate 90 / Flip) while something is being dragged ----------------------------------------------

fn pose(dm: &DataModel, id: Id) -> (Vec3, Quat) {
    dm.get_pivot(id).unwrap()
}

fn same_pose(a: (Vec3, Quat), b: (Vec3, Quat)) -> bool {
    (a.0 - b.0).length() < 1e-3 && a.1.angle_between(b.1) < 1e-3
}

/// A move drag on the X arrow of `targets`, begun on the arrow and moved `along` studs, snapping off.
fn move_drag(dm: &mut DataModel, targets: &[Id], active: Id, along: f32) -> (Drag, Vec3, Vec3) {
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame_for(dm, Tool::Move, Space::World, active, targets, cam).unwrap();
    let grab = f.origin + Vec3::X * (f.len * 0.5);
    let handle = hit_at(Tool::Move, &f, cam, grab).expect("an arrow");
    let mut drag = begin_drag(dm, handle, f, ray(cam, grab), targets, active).unwrap();
    assert!(update_drag(dm, &mut drag, ray(cam, grab + Vec3::X * along), &free()));
    (drag, cam, grab)
}

#[test]
fn a_quarter_turn_during_a_move_drag_is_kept_by_the_drag_and_the_object_does_not_go_back() {
    let (mut dm, part) = dm_with_part();
    let (mut drag, cam, grab) = move_drag(&mut dm, &[part], part, 5.0);
    let before = pose(&dm, part);
    assert!((before.0.x - 15.0).abs() < 1e-3);

    assert!(quarter_turn(&mut dm, Some(&mut drag), Space::World, &[part], Some(part), 1));
    let turned = pose(&dm, part);
    assert!((turned.0 - before.0).length() < 1e-3, "a single object turns about its own pivot: it stays where the drag put it");
    assert!(turned.1.angle_between(before.1) > 1.5, "and is a quarter turn round");

    // The drag's next frames, with the mouse where it was: nothing is put back.
    for _ in 0..3 {
        update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 5.0), &free());
        assert!(same_pose(pose(&dm, part), turned), "the turn and the place survive the drag's own updates");
    }
    // Moving the mouse on carries the turned object on: the position follows, the turn stays.
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 8.0), &free());
    let (p, q) = pose(&dm, part);
    assert!((p.x - 18.0).abs() < 1e-3 && (p.y - before.0.y).abs() < 1e-3, "{p}");
    assert!(q.angle_between(turned.1) < 1e-3);
    // Another one on top: each press turns it a further quarter.
    quarter_turn(&mut dm, Some(&mut drag), Space::World, &[part], Some(part), 1);
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 8.0), &free());
    let twice = Quat::from_rotation_y(std::f32::consts::PI) * dm_start_rotation();
    assert!(pose(&dm, part).1.angle_between(twice) < 1e-3, "two quarter turns are a half turn");
    // The snapping goes on as before: a step of 5 rounds the 8 to 10.
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 8.0), &Snap::steps(5.0, 15.0));
    assert!((pose(&dm, part).0.x - 20.0).abs() < 1e-3, "the snapping setting still applies to the drag");
}

/// What the part from `dm_with_part` is turned like before anything.
fn dm_start_rotation() -> Quat {
    Quat::IDENTITY
}

#[test]
fn turning_several_objects_during_a_drag_turns_them_as_one_group_and_the_drag_carries_the_group() {
    let (mut dm, a) = dm_with_part();
    let b = add_part(&mut dm, Vec3::new(20.0, 5.0, 0.0), Vec3::splat(2.0));
    let (mut drag, cam, grab) = move_drag(&mut dm, &[a, b], a, 4.0);
    let (pa, pb) = (pose(&dm, a), pose(&dm, b));
    // The middle of the boxes of the two (what the group turns about).
    let middle = {
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for id in [a, b] {
            let (x, y) = dm.collider(id).unwrap().aabb();
            lo = lo.min(x);
            hi = hi.max(y);
        }
        (lo + hi) * 0.5
    };
    assert!(quarter_turn(&mut dm, Some(&mut drag), Space::World, &[a, b], Some(a), 1));
    let (ta, tb) = (pose(&dm, a), pose(&dm, b));
    let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    assert!((ta.0 - (middle + turn * (pa.0 - middle))).length() < 1e-3 && (tb.0 - (middle + turn * (pb.0 - middle))).length() < 1e-3, "swung round the middle of the group");
    assert!(((ta.0 - tb.0).length() - (pa.0 - pb.0).length()).abs() < 1e-3, "rigidly: the distance between them is the same");

    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 4.0), &free());
    assert!(same_pose(pose(&dm, a), ta) && same_pose(pose(&dm, b), tb), "the next frame of the drag keeps the group as turned");
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 9.0), &free());
    assert!(((pose(&dm, a).0 - ta.0) - Vec3::X * 5.0).length() < 1e-3 && ((pose(&dm, b).0 - tb.0) - Vec3::X * 5.0).length() < 1e-3, "and both follow the mouse");
    assert!(pose(&dm, a).1.angle_between(ta.1) < 1e-3 && pose(&dm, b).1.angle_between(tb.1) < 1e-3);
}

#[test]
fn a_nested_model_is_turned_once_with_everything_in_it() {
    let (mut dm, part) = dm_with_part();
    let ws = dm.workspace;
    let model = dm.create("Model").unwrap();
    dm.set_parent(model, Some(ws)).unwrap();
    dm.set_parent(part, Some(model)).unwrap();
    let inner = add_part(&mut dm, Vec3::new(10.0, 9.0, 0.0), Vec3::splat(2.0));
    dm.set_parent(inner, Some(model)).unwrap();
    let (mut drag, cam, grab) = move_drag(&mut dm, &[model], model, 3.0);
    let (a, b) = (pose(&dm, part), pose(&dm, inner));
    assert!(quarter_turn(&mut dm, Some(&mut drag), Space::World, &[model], Some(model), 0));
    let (ta, tb) = (pose(&dm, part), pose(&dm, inner));
    let turn = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    assert!(ta.1.angle_between((turn * a.1).normalize()) < 1e-3 && tb.1.angle_between((turn * b.1).normalize()) < 1e-3, "each part turned exactly once, not once per level of nesting");
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 3.0), &free());
    assert!(same_pose(pose(&dm, part), ta) && same_pose(pose(&dm, inner), tb));
}

#[test]
fn a_turn_during_a_picked_up_object_s_drag_keeps_it_turned_and_sets_it_down_by_its_new_shape() {
    let (mut dm, part) = dm_with_part(); // 4 x 2 x 6, centred at y = 5
    let point = Vec3::new(10.0, 6.0, 0.0);
    let cam = Vec3::new(10.0, 30.0, 40.0);
    let r = ray(cam, point);
    let mut drag = begin_drag(&dm, Handle::Surface, surface_frame(point), r, &[part], part).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, point + Vec3::X * 6.0), &free());
    let moved = pose(&dm, part);
    assert!(quarter_turn(&mut dm, Some(&mut drag), Space::World, &[part], Some(part), 0));
    let turned = pose(&dm, part);
    assert!(turned.1.angle_between(moved.1) > 1.5 && (turned.0 - moved.0).length() < 1e-3);
    // Same mouse, next frame: still turned, still there.
    update_drag(&mut dm, &mut drag, ray(cam, point + Vec3::X * 6.0), &free());
    assert!(same_pose(pose(&dm, part), turned), "the picked-up object is not put back as it was");
    // The box that is set down is the turned one: 6 high now, 2 deep.
    let b = drag.snap_box.unwrap();
    assert!((b.half - Vec3::new(2.0, 3.0, 1.0)).length() < 1e-3, "{:?}", b.half);
}

#[test]
fn a_turn_during_a_ring_drag_is_kept_and_the_ring_goes_on_turning_from_there() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 40.0, 0.1);
    let f = frame(&dm, Tool::Rotate, part, cam);
    let grab = f.origin + Vec3::X * (f.len * 0.9);
    let mut drag = begin_drag(&dm, Handle::Ring(1), f, ray(cam, grab), &[part], part).unwrap();
    let around = |degrees: f32| f.origin + Quat::from_axis_angle(Vec3::Y, degrees.to_radians()) * Vec3::X * (f.len * 0.9);
    update_drag(&mut dm, &mut drag, ray(cam, around(-30.0)), &free());
    let before = pose(&dm, part).1;
    assert!(quarter_turn(&mut dm, Some(&mut drag), Space::World, &[part], Some(part), 0));
    let turned = pose(&dm, part).1;
    assert!(turned.angle_between(before) > 1.5);
    update_drag(&mut dm, &mut drag, ray(cam, around(-30.0)), &free());
    assert!(pose(&dm, part).1.angle_between(turned) < 1e-3, "the same mouse keeps the turn");
    update_drag(&mut dm, &mut drag, ray(cam, around(-60.0)), &free());
    let after = pose(&dm, part).1;
    assert!((after.angle_between(turned).to_degrees() - 30.0).abs() < 0.5, "the ring turned a further 30 degrees from the turned object, got {}", after.angle_between(turned).to_degrees());
}

#[test]
fn a_scale_drag_cannot_take_a_turn_and_refuses_it_leaving_everything_alone() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame(&dm, Tool::Scale, part, cam);
    let grab = f.bounds.unwrap().handle(cam, 0, 1.0);
    let mut drag = begin_drag(&dm, Handle::Scale(0, 1.0), f, ray(cam, grab), &[part], part).unwrap();
    let before = pose(&dm, part);
    assert!(!quarter_turn(&mut dm, Some(&mut drag), Space::World, &[part], Some(part), 1), "nothing was turned");
    assert!(same_pose(pose(&dm, part), before));
}

#[test]
fn without_a_drag_it_is_the_same_group_turn() {
    let (mut dm, a) = dm_with_part();
    let before = pose(&dm, a);
    assert!(quarter_turn(&mut dm, None, Space::World, &[a], Some(a), 1));
    assert!(pose(&dm, a).1.angle_between(before.1) > 1.5 && (pose(&dm, a).0 - before.0).length() < 1e-4);
    assert!(!quarter_turn(&mut dm, None, Space::World, &[], None, 1), "nothing to turn");
}

#[test]
fn taking_hold_of_ctrl_mid_drag_changes_the_snapping_only_when_the_mouse_moves() {
    let (dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame(&dm, Tool::Move, part, cam);
    let grab = f.origin + Vec3::X * (f.len * 0.5);
    let mut drag = begin_drag(&dm, Handle::Move(MoveHandle::Arrow(0, 1.0)), f, ray(cam, grab), &[part], part).unwrap();
    drag.begin_with_ctrl(false);
    let (p1, p2) = (Some(Vec2::new(100.0, 100.0)), Some(Vec2::new(101.0, 100.0)));
    assert!(!drag.ctrl_effective(false, p1));
    // Ctrl goes down (for Ctrl+R): the snapping does not change yet, however many frames the key takes ...
    for _ in 0..3 {
        assert!(!drag.ctrl_effective(true, p1), "the objects stay where the old snapping put them");
    }
    // ... and does once the mouse moves.
    assert!(drag.ctrl_effective(true, p2));
    assert!(drag.ctrl_effective(true, p2));
    // Letting go works the same way, and pressing and letting go with no movement changes nothing at all.
    assert!(drag.ctrl_effective(false, p2), "held until the mouse moves");
    assert!(drag.ctrl_effective(true, p2), "back to what it was before: no change");
    assert!(drag.ctrl_effective(false, p1));
    assert!(!drag.ctrl_effective(false, p2), "the mouse moved: now it is released");
    // A drag begun with Ctrl already held starts with it.
    drag.begin_with_ctrl(true);
    assert!(drag.ctrl_effective(true, p1));
}

#[test]
fn a_pivot_drag_takes_a_turn_without_the_drag_overwriting_it() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame(&dm, Tool::Pivot, part, cam);
    let grab = f.origin + Vec3::X * (f.len * 0.5);
    let handle = hit_at(Tool::Pivot, &f, cam, grab).expect("an arrow");
    let mut drag = begin_drag(&dm, handle, f, ray(cam, grab), &[part], part).unwrap();
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 2.0), &free());
    let before = pose(&dm, part).1;
    assert!(quarter_turn(&mut dm, Some(&mut drag), Space::World, &[part], Some(part), 1));
    let turned = dm.world_cframe(part).unwrap().1;
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 3.0), &free());
    assert!(dm.world_cframe(part).unwrap().1.angle_between(turned) < 1e-3 && turned.angle_between(before) > 1.5, "the object stays turned: the pivot drag only moves the pivot");
}
// ---- scaling a model with the handles ---------------------------------------------------------------------------------

/// A model of two parts (one 10 studs east of the other) with a nested model inside, for the Scale tool.
fn scalable_model() -> (DataModel, Id, Id, Id, Id) {
    let mut dm = DataModel::new();
    let ws = dm.workspace;
    let model = dm.create("Model").unwrap();
    dm.set_parent(model, Some(ws)).unwrap();
    let a = add_part(&mut dm, Vec3::new(0.0, 0.0, 0.0), Vec3::new(4.0, 2.0, 2.0));
    let b = add_part(&mut dm, Vec3::new(10.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
    let nested = dm.create("Model").unwrap();
    dm.set_parent(nested, Some(model)).unwrap();
    dm.set_parent(a, Some(model)).unwrap();
    dm.set_parent(b, Some(nested)).unwrap();
    dm.set_prop(model, "PrimaryPart", Value::Ref(Some(a))).unwrap();
    (dm, model, a, b, nested)
}

fn scale_drag(dm: &DataModel, model: Id) -> (Drag, Vec3, Vec3) {
    let cam = Vec3::new(5.0, 6.0, 40.0);
    let f = frame_for(dm, Tool::Scale, Space::World, model, &[model], cam).unwrap();
    let grab = f.bounds.unwrap().handle(cam, 0, 1.0);
    (begin_drag(dm, Handle::Scale(0, 1.0), f, ray(cam, grab), &[model], model).unwrap(), cam, grab)
}

#[test]
fn scaling_a_model_with_shift_keeps_its_scale_property_in_step_and_without_shift_it_is_left_alone() {
    let (mut dm, model, a, b, nested) = scalable_model();
    let (start_a, start_b) = (dm.size(a), dm.size(b));
    let (mut drag, cam, grab) = scale_drag(&dm, model);
    let proportional = Snap { proportional: true, ..Snap::steps(0.0, 0.0) };
    // Pull the +X face out to double the box's length (a box 13 long, from x -2 to 11): a factor of two.
    let along = 13.0;
    assert!(update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * along), &proportional));
    assert!((drag.done.factor - 2.0).abs() < 1e-3, "{}", drag.done.factor);
    assert!((dm.size(a) - start_a * 2.0).length() < 1e-3 && (dm.size(b) - start_b * 2.0).length() < 1e-3, "every part is twice the size");
    assert!((dm.applied_scale(model) - 2.0).abs() < 1e-3, "so the model is twice what it was made as");
    assert!((dm.applied_scale(nested) - 2.0).abs() < 1e-3, "and so is the model inside it");

    // Shift let go in the middle of the drag: the contents are stretched along one axis only, and the models' scale,
    // which only means something for an even scale, goes back to what it was.
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * along), &free());
    assert!((dm.applied_scale(model) - 1.0).abs() < 1e-6 && (dm.applied_scale(nested) - 1.0).abs() < 1e-6);
    // And evenly again: back to double, from the same start (nothing has added up).
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * along), &proportional);
    assert!((dm.applied_scale(model) - 2.0).abs() < 1e-3 && (dm.size(a) - start_a * 2.0).length() < 1e-3);
    // The scale from the panel and the gizmo are the same thing: Scale = 1 now puts the parts back to their start size.
    dm.set_prop(model, "Scale", Value::Num(1.0)).unwrap();
    assert!((dm.size(a) - start_a).length() < 1e-3 && (dm.size(b) - start_b).length() < 1e-3);
}

#[test]
fn a_single_part_has_no_model_scale_to_keep() {
    let (mut dm, part) = dm_with_part();
    let cam = Vec3::new(10.0, 8.0, 40.0);
    let f = frame(&dm, Tool::Scale, part, cam);
    let grab = f.bounds.unwrap().handle(cam, 0, 1.0);
    let mut drag = begin_drag(&dm, Handle::Scale(0, 1.0), f, ray(cam, grab), &[part], part).unwrap();
    assert!(drag.models.is_empty());
    update_drag(&mut dm, &mut drag, ray(cam, grab + Vec3::X * 2.0), &Snap { proportional: true, ..Snap::steps(0.0, 0.0) });
    assert!(dm.size(part).x > 4.0);
}
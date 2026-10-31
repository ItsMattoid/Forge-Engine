//! `CFrame`: a position and a rotation. The rotation is kept as a quaternion; the matrix members
//! (`R00` ... `R22`, `GetComponents`) are derived from it. Rotation matrices are row-major as in Roblox:
//! `R00 R10 R20` is the `RightVector`, `R01 R11 R21` the `UpVector` and `R02 R12 R22` the `-LookVector`.

use bevy::math::{EulerRot, Mat3, Quat, Vec3};
use mlua::{Lua, MetaMethod, Result as LuaResult, UserData, UserDataFields, UserDataMethods, UserDataRef, Value, Variadic, Vector};

use super::{num, rt, unvec, vec};

#[derive(Clone, Copy, Debug)]
pub struct CF {
    pub p: Vec3,
    pub q: Quat,
}

impl CF {
    pub const IDENTITY: CF = CF { p: Vec3::ZERO, q: Quat::IDENTITY };

    pub fn mul(self, o: CF) -> CF {
        CF { p: self.p + self.q * o.p, q: (self.q * o.q).normalize() }
    }

    pub fn inverse(self) -> CF {
        let qi = self.q.inverse();
        CF { p: -(qi * self.p), q: qi }
    }

    /// `R00 R01 R02 R10 ... R22`.
    fn components(self) -> [f32; 12] {
        let m = Mat3::from_quat(self.q);
        let (r, u, b) = (m.x_axis, m.y_axis, m.z_axis);
        [self.p.x, self.p.y, self.p.z, r.x, u.x, b.x, r.y, u.y, b.y, r.z, u.z, b.z]
    }

    /// From the nine rotation entries in row-major order.
    fn from_matrix_rows(p: Vec3, r: [f32; 9]) -> CF {
        let m = Mat3::from_cols(Vec3::new(r[0], r[3], r[6]), Vec3::new(r[1], r[4], r[7]), Vec3::new(r[2], r[5], r[8]));
        CF { p, q: Quat::from_mat3(&m).normalize() }
    }

    fn same_rotation(self, o: CF) -> bool {
        self.q.dot(o.q).abs() > 1.0 - 1e-6
    }
}

pub fn as_cf(v: &Value) -> Option<CF> {
    match v {
        Value::UserData(ud) => ud.borrow::<CF>().ok().map(|c| *c),
        _ => None,
    }
}

fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Quat {
    let forward = (target - eye).normalize_or_zero();
    if forward == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    let mut right = forward.cross(up).normalize_or_zero();
    if right == Vec3::ZERO {
        right = forward.cross(Vec3::X).normalize_or_zero();
    }
    let true_up = right.cross(forward);
    Quat::from_mat3(&Mat3::from_cols(right, true_up, -forward))
}

impl UserData for CF {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "CFrame");
        f.add_field_method_get("Position", |_, t| Ok(vec(t.p)));
        f.add_field_method_get("X", |_, t| Ok(t.p.x));
        f.add_field_method_get("Y", |_, t| Ok(t.p.y));
        f.add_field_method_get("Z", |_, t| Ok(t.p.z));
        f.add_field_method_get("LookVector", |_, t| Ok(vec(t.q * Vec3::NEG_Z)));
        f.add_field_method_get("RightVector", |_, t| Ok(vec(t.q * Vec3::X)));
        f.add_field_method_get("UpVector", |_, t| Ok(vec(t.q * Vec3::Y)));
        f.add_field_method_get("XVector", |_, t| Ok(vec(t.q * Vec3::X)));
        f.add_field_method_get("YVector", |_, t| Ok(vec(t.q * Vec3::Y)));
        f.add_field_method_get("ZVector", |_, t| Ok(vec(t.q * Vec3::Z)));
        f.add_field_method_get("Rotation", |_, t| Ok(CF { p: Vec3::ZERO, q: t.q }));
        for (i, name) in ["R00", "R01", "R02", "R10", "R11", "R12", "R20", "R21", "R22"].into_iter().enumerate() {
            f.add_field_method_get(name, move |_, t| Ok(t.components()[3 + i]));
        }
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Inverse", |_, t, ()| Ok(t.inverse()));
        // `ToWorldSpace(cf, ...)` / `ToObjectSpace(cf, ...)`: every CFrame given is converted, and all are returned.
        m.add_method("ToWorldSpace", |_, t, others: Variadic<UserDataRef<CF>>| Ok(others.iter().map(|o| t.mul(**o)).collect::<Variadic<CF>>()));
        m.add_method("ToObjectSpace", |_, t, others: Variadic<UserDataRef<CF>>| {
            let inv = t.inverse();
            Ok(others.iter().map(|o| inv.mul(**o)).collect::<Variadic<CF>>())
        });
        m.add_method("PointToWorldSpace", |_, t, v: Vector| Ok(vec(t.p + t.q * unvec(&v))));
        m.add_method("PointToObjectSpace", |_, t, v: Vector| Ok(vec(t.q.inverse() * (unvec(&v) - t.p))));
        m.add_method("VectorToWorldSpace", |_, t, v: Vector| Ok(vec(t.q * unvec(&v))));
        m.add_method("VectorToObjectSpace", |_, t, v: Vector| Ok(vec(t.q.inverse() * unvec(&v))));
        m.add_method("Lerp", |_, t, (o, a): (UserDataRef<CF>, f32)| Ok(CF { p: t.p.lerp(o.p, a), q: t.q.slerp(o.q, a) }));
        // Euler angles in radians. `ToEulerAnglesXYZ` is the inverse of `CFrame.Angles`; `ToOrientation` /
        // `ToEulerAnglesYXZ` the inverse of `CFrame.fromOrientation` (X = pitch, Y = yaw, Z = roll).
        m.add_method("ToEulerAnglesXYZ", |_, t, ()| Ok(t.q.to_euler(EulerRot::XYZ)));
        let orientation = |_: &Lua, t: &CF, ()| {
            let (y, x, z) = t.q.to_euler(EulerRot::YXZ);
            Ok((x, y, z))
        };
        m.add_method("ToEulerAnglesYXZ", orientation);
        m.add_method("ToOrientation", orientation);
        m.add_method("ToAxisAngle", |_, t, ()| {
            let (axis, angle) = t.q.to_axis_angle();
            Ok((vec(axis), angle))
        });
        m.add_method("GetComponents", |_, t, ()| Ok(t.components().into_iter().collect::<Variadic<f32>>()));
        m.add_method("components", |_, t, ()| Ok(t.components().into_iter().collect::<Variadic<f32>>()));
        m.add_method("Orthonormalize", |_, t, ()| Ok(CF { p: t.p, q: t.q.normalize() }));
        m.add_method("FuzzyEq", |_, t, (o, eps): (UserDataRef<CF>, Option<f32>)| {
            let eps = eps.unwrap_or(1e-5);
            Ok(t.components().iter().zip(o.components()).all(|(a, b)| (a - b).abs() <= eps))
        });
        m.add_meta_method(MetaMethod::Mul, |lua, a, b: Value| -> LuaResult<Value> {
            match b {
                Value::Vector(v) => Ok(Value::Vector(vec(a.p + a.q * unvec(&v)))),
                other => {
                    let o = as_cf(&other).ok_or_else(|| rt("attempt to multiply a CFrame by an unsupported value"))?;
                    Ok(Value::UserData(lua.create_userdata(a.mul(o))?))
                }
            }
        });
        m.add_meta_method(MetaMethod::Add, |_, a, v: Vector| Ok(CF { p: a.p + unvec(&v), q: a.q }));
        m.add_meta_method(MetaMethod::Sub, |_, a, v: Vector| Ok(CF { p: a.p - unvec(&v), q: a.q }));
        m.add_meta_method(MetaMethod::Eq, |_, a, b: Value| Ok(as_cf(&b).is_some_and(|b| a.p == b.p && a.same_rotation(b))));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(t.components().iter().map(|f| f.to_string()).collect::<Vec<_>>().join(", ")));
    }
}

/// `CFrame.new(...)` in all its forms.
fn new(_: &Lua, args: Variadic<Value>) -> LuaResult<CF> {
    let n = |i: usize| num(&args[i]);
    Ok(match args.as_slice() {
        [] => CF::IDENTITY,
        [Value::Vector(p)] => CF { p: unvec(p), q: Quat::IDENTITY },
        [Value::Vector(p), Value::Vector(l)] => CF { p: unvec(p), q: look_at(unvec(p), unvec(l), Vec3::Y) },
        [_, _, _] => CF { p: Vec3::new(n(0)?, n(1)?, n(2)?), q: Quat::IDENTITY },
        // Position and a quaternion (x, y, z, w).
        [_, _, _, _, _, _, _] => {
            let q = Quat::from_xyzw(n(3)?, n(4)?, n(5)?, n(6)?);
            CF { p: Vec3::new(n(0)?, n(1)?, n(2)?), q: if q.length_squared() > 0.0 { q.normalize() } else { Quat::IDENTITY } }
        }
        // Position and the rotation matrix, row by row.
        [_, _, _, _, _, _, _, _, _, _, _, _] => {
            let mut r = [0.0; 9];
            for (i, entry) in r.iter_mut().enumerate() {
                *entry = n(3 + i)?;
            }
            CF::from_matrix_rows(Vec3::new(n(0)?, n(1)?, n(2)?), r)
        }
        _ => return Err(rt("invalid arguments to CFrame.new")),
    })
}

pub fn install(lua: &Lua) -> LuaResult<()> {
    let cf = lua.create_table()?;
    cf.set("new", lua.create_function(new)?)?;
    let angles = |_: &Lua, (x, y, z): (f32, f32, f32)| Ok(CF { p: Vec3::ZERO, q: Quat::from_euler(EulerRot::XYZ, x, y, z) });
    cf.set("Angles", lua.create_function(angles)?)?;
    cf.set("fromEulerAnglesXYZ", lua.create_function(angles)?)?;
    cf.set("fromEulerAngles", lua.create_function(angles)?)?;
    let orientation = |_: &Lua, (x, y, z): (f32, f32, f32)| Ok(CF { p: Vec3::ZERO, q: Quat::from_euler(EulerRot::YXZ, y, x, z) });
    cf.set("fromOrientation", lua.create_function(orientation)?)?;
    cf.set("fromEulerAnglesYXZ", lua.create_function(orientation)?)?;
    cf.set(
        "fromAxisAngle",
        lua.create_function(|_, (axis, angle): (Vector, f32)| {
            let axis = unvec(&axis).normalize_or_zero();
            Ok(CF { p: Vec3::ZERO, q: if axis == Vec3::ZERO { Quat::IDENTITY } else { Quat::from_axis_angle(axis, angle) } })
        })?,
    )?;
    // The axes need not be unit length or perpendicular; the rotation is made orthonormal from the first two.
    cf.set(
        "fromMatrix",
        lua.create_function(|_, (pos, vx, vy, vz): (Vector, Vector, Vector, Option<Vector>)| {
            let (x, y) = (unvec(&vx).normalize_or_zero(), unvec(&vy).normalize_or_zero());
            let z = vz.map_or_else(|| x.cross(y), |z| unvec(&z)).normalize_or_zero();
            if x == Vec3::ZERO || z == Vec3::ZERO {
                return Ok(CF { p: unvec(&pos), q: Quat::IDENTITY });
            }
            let y = z.cross(x);
            Ok(CF { p: unvec(&pos), q: Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize() })
        })?,
    )?;
    cf.set(
        "lookAt",
        lua.create_function(|_, (at, target, up): (Vector, Vector, Option<Vector>)| {
            let up = up.map_or(Vec3::Y, |u| unvec(&u));
            Ok(CF { p: unvec(&at), q: look_at(unvec(&at), unvec(&target), up) })
        })?,
    )?;
    // `CFrame.lookAlong(at, direction, up?)`: at `at`, facing along `direction`.
    cf.set(
        "lookAlong",
        lua.create_function(|_, (at, direction, up): (Vector, Vector, Option<Vector>)| {
            let up = up.map_or(Vec3::Y, |u| unvec(&u));
            let at = unvec(&at);
            Ok(CF { p: at, q: look_at(at, at + unvec(&direction), up) })
        })?,
    )?;
    cf.set("identity", lua.create_userdata(CF::IDENTITY)?)?;
    lua.globals().set("CFrame", cf)
}

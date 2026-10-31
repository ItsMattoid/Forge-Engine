//! `Vector3` (Luau's native vector, given Roblox's members through the vector type's metatable) and `Vector2`.

use bevy::math::{Vec2, Vec3};
use mlua::{Lua, MetaMethod, Result as LuaResult, UserData, UserDataFields, UserDataMethods, UserDataRef, Value, Vector};

use super::rt;

pub fn vec(v: Vec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}

pub fn unvec(v: &Vector) -> Vec3 {
    Vec3::new(v.x(), v.y(), v.z())
}

// ---- Vector2 ------------------------------------------------------------------------------

/// Roblox's `Vector2` (a userdata: Luau's native vector has three components).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct V2(pub Vec2);

pub fn as_v2(v: &Value) -> Option<Vec2> {
    match v {
        Value::UserData(ud) => ud.borrow::<V2>().ok().map(|v| v.0),
        _ => None,
    }
}

/// An operand of `+ - * /`: another vector or a number.
enum Operand {
    Vector(Vec2),
    Number(f32),
}

fn operand(v: &Value) -> LuaResult<Operand> {
    match v {
        Value::Number(n) => Ok(Operand::Number(*n as f32)),
        Value::Integer(i) => Ok(Operand::Number(*i as f32)),
        other => as_v2(other).map(Operand::Vector).ok_or_else(|| rt("attempt to perform arithmetic on a Vector2 and an unsupported value")),
    }
}

/// `a op b` for the two operand orders Luau may call a metamethod with.
fn arithmetic(a: &Value, b: &Value, vv: impl Fn(Vec2, Vec2) -> Vec2, vn: impl Fn(Vec2, f32) -> Vec2, nv: impl Fn(f32, Vec2) -> Vec2) -> LuaResult<V2> {
    Ok(V2(match (operand(a)?, operand(b)?) {
        (Operand::Vector(a), Operand::Vector(b)) => vv(a, b),
        (Operand::Vector(a), Operand::Number(b)) => vn(a, b),
        (Operand::Number(a), Operand::Vector(b)) => nv(a, b),
        (Operand::Number(_), Operand::Number(_)) => return Err(rt("attempt to perform arithmetic on two numbers as Vector2s")),
    }))
}

impl UserData for V2 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Vector2");
        f.add_field_method_get("X", |_, t| Ok(t.0.x));
        f.add_field_method_get("Y", |_, t| Ok(t.0.y));
        f.add_field_method_get("Magnitude", |_, t| Ok(t.0.length()));
        f.add_field_method_get("Unit", |_, t| Ok(V2(t.0.normalize_or_zero())));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Dot", |_, t, o: UserDataRef<V2>| Ok(t.0.dot(o.0)));
        // The 2D cross product: the z component of the 3D one.
        m.add_method("Cross", |_, t, o: UserDataRef<V2>| Ok(t.0.perp_dot(o.0)));
        m.add_method("Lerp", |_, t, (o, a): (UserDataRef<V2>, f32)| Ok(V2(t.0.lerp(o.0, a))));
        m.add_method("Abs", |_, t, ()| Ok(V2(t.0.abs())));
        m.add_method("Ceil", |_, t, ()| Ok(V2(t.0.ceil())));
        m.add_method("Floor", |_, t, ()| Ok(V2(t.0.floor())));
        m.add_method("Sign", |_, t, ()| Ok(V2(Vec2::new(sign(t.0.x), sign(t.0.y)))));
        m.add_method("Max", |_, t, o: UserDataRef<V2>| Ok(V2(t.0.max(o.0))));
        m.add_method("Min", |_, t, o: UserDataRef<V2>| Ok(V2(t.0.min(o.0))));
        m.add_method("Angle", |_, t, (o, signed): (UserDataRef<V2>, Option<bool>)| {
            let angle = t.0.angle_to(o.0);
            Ok(if signed == Some(true) { angle } else { angle.abs() })
        });
        m.add_method("FuzzyEq", |_, t, (o, eps): (UserDataRef<V2>, Option<f32>)| Ok(t.0.distance(o.0) <= eps.unwrap_or(1e-5)));
        m.add_meta_function(MetaMethod::Add, |_, (a, b): (Value, Value)| arithmetic(&a, &b, |a, b| a + b, |a, n| a + Vec2::splat(n), |n, b| Vec2::splat(n) + b));
        m.add_meta_function(MetaMethod::Sub, |_, (a, b): (Value, Value)| arithmetic(&a, &b, |a, b| a - b, |a, n| a - Vec2::splat(n), |n, b| Vec2::splat(n) - b));
        m.add_meta_function(MetaMethod::Mul, |_, (a, b): (Value, Value)| arithmetic(&a, &b, |a, b| a * b, |a, n| a * n, |n, b| b * n));
        m.add_meta_function(MetaMethod::Div, |_, (a, b): (Value, Value)| arithmetic(&a, &b, |a, b| a / b, |a, n| a / n, |n, b| Vec2::splat(n) / b));
        m.add_meta_function(MetaMethod::IDiv, |_, (a, b): (Value, Value)| arithmetic(&a, &b, |a, b| (a / b).floor(), |a, n| (a / n).floor(), |n, b| (Vec2::splat(n) / b).floor()));
        m.add_meta_method(MetaMethod::Unm, |_, t, ()| Ok(V2(-t.0)));
        m.add_meta_method(MetaMethod::Eq, |_, t, o: Value| Ok(as_v2(&o).is_some_and(|o| o == t.0)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("{}, {}", t.0.x, t.0.y)));
    }
}

fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

// ---- installation ---------------------------------------------------------------------------

pub fn install(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();

    // Vector2
    let v2 = lua.create_table()?;
    v2.set("new", lua.create_function(|_, (x, y): (Option<f32>, Option<f32>)| Ok(V2(Vec2::new(x.unwrap_or(0.0), y.unwrap_or(0.0)))))?)?;
    v2.set("zero", V2(Vec2::ZERO))?;
    v2.set("one", V2(Vec2::ONE))?;
    v2.set("xAxis", V2(Vec2::X))?;
    v2.set("yAxis", V2(Vec2::Y))?;
    globals.set("Vector2", v2)?;

    // Vector3
    let v3 = lua.create_table()?;
    v3.set("new", lua.create_function(|_, (x, y, z): (Option<f32>, Option<f32>, Option<f32>)| Ok(Vector::new(x.unwrap_or(0.0), y.unwrap_or(0.0), z.unwrap_or(0.0))))?)?;
    v3.set("zero", Vector::new(0.0, 0.0, 0.0))?;
    v3.set("one", Vector::new(1.0, 1.0, 1.0))?;
    v3.set("xAxis", Vector::new(1.0, 0.0, 0.0))?;
    v3.set("yAxis", Vector::new(0.0, 1.0, 0.0))?;
    v3.set("zAxis", Vector::new(0.0, 0.0, 1.0))?;
    globals.set("Vector3", v3)?;

    // Members of Vector3 values: `v.Magnitude`, `v.Unit`, `v:Dot(w)` ...
    let methods = lua.create_table()?;
    methods.set("Dot", lua.create_function(|_, (a, b): (Vector, Vector)| Ok(unvec(&a).dot(unvec(&b))))?)?;
    methods.set("Cross", lua.create_function(|_, (a, b): (Vector, Vector)| Ok(vec(unvec(&a).cross(unvec(&b)))))?)?;
    methods.set("Lerp", lua.create_function(|_, (a, b, t): (Vector, Vector, f32)| Ok(vec(unvec(&a).lerp(unvec(&b), t))))?)?;
    methods.set("Abs", lua.create_function(|_, a: Vector| Ok(vec(unvec(&a).abs())))?)?;
    methods.set("Ceil", lua.create_function(|_, a: Vector| Ok(vec(unvec(&a).ceil())))?)?;
    methods.set("Floor", lua.create_function(|_, a: Vector| Ok(vec(unvec(&a).floor())))?)?;
    methods.set("Sign", lua.create_function(|_, a: Vector| Ok(vec(Vec3::new(sign(a.x()), sign(a.y()), sign(a.z())))))?)?;
    methods.set("Max", lua.create_function(|_, (a, b): (Vector, Vector)| Ok(vec(unvec(&a).max(unvec(&b)))))?)?;
    methods.set("Min", lua.create_function(|_, (a, b): (Vector, Vector)| Ok(vec(unvec(&a).min(unvec(&b)))))?)?;
    methods.set("FuzzyEq", lua.create_function(|_, (a, b, eps): (Vector, Vector, Option<f32>)| Ok(unvec(&a).distance(unvec(&b)) <= eps.unwrap_or(1e-5)))?)?;
    // The angle between two vectors in radians; with an axis the angle is signed about it.
    methods.set(
        "Angle",
        lua.create_function(|_, (a, b, axis): (Vector, Vector, Option<Vector>)| {
            let (a, b) = (unvec(&a), unvec(&b));
            let angle = a.angle_between(b);
            Ok(match axis {
                Some(axis) if a.cross(b).dot(unvec(&axis)) < 0.0 => -angle,
                _ => angle,
            })
        })?,
    )?;
    let mt = lua.create_table()?;
    mt.set(
        "__index",
        lua.create_function(move |_, (v, key): (Vector, mlua::String)| -> LuaResult<Value> {
            let k = key.to_str()?;
            let k: &str = &k;
            match k {
                // (`v.X` on a vector the VM reaches by a chain of names, as in `Vector3.one.X`, comes through here too.)
                "X" | "x" => Ok(Value::Number(f64::from(v.x()))),
                "Y" | "y" => Ok(Value::Number(f64::from(v.y()))),
                "Z" | "z" => Ok(Value::Number(f64::from(v.z()))),
                "Magnitude" => Ok(Value::Number(unvec(&v).length() as f64)),
                "Unit" => Ok(Value::Vector(vec(unvec(&v).normalize_or_zero()))),
                other => methods.get::<Value>(other),
            }
        })?,
    )?;
    lua.set_type_metatable::<Vector>(Some(mt));
    Ok(())
}

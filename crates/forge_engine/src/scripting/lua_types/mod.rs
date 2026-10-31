//! Roblox-style value types implemented in Rust for Luau: `Vector3` (Luau's native vector with a metatable for
//! `.Magnitude`, `:Dot()` ...), `Vector2`, `CFrame`, `Color3` and `Random`. The types that are only data
//! (`UDim2`, `NumberSequence`, `BrickColor` ...) are Luau (`datatypes.luau`).

mod cframe;
mod color;
mod random;
mod vector;

use mlua::{Lua, Result as LuaResult};

pub use cframe::{as_cf, CF};
pub use color::{as_color, Color3};
pub use vector::{as_v2, unvec, vec, V2};

pub fn rt(msg: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(msg)
}

fn num(v: &mlua::Value) -> LuaResult<f32> {
    match v {
        mlua::Value::Number(n) => Ok(*n as f32),
        mlua::Value::Integer(i) => Ok(*i as f32),
        _ => Err(rt("expected a number")),
    }
}

pub fn install(lua: &Lua) -> LuaResult<()> {
    cframe::install(lua)?;
    color::install(lua)?;
    vector::install(lua)?;
    random::install(lua)
}

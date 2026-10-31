//! `Color3`: red, green and blue in 0..1.

use bevy::math::Vec3;
use mlua::{Lua, MetaMethod, Result as LuaResult, UserData, UserDataFields, UserDataMethods, UserDataRef, Value};

use super::rt;

#[derive(Clone, Copy, Debug)]
pub struct Color3(pub Vec3);

pub fn as_color(v: &Value) -> Option<Color3> {
    match v {
        Value::UserData(ud) => ud.borrow::<Color3>().ok().map(|c| *c),
        _ => None,
    }
}

/// Hue, saturation and value (each 0..1) of a colour.
fn to_hsv(c: Vec3) -> (f32, f32, f32) {
    let max = c.max_element();
    let delta = max - c.min_element();
    let value = max;
    let saturation = if max <= 0.0 { 0.0 } else { delta / max };
    let hue = if delta <= 0.0 {
        0.0
    } else if max == c.x {
        ((c.y - c.z) / delta).rem_euclid(6.0)
    } else if max == c.y {
        (c.z - c.x) / delta + 2.0
    } else {
        (c.x - c.y) / delta + 4.0
    } / 6.0;
    (hue, saturation, value)
}

fn from_hsv(h: f32, s: f32, v: f32) -> Vec3 {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let (i, f) = (h.floor() as i32, h - h.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i {
        0 => Vec3::new(v, t, p),
        1 => Vec3::new(q, v, p),
        2 => Vec3::new(p, v, t),
        3 => Vec3::new(p, q, v),
        4 => Vec3::new(t, p, v),
        _ => Vec3::new(v, p, q),
    }
}

fn byte(f: f32) -> u8 {
    (f.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `#RRGGBB`, `RRGGBB`, `#RGB` or `RGB`.
fn parse_hex(text: &str) -> Option<Vec3> {
    let digits = text.trim().trim_start_matches('#');
    let channel = |s: &str, short: bool| u8::from_str_radix(s, 16).ok().map(|v| if short { v * 17 } else { v });
    let (r, g, b) = match digits.len() {
        6 => (channel(&digits[0..2], false)?, channel(&digits[2..4], false)?, channel(&digits[4..6], false)?),
        3 => (channel(&digits[0..1], true)?, channel(&digits[1..2], true)?, channel(&digits[2..3], true)?),
        _ => return None,
    };
    Some(Vec3::new(r as f32, g as f32, b as f32) / 255.0)
}

impl UserData for Color3 {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Color3");
        f.add_field_method_get("R", |_, t| Ok(t.0.x));
        f.add_field_method_get("G", |_, t| Ok(t.0.y));
        f.add_field_method_get("B", |_, t| Ok(t.0.z));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Lerp", |_, t, (o, a): (UserDataRef<Color3>, f32)| Ok(Color3(t.0.lerp(o.0, a))));
        m.add_method("ToHSV", |_, t, ()| Ok(to_hsv(t.0)));
        m.add_method("ToHex", |_, t, ()| Ok(format!("{:02x}{:02x}{:02x}", byte(t.0.x), byte(t.0.y), byte(t.0.z))));
        m.add_meta_method(MetaMethod::Eq, |_, a, b: Value| Ok(as_color(&b).is_some_and(|b| a.0 == b.0)));
        m.add_meta_method(MetaMethod::ToString, |_, t, ()| Ok(format!("{}, {}, {}", t.0.x, t.0.y, t.0.z)));
    }
}

pub fn install(lua: &Lua) -> LuaResult<()> {
    let c3 = lua.create_table()?;
    c3.set("new", lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| Ok(Color3(Vec3::new(r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0)))))?)?;
    // 0..255 per channel; anything outside is held to that range.
    c3.set(
        "fromRGB",
        lua.create_function(|_, (r, g, b): (Option<f32>, Option<f32>, Option<f32>)| {
            Ok(Color3(Vec3::new(r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0)).clamp(Vec3::ZERO, Vec3::splat(255.0)).round() / 255.0))
        })?,
    )?;
    c3.set("fromHSV", lua.create_function(|_, (h, s, v): (f32, f32, f32)| Ok(Color3(from_hsv(h, s, v))))?)?;
    c3.set("fromHex", lua.create_function(|_, hex: String| parse_hex(&hex).map(Color3).ok_or_else(|| rt(format!("'{hex}' is not a valid hex colour (expected RRGGBB or #RRGGBB)"))))?)?;
    lua.globals().set("Color3", c3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_and_hex_is_parsed() {
        for c in [Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.2, 0.6, 0.4), Vec3::new(0.0, 0.0, 1.0), Vec3::splat(0.5), Vec3::ZERO] {
            let (h, s, v) = to_hsv(c);
            assert!((from_hsv(h, s, v) - c).length() < 1e-5, "{c:?} -> {:?}", (h, s, v));
        }
        assert_eq!(to_hsv(Vec3::new(1.0, 0.0, 0.0)), (0.0, 1.0, 1.0));
        assert_eq!(parse_hex("#FF8000"), Some(Vec3::new(1.0, 128.0 / 255.0, 0.0)));
        assert_eq!(parse_hex("f00"), Some(Vec3::new(1.0, 0.0, 0.0)));
        assert_eq!(parse_hex("12345"), None);
        assert_eq!(parse_hex("GGGGGG"), None);
    }
}

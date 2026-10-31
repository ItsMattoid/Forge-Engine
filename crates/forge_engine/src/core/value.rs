//! Basic value types shared by every layer: instance handles, property values,
//! property types and const-friendly default values.

use std::{
    collections::{HashMap, HashSet},
    hash::{BuildHasherDefault, Hasher},
};

use bevy::math::{EulerRot, Quat, Vec2, Vec3};

/// A fast, non-cryptographic hasher (the multiply-and-rotate scheme rustc itself uses) for the
/// engine's small keys: property and method names, instance ids, grid cells. The standard hasher is
/// built to resist hostile keys, which costs several times as much per lookup and buys nothing
/// here: these maps are small, and a lookup happens for every property a script touches.
#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

const FAST_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FastHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(FAST_SEED);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            // (The length goes into the top byte, which the up-to-seven bytes leave free, so that
            // "A" and "A\0" differ: a zero word would otherwise leave the hash where it was.)
            let mut word = (rest.len() as u64) << 56;
            for (i, b) in rest.iter().enumerate() {
                word |= (*b as u64) << (8 * i);
            }
            self.add(word);
        }
    }
    #[inline]
    fn write_u8(&mut self, v: u8) {
        self.add(v as u64);
    }
    #[inline]
    fn write_u32(&mut self, v: u32) {
        self.add(v as u64);
    }
    #[inline]
    fn write_i32(&mut self, v: i32) {
        self.add(v as u32 as u64);
    }
    #[inline]
    fn write_u64(&mut self, v: u64) {
        self.add(v);
    }
    #[inline]
    fn write_usize(&mut self, v: usize) {
        self.add(v as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

pub type FastBuild = BuildHasherDefault<FastHasher>;
/// A `HashMap` that hashes with [`FastHasher`].
pub type FastMap<K, V> = HashMap<K, V, FastBuild>;
/// A `HashSet` that hashes with [`FastHasher`].
pub type FastSet<K> = HashSet<K, FastBuild>;

/// Handle to an instance. The uid is never reused, so stale handles are detected.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Id {
    pub slot: u32,
    pub uid: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Num(f32),
    Str(String),
    V2(Vec2),
    V3(Vec3),
    Color(Vec3),
    CF(Vec3, Quat),
    Enum(String),
    Ref(Option<Id>),
    /// A value type that is only numbers (see [`DATA_TYPES`]): the tag says which one, the numbers are its
    /// flattened fields. Scripts see the real `NumberSequence`, `BrickColor` ... (the scripting layer converts).
    Data(&'static str, Vec<f32>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropType {
    Bool,
    Number,
    String,
    Vector2,
    Vector3,
    Color3,
    CFrame,
    /// Name of a registered enum (see `EnumDesc`).
    Enum(&'static str),
    /// Reference to another instance; the string is the required class (or "Instance").
    Ref(&'static str),
    /// Project-relative path to a Luau file.
    ScriptPath,
    /// Project-relative path to an asset with the given file extension (e.g. "obj").
    Asset(&'static str),
    /// One of the [`DATA_TYPES`] (a `NumberRange`, a `BrickColor` ...).
    Data(&'static str),
}

/// The Roblox value types that are stored on instances as plain numbers (`Value::Data`). The scripting layer
/// knows how to build each one from its numbers and back (`datatypes.luau`); nothing else needs to.
///
/// | type                 | numbers                                                                 |
/// |----------------------|-------------------------------------------------------------------------|
/// | `NumberRange`        | min, max                                                                |
/// | `NumberSequence`     | time, value, envelope for each keypoint                                 |
/// | `ColorSequence`      | time, r, g, b for each keypoint                                         |
/// | `PhysicalProperties` | density, friction, elasticity, frictionWeight, elasticityWeight (none: unset) |
/// | `BrickColor`         | the palette number                                                      |
/// | `UDim`, `UDim2`      | scale, offset (each axis)                                               |
/// | `Rect`               | min x, min y, max x, max y                                              |
/// | `Vector2int16`, `Vector3int16` | the components                                                |
pub const DATA_TYPES: &[&str] = &["NumberRange", "NumberSequence", "ColorSequence", "PhysicalProperties", "BrickColor", "UDim", "UDim2", "Rect", "Vector2int16", "Vector3int16"];

/// The `'static` name of a [`DATA_TYPES`] entry (what `Value::Data` and `PropType::Data` carry).
pub fn data_type(name: &str) -> Option<&'static str> {
    DATA_TYPES.iter().copied().find(|t| *t == name)
}

/// Const-friendly default values (usable in `static` class definitions).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum D {
    B(bool),
    N(f32),
    S(&'static str),
    V2(f32, f32),
    V(f32, f32, f32),
    C(f32, f32, f32),
    Cf,
    E(&'static str),
    R,
    /// A [`DATA_TYPES`] value: its tag and numbers.
    Dn(&'static str, &'static [f32]),
}

impl D {
    pub fn to_value(self) -> Value {
        match self {
            D::B(b) => Value::Bool(b),
            D::N(n) => Value::Num(n),
            D::S(s) => Value::Str(s.to_owned()),
            D::V2(x, y) => Value::V2(Vec2::new(x, y)),
            D::V(x, y, z) => Value::V3(Vec3::new(x, y, z)),
            D::C(r, g, b) => Value::Color(Vec3::new(r, g, b)),
            D::Cf => Value::CF(Vec3::ZERO, Quat::IDENTITY),
            D::E(s) => Value::Enum(s.to_owned()),
            D::R => Value::Ref(None),
            D::Dn(tag, numbers) => Value::Data(tag, numbers.to_vec()),
        }
    }
}

pub fn value_matches(ty: PropType, v: &Value) -> bool {
    use PropType as T;
    match (ty, v) {
        (T::Data(want), Value::Data(have, _)) => want == *have,
        _ => matches!(
            (ty, v),
            (T::Bool, Value::Bool(_))
                | (T::Number, Value::Num(_))
                | (T::String | T::ScriptPath | T::Asset(_), Value::Str(_))
                | (T::Vector2, Value::V2(_))
                | (T::Vector3, Value::V3(_))
                | (T::Color3, Value::Color(_))
                | (T::CFrame, Value::CF(..))
                | (T::Enum(_), Value::Enum(_))
                | (T::Ref(_), Value::Ref(_))
        ),
    }
}

/// Euler angles in degrees (applied Y, X, Z) to a rotation.
pub fn euler_deg(v: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, v.y.to_radians(), v.x.to_radians(), v.z.to_radians())
}

pub fn to_euler_deg(q: Quat) -> Vec3 {
    let (y, x, z) = q.to_euler(EulerRot::YXZ);
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fast_hasher_keeps_short_strings_ids_and_cells_apart() {
        let mut names: FastMap<String, usize> = FastMap::default();
        for i in 0..5000 {
            names.insert(format!("Property{i}"), i);
            names.insert(format!("{i}"), i + 10_000);
        }
        for i in 0..5000 {
            assert_eq!(names[&format!("Property{i}")], i);
            assert_eq!(names[&format!("{i}")], i + 10_000);
        }
        let mut ids: FastSet<Id> = FastSet::default();
        for slot in 0..200 {
            for uid in 100..150 {
                assert!(ids.insert(Id { slot, uid }));
            }
        }
        assert_eq!(ids.len(), 10_000);
        assert!(ids.contains(&Id { slot: 7, uid: 109 }) && !ids.contains(&Id { slot: 109, uid: 7 }), "slot and uid are not interchangeable");
        let mut cells: FastMap<(i32, i32, i32), u32> = FastMap::default();
        for x in -10..10 {
            for y in -10..10 {
                for z in -10..10 {
                    cells.insert((x, y, z), (x * 10_000 + y * 100 + z) as u32);
                }
            }
        }
        assert_eq!(cells.len(), 8000);
        assert_eq!(cells[&(-3, 4, -5)], (-30_000 + 400 - 5) as u32);
        // Strings that differ only after the first word, or only in length, hash differently.
        let hash = |s: &str| std::hash::BuildHasher::hash_one(&FastBuild::default(), s);
        assert_ne!(hash("Position"), hash("Positions"));
        assert_ne!(hash("AAAAAAAA1"), hash("AAAAAAAA2"));
        assert_ne!(hash(""), hash("\0"));
    }
}

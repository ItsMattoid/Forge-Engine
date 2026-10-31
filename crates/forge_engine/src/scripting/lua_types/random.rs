//! `Random`: a seeded generator (PCG32). The same seed gives the same numbers on every run (they are not the
//! numbers Roblox's generator gives for that seed).

use std::cell::Cell;

use bevy::math::Vec3;
use mlua::{Lua, MetaMethod, Result as LuaResult, Table, UserData, UserDataFields, UserDataMethods, Value};

use super::{rt, vec};

#[derive(Clone)]
pub struct Random {
    state: Cell<u64>,
}

const MULTIPLIER: u64 = 6364136223846793005;
const INCREMENT: u64 = 1442695040888963407;

impl Random {
    fn seeded(seed: u64) -> Random {
        let r = Random { state: Cell::new(0) };
        r.next_u32();
        r.state.set(r.state.get().wrapping_add(seed));
        r.next_u32();
        r
    }

    fn next_u32(&self) -> u32 {
        let old = self.state.get();
        self.state.set(old.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT));
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// A number in [0, 1) with 53 random bits.
    fn unit(&self) -> f64 {
        let bits = (u64::from(self.next_u32()) << 21) ^ u64::from(self.next_u32());
        (bits & ((1 << 53) - 1)) as f64 / (1u64 << 53) as f64
    }

    /// A whole number in `lo..=hi`.
    fn integer(&self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo) as u64 + 1;
        lo + ((self.unit() * span as f64) as u64).min(span - 1) as i64
    }
}

impl UserData for Random {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_meta_field(MetaMethod::Type, "Random");
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("NextInteger", |_, t, (lo, hi): (f64, f64)| {
            let (lo, hi) = (lo.floor() as i64, hi.floor() as i64);
            if lo > hi {
                return Err(rt("NextInteger: the minimum is larger than the maximum"));
            }
            Ok(t.integer(lo, hi))
        });
        m.add_method("NextNumber", |_, t, (lo, hi): (Option<f64>, Option<f64>)| {
            let (lo, hi) = (lo.unwrap_or(0.0), hi.unwrap_or(1.0));
            if lo > hi {
                return Err(rt("NextNumber: the minimum is larger than the maximum"));
            }
            Ok(lo + t.unit() * (hi - lo))
        });
        m.add_method("NextUnitVector", |_, t, ()| {
            // A point on the sphere: z uniform in [-1, 1], the angle uniform around it.
            let z = (t.unit() * 2.0 - 1.0) as f32;
            let angle = (t.unit() * std::f64::consts::TAU) as f32;
            let r = (1.0 - z * z).max(0.0).sqrt();
            Ok(vec(Vec3::new(r * angle.cos(), r * angle.sin(), z)))
        });
        m.add_method("Shuffle", |_, t, list: Table| {
            let n = list.raw_len();
            for i in (2..=n).rev() {
                let j = t.integer(1, i as i64) as usize;
                let (a, b): (Value, Value) = (list.raw_get(i)?, list.raw_get(j)?);
                list.raw_set(i, b)?;
                list.raw_set(j, a)?;
            }
            Ok(())
        });
        m.add_method("Clone", |_, t, ()| Ok(Random { state: Cell::new(t.state.get()) }));
        m.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("Random"));
    }
}

pub fn install(lua: &Lua) -> LuaResult<()> {
    let random = lua.create_table()?;
    random.set(
        "new",
        lua.create_function(|_, seed: Option<f64>| {
            let seed = seed.map_or_else(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64), |s| s as i64 as u64);
            Ok(Random::seeded(seed))
        })?,
    )?;
    lua.globals().set("Random", random)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers_and_ranges_are_respected() {
        let (a, b) = (Random::seeded(42), Random::seeded(42));
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        let r = Random::seeded(7);
        let mut seen = [false; 6];
        for _ in 0..1000 {
            let n = r.integer(1, 6);
            assert!((1..=6).contains(&n));
            seen[(n - 1) as usize] = true;
            let u = r.unit();
            assert!((0.0..1.0).contains(&u));
        }
        assert!(seen.iter().all(|s| *s), "every face comes up");
        assert_ne!(Random::seeded(1).next_u32(), Random::seeded(2).next_u32());
        assert_eq!(r.integer(5, 5), 5);
    }
}

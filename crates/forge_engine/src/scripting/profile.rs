//! What scripts cost, for the debug overlays.
//!
//! The host always measures how long each phase of a frame took (a few clock reads per frame).
//! Per-script costs are only measured while [`super::ScriptHost::set_profiling`] is on: the Luau
//! prelude then times every handler, task and script chunk and charges it to the script it was
//! written in (see the "profiling" part of `prelude.luau`). Those times are *inclusive*: a handler
//! that starts another one is charged for the other one's time as well.

use std::time::Instant;

use crate::core::{FastMap, FastSet};

/// The phases of one frame, in the order they run (`frame` in `scripting`).
pub const PHASES: [&str; 8] = ["Start scripts", "Input events", "Stepped", "Instance events", "Physics", "Heartbeat", "Timers (wait / delay)", "RenderStepped"];

pub const P_START: usize = 0;
pub const P_INPUT: usize = 1;
pub const P_STEPPED: usize = 2;
pub const P_EVENTS: usize = 3;
pub const P_PHYSICS: usize = 4;
pub const P_HEARTBEAT: usize = 5;
pub const P_TIMERS: usize = 6;
pub const P_RENDER_STEPPED: usize = 7;

/// How quickly [`ScriptCost::avg_ms`] follows the per-frame cost (a fraction per frame).
const AVERAGING: f32 = 0.05;
/// Scripts remembered; the cheapest are forgotten beyond this.
const MAX_SCRIPTS: usize = 512;

/// What one script has cost.
#[derive(Clone, Debug, Default)]
pub struct ScriptCost {
    pub uid: u32,
    pub name: String,
    /// The script's source file (empty when it could not be found).
    pub path: String,
    /// Milliseconds spent in the last frame.
    pub frame_ms: f32,
    /// A running average of `frame_ms` (over roughly the last second).
    pub avg_ms: f32,
    /// The most expensive single frame since the numbers were last cleared.
    pub peak_ms: f32,
    pub total_ms: f64,
    /// Handler / task / chunk runs charged to the script, in total and in the last frame.
    pub calls: u64,
    pub frame_calls: u32,
}

/// The cost of the last frame of scripting.
#[derive(Clone, Debug, Default)]
pub struct ScriptProfile {
    /// Milliseconds per phase of the last frame (indexes are the `P_*` constants).
    pub phase_ms: [f32; PHASES.len()],
    /// Everything `ScriptHost::frame` did, physics included.
    pub frame_ms: f32,
    pub frames: u64,
    /// Whether `scripts` is being filled in.
    pub per_script: bool,
    pub scripts: Vec<ScriptCost>,
}

impl ScriptProfile {
    /// What scripts (not physics) took in the last frame.
    pub fn scripts_ms(&self) -> f32 {
        (self.frame_ms - self.phase_ms[P_PHYSICS]).max(0.0)
    }

    pub fn physics_ms(&self) -> f32 {
        self.phase_ms[P_PHYSICS]
    }

    /// Forgets the totals and peaks (the running averages start again).
    pub fn clear_totals(&mut self) {
        for s in &mut self.scripts {
            s.peak_ms = 0.0;
            s.total_ms = 0.0;
            s.calls = 0;
        }
    }

    /// Takes in one frame's per-script seconds: `(uid, seconds, calls)`. `describe` names a script
    /// the first time it is seen.
    pub(super) fn absorb(&mut self, seen: &[(u32, f64, u32)], mut describe: impl FnMut(u32) -> (String, String)) {
        // (Looked up by uid, not by scanning: with thousands of scripts a scan per script is a stall.)
        let mut known: FastSet<u32> = self.scripts.iter().map(|s| s.uid).collect();
        for &(uid, _, _) in seen {
            if known.insert(uid) {
                let (name, path) = describe(uid);
                self.scripts.push(ScriptCost { uid, name, path, ..Default::default() });
            }
        }
        let this_frame: FastMap<u32, (f64, u32)> = seen.iter().map(|&(uid, secs, calls)| (uid, (secs, calls))).collect();
        for s in &mut self.scripts {
            let (ms, calls) = this_frame.get(&s.uid).map_or((0.0, 0), |&(secs, calls)| ((secs * 1000.0) as f32, calls));
            s.frame_ms = ms;
            s.frame_calls = calls;
            s.avg_ms += (ms - s.avg_ms) * AVERAGING;
            s.peak_ms = s.peak_ms.max(ms);
            s.total_ms += ms as f64;
            s.calls += calls as u64;
        }
        if self.scripts.len() > MAX_SCRIPTS {
            self.scripts.sort_by(|a, b| b.total_ms.total_cmp(&a.total_ms));
            self.scripts.truncate(MAX_SCRIPTS);
        }
    }

    /// The scripts ordered by their running average, most expensive first.
    pub fn by_cost(&self) -> Vec<&ScriptCost> {
        let mut list: Vec<&ScriptCost> = self.scripts.iter().collect();
        list.sort_by(|a, b| b.avg_ms.total_cmp(&a.avg_ms).then(b.total_ms.total_cmp(&a.total_ms)));
        list
    }
}

/// Measures the phases of one frame.
pub(super) struct Lap {
    at: Instant,
    pub ms: [f32; PHASES.len()],
}

impl Lap {
    pub fn new() -> Lap {
        Lap { at: Instant::now(), ms: [0.0; PHASES.len()] }
    }

    /// Ends the stretch since the last call and charges it to `phase`.
    pub fn to(&mut self, phase: usize) {
        let now = Instant::now();
        self.ms[phase] += (now - self.at).as_secs_f32() * 1000.0;
        self.at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_are_charged_per_script_and_averaged() {
        let mut p = ScriptProfile::default();
        let describe = |uid: u32| (format!("S{uid}"), format!("s{uid}.luau"));
        p.absorb(&[(1, 0.004, 2), (2, 0.001, 1)], describe);
        assert_eq!(p.scripts.len(), 2);
        assert_eq!(p.by_cost()[0].uid, 1, "the expensive one is first");
        assert!((p.scripts[0].frame_ms - 4.0).abs() < 1e-3 && p.scripts[0].frame_calls == 2);
        // A frame in which script 1 did nothing: its frame cost is 0, its average decays, its total stays.
        p.absorb(&[(2, 0.001, 1)], describe);
        let s1 = p.scripts.iter().find(|s| s.uid == 1).unwrap();
        assert_eq!(s1.frame_ms, 0.0);
        assert!(s1.avg_ms > 0.0 && s1.avg_ms < 0.2 + 1e-3, "{}", s1.avg_ms);
        assert!((s1.total_ms - 4.0).abs() < 1e-3 && s1.peak_ms > 3.9);
        p.clear_totals();
        assert_eq!(p.scripts.iter().find(|s| s.uid == 1).unwrap().total_ms, 0.0);
    }

    #[test]
    fn scripts_ms_leaves_out_physics() {
        let mut p = ScriptProfile { frame_ms: 5.0, ..Default::default() };
        p.phase_ms[P_PHYSICS] = 2.0;
        assert_eq!((p.scripts_ms(), p.physics_ms()), (3.0, 2.0));
    }
}

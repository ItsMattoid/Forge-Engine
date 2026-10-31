//! Frame-time history and what is worked out of it: smoothed and instantaneous FPS, averages over a
//! window, the 1% low, the frame budget, lag spikes and the points of the graphs.
//!
//! Recording a frame is one push on a ring buffer. Everything else is computed on demand, at the
//! refresh rate of the overlay (a few times per second), into buffers that are reused.

use std::collections::VecDeque;

/// The longest history that is kept, in seconds (the settings never ask for more).
pub const MAX_HISTORY_S: f64 = 300.0;
/// A hard cap on the number of samples (a 1000 FPS game for 120 s), so memory stays bounded.
const MAX_SAMPLES: usize = 120_000;
/// A single frame longer than this is clamped (a debugger pause or a minimised window is not a frame time).
const MAX_FRAME_S: f32 = 10.0;

#[derive(Clone, Copy, Debug)]
struct Sample {
    /// When the frame ended, in seconds since recording began.
    end: f64,
    /// How long it took, in seconds.
    dt: f32,
}

/// The durations of the latest frames.
#[derive(Debug, Default)]
pub struct FrameHistory {
    samples: VecDeque<Sample>,
    now: f64,
    frames: u64,
    /// Reused by the statistics that sort.
    scratch: Vec<f32>,
}

/// What the statistics are measured against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerfConfig {
    /// Seconds the displayed FPS is averaged over.
    pub smoothing_s: f32,
    /// Seconds the averages, the 1% low and the spike count cover.
    pub average_window_s: f32,
    pub target_fps: f32,
    /// A frame is a spike above this many times the median frame time.
    pub spike_factor: f32,
}

impl Default for PerfConfig {
    fn default() -> Self {
        PerfConfig { smoothing_s: 0.5, average_window_s: 10.0, target_fps: 60.0, spike_factor: 2.0 }
    }
}

/// The numbers the text overlays show; recomputed a few times per second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerfSnapshot {
    /// Whether any frame has been recorded.
    pub valid: bool,
    /// Frames per second over the smoothing window.
    pub fps: f32,
    /// Milliseconds per frame over the smoothing window.
    pub frame_ms: f32,
    /// The last frame on its own.
    pub instant_fps: f32,
    pub instant_ms: f32,
    /// Averages over the averaging window.
    pub avg_fps: f32,
    pub avg_ms: f32,
    /// The average FPS of the slowest 1% of the frames in the averaging window.
    pub low1_fps: f32,
    pub median_ms: f32,
    pub worst_ms: f32,
    /// Frames in the averaging window.
    pub window_frames: u32,
    /// Milliseconds one frame may take at the target frame rate.
    pub budget_ms: f32,
    /// `frame_ms` as a fraction of the budget (1.0 = exactly on budget).
    pub budget_used: f32,
    /// Percent of the frames in the averaging window that took longer than the budget.
    pub over_budget_pct: f32,
    /// A frame longer than this is a spike, milliseconds.
    pub spike_ms: f32,
    /// Spikes in the averaging window.
    pub spikes: u32,
    /// The smoothed frame rate is under the target.
    pub below_target: bool,
}

/// One column of a graph: the frames that fell in one slice of the time window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GraphPoint {
    /// Seconds before now at the slice's centre.
    pub age: f32,
    pub mean_ms: f32,
    /// The slowest frame of the slice (so a one-frame spike is not averaged away).
    pub max_ms: f32,
    /// Whether the slice contains a spike.
    pub spike: bool,
    /// Whether the slice's slowest frame is over the frame budget.
    pub over: bool,
}

impl FrameHistory {
    /// Records one frame of `dt` seconds. Frames that are not positive and finite are ignored.
    pub fn push(&mut self, dt: f32) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        let dt = dt.min(MAX_FRAME_S);
        self.now += dt as f64;
        self.frames += 1;
        self.samples.push_back(Sample { end: self.now, dt });
        let oldest = self.now - MAX_HISTORY_S;
        while self.samples.len() > MAX_SAMPLES || self.samples.front().is_some_and(|s| s.end < oldest) {
            self.samples.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.now = 0.0;
        self.frames = 0;
    }

    /// Frames recorded since the start (or the last [`FrameHistory::clear`]).
    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn last_ms(&self) -> Option<f32> {
        self.samples.back().map(|s| s.dt * 1000.0)
    }

    /// The frames that ended within the last `secs` seconds.
    fn window(&self, secs: f32) -> impl Iterator<Item = &Sample> {
        let cutoff = self.now - secs as f64;
        let start = self.samples.partition_point(|s| s.end <= cutoff);
        self.samples.range(start..)
    }

    /// Frames per second over the last `secs`: the number of frames divided by the time they took
    /// (not the average of the instantaneous rates, which a few fast frames would inflate).
    pub fn fps_over(&self, secs: f32) -> Option<f32> {
        let (n, total) = self.window(secs).fold((0u32, 0.0f64), |(n, t), s| (n + 1, t + s.dt as f64));
        (n > 0 && total > 0.0).then(|| (n as f64 / total) as f32)
    }

    /// Everything the text overlays show.
    pub fn snapshot(&mut self, cfg: &PerfConfig) -> PerfSnapshot {
        let Some(last) = self.samples.back().copied() else { return PerfSnapshot::default() };
        let budget_ms = 1000.0 / cfg.target_fps.max(1.0);
        let fps = self.fps_over(cfg.smoothing_s).unwrap_or(0.0);

        // The averaging window: sort a copy of its frame times, slowest first.
        let mut times = std::mem::take(&mut self.scratch);
        times.clear();
        times.extend(self.window(cfg.average_window_s).map(|s| s.dt * 1000.0));
        times.sort_unstable_by(|a, b| b.total_cmp(a));
        let n = times.len();
        let total_ms: f64 = times.iter().map(|t| *t as f64).sum();

        let avg_ms = if n > 0 { (total_ms / n as f64) as f32 } else { 0.0 };
        let worst_ms = times.first().copied().unwrap_or(0.0);
        let median_ms = if n > 0 { times[n / 2] } else { 0.0 };
        // The slowest 1% (at least one frame).
        let slow = n.div_ceil(100).max(1).min(n);
        let slow_total: f64 = times[..slow].iter().map(|t| *t as f64).sum();
        let low1_fps = if slow_total > 0.0 { (slow as f64 * 1000.0 / slow_total) as f32 } else { 0.0 };
        let spike_ms = (median_ms * cfg.spike_factor).max(median_ms + MIN_SPIKE_EXCESS_MS);
        let spikes = times.iter().take_while(|t| **t > spike_ms).count() as u32;
        let over = times.iter().take_while(|t| **t > budget_ms * BUDGET_TOLERANCE).count();
        self.scratch = times;

        PerfSnapshot {
            valid: true,
            fps,
            frame_ms: if fps > 0.0 { 1000.0 / fps } else { 0.0 },
            instant_fps: 1.0 / last.dt,
            instant_ms: last.dt * 1000.0,
            avg_fps: if avg_ms > 0.0 { 1000.0 / avg_ms } else { 0.0 },
            avg_ms,
            low1_fps,
            median_ms,
            worst_ms,
            window_frames: n as u32,
            budget_ms,
            budget_used: if fps > 0.0 { 1000.0 / fps / budget_ms } else { 0.0 },
            over_budget_pct: if n > 0 { over as f32 * 100.0 / n as f32 } else { 0.0 },
            spike_ms,
            spikes,
            below_target: fps > 0.0 && fps < cfg.target_fps * BELOW_TARGET_TOLERANCE,
        }
    }

    /// The columns of a graph covering the last `secs` seconds in `columns` slices (fewer when the
    /// history is shorter). `out` is reused.
    pub fn graph(&self, secs: f32, columns: usize, snapshot: &PerfSnapshot, out: &mut Vec<GraphPoint>) {
        out.clear();
        if columns == 0 || secs <= 0.0 || self.samples.is_empty() {
            return;
        }
        let width = secs as f64 / columns as f64;
        let start = self.now - secs as f64;
        // (sum of ms, frames, slowest ms) per slice.
        let mut slices: Vec<(f64, u32, f32)> = vec![(0.0, 0, 0.0); columns];
        for s in self.window(secs) {
            let i = (((s.end - start) / width) as usize).min(columns - 1);
            let ms = s.dt * 1000.0;
            let slot = &mut slices[i];
            slot.0 += ms as f64;
            slot.1 += 1;
            slot.2 = slot.2.max(ms);
        }
        for (i, (sum, n, max)) in slices.into_iter().enumerate() {
            if n == 0 {
                continue;
            }
            let age = (self.now - (start + (i as f64 + 0.5) * width)).max(0.0) as f32;
            out.push(GraphPoint { age, mean_ms: (sum / n as f64) as f32, max_ms: max, spike: snapshot.spike_ms > 0.0 && max > snapshot.spike_ms, over: max > snapshot.budget_ms * BUDGET_TOLERANCE });
        }
    }
}

/// A frame must also be this much slower than the median to count as a spike (a 1 ms hiccup in a
/// 0.4 ms frame is not a lag spike).
const MIN_SPIKE_EXCESS_MS: f32 = 2.0;
/// A frame is over the budget only when it exceeds it by more than this factor (a frame of exactly
/// 1/60 s is a hair over 16.67 ms, and vsync rounds frame times).
const BUDGET_TOLERANCE: f32 = 1.05;
/// The smoothed frame rate may be this fraction under the target before it counts as "below target"
/// (a vsynced 59.9 on a 60 FPS target is on target).
const BELOW_TARGET_TOLERANCE: f32 = 0.97;

/// A round number at or above `v` (1, 2, 2.5, 5 times a power of ten): the top of a graph's axis.
pub fn nice_ceil(v: f32) -> f32 {
    if !(v.is_finite() && v > 0.0) {
        return 1.0;
    }
    let power = 10f32.powf(v.log10().floor());
    for step in [1.0, 2.0, 2.5, 5.0, 10.0] {
        if v <= step * power * 1.0001 {
            return step * power;
        }
    }
    10.0 * power
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady(h: &mut FrameHistory, fps: f32, seconds: f32) {
        for _ in 0..(fps * seconds) as usize {
            h.push(1.0 / fps);
        }
    }

    #[test]
    fn a_steady_frame_rate_reads_back_as_itself() {
        let mut h = FrameHistory::default();
        steady(&mut h, 60.0, 12.0);
        let s = h.snapshot(&PerfConfig::default());
        assert!(s.valid);
        assert!((s.fps - 60.0).abs() < 0.5 && (s.avg_fps - 60.0).abs() < 0.5 && (s.low1_fps - 60.0).abs() < 0.5, "{s:?}");
        assert!((s.frame_ms - 16.67).abs() < 0.1 && (s.budget_ms - 16.67).abs() < 0.1);
        assert!(!s.below_target && s.spikes == 0 && s.over_budget_pct < 1.0, "{s:?}");
        assert!((s.budget_used - 1.0).abs() < 0.02);
    }

    #[test]
    fn nothing_recorded_is_not_a_number() {
        let mut h = FrameHistory::default();
        assert_eq!(h.snapshot(&PerfConfig::default()), PerfSnapshot::default());
        assert!(h.fps_over(1.0).is_none() && h.last_ms().is_none());
        let mut points = vec![GraphPoint::default()];
        h.graph(30.0, 100, &PerfSnapshot::default(), &mut points);
        assert!(points.is_empty());
        for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            h.push(bad);
        }
        assert!(h.is_empty(), "frames that are not positive and finite are ignored");
    }

    #[test]
    fn the_smoothed_value_follows_a_change_within_its_window_and_the_instant_one_follows_at_once() {
        let mut h = FrameHistory::default();
        steady(&mut h, 120.0, 5.0);
        steady(&mut h, 30.0, 1.0);
        let slow = PerfConfig { smoothing_s: 0.5, ..Default::default() };
        let s = h.snapshot(&slow);
        assert!((s.fps - 30.0).abs() < 1.0, "half a second of 30 FPS fills a 0.5 s window, got {}", s.fps);
        assert!((s.instant_fps - 30.0).abs() < 0.01);
        // A longer window is slower to follow.
        let lazy = PerfConfig { smoothing_s: 3.0, ..Default::default() };
        let l = h.snapshot(&lazy);
        assert!(l.fps > 45.0 && l.fps < 100.0, "a 3 s window still remembers the fast part: {}", l.fps);
    }

    #[test]
    fn the_one_percent_low_is_the_average_of_the_slowest_one_percent() {
        let mut h = FrameHistory::default();
        // 200 frames: 198 at 10 ms and 2 at 100 ms. The slowest 1% is 2 frames.
        for i in 0..200 {
            h.push(if i == 50 || i == 150 { 0.1 } else { 0.01 });
        }
        let s = h.snapshot(&PerfConfig { average_window_s: 100.0, ..Default::default() });
        assert!((s.low1_fps - 10.0).abs() < 0.01, "two 100 ms frames average 10 FPS, got {}", s.low1_fps);
        assert!(s.avg_fps > 50.0, "the average hardly notices them: {}", s.avg_fps);
        assert!((s.worst_ms - 100.0).abs() < 0.01 && (s.median_ms - 10.0).abs() < 0.01);
        assert_eq!(s.spikes, 2);
        // Fewer than 100 frames: the slowest single frame is the 1% low.
        let mut few = FrameHistory::default();
        for dt in [0.01, 0.01, 0.05, 0.01] {
            few.push(dt);
        }
        assert!((few.snapshot(&PerfConfig::default()).low1_fps - 20.0).abs() < 0.01);
    }

    #[test]
    fn spikes_and_budget_overruns_are_counted_separately() {
        let mut h = FrameHistory::default();
        // A game that runs a steady 30 FPS against a 60 FPS target: every frame is over budget, none is a spike.
        steady(&mut h, 30.0, 5.0);
        let s = h.snapshot(&PerfConfig::default());
        assert!(s.below_target && s.spikes == 0 && s.over_budget_pct > 99.0, "{s:?}");
        assert!(s.budget_used > 1.9);
        // The same game with one hitch.
        h.push(0.2);
        let s = h.snapshot(&PerfConfig::default());
        assert_eq!(s.spikes, 1);
        // A tiny hiccup in very short frames is not a spike.
        let mut fast = FrameHistory::default();
        for i in 0..100 {
            fast.push(if i == 3 { 0.0012 } else { 0.0004 });
        }
        assert_eq!(fast.snapshot(&PerfConfig::default()).spikes, 0);
    }

    #[test]
    fn on_target_with_vsync_jitter_is_not_below_target() {
        let mut h = FrameHistory::default();
        steady(&mut h, 59.5, 3.0);
        assert!(!h.snapshot(&PerfConfig::default()).below_target);
    }

    #[test]
    fn the_history_is_bounded_and_windows_cut_it() {
        let mut h = FrameHistory::default();
        steady(&mut h, 100.0, 400.0);
        assert!(h.len() as f64 <= MAX_HISTORY_S * 100.0 + 2.0, "older frames are dropped, kept {}", h.len());
        assert_eq!(h.frames(), 40_000);
        let ten = h.window(10.0).count();
        assert!((ten as i32 - 1000).abs() <= 2, "a 10 s window of 100 FPS holds about 1000 frames, got {ten}");
        h.clear();
        assert!(h.is_empty() && h.frames() == 0);
    }

    #[test]
    fn a_pause_is_clamped_instead_of_wrecking_the_numbers() {
        let mut h = FrameHistory::default();
        h.push(3600.0);
        assert_eq!(h.last_ms(), Some(10_000.0));
    }

    #[test]
    fn graph_columns_cover_the_window_and_keep_spikes_visible() {
        let mut h = FrameHistory::default();
        steady(&mut h, 60.0, 9.0);
        h.push(0.25);
        steady(&mut h, 60.0, 0.9);
        let snap = h.snapshot(&PerfConfig::default());
        let mut points = Vec::new();
        h.graph(10.0, 100, &snap, &mut points);
        assert!(!points.is_empty() && points.len() <= 100);
        assert!(points.windows(2).all(|w| w[0].age > w[1].age), "oldest first");
        assert!(points.iter().all(|p| p.age >= 0.0 && p.age <= 10.0));
        let spiky: Vec<_> = points.iter().filter(|p| p.spike).collect();
        assert_eq!(spiky.len(), 1, "exactly the slice with the hitch is a spike");
        assert!((spiky[0].max_ms - 250.0).abs() < 0.01 && spiky[0].mean_ms <= spiky[0].max_ms, "the slice reports its slowest frame, not just its mean");
        assert!(spiky[0].over, "250 ms is over the budget");
        assert!(points.iter().filter(|p| !p.spike).all(|p| !p.over), "ordinary 60 FPS frames are within the budget");
        assert!((spiky[0].age - 0.95).abs() < 0.3, "the hitch was about a second ago, at {}", spiky[0].age);
        // A window longer than the history leaves the old part empty.
        h.graph(100.0, 100, &snap, &mut points);
        assert!(points.len() < 20, "ten seconds of history fill ten columns of a hundred, got {}", points.len());
    }

    #[test]
    fn axis_tops_are_round_numbers() {
        for (v, want) in [(0.7, 1.0), (1.0, 1.0), (1.3, 2.0), (2.2, 2.5), (4.0, 5.0), (7.0, 10.0), (16.7, 20.0), (33.4, 50.0), (144.0, 200.0), (1000.0, 1000.0)] {
            assert_eq!(nice_ceil(v), want, "{v}");
        }
        assert_eq!(nice_ceil(f32::NAN), 1.0);
        assert_eq!(nice_ceil(-3.0), 1.0);
    }
}

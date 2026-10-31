//! The debug overlays as egui: one window per overlay drawn over the viewport, the Debug menu and
//! the controls that configure them. Used by the editor (over its viewport) and by the player.
//!
//! The windows only read [`Metrics`]; the one thing they change is where they sit (drag the title,
//! drop it near a corner to dock it). Windows docked to the same corner stack up, and a full
//! column continues beside itself. The numbers are refreshed at the rate the metrics were
//! collected at, so drawing is cheap: a few labels and two polylines per window.

use bevy_egui::egui::{self, Align, Align2, Color32, CursorIcon, FontId, Id, Layout, Order, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui};

use super::{
    frames::{nice_ceil, GraphPoint, PerfSnapshot},
    log::{self, Severity},
    metrics::{timing, Metrics},
    overlay::{DebugSettings, Dock, Group, Overlay, AVERAGE_WINDOW, COMMON_TARGETS, HISTORY, OPACITY, OVERLAYS, REFRESH, SCALE, SMOOTHING, SPIKE_FACTOR, TARGET_FPS},
};

// ---- colours -------------------------------------------------------------------------------------------

const TEXT: Color32 = Color32::from_rgb(236, 236, 236);
const DIM: Color32 = Color32::from_rgb(160, 164, 170);
const GOOD: Color32 = Color32::from_rgb(110, 214, 120);
const WARN: Color32 = Color32::from_rgb(255, 202, 84);
const BAD: Color32 = Color32::from_rgb(255, 104, 92);
const ESTIMATE: Color32 = Color32::from_rgb(190, 170, 255);

/// How far from a corner a dragged panel may be dropped to dock there (points).
const DOCK_DISTANCE: f32 = 56.0;
/// Space between a docked panel and the edge of the viewport.
const EDGE: f32 = 8.0;

// ---- number formatting -----------------------------------------------------------------------------------

/// `1234567` -> `1,234,567`.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `1536` -> `1.5 KB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn ms(v: f32) -> String {
    if v >= 100.0 {
        format!("{v:.0} ms")
    } else if v >= 10.0 {
        format!("{v:.1} ms")
    } else {
        format!("{v:.2} ms")
    }
}

/// Green when the frame rate is on target, amber when somewhat under, red when far under.
pub fn rate_color(fps: f32, target: f32) -> Color32 {
    if fps >= target * 0.97 {
        GOOD
    } else if fps >= target * 0.7 {
        WARN
    } else {
        BAD
    }
}

// ---- placing a panel -----------------------------------------------------------------------------------------

/// Space between two stacked windows.
const GAP: f32 = 6.0;

/// The corner a window dropped at `rect` docks to, if it is close enough to one.
pub fn dock_for_drop(rect: Rect, viewport: Rect) -> Option<Dock> {
    let near = |a: Pos2, b: Pos2| (a - b).length() <= DOCK_DISTANCE;
    let pad = egui::vec2(EDGE, EDGE);
    [
        (Dock::TopLeft, rect.left_top(), viewport.left_top() + pad),
        (Dock::TopRight, rect.right_top(), viewport.right_top() + egui::vec2(-pad.x, pad.y)),
        (Dock::BottomLeft, rect.left_bottom(), viewport.left_bottom() + egui::vec2(pad.x, -pad.y)),
        (Dock::BottomRight, rect.right_bottom(), viewport.right_bottom() - pad),
    ]
    .into_iter()
    .filter(|(_, a, b)| near(*a, *b))
    .min_by(|x, y| (x.1 - x.2).length().total_cmp(&(y.1 - y.2).length()))
    .map(|(dock, ..)| dock)
}

/// Windows docked to one corner are stacked away from it, one under (or over) the other; when a
/// column is full the next window starts a new column beside it.
#[derive(Clone, Copy, Default, Debug)]
struct Column {
    /// How far the next window starts from the corner, along the stack.
    used: f32,
    /// How far this column is from the corner's side.
    across: f32,
    width: f32,
}

impl Column {
    /// Where a window of `size` goes in the stack of `dock` and what to remember about it.
    fn place(&mut self, dock: Dock, size: egui::Vec2, viewport: Rect) -> (Align2, Pos2) {
        if self.used > 0.0 && self.used + size.y + EDGE > viewport.height() - EDGE {
            self.across += self.width + GAP;
            self.used = 0.0;
            self.width = 0.0;
        }
        let (x, y) = (self.across, self.used);
        self.used += size.y + GAP;
        self.width = self.width.max(size.x);
        match dock {
            Dock::TopLeft => (Align2::LEFT_TOP, viewport.left_top() + egui::vec2(EDGE + x, EDGE + y)),
            Dock::TopRight => (Align2::RIGHT_TOP, viewport.right_top() + egui::vec2(-EDGE - x, EDGE + y)),
            Dock::BottomLeft => (Align2::LEFT_BOTTOM, viewport.left_bottom() + egui::vec2(EDGE + x, -EDGE - y)),
            Dock::BottomRight => (Align2::RIGHT_BOTTOM, viewport.right_bottom() + egui::vec2(-EDGE - x, -EDGE - y)),
            Dock::Free => unreachable!("free windows are not stacked"),
        }
    }
}

/// The id of an overlay's window.
fn window_id(o: Overlay) -> Id {
    Id::new(("forge_debug_window", o.def().id))
}

/// How wide an overlay's window is at normal size (points); tables and graphs get more room.
fn base_width(o: Overlay) -> f32 {
    match o {
        Overlay::Fps | Overlay::FrameTime | Overlay::OnePercentLow | Overlay::FrameBudget | Overlay::InstanceCount | Overlay::DrawCalls | Overlay::Triangles | Overlay::Vertices | Overlay::Lights => 250.0,
        Overlay::FpsGraph | Overlay::FrameTimeGraph | Overlay::FrameBreakdown => 340.0,
        _ => 330.0,
    }
}

/// What dragging a window did.
struct Moved {
    rect: Rect,
    dragged: egui::Vec2,
    dropped: bool,
}

/// The layer the overlay windows are on: over the 3D view and the editor's panels, under everything the
/// editor puts in front of them (its settings window, dialogs, menus and popups are `Order::Foreground`).
pub const WINDOW_ORDER: Order = Order::Middle;

/// One window over the viewport: a dark panel with a title bar to drag it by.
///
/// It is as big as what it shows, in height and in width: nothing is cut off or put behind a scroll bar,
/// even when it is taller than the viewport (it then reaches past it, over the panels). Only a window taller
/// than the whole screen has to scroll.
fn window(ctx: &egui::Context, o: Overlay, viewport: Rect, (pivot, pos): (Align2, Pos2), s: &DebugSettings, m: &Metrics) -> Moved {
    let scale = s.scale;
    let screen_height = ctx.content_rect().height();
    let last_height = ctx.memory(|mem| mem.area_rect(window_id(o))).map_or(0.0, |r| r.height());
    let too_tall_for_the_screen = last_height > screen_height - 16.0;
    let mut dragged = egui::Vec2::ZERO;
    let mut dropped = false;
    let shown = egui::Area::new(window_id(o)).order(WINDOW_ORDER).pivot(pivot).fixed_pos(pos).constrain_to(viewport).interactable(true).show(ctx, |ui| {
        egui::Frame::NONE
            .fill(Color32::from_black_alpha((s.opacity * 255.0) as u8))
            .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(34)))
            .corner_radius(6)
            .inner_margin(egui::Margin::same((8.0 * scale).round() as i8))
            .show(ui, |ui| {
                ui.visuals_mut().override_text_color = Some(TEXT);
                for font in ui.style_mut().text_styles.values_mut() {
                    font.size *= scale;
                }
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 3.0) * scale;
                ui.set_width(base_width(o) * scale);
                let title = RichText::new(o.def().name.to_uppercase()).small().strong().color(DIM);
                let handle = ui.add(egui::Label::new(title).sense(Sense::drag()));
                let handle = handle.on_hover_cursor(CursorIcon::Grab).on_hover_text("Drag to move. Drop it near a corner of the viewport to dock it there.");
                dragged = handle.drag_delta();
                dropped = handle.drag_stopped();
                if too_tall_for_the_screen {
                    egui::ScrollArea::vertical().id_salt(("debug_scroll", o.def().id)).max_height((screen_height - 48.0).max(120.0)).auto_shrink([true, true]).show(ui, |ui| body(ui, o, s, m));
                } else {
                    body(ui, o, s, m);
                }
            });
    });
    Moved { rect: shown.response.rect, dragged, dropped }
}

// ---- the windows -------------------------------------------------------------------------------------------------

/// Draws the overlays that are on, each in a window of its own over `viewport`. Windows docked to the
/// same corner stack up. Returns whether the settings changed (a window was moved).
pub fn overlays(ctx: &egui::Context, viewport: Rect, settings: &mut DebugSettings, m: &Metrics) -> bool {
    let mut changed = false;
    let view = settings.clone();
    // One stack per corner, in the order of Dock::ALL (without Free).
    let mut stacks = [Column::default(); 4];
    let slot = |dock: Dock| Dock::ALL.iter().position(|d| *d == dock).unwrap_or(0);

    for def in OVERLAYS.iter().filter(|d| view.on(d.overlay)) {
        let o = def.overlay;
        let mut placement = view.placement(o);
        let anchor = match placement.dock {
            Dock::Free => (Align2::LEFT_TOP, viewport.left_top() + egui::vec2(placement.offset[0], placement.offset[1])),
            dock => {
                // The size is what the window had last frame (a new window is placed as a small one and settles at once).
                let size = ctx.memory(|mem| mem.area_rect(window_id(o))).map_or(egui::vec2(base_width(o) * view.scale, 60.0 * view.scale), |r| r.size());
                stacks[slot(dock)].place(dock, size, viewport)
            }
        };
        let moved = window(ctx, o, viewport, anchor, &view, m);

        if moved.dragged != egui::Vec2::ZERO {
            if placement.dock != Dock::Free {
                // Leaving a corner: start from where the window is now.
                let from = moved.rect.left_top() - viewport.left_top();
                placement.offset = [from.x, from.y];
                placement.dock = Dock::Free;
            }
            placement.offset[0] += moved.dragged.x;
            placement.offset[1] += moved.dragged.y;
            settings.set_placement(o, placement);
            changed = true;
        }
        if moved.dropped {
            if let Some(dock) = dock_for_drop(moved.rect, viewport) {
                placement.dock = dock;
                settings.set_placement(o, placement);
            }
            changed = true;
        }
    }
    changed
}

/// What one overlay's window shows.
fn body(ui: &mut Ui, o: Overlay, s: &DebugSettings, m: &Metrics) {
    let id = o.def().id;
    let p = &m.perf;
    // The frame-rate windows need at least one recorded frame.
    let frame_based = matches!(o, Overlay::Fps | Overlay::FpsGraph | Overlay::FrameTime | Overlay::FrameTimeGraph | Overlay::FrameBudget | Overlay::OnePercentLow | Overlay::FrameBreakdown);
    if frame_based && !p.valid {
        ui.colored_label(DIM, "Collecting frame times...");
        return;
    }
    match o {
        Overlay::Fps => fps_line(ui, s, p),
        Overlay::FrameTime => frame_time_line(ui, p),
        Overlay::FrameBudget => budget(ui, s, p),
        Overlay::OnePercentLow => low_line(ui, s, p),
        Overlay::FpsGraph => graph(ui, s, m, GraphKind::Fps),
        Overlay::FrameTimeGraph => graph(ui, s, m, GraphKind::FrameTime),
        Overlay::FrameBreakdown => breakdown(ui, s, m),
        Overlay::InstanceCount => instance_count(ui, id, m),
        Overlay::DrawCalls => draw_calls(ui, id, m),
        Overlay::Triangles => triangles(ui, id, m),
        Overlay::Vertices => vertices(ui, id, m),
        Overlay::Lights => lights(ui, id, m),
        Overlay::RenderStats => render_stats(ui, id, m),
        Overlay::ScriptPerformance => scripts(ui, id, m),
        Overlay::EngineTimings => engine_timings(ui, id, m),
        Overlay::Physics => physics(ui, id, m),
        Overlay::Memory => memory(ui, id, m),
        Overlay::Network => network(ui),
        Overlay::AssetLoading => assets(ui, id, m),
        Overlay::Warnings => warnings(ui, id, m),
    }
}

fn big(text: String, color: Color32, scale: f32) -> RichText {
    RichText::new(text).size(24.0 * scale).strong().color(color).monospace()
}

fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).monospace()
}

fn fps_line(ui: &mut Ui, s: &DebugSettings, p: &PerfSnapshot) {
    ui.horizontal(|ui| {
        ui.label(big(format!("{:.1}", p.fps), rate_color(p.fps, s.target_fps), s.scale));
        ui.label(RichText::new("FPS").color(DIM));
        if s.show_instant_fps {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(mono(format!("{:.0}", p.instant_fps)).color(DIM)).on_hover_text("The last frame on its own (the big number is averaged over the smoothing interval)");
                ui.label(RichText::new("instant").small().color(DIM));
            });
        }
    });
    if p.below_target {
        ui.colored_label(WARN, format!("Below the {:.0} FPS target", s.target_fps));
    }
}

fn frame_time_line(ui: &mut Ui, p: &PerfSnapshot) {
    egui::Grid::new("debug_frame_time").num_columns(2).spacing([10.0, 1.0]).show(ui, |ui| {
        ui.colored_label(DIM, "Frame time");
        ui.label(mono(ms(p.frame_ms)).strong());
        ui.end_row();
        ui.colored_label(DIM, "Average");
        ui.label(mono(format!("{}  ({:.1} FPS)", ms(p.avg_ms), p.avg_fps)));
        ui.end_row();
        ui.colored_label(DIM, "Worst");
        ui.label(mono(ms(p.worst_ms)).color(if p.spikes > 0 { BAD } else { TEXT }));
        ui.end_row();
    });
}

fn low_line(ui: &mut Ui, s: &DebugSettings, p: &PerfSnapshot) {
    egui::Grid::new("debug_low").num_columns(2).spacing([10.0, 1.0]).show(ui, |ui| {
        ui.colored_label(DIM, "1% low");
        ui.label(mono(format!("{:.1} FPS", p.low1_fps)).strong().color(rate_color(p.low1_fps, s.target_fps * 0.75)));
        ui.end_row();
        ui.colored_label(DIM, format!("Average ({:.0} s)", s.average_window_s));
        ui.label(mono(format!("{:.1} FPS", p.avg_fps)));
        ui.end_row();
    });
}

fn budget(ui: &mut Ui, s: &DebugSettings, p: &PerfSnapshot) {
    ui.horizontal(|ui| {
        ui.colored_label(DIM, format!("Budget @ {:.0} FPS", s.target_fps));
        ui.label(mono(ms(p.budget_ms)).strong());
    });
    // A bar: how much of the budget the (smoothed) frame uses; it turns red past 100%.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 11.0 * s.scale), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_white_alpha(22));
    let full = 1.5;
    let used = (p.budget_used / full).clamp(0.0, 1.0);
    let color = if p.budget_used <= 0.85 { GOOD } else if p.budget_used <= 1.0 { WARN } else { BAD };
    painter.rect_filled(Rect::from_min_size(rect.min, egui::vec2(rect.width() * used, rect.height())), 3.0, color);
    let x = rect.left() + rect.width() / full;
    painter.line_segment([Pos2::new(x, rect.top() - 1.0), Pos2::new(x, rect.bottom() + 1.0)], Stroke::new(1.5_f32, TEXT));
    ui.label(mono(format!("{:.0}% used   {:.1}% of frames over   {} spikes", p.budget_used * 100.0, p.over_budget_pct, p.spikes)).small().color(if p.over_budget_pct > 5.0 { WARN } else { DIM }))
        .on_hover_text(format!("A spike is a frame over {} (more than {:.1}x the typical frame).", ms(p.spike_ms), s.spike_factor));
}

// ---- the graphs ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphKind {
    Fps,
    FrameTime,
}

/// Maps graph data to pixels. Kept apart from the painting so it can be tested.
#[derive(Clone, Copy, Debug)]
pub struct Plot {
    pub area: Rect,
    pub history_s: f32,
    pub top: f32,
}

impl Plot {
    pub fn x(&self, age: f32) -> f32 {
        self.area.right() - (age / self.history_s).clamp(0.0, 1.0) * self.area.width()
    }

    /// Pixel row of a value on the vertical axis (values above the top are clipped to it).
    pub fn y(&self, value: f32) -> f32 {
        self.area.bottom() - (value / self.top).clamp(0.0, 1.0) * self.area.height()
    }
}

/// The top of a graph's vertical axis. Frame times: room for the budget line, capped so one huge
/// hitch does not flatten everything else (it is drawn clipped). Frame rates: the best frame rate
/// seen, or the target with headroom.
pub fn axis_top(kind: GraphKind, points: &[GraphPoint], s: &DebugSettings) -> f32 {
    let budget = s.budget_ms();
    match kind {
        GraphKind::FrameTime => {
            let peak = points.iter().map(|p| p.max_ms).fold(0.0, f32::max);
            nice_ceil(peak.min((budget * 8.0).max(34.0)).max(budget * 1.5) * 1.05)
        }
        GraphKind::Fps => {
            let best = points.iter().map(|p| if p.mean_ms > 0.0 { 1000.0 / p.mean_ms } else { 0.0 }).fold(0.0, f32::max);
            nice_ceil((best * 1.05).max(s.target_fps * 1.25).min(1000.0))
        }
    }
}

/// The frame-rate reference lines to draw: the common targets, plus the chosen one.
fn reference_fps(s: &DebugSettings) -> Vec<f32> {
    let mut lines: Vec<f32> = COMMON_TARGETS.to_vec();
    if !lines.iter().any(|t| (t - s.target_fps).abs() < 0.5) {
        lines.push(s.target_fps);
    }
    lines
}

fn graph(ui: &mut Ui, s: &DebugSettings, m: &Metrics, kind: GraphKind) {
    let unit = match kind {
        GraphKind::Fps => "frames per second",
        GraphKind::FrameTime => "milliseconds per frame",
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(unit).small().color(DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if m.perf.spikes > 0 && s.highlight_spikes {
                ui.colored_label(BAD, format!("{} spikes", m.perf.spikes));
            }
            if m.perf.below_target {
                ui.colored_label(WARN, "below target");
            }
        });
    });
    let k = s.scale;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0 * k), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, Color32::from_white_alpha(10));

    let plot = {
        let area = Rect::from_min_max(rect.min + egui::vec2(38.0 * k, 6.0 * k), rect.max - egui::vec2(6.0 * k, 18.0 * k));
        Plot { area, history_s: s.history_s, top: axis_top(kind, &m.graph, s) }
    };
    let font = FontId::monospace(10.0 * k);
    let faint = Stroke::new(1.0_f32, Color32::from_white_alpha(26));

    // Axes: three labelled gridlines on the left, times along the bottom.
    for frac in [0.0, 0.5, 1.0] {
        let value = plot.top * frac;
        let y = plot.y(value);
        painter.line_segment([Pos2::new(plot.area.left(), y), Pos2::new(plot.area.right(), y)], faint);
        let label = if value >= 100.0 || value.fract() == 0.0 { format!("{value:.0}") } else { format!("{value:.1}") };
        painter.text(Pos2::new(plot.area.left() - 4.0, y), Align2::RIGHT_CENTER, label, font.clone(), DIM);
    }
    painter.text(Pos2::new(plot.area.left(), rect.bottom() - 1.0), Align2::LEFT_BOTTOM, format!("-{:.0}s", s.history_s), font.clone(), DIM);
    painter.text(Pos2::new(plot.area.center().x, rect.bottom() - 1.0), Align2::CENTER_BOTTOM, format!("-{:.0}s", s.history_s / 2.0), font.clone(), DIM);
    painter.text(Pos2::new(plot.area.right(), rect.bottom() - 1.0), Align2::RIGHT_BOTTOM, "now", font.clone(), DIM);

    // Reference lines for the common frame-rate targets.
    if s.show_target_lines {
        for target in reference_fps(s) {
            let value = match kind {
                GraphKind::Fps => target,
                GraphKind::FrameTime => 1000.0 / target,
            };
            if value > plot.top * 0.98 || value <= 0.0 {
                continue;
            }
            let y = plot.y(value);
            let chosen = (target - s.target_fps).abs() < 0.5;
            let color = if chosen { Color32::from_rgba_unmultiplied(110, 176, 255, 200) } else { Color32::from_white_alpha(60) };
            painter.extend(Shape::dashed_line(&[Pos2::new(plot.area.left(), y), Pos2::new(plot.area.right(), y)], Stroke::new(if chosen { 1.4_f32 } else { 1.0 }, color), 4.0, 3.0));
            painter.text(Pos2::new(plot.area.right() - 2.0, y - 1.0), Align2::RIGHT_BOTTOM, format!("{target:.0}"), font.clone(), color);
        }
    }

    if m.graph.is_empty() {
        painter.text(plot.area.center(), Align2::CENTER_CENTER, "collecting...", FontId::proportional(11.0), DIM);
        return;
    }

    let column = (plot.area.width() / m.graph.len().max(1) as f32 * 0.9).clamp(1.0, 4.0);
    match kind {
        GraphKind::FrameTime => {
            // One bar per slice, as tall as the slowest frame in it: a lag spike cannot hide in an average.
            for p in &m.graph {
                let x = plot.x(p.age);
                let color = bar_color(p, s);
                painter.rect_filled(Rect::from_min_max(Pos2::new(x - column * 0.5, plot.y(p.max_ms)), Pos2::new(x + column * 0.5, plot.area.bottom())), 0.0, color);
                if p.max_ms > plot.top {
                    // Off the top of the axis: a marker.
                    painter.circle_filled(Pos2::new(x, plot.area.top() + 2.0), 2.0, BAD);
                }
            }
        }
        GraphKind::Fps => {
            let fps = |ms: f32| if ms > 0.0 { 1000.0 / ms } else { 0.0 };
            // The slowest frame of each slice as a faint band under the line, the average as the line.
            for p in &m.graph {
                let x = plot.x(p.age);
                painter.line_segment([Pos2::new(x, plot.y(fps(p.mean_ms))), Pos2::new(x, plot.y(fps(p.max_ms)))], Stroke::new(column, Color32::from_white_alpha(34)));
            }
            // The line is amber where the average is under the target, green elsewhere: it is cut into runs.
            let flush = |run: Vec<Pos2>, below: bool| {
                if run.len() > 1 {
                    painter.add(Shape::line(run, Stroke::new(1.6_f32, if below { WARN } else { GOOD })));
                }
            };
            let mut run: Vec<Pos2> = Vec::new();
            let mut run_below = false;
            for p in &m.graph {
                let value = fps(p.mean_ms);
                let below = value < s.target_fps * 0.97;
                let point = Pos2::new(plot.x(p.age), plot.y(value));
                if !run.is_empty() && below != run_below {
                    let join = run[run.len() - 1];
                    flush(std::mem::take(&mut run), run_below);
                    run.push(join); // the next run starts where this one ended: no gap
                }
                run_below = below;
                run.push(point);
            }
            flush(run, run_below);
            if s.highlight_spikes {
                for p in m.graph.iter().filter(|p| p.spike) {
                    painter.circle_filled(Pos2::new(plot.x(p.age), plot.y(fps(p.max_ms))), 2.6, BAD);
                }
            }
        }
    }
    response.on_hover_text(format!("Last {:.0} seconds. Dashed lines mark 30 / 60 / 120 / 144 FPS; the chosen target is blue. Red = lag spike, amber = over the frame budget.", s.history_s));
}

fn bar_color(p: &GraphPoint, s: &DebugSettings) -> Color32 {
    if p.spike && s.highlight_spikes {
        BAD
    } else if p.over {
        WARN
    } else {
        Color32::from_rgb(96, 190, 130)
    }
}

// ---- frame time breakdown ---------------------------------------------------------------------------------------

/// One slice of the frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Slice {
    pub name: String,
    pub ms: f32,
    /// Worked out by subtraction rather than read from a clock.
    pub estimated: bool,
}

/// The main thread's share of the frame, split by system, plus what is left over. `frame_ms` is
/// the frame as the clock saw it. `None` until the main thread has been timed.
pub fn main_thread_slices(m: &Metrics, frame_ms: f32) -> Option<Vec<Slice>> {
    let main = m.timings.get(timing::MAIN)?.avg_ms;
    if main <= 0.0 {
        return None;
    }
    let mut slices: Vec<Slice> = Vec::new();
    let mut named = 0.0;
    for row in m.timings.rows().iter().filter(|r| r.name != timing::MAIN && r.avg_ms > 0.0) {
        named += row.avg_ms;
        slices.push(Slice { name: row.name.to_owned(), ms: row.avg_ms, estimated: false });
    }
    slices.push(Slice { name: "Other main-thread work".into(), ms: (main - named).max(0.0), estimated: true });
    slices.push(Slice { name: "Waiting (vsync, GPU, render thread)".into(), ms: (frame_ms - main).max(0.0), estimated: true });
    Some(slices)
}

fn slice_color(i: usize) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(110, 176, 255),
        Color32::from_rgb(255, 170, 90),
        Color32::from_rgb(150, 220, 130),
        Color32::from_rgb(220, 130, 220),
        Color32::from_rgb(240, 220, 110),
        Color32::from_rgb(120, 210, 210),
        Color32::from_rgb(230, 130, 130),
        Color32::from_rgb(170, 170, 200),
    ];
    PALETTE[i % PALETTE.len()]
}

fn breakdown(ui: &mut Ui, s: &DebugSettings, m: &Metrics) {
    let frame = m.perf.frame_ms;
    let Some(slices) = main_thread_slices(m, frame) else {
        ui.colored_label(DIM, "Collecting timings...");
        return;
    };
    let total: f32 = slices.iter().map(|s| s.ms).sum::<f32>().max(0.001);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0 * s.scale), Sense::hover());
    let painter = ui.painter_at(rect);
    let mut x = rect.left();
    for (i, slice) in slices.iter().enumerate() {
        let w = rect.width() * slice.ms / total;
        let color = if slice.name.starts_with("Waiting") { Color32::from_white_alpha(40) } else { slice_color(i) };
        painter.rect_filled(Rect::from_min_size(Pos2::new(x, rect.top()), egui::vec2(w, rect.height())), 0.0, color);
        x += w;
    }
    egui::Grid::new("debug_breakdown").num_columns(3).spacing([8.0, 1.0]).show(ui, |ui| {
        for (i, slice) in slices.iter().enumerate() {
            let color = if slice.name.starts_with("Waiting") { DIM } else { slice_color(i) };
            ui.colored_label(color, "â– ");
            ui.label(RichText::new(&slice.name).small());
            let text = mono(format!("{}{}", if slice.estimated { "~" } else { "" }, ms(slice.ms)));
            ui.label(if slice.estimated { text.color(ESTIMATE) } else { text });
            ui.end_row();
        }
        ui.label("");
        ui.label(RichText::new("Render thread (CPU)").small());
        match m.gpu.render_cpu_ms {
            Some(v) => ui.label(mono(ms(v))),
            None => ui.colored_label(DIM, "n/a"),
        };
        ui.end_row();
        ui.label("");
        ui.label(RichText::new("GPU").small());
        match m.gpu.gpu_ms {
            Some(v) => ui.label(mono(ms(v))),
            None => ui.colored_label(DIM, "n/a"),
        };
        ui.end_row();
    });
    ui.label(RichText::new("~ estimated (by subtraction). The render thread and the GPU run alongside the main thread, so they are not part of the bar.").small().color(DIM));
    if m.gpu.gpu_ms.is_none() {
        ui.colored_label(DIM, format!("GPU time: {}", gpu_unavailable_reason(m)));
    }
}

/// Why there are no GPU timings (an honest reason instead of a zero).
pub fn gpu_unavailable_reason(m: &Metrics) -> String {
    if !m.gpu_collection {
        "off: recording it costs frame time, so it is only on when the Frame breakdown or Render statistics overlay is on at start-up (turn one on, then restart)".to_owned()
    } else if m.gpu.adapter.is_none() {
        "waiting for the renderer".to_owned()
    } else if !m.gpu.timestamps {
        format!("this GPU/driver ({}) has no timestamp queries", m.gpu.backend.as_deref().unwrap_or("unknown backend"))
    } else {
        "waiting for the first measurements".to_owned()
    }
}

// ---- the statistics panel ------------------------------------------------------------------------------------------

fn row(ui: &mut Ui, label: &str, value: RichText) {
    ui.colored_label(DIM, label);
    ui.label(value);
    ui.end_row();
}

fn estimate_row(ui: &mut Ui, label: &str, value: String, how: &str) {
    ui.colored_label(DIM, label);
    ui.label(mono(format!("~{value}")).color(ESTIMATE)).on_hover_text(format!("Estimated: {how}"));
    ui.end_row();
}

fn unavailable(ui: &mut Ui, why: &str) {
    ui.colored_label(DIM, format!("Unavailable: {why}"));
}

fn grid(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    egui::Grid::new(("debug_grid", id)).num_columns(2).spacing([10.0, 1.0]).show(ui, add);
}

fn instance_count(ui: &mut Ui, id: &str, m: &Metrics) {
    grid(ui, id, |ui| {
        match m.instances {
            Some((total, scripts)) => {
                row(ui, "Instances", mono(thousands(total as u64)).strong());
                row(ui, "Scripts", mono(thousands(scripts as u64)));
            }
            None => {
                ui.colored_label(DIM, "Instances");
                ui.colored_label(DIM, "n/a (no scene)");
                ui.end_row();
            }
        }
        row(ui, "Drawn meshes", mono(format!("{} of {}", thousands(m.scene.visible as u64), thousands(m.scene.mesh_entities as u64))));
    });
}

fn draw_calls(ui: &mut Ui, id: &str, m: &Metrics) {
    grid(ui, id, |ui| {
        estimate_row(ui, "Draw calls", thousands(m.scene.draw_calls_est as u64), "one call per distinct mesh+material among opaque meshes (they are drawn instanced) plus one per see-through mesh; shadow passes are not included");
        row(ui, "  batches", mono(thousands(m.scene.batches_est as u64)));
        row(ui, "  see-through", mono(thousands(m.scene.transparent as u64)));
    });
}

fn triangles(ui: &mut Ui, id: &str, m: &Metrics) {
    grid(ui, id, |ui| {
        row(ui, "Triangles", mono(thousands(m.scene.triangles)).strong());
        if let Some(p) = m.gpu.primitives {
            row(ui, "GPU primitives", mono(thousands(p)));
        }
    });
    ui.label(RichText::new("Counted from the meshes that are visible; shadow passes are not included.").small().color(DIM));
}

fn vertices(ui: &mut Ui, id: &str, m: &Metrics) {
    grid(ui, id, |ui| {
        row(ui, "Vertices", mono(thousands(m.scene.vertices)).strong());
        if let Some(v) = m.gpu.vertex_invocations {
            row(ui, "GPU vertex shader runs", mono(thousands(v)));
        }
    });
}

fn lights(ui: &mut Ui, id: &str, m: &Metrics) {
    grid(ui, id, |ui| {
        row(ui, "Active lights", mono(m.lights.total().to_string()).strong());
        row(ui, "  point / spot / directional", mono(format!("{} / {} / {}", m.lights.point, m.lights.spot, m.lights.directional)));
        row(ui, "  casting shadows", mono(m.lights.shadows.to_string()));
    });
}

fn render_stats(ui: &mut Ui, id: &str, m: &Metrics) {
    grid(ui, id, |ui| {
        if let Some(name) = &m.gpu.adapter {
            row(ui, "GPU", RichText::new(name).small());
        }
        if let Some(backend) = &m.gpu.backend {
            row(ui, "Backend", mono(backend.clone()));
        }
        estimate_row(ui, "Batches", m.scene.batches_est.to_string(), "distinct mesh+material pairs among the visible opaque meshes");
        row(ui, "Materials in view", mono(m.scene.materials.to_string()));
        row(ui, "Meshes in view", mono(m.scene.meshes.to_string()));
        row(ui, "See-through meshes", mono(m.scene.transparent.to_string()));
        match m.gpu.fragment_invocations {
            Some(v) => row(ui, "Pixel shader runs", mono(thousands(v))),
            None => {
                ui.colored_label(DIM, "Pixel shader runs");
                ui.colored_label(DIM, if m.gpu.pipeline_statistics { "n/a (waiting)" } else { "n/a (needs pipeline statistics)" });
                ui.end_row();
            }
        }
    });
    match m.gpu.gpu_ms {
        Some(total) => {
            ui.label(RichText::new(format!("GPU time per pass (total {})", ms(total))).small().color(DIM));
            grid(ui, "passes", |ui| {
                for pass in m.gpu.passes.iter().take(6) {
                    row(ui, &pass.name, mono(pass.gpu_ms.map_or("n/a".to_owned(), ms)));
                }
            });
        }
        None => unavailable(ui, &format!("GPU pass timings: {}", gpu_unavailable_reason(m))),
    }
}

fn cost_color(avg_ms: f32) -> Color32 {
    if avg_ms >= 4.0 {
        BAD
    } else if avg_ms >= 1.0 {
        WARN
    } else {
        TEXT
    }
}

fn scripts(ui: &mut Ui, id: &str, m: &Metrics) {
    let Some(p) = &m.scripts else {
        unavailable(ui, "no game is running (press Play)");
        return;
    };
    grid(ui, id, |ui| {
        row(ui, "Scripts this frame", mono(ms(p.scripts_ms())).strong());
        for (i, name) in super::super::scripting::PHASES.iter().enumerate() {
            if i == 4 || p.phase_ms[i] <= 0.0005 {
                continue; // (physics has its own panel section)
            }
            row(ui, &format!("  {name}"), mono(ms(p.phase_ms[i])));
        }
    });
    if !p.per_script {
        unavailable(ui, "per-script costs are not being measured");
        return;
    }
    let ranked = p.by_cost();
    if ranked.is_empty() {
        ui.colored_label(DIM, "No script has run yet.");
        return;
    }
    ui.label(RichText::new("Most expensive (average per frame, inclusive)").small().color(DIM));
    egui::Grid::new(("debug_scripts", id)).num_columns(4).spacing([8.0, 1.0]).show(ui, |ui| {
        ui.colored_label(DIM, "script");
        ui.colored_label(DIM, "avg");
        ui.colored_label(DIM, "peak");
        ui.colored_label(DIM, "runs");
        ui.end_row();
        for cost in ranked.iter().take(8) {
            ui.label(RichText::new(&cost.name).small()).on_hover_text(if cost.path.is_empty() { cost.name.clone() } else { cost.path.clone() });
            ui.label(mono(ms(cost.avg_ms)).color(cost_color(cost.avg_ms)));
            ui.label(mono(ms(cost.peak_ms)).color(DIM));
            ui.label(mono(cost.frame_calls.to_string()).color(DIM));
            ui.end_row();
        }
    });
    if ranked.len() > 8 {
        ui.label(RichText::new(format!("... and {} more", ranked.len() - 8)).small().color(DIM));
    }
}

fn engine_timings(ui: &mut Ui, id: &str, m: &Metrics) {
    let rows = m.timings.rows();
    if rows.is_empty() {
        ui.colored_label(DIM, "Collecting timings...");
        return;
    }
    egui::Grid::new(("debug_timings", id)).num_columns(4).spacing([8.0, 1.0]).show(ui, |ui| {
        ui.colored_label(DIM, "system");
        ui.colored_label(DIM, "avg");
        ui.colored_label(DIM, "last");
        ui.colored_label(DIM, "peak");
        ui.end_row();
        let mut ordered: Vec<_> = rows.iter().collect();
        ordered.sort_by(|a, b| (b.name == timing::MAIN).cmp(&(a.name == timing::MAIN)).then(b.avg_ms.total_cmp(&a.avg_ms)));
        for r in ordered {
            ui.label(RichText::new(r.name).small());
            ui.label(mono(ms(r.avg_ms)).color(cost_color(r.avg_ms)));
            ui.label(mono(ms(r.last_ms)).color(DIM));
            ui.label(mono(ms(r.peak_ms)).color(DIM));
            ui.end_row();
        }
    });
}

fn physics(ui: &mut Ui, id: &str, m: &Metrics) {
    let Some(p) = &m.physics else {
        unavailable(ui, "no game is running (press Play)");
        return;
    };
    grid(ui, id, |ui| {
        row(ui, "Step time", mono(ms(p.step_ms)).strong().color(cost_color(p.step_ms)));
        row(ui, "Sub-steps", mono(p.substeps.to_string()));
        for (kind, simulated, awake) in &p.bodies {
            row(ui, &format!("{kind} bodies"), mono(format!("{simulated}  ({awake} awake)")));
        }
        row(ui, "Static bodies", mono(thousands(p.static_boxes as u64)));
        row(ui, "Constraints", mono(p.joints.to_string()));
        row(ui, "Moved this step", mono(p.movers.to_string()));
        row(ui, "Touching pairs", mono(p.touch_pairs.to_string()));
        row(ui, "Touch events", mono(p.touch_events.to_string()));
    });
}

fn memory(ui: &mut Ui, id: &str, m: &Metrics) {
    grid(ui, id, |ui| {
        match m.memory.process {
            Some(b) => row(ui, "Process", mono(bytes(b)).strong()),
            None => {
                ui.colored_label(DIM, "Process");
                ui.colored_label(DIM, "n/a (waiting)");
                ui.end_row();
            }
        }
        if let (Some(used), Some(total)) = (m.memory.system_used, m.memory.system_total) {
            row(ui, "System", mono(format!("{} / {}", bytes(used), bytes(total))));
        }
        row(ui, "Meshes", mono(format!("{}  ({})", bytes(m.memory.mesh_bytes), m.memory.meshes)));
        row(ui, "Images", mono(format!("{}  ({})", bytes(m.memory.image_bytes), m.memory.images)));
        ui.colored_label(DIM, "GPU memory");
        ui.colored_label(DIM, "n/a (not exposed by wgpu)");
        ui.end_row();
    });
    ui.label(RichText::new("Meshes and images are the data held for them; what the GPU keeps is about the same.").small().color(DIM));
}

fn network(ui: &mut Ui) {
    unavailable(ui, "Forge has no multiplayer networking yet, so there is no ping, traffic or packet loss to report");
}

fn assets(ui: &mut Ui, id: &str, m: &Metrics) {
    let a = &m.assets;
    grid(ui, id, |ui| {
        row(ui, "Files loaded", mono(a.loads.to_string()).strong());
        row(ui, "Failed", mono(a.failures.to_string()).color(if a.failures > 0 { BAD } else { TEXT }));
        row(ui, "Total load time", mono(ms(a.total_ms as f32)));
        if a.loads > 0 {
            row(ui, "Average", mono(ms((a.total_ms / a.loads as f64) as f32)));
            row(ui, "Slowest", RichText::new(format!("{} ({})", a.slowest, ms(a.slowest_ms))).small());
        }
        row(ui, "Cached meshes / materials", mono(format!("{} / {}", m.cached.0, m.cached.1)));
    });
    if a.recent.is_empty() {
        ui.colored_label(DIM, "Nothing has been loaded from a file.");
    } else {
        ui.label(RichText::new("Latest").small().color(DIM));
        for load in a.recent.iter().rev().take(5) {
            ui.label(RichText::new(format!("{} {}  {}", if load.ok { "ok " } else { "ERR" }, ms(load.millis), load.path)).small().monospace().color(if load.ok { TEXT } else { BAD }));
        }
    }
    ui.label(RichText::new("Files load synchronously when a part first needs them, so there is no queue to show.").small().color(DIM));
}

fn warnings(ui: &mut Ui, _id: &str, m: &Metrics) {
    let d = &m.log;
    ui.horizontal(|ui| {
        ui.colored_label(if d.errors > 0 { BAD } else { DIM }, format!("{} errors", d.errors));
        ui.colored_label(if d.warnings > 0 { WARN } else { DIM }, format!("{} warnings", d.warnings));
        if (d.errors + d.warnings > 0) && ui.small_button("Clear").clicked() {
            log::clear();
        }
    });
    if d.recent.is_empty() {
        ui.colored_label(DIM, "Nothing reported.");
    }
    for e in d.recent.iter().rev().take(6) {
        let color = if e.severity == Severity::Error { BAD } else { WARN };
        let times = if e.repeats > 1 { format!(" (x{})", e.repeats) } else { String::new() };
        let text: String = e.text.chars().take(120).collect();
        ui.label(RichText::new(format!("[{}] {text}{times}", e.source)).small().color(color)).on_hover_text(&e.text);
    }
}

// ---- the Debug menu ------------------------------------------------------------------------------------------------------------

/// What the user did in the menu besides toggling overlays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuOutcome {
    /// The settings changed (an overlay was toggled, or something was configured).
    pub changed: bool,
    /// "Debug settings..." was chosen: open the settings window on its Debug tab.
    pub open_settings: bool,
    /// "Clear collected data" was chosen.
    pub clear_data: bool,
}

/// The Debug button with its dropdown. `shortcut` names the key bound to an overlay (shown beside it).
pub fn menu_button(ui: &mut Ui, settings: &mut DebugSettings, shortcut: &dyn Fn(Overlay) -> Option<String>) -> MenuOutcome {
    menu_button_without(ui, settings, shortcut, &|_| false)
}

/// [`menu_button`] without the overlays `hidden` names: the host has no use for them (the editor hides the network
/// statistics for a single-player project). A hidden overlay that is on stays on and can still be switched by its
/// shortcut; it just is not offered.
pub fn menu_button_without(ui: &mut Ui, settings: &mut DebugSettings, shortcut: &dyn Fn(Overlay) -> Option<String>, hidden: &dyn Fn(Overlay) -> bool) -> MenuOutcome {
    let mut out = MenuOutcome::default();
    let on = settings.enabled.len();
    let label = if on == 0 { RichText::new("Debug") } else { RichText::new(format!("Debug ({on} on)")).color(GOOD).strong() };
    let response = ui.menu_button(label, |ui| {
        ui.set_min_width(360.0);
        for group in Group::ALL {
            let count = OVERLAYS.iter().filter(|d| d.group == group && !hidden(d.overlay) && settings.on(d.overlay)).count();
            let title = if count == 0 { RichText::new(group.label()).strong() } else { RichText::new(format!("{}  ({count} on)", group.label())).strong().color(GOOD) };
            egui::CollapsingHeader::new(title).id_salt(("debug_group", group.label())).default_open(group == Group::Performance || count > 0).show(ui, |ui| {
                for def in OVERLAYS.iter().filter(|d| d.group == group && !hidden(d.overlay)) {
                    ui.horizontal(|ui| {
                        let mut enabled = settings.on(def.overlay);
                        if ui.checkbox(&mut enabled, def.name).on_hover_text(def.description).changed() {
                            settings.enabled.set(def.overlay, enabled);
                            out.changed = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match shortcut(def.overlay) {
                            Some(keys) => ui.label(RichText::new(keys).monospace().small().color(DIM)),
                            None => ui.label(RichText::new("unbound").small().italics().color(Color32::from_gray(110))),
                        });
                    });
                }
            });
        }
        ui.separator();
        if ui.add_enabled(!settings.enabled.is_empty(), egui::Button::new("Disable All Debug Overlays")).clicked() {
            settings.disable_all();
            out.changed = true;
        }
        if ui.add_enabled(!settings.is_default(), egui::Button::new("Reset Debug Settings")).on_hover_text("Switch every overlay off and put every debug setting back to its default").clicked() {
            settings.reset();
            out.changed = true;
        }
        if ui.button("Clear Collected Data").on_hover_text("Forget the frame history, peaks, script totals and recorded warnings").clicked() {
            out.clear_data = true;
        }
        ui.separator();
        egui::CollapsingHeader::new("Performance settings").id_salt("debug_menu_config").show(ui, |ui| {
            out.changed |= config(ui, settings);
        });
        if ui.button("All debug settings...").clicked() {
            out.open_settings = true;
            ui.close();
        }
    });
    response.response.on_hover_text("Performance and debugging overlays over the viewport");
    out
}

/// A floating Debug button in the top-left corner of `viewport` (for hosts without a toolbar of their own).
pub fn floating_menu(ctx: &egui::Context, viewport: Rect, settings: &mut DebugSettings, shortcut: &dyn Fn(Overlay) -> Option<String>) -> MenuOutcome {
    egui::Area::new(Id::new("forge_debug_menu")).order(Order::Foreground).fixed_pos(viewport.left_top() + egui::vec2(4.0, 4.0)).show(ctx, |ui| {
        egui::Frame::NONE.fill(Color32::from_black_alpha(150)).corner_radius(4).inner_margin(egui::Margin::symmetric(4, 1)).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| menu_button(ui, settings, shortcut)).inner
        })
        .inner
    })
    .inner
}

// ---- configuration controls ------------------------------------------------------------------------------------------------------------

/// The settings that shape the overlays (also the Debug tab of the editor's settings window).
/// Returns whether anything changed.
pub fn config(ui: &mut Ui, s: &mut DebugSettings) -> bool {
    let mut changed = false;
    egui::Grid::new("debug_config").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
        ui.label("Target frame rate");
        ui.horizontal(|ui| {
            for t in COMMON_TARGETS {
                if ui.selectable_label((s.target_fps - t).abs() < 0.5, format!("{t:.0}")).clicked() {
                    s.target_fps = t;
                    changed = true;
                }
            }
            changed |= ui.add(egui::DragValue::new(&mut s.target_fps).range(TARGET_FPS).speed(0.5).suffix(" FPS")).on_hover_text("The frame budget, the below-target warning and the highlighted reference line use this").changed();
        });
        ui.end_row();

        ui.label("FPS smoothing interval");
        changed |= ui.add(egui::Slider::new(&mut s.smoothing_s, SMOOTHING).logarithmic(true).suffix(" s")).on_hover_text("The big FPS number is averaged over this long: longer is easier to read, shorter reacts faster").changed();
        ui.end_row();

        ui.label("Graph history");
        changed |= ui.add(egui::Slider::new(&mut s.history_s, HISTORY).suffix(" s")).on_hover_text("How many seconds the graphs show").changed();
        ui.end_row();

        ui.label("Average / 1% low window");
        changed |= ui.add(egui::Slider::new(&mut s.average_window_s, AVERAGE_WINDOW).logarithmic(true).suffix(" s")).on_hover_text("The average FPS, the 1% low, the worst frame and the spike count cover this many seconds").changed();
        ui.end_row();

        ui.label("Update interval");
        changed |= ui.add(egui::Slider::new(&mut s.refresh_s, REFRESH).logarithmic(true).suffix(" s")).on_hover_text("How often the numbers refresh (the graphs refresh at least every 0.1 s). Longer costs less.").changed();
        ui.end_row();

        ui.label("Spike threshold");
        changed |= ui.add(egui::Slider::new(&mut s.spike_factor, SPIKE_FACTOR).prefix("x").suffix(" typical frame")).on_hover_text("A frame is a lag spike when it takes this many times the typical (median) frame").changed();
        ui.end_row();

        ui.label("Window size");
        changed |= ui.add(egui::Slider::new(&mut s.scale, SCALE).fixed_decimals(2).prefix("x")).on_hover_text("How big the overlay windows' text and graphs are").changed();
        ui.end_row();

        ui.label("Window opacity");
        changed |= ui.add(egui::Slider::new(&mut s.opacity, OPACITY)).on_hover_text("How solid the dark background behind the windows is").changed();
        ui.end_row();

        // Every overlay is a window of its own; these put all the windows of a group in one corner.
        // (Drag a window by its title to place just that one.)
        for group in Group::ALL {
            ui.label(format!("{} windows", group.label()));
            changed |= dock_picker(ui, ("debug_dock", group.label()), group, s);
            ui.end_row();
        }
    });
    if ui.button("Reset window layout").on_hover_text("Put every window back in its group's corner").clicked() {
        s.reset_layout();
        changed = true;
    }
    changed |= ui.checkbox(&mut s.show_instant_fps, "Show the instantaneous FPS next to the smoothed one").changed();
    changed |= ui.checkbox(&mut s.show_target_lines, "Draw 30 / 60 / 120 / 144 FPS reference lines in the graphs").changed();
    changed |= ui.checkbox(&mut s.highlight_spikes, "Highlight lag spikes").changed();
    changed |= ui.checkbox(&mut s.shortcuts_while_typing, "Debug shortcuts also work while typing in a text field").on_hover_text("Off by default: a shortcut should not fire in the middle of typing a name or a script").changed();
    if changed {
        s.sanitize();
    }
    changed
}

/// Where the windows of a group are docked (or "Mixed" when they were moved separately).
fn dock_picker(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug, group: Group, s: &mut DebugSettings) -> bool {
    let mut changed = false;
    let current = s.group_dock(group);
    egui::ComboBox::from_id_salt(id).selected_text(current.map_or("Mixed (moved separately)", Dock::label)).show_ui(ui, |ui| {
        for dock in Dock::ALL.into_iter().filter(|d| *d != Dock::Free) {
            if ui.selectable_label(current == Some(dock), dock.label()).clicked() && current != Some(dock) {
                s.dock_group(group, dock);
                changed = true;
            }
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        diag::{
            frames::FrameHistory,
            metrics::{GpuStats, LightStats, PassTime, SceneStats},
            overlay::{Placement, DEFAULT_SCALE},
        },
        physics::PhysicsStats,
        render::AssetStats,
        scripting::{ScriptCost, ScriptProfile},
    };

    fn viewport() -> Rect {
        Rect::from_min_size(Pos2::new(250.0, 60.0), egui::vec2(1000.0, 700.0))
    }

    /// A Metrics with something in every field, as a game that has been playing for a while.
    fn busy_metrics(settings: &DebugSettings) -> Metrics {
        let mut m = Metrics::default();
        m.apply(settings);
        m.timings.enabled = true;
        for i in 0..2000 {
            m.history.push(if i % 400 == 399 { 0.12 } else { 1.0 / 58.0 });
            m.timings.add_ms(timing::MAIN, 7.0);
            m.timings.add_ms(timing::SCRIPTS, 1.5);
            m.timings.add_ms(timing::PHYSICS, 0.5);
            m.timings.add_ms(timing::SCENE_SYNC, 0.8);
            m.timings.end_frame();
        }
        m.timings.roll_peaks();
        m.perf = m.history.snapshot(&m.cfg);
        m.history.graph(m.history_s, 240, &m.perf, &mut m.graph);
        m.scene = SceneStats { mesh_entities: 1200, visible: 800, triangles: 123_456, vertices: 98_765, draw_calls_est: 40, batches_est: 35, transparent: 5, materials: 12, meshes: 4 };
        m.lights = LightStats { point: 3, spot: 2, directional: 1, shadows: 1 };
        m.gpu = GpuStats {
            adapter: Some("Test GPU".into()),
            backend: Some("Vulkan".into()),
            timestamps: true,
            pipeline_statistics: true,
            gpu_ms: Some(4.2),
            render_cpu_ms: Some(1.1),
            passes: vec![PassTime { name: "main_opaque_pass_3d".into(), gpu_ms: Some(3.0), cpu_ms: Some(0.5) }],
            vertex_invocations: Some(500_000),
            primitives: Some(170_000),
            fragment_invocations: Some(9_000_000),
        };
        m.gpu_collection = true;
        m.instances = Some((2500, 4));
        let mut profile = ScriptProfile { frame_ms: 2.0, per_script: true, ..Default::default() };
        profile.phase_ms[5] = 1.5;
        profile.scripts = vec![ScriptCost { uid: 1, name: "Spin".into(), path: "scripts/spin.luau".into(), avg_ms: 1.4, peak_ms: 3.0, frame_calls: 2, ..Default::default() }];
        m.scripts = Some(profile);
        m.physics = Some(PhysicsStats { step_ms: 0.4, substeps: 1, static_boxes: 40, bodies: vec![("Dynamic", 10, 3)], touch_pairs: 2, touch_events: 1, movers: 3, joints: 2 });
        m.assets = AssetStats { loads: 3, failures: 1, total_ms: 12.0, slowest_ms: 8.0, slowest: "meshes/a.obj".into(), ..Default::default() };
        m
    }

    fn everything_on() -> DebugSettings {
        let mut s = DebugSettings::default();
        for d in OVERLAYS {
            s.enabled.set(d.overlay, true);
        }
        s
    }

    fn run(ctx: &egui::Context, mut f: impl FnMut(&egui::Context)) {
        // (egui may run the closure twice in one frame: the first pass of a new area only measures it.)
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 900.0))), ..Default::default() };
        let _ = crate::compat::run_frame(ctx, raw, |ctx| f(ctx));
    }
    fn rect_of(ctx: &egui::Context, o: Overlay) -> Option<Rect> {
        ctx.memory(|m| m.area_rect(window_id(o)))
    }

    /// Draws the overlays a few frames (a new window is measured first, then placed).
    fn settle(ctx: &egui::Context, vp: Rect, s: &mut DebugSettings, m: &Metrics) {
        for _ in 0..3 {
            run(ctx, |ctx| {
                overlays(ctx, vp, s, m);
            });
        }
    }

    #[test]
    fn every_overlay_draws_in_a_window_of_its_own_without_data_and_with_data() {
        let settings = everything_on();
        // Nothing collected yet.
        let empty = Metrics { history: FrameHistory::default(), ..Default::default() };
        let ctx = egui::Context::default();
        let mut s = settings.clone();
        settle(&ctx, viewport(), &mut s, &empty);
        // A busy game, drawn twice so the second pass runs with the memory of the first (layout, scroll areas).
        let busy = busy_metrics(&settings);
        settle(&ctx, viewport(), &mut s, &busy);
        for d in OVERLAYS {
            let rect = rect_of(&ctx, d.overlay).unwrap_or_else(|| panic!("{} has no window", d.name));
            assert!(rect.width() > 100.0 && rect.height() > 20.0, "{}: {rect:?}", d.name);
            assert!(viewport().expand(1.0).contains_rect(rect), "{}: {rect:?} is outside the viewport", d.name);
        }
        assert_eq!(s, settings, "drawing does not change the settings");
    }

    #[test]
    fn with_nothing_on_nothing_is_drawn() {
        let s = DebugSettings::default();
        let ctx = egui::Context::default();
        settle(&ctx, viewport(), &mut s.clone(), &busy_metrics(&s));
        assert!(OVERLAYS.iter().all(|d| rect_of(&ctx, d.overlay).is_none()));
    }

    #[test]
    fn only_the_overlays_that_are_on_get_a_window() {
        let mut s = DebugSettings::default();
        s.toggle(Overlay::Fps);
        let ctx = egui::Context::default();
        let m = busy_metrics(&s);
        settle(&ctx, viewport(), &mut s, &m);
        assert!(rect_of(&ctx, Overlay::Fps).is_some());
        assert!(OVERLAYS.iter().filter(|d| d.overlay != Overlay::Fps).all(|d| rect_of(&ctx, d.overlay).is_none()), "the FPS graph is a separate overlay with its own switch");
        // Switching a second one on adds a second window.
        s.toggle(Overlay::FpsGraph);
        settle(&ctx, viewport(), &mut s, &m);
        assert!(rect_of(&ctx, Overlay::Fps).is_some() && rect_of(&ctx, Overlay::FpsGraph).is_some());
    }

    #[test]
    fn a_window_sits_in_the_corner_it_is_docked_to() {
        let vp = viewport();
        let m = busy_metrics(&everything_on());
        for dock in [Dock::TopLeft, Dock::TopRight, Dock::BottomLeft, Dock::BottomRight] {
            let mut s = DebugSettings::default();
            s.toggle(Overlay::Fps);
            s.set_placement(Overlay::Fps, Placement { dock, offset: [8.0, 8.0] });
            let ctx = egui::Context::default();
            settle(&ctx, vp, &mut s, &m);
            let rect = rect_of(&ctx, Overlay::Fps).unwrap();
            let near = |a: f32, b: f32| (a - b).abs() < 2.0;
            match dock {
                Dock::TopLeft => assert!(near(rect.left(), vp.left() + EDGE) && near(rect.top(), vp.top() + EDGE), "{rect:?}"),
                Dock::TopRight => assert!(near(rect.right(), vp.right() - EDGE) && near(rect.top(), vp.top() + EDGE), "{rect:?}"),
                Dock::BottomLeft => assert!(near(rect.left(), vp.left() + EDGE) && near(rect.bottom(), vp.bottom() - EDGE), "{rect:?}"),
                Dock::BottomRight => assert!(near(rect.right(), vp.right() - EDGE) && near(rect.bottom(), vp.bottom() - EDGE), "{rect:?}"),
                Dock::Free => unreachable!(),
            }
        }
    }

    #[test]
    fn windows_in_one_corner_stack_without_overlapping_and_never_leave_the_viewport() {
        let vp = viewport();
        let m = busy_metrics(&everything_on());
        let mut s = everything_on();
        let ctx = egui::Context::default();
        settle(&ctx, vp, &mut s, &m);
        let rects: Vec<(Overlay, Rect)> = OVERLAYS.iter().map(|d| (d.overlay, rect_of(&ctx, d.overlay).unwrap())).collect();
        for (o, r) in &rects {
            assert!(vp.expand(1.0).contains_rect(*r), "{:?}: {r:?} outside {vp:?}", o);
        }
        for (i, (a, ra)) in rects.iter().enumerate() {
            for (b, rb) in &rects[i + 1..] {
                let overlap = ra.intersect(*rb);
                assert!(overlap.width() <= 1.0 || overlap.height() <= 1.0, "{a:?} {ra:?} overlaps {b:?} {rb:?}");
            }
        }
        // The seven performance windows are taller than the viewport together: they continue in a second column.
        let performance: Vec<Rect> = rects.iter().filter(|(o, _)| o.def().group == Group::Performance).map(|(_, r)| *r).collect();
        let columns: std::collections::BTreeSet<i32> = performance.iter().map(|r| r.left().round() as i32).collect();
        assert!(columns.len() >= 2, "a full column wraps into the next: {performance:?}");
    }

    #[test]
    fn windows_are_big_enough_to_read_and_grow_with_the_size_setting() {
        let vp = viewport();
        let width_at = |scale: f32, o: Overlay| {
            let mut s = DebugSettings::default();
            s.scale = scale;
            s.toggle(o);
            let ctx = egui::Context::default();
            let m = busy_metrics(&s);
            settle(&ctx, vp, &mut s, &m);
            rect_of(&ctx, o).unwrap()
        };
        let normal = width_at(DEFAULT_SCALE, Overlay::FpsGraph);
        assert!(normal.width() >= 450.0 && normal.height() >= 150.0, "the graph window starts large: {normal:?}");
        let (small, large) = (width_at(1.0, Overlay::Fps), width_at(2.0, Overlay::Fps));
        assert!(large.width() > small.width() * 1.7 && large.height() > small.height() * 1.5, "{small:?} -> {large:?}");
    }

    /// A window's size when it is the only one on, in a viewport of this size.
    fn size_in(viewport: Rect, o: Overlay, scale: f32, m: &Metrics) -> egui::Vec2 {
        let mut s = DebugSettings::default();
        s.scale = scale;
        s.toggle(o);
        let ctx = egui::Context::default();
        settle(&ctx, viewport, &mut s, m);
        rect_of(&ctx, o).unwrap().size()
    }

    #[test]
    fn no_window_is_cropped_or_made_to_scroll_however_small_the_viewport_is() {
        // A window that is clipped (it scrolls, or its text is cut off at the edge) is a different size from the same
        // window with all the room it wants: so every window must be the same size in a short, narrow viewport as in a huge one.
        let huge = Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(6000.0, 6000.0));
        let small = Rect::from_min_size(Pos2::new(250.0, 60.0), egui::vec2(1000.0, 330.0));
        for scale in [DEFAULT_SCALE, 2.0, 3.0] {
            let m = busy_metrics(&everything_on());
            for d in OVERLAYS {
                let (wanted, got) = (size_in(huge, d.overlay, scale, &m), size_in(small, d.overlay, scale, &m));
                assert!((wanted - got).abs().max_elem() < 1.5, "{} at {scale}x wants {wanted:?} but gets {got:?} in a viewport {:?} high: it is cropped", d.name, small.height());
            }
        }
    }
    #[test]
    fn a_free_window_sits_where_its_offset_says_and_stays_inside_the_viewport() {
        let vp = viewport();
        let m = busy_metrics(&everything_on());
        let mut s = DebugSettings::default();
        s.toggle(Overlay::Memory);
        s.set_placement(Overlay::Memory, Placement { dock: Dock::Free, offset: [120.0, 90.0] });
        let ctx = egui::Context::default();
        settle(&ctx, vp, &mut s, &m);
        let rect = rect_of(&ctx, Overlay::Memory).unwrap();
        assert!((rect.left() - (vp.left() + 120.0)).abs() < 2.0 && (rect.top() - (vp.top() + 90.0)).abs() < 2.0, "{rect:?}");

        // An offset from a bigger window than this one would put it outside: it is pulled back in.
        s.set_placement(Overlay::Memory, Placement { dock: Dock::Free, offset: [5000.0, 5000.0] });
        let ctx = egui::Context::default();
        settle(&ctx, vp, &mut s, &m);
        let rect = rect_of(&ctx, Overlay::Memory).unwrap();
        assert!(vp.expand(1.0).contains_rect(rect), "{rect:?} must be inside {vp:?}");
    }

    fn mouse(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() }
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }
    }

    #[test]
    fn dragging_a_windows_title_moves_only_that_window_and_dropping_near_a_corner_docks_it() {
        let vp = viewport();
        let m = busy_metrics(&everything_on());
        let mut s = DebugSettings::default();
        s.toggle(Overlay::Fps);
        s.toggle(Overlay::FrameTime);
        let ctx = egui::Context::default();
        settle(&ctx, vp, &mut s, &m);
        let before = rect_of(&ctx, Overlay::Fps).unwrap();
        let frame_time_before = rect_of(&ctx, Overlay::FrameTime).unwrap();

        let grab = before.left_top() + egui::vec2(14.0, 10.0);
        let mut changed = false;
        let mut step = |events: Vec<egui::Event>, s: &mut DebugSettings| {
            let _ = crate::compat::run_frame(&ctx, mouse(events), |ctx| changed |= overlays(ctx, vp, s, &m));
        };
        step(vec![egui::Event::PointerMoved(grab)], &mut s);
        step(vec![button(grab, true)], &mut s);
        // Drag to the middle of the viewport.
        let to = vp.center();
        step(vec![egui::Event::PointerMoved(grab + (to - grab) * 0.5)], &mut s);
        step(vec![egui::Event::PointerMoved(to)], &mut s);
        step(vec![button(to, false)], &mut s);
        step(vec![], &mut s);

        assert!(changed, "moving a window is a change to save");
        let p = s.placement(Overlay::Fps);
        assert_eq!(p.dock, Dock::Free, "dragged out of its corner: {p:?}");
        let after = rect_of(&ctx, Overlay::Fps).unwrap();
        assert!((after.center().x - before.center().x - (to.x - grab.x)).abs() < 6.0 && (after.center().y - before.center().y - (to.y - grab.y)).abs() < 6.0, "{before:?} -> {after:?}");
        assert_eq!(s.placement(Overlay::FrameTime).dock, Dock::TopLeft, "the other window did not move");
        let ft_after = rect_of(&ctx, Overlay::FrameTime).unwrap();
        assert!(ft_after.top() <= frame_time_before.top() + 1.0, "it moves up to fill the corner the FPS left: {frame_time_before:?} -> {ft_after:?}");
    }

    #[test]
    fn dropping_a_window_near_a_corner_docks_it_there() {
        let vp = viewport();
        let size = egui::vec2(290.0, 200.0);
        let at = |min: Pos2| Rect::from_min_size(min, size);
        assert_eq!(dock_for_drop(at(vp.left_top() + egui::vec2(20.0, 30.0)), vp), Some(Dock::TopLeft));
        assert_eq!(dock_for_drop(at(Pos2::new(vp.right() - size.x - 12.0, vp.top() + 10.0)), vp), Some(Dock::TopRight));
        assert_eq!(dock_for_drop(at(Pos2::new(vp.left() + 5.0, vp.bottom() - size.y - 5.0)), vp), Some(Dock::BottomLeft));
        assert_eq!(dock_for_drop(at(Pos2::new(vp.right() - size.x, vp.bottom() - size.y)), vp), Some(Dock::BottomRight));
        assert_eq!(dock_for_drop(at(vp.center() - size * 0.5), vp), None, "in the middle it stays where it was dropped");
    }

    #[test]
    fn graph_geometry_maps_time_and_value_into_the_plot() {
        let plot = Plot { area: Rect::from_min_max(Pos2::new(40.0, 10.0), Pos2::new(340.0, 90.0)), history_s: 30.0, top: 40.0 };
        assert_eq!(plot.x(0.0), 340.0, "now is the right edge");
        assert_eq!(plot.x(30.0), 40.0, "the oldest is the left edge");
        assert_eq!(plot.x(15.0), 190.0);
        assert_eq!(plot.x(500.0), 40.0, "older than the window is clamped");
        assert_eq!(plot.y(0.0), 90.0);
        assert_eq!(plot.y(40.0), 10.0);
        assert_eq!(plot.y(400.0), 10.0, "a value above the axis is clipped to the top");
    }

    #[test]
    fn the_frame_time_axis_makes_room_for_the_budget_and_is_not_flattened_by_one_hitch() {
        let s = DebugSettings::default(); // 60 FPS: 16.7 ms budget
        let calm = [GraphPoint { age: 1.0, mean_ms: 8.0, max_ms: 9.0, ..Default::default() }];
        let top = axis_top(GraphKind::FrameTime, &calm, &s);
        assert!(top >= 25.0, "the 16.7 ms line must fit, got {top}");
        let hitch = [GraphPoint { age: 1.0, mean_ms: 8.0, max_ms: 900.0, spike: true, ..Default::default() }];
        let top = axis_top(GraphKind::FrameTime, &hitch, &s);
        assert!(top <= 200.0, "a 900 ms hitch is clipped, not allowed to flatten the graph: {top}");
        let fast = [GraphPoint { age: 1.0, mean_ms: 2.0, max_ms: 3.0, ..Default::default() }];
        assert!(axis_top(GraphKind::Fps, &fast, &s) >= 500.0, "500 FPS has to fit");
        assert!(axis_top(GraphKind::Fps, &[], &s) >= 75.0, "with no data the target still fits");
    }

    #[test]
    fn the_breakdown_adds_up_to_the_frame() {
        let m = busy_metrics(&DebugSettings::default());
        let slices = main_thread_slices(&m, 17.0).expect("timed");
        let names: Vec<_> = slices.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&timing::SCRIPTS) && names.contains(&timing::PHYSICS) && names.contains(&timing::SCENE_SYNC), "{names:?}");
        let total: f32 = slices.iter().map(|s| s.ms).sum();
        assert!((total - 17.0).abs() < 0.05, "the slices (main thread, the unexplained rest of it, waiting) are the frame: {total}");
        let other = slices.iter().find(|s| s.name.starts_with("Other")).unwrap();
        assert!(other.estimated && (other.ms - (7.0 - 1.5 - 0.5 - 0.8)).abs() < 0.1, "{other:?}");
        let waiting = slices.iter().find(|s| s.name.starts_with("Waiting")).unwrap();
        assert!(waiting.estimated && (waiting.ms - 10.0).abs() < 0.1);
        // The main thread was never timed: nothing to break down.
        assert!(main_thread_slices(&Metrics::default(), 16.0).is_none());
    }

    #[test]
    fn a_missing_gpu_measurement_says_why() {
        let mut m = Metrics::default();
        assert!(gpu_unavailable_reason(&m).starts_with("off:") && gpu_unavailable_reason(&m).contains("restart"));
        m.gpu_collection = true;
        assert_eq!(gpu_unavailable_reason(&m), "waiting for the renderer");
        m.gpu.adapter = Some("X".into());
        m.gpu.backend = Some("Metal".into());
        assert!(gpu_unavailable_reason(&m).contains("no timestamp queries") && gpu_unavailable_reason(&m).contains("Metal"));
        m.gpu.timestamps = true;
        assert!(gpu_unavailable_reason(&m).contains("waiting"));
    }

    #[test]
    fn numbers_are_formatted_for_reading() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(ms(0.256), "0.26 ms");
        assert_eq!(ms(16.666), "16.7 ms");
        assert_eq!(ms(250.0), "250 ms");
        assert_eq!(rate_color(60.0, 60.0), GOOD);
        assert_eq!(rate_color(50.0, 60.0), WARN);
        assert_eq!(rate_color(20.0, 60.0), BAD);
    }

    /// Runs the menu button for a frame with the given events and returns the outcome.
    fn frame_with_menu(ctx: &egui::Context, settings: &mut DebugSettings, events: Vec<egui::Event>) -> MenuOutcome {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() };
        let mut out = MenuOutcome::default();
        let _ = crate::compat::run_frame(ctx, raw, |ctx| {
            out = floating_menu(ctx, viewport(), settings, &|o| (o == Overlay::Fps).then(|| "Ctrl+Shift+F5".to_owned()));
        });
        out
    }

    fn click(pos: Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
        ]
    }

    #[test]
    fn the_debug_menu_opens_and_toggles_overlays() {
        let ctx = egui::Context::default();
        let mut s = DebugSettings::default();
        // Settle the layout, then find the button.
        frame_with_menu(&ctx, &mut s, vec![]);
        frame_with_menu(&ctx, &mut s, vec![]);
        let button = ctx.memory(|m| m.area_rect(Id::new("forge_debug_menu"))).expect("the Debug button exists in the top-left of the viewport");
        assert!(viewport().contains(button.left_top() + egui::vec2(1.0, 1.0)) && button.left() < viewport().left() + 20.0, "{button:?}");
        let (outcome, _) = (frame_with_menu(&ctx, &mut s, click(button.center())), frame_with_menu(&ctx, &mut s, vec![]));
        assert!(!outcome.changed, "opening the menu changes nothing");
        // The menu is open now: its popup is an area of its own.
        frame_with_menu(&ctx, &mut s, vec![]);
        let popups = ctx.memory(|m| m.areas().visible_layer_ids().len());
        assert!(popups >= 2, "the dropdown is showing ({popups} layers)");
    }

    #[test]
    fn the_config_controls_keep_values_in_range_and_report_changes() {
        let ctx = egui::Context::default();
        let mut s = DebugSettings::default();
        let before = s.clone();
        let mut changed = true;
        run(&ctx, |ctx| {
            let mut root = crate::compat::root_ui(ctx);
            egui::CentralPanel::default().show(&mut root, |ui| changed = config(ui, &mut s));
        });
        assert!(!changed && s == before, "drawing the controls alone changes nothing");
    }
}

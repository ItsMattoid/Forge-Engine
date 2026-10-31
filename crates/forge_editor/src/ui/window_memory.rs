//! Windows of their own - the Editor Settings and the Project Settings - and how they remember where they were.
//!
//! These two are plain egui windows, not tabs of the dock: they cannot be dragged into a group of panels, and
//! the panels cannot be dragged into them. What they share with the dock's floating windows is the memory: each
//! one is put back where it was last left, at the size it was left, when it opens again - also after the editor
//! was closed (the places are part of the editor settings, see `settings::WindowPlace`).

use bevy_egui::egui::{self, Pos2, Rect, Vec2};

use crate::{settings::WindowPlace, state::Editor};

/// One such window.
pub(super) struct Spec {
    /// The name its place is saved under.
    pub key: &'static str,
    pub title: &'static str,
    /// The size of what is inside when it was never opened before.
    pub default_size: Vec2,
    pub min_size: Vec2,
}

/// What the title bar adds to the height of a window (measured with the default style). Since egui 0.36 a window's
/// `default_size` is the size of the whole window, frame and title bar included, while what is remembered here is the
/// size of what is inside: this is the difference.
const TITLE_BAR: f32 = 34.0;

/// What a window's frame adds around what is inside it (egui sizes windows by their contents).
fn frame_size(ctx: &egui::Context) -> Vec2 {
    egui::Frame::window(&ctx.global_style()).total_margin().sum()
}

/// A remembered place, made to fit the screen as it is now (a smaller screen or a monitor that is gone must not
/// leave a window out of reach): no bigger than the screen, and fully on it.
pub(super) fn fit(place: WindowPlace, screen: Rect, frame: Vec2, min_size: Vec2) -> (Pos2, Vec2) {
    let room = (screen.size() - frame).max(Vec2::splat(1.0));
    let size = Vec2::new(place.width, place.height).max(min_size).min(room);
    let outer = size + frame + Vec2::new(0.0, TITLE_BAR);
    let x = place.x.clamp(screen.min.x, (screen.max.x - outer.x).max(screen.min.x));
    let y = place.y.clamp(screen.min.y, (screen.max.y - outer.y).max(screen.min.y));
    (Pos2::new(x, y), size)
}

/// Draws a window of its own with `body` inside. `placed` is false on the first frame after it was opened: then it is
/// put where it was last time. Returns whether it is still open (its close button was not pressed).
pub(super) fn show(ctx: &egui::Context, ed: &mut Editor, spec: &Spec, placed: &mut bool, body: impl FnOnce(&mut egui::Ui, &mut Editor)) -> bool {
    let screen = ctx.content_rect();
    let frame = frame_size(ctx);
    let mut window = egui::Window::new(spec.title).id(egui::Id::new(("forge_window", spec.key))).collapsible(false).resizable(true).min_size(spec.min_size).default_size(spec.default_size + frame + Vec2::new(0.0, TITLE_BAR));
    match ed.settings.window_places.get(spec.key).copied() {
        Some(place) => {
            let (pos, size) = fit(place, screen, frame, spec.min_size);
            window = window.default_size(size + frame + Vec2::new(0.0, TITLE_BAR)).default_pos(pos);
            if !*placed {
                window = window.current_pos(pos);
            }
        }
        None => window = window.default_pos(screen.center() - (spec.default_size + frame) * 0.5),
    }
    let mut open = true;
    // (What is remembered is what is inside - not the title bar or the frame - which `default_size` gets them added back to.)
    let mut inside = Vec2::ZERO;
    let shown = window.open(&mut open).show(ctx, |ui| {
        inside = ui.max_rect().size();
        body(ui, ed)
    });
    if let Some(shown) = shown {
        *placed = true;
        remember(ctx, ed, spec.key, shown.response.rect.min, inside);
    }
    open
}

/// Notes where the window is once nothing is being dragged (a move or a resize changes it every frame).
fn remember(ctx: &egui::Context, ed: &mut Editor, key: &str, min: Pos2, size: Vec2) {
    if ctx.input(|i| i.pointer.any_down()) {
        return;
    }
    let place = WindowPlace { x: min.x, y: min.y, width: size.x, height: size.y };
    let unchanged = ed.settings.window_places.get(key).is_some_and(|p| (p.x - place.x).abs() < 0.5 && (p.y - place.y).abs() < 0.5 && (p.width - place.width).abs() < 0.5 && (p.height - place.height).abs() < 0.5);
    if !unchanged && place.width >= 1.0 && place.height >= 1.0 {
        ed.settings.window_places.insert(key.to_owned(), place);
        ed.settings.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 900.0))
    }

    #[test]
    fn a_remembered_place_is_kept_when_it_fits_and_pulled_back_when_it_does_not() {
        let frame = Vec2::new(14.0, 14.0);
        let min = Vec2::new(300.0, 200.0);
        let fine = WindowPlace { x: 200.0, y: 100.0, width: 640.0, height: 480.0 };
        assert_eq!(fit(fine, screen(), frame, min), (Pos2::new(200.0, 100.0), Vec2::new(640.0, 480.0)));

        // On a monitor that is gone: moved back onto the screen, not lost.
        let far = WindowPlace { x: 5000.0, y: -300.0, width: 640.0, height: 480.0 };
        let (pos, size) = fit(far, screen(), frame, min);
        assert!(screen().expand(0.01).contains_rect(Rect::from_min_size(pos, size + frame)), "{pos:?} {size:?}");

        // Bigger than the screen: shrunk to it.
        let huge = WindowPlace { x: 0.0, y: 0.0, width: 9000.0, height: 9000.0 };
        let (pos, size) = fit(huge, screen(), frame, min);
        assert!(pos == Pos2::ZERO && size.x + 14.0 <= 1600.0 && size.y + 14.0 <= 900.0);

        // Smaller than the least allowed: grown to it.
        let tiny = WindowPlace { x: 10.0, y: 10.0, width: 5.0, height: 5.0 };
        assert_eq!(fit(tiny, screen(), frame, min).1, min);
    }
}

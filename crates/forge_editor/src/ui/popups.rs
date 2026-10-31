//! Small popups opened by shortcuts: the "insert object" search and the snapping-value editors.

use bevy_egui::egui;
use crate::theme::SecondaryText;
use forge_engine::core::registry;

use crate::{
    state::{Editor, Increment},
    viewport::ViewportRect,
};

pub(super) fn popups(ctx: &egui::Context, ed: &mut Editor, vp: &ViewportRect) {
    quick_insert(ctx, ed, vp);
    increment(ctx, ed, vp);
}

/// Whether the mouse was pressed this frame somewhere outside `rect`: how these popups are dismissed
/// (click anywhere else), besides Escape.
fn pressed_outside(ctx: &egui::Context, rect: egui::Rect) -> bool {
    ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)))
}

/// Where popups open: the upper middle of the viewport.
fn anchor(vp: &ViewportRect) -> egui::Pos2 {
    let w = vp.max_pt.x - vp.min_pt.x;
    egui::pos2(vp.min_pt.x + (w * 0.5 - 170.0).max(8.0), vp.min_pt.y + 60.0)
}

/// A search over every class the Insert menu offers; Enter inserts the first match.
fn quick_insert(ctx: &egui::Context, ed: &mut Editor, vp: &ViewportRect) {
    let Some(mut text) = ed.quick_insert.take() else { return };
    let needle = text.trim().to_lowercase();
    let matches: Vec<&str> = registry()
        .creatable()
        .into_iter()
        .filter(|c| needle.is_empty() || c.name.to_lowercase().contains(&needle) || c.desc.category.to_lowercase().contains(&needle))
        .map(|c| c.name)
        .take(14)
        .collect();

    let mut chosen: Option<&str> = None;
    let mut keep = true;
    let shown = egui::Window::new("Insert Object").id(egui::Id::new("Insert Object")).order(egui::Order::Foreground).fixed_pos(anchor(vp)).fixed_size([340.0, 0.0]).collapsible(false).resizable(false).show(ctx, |ui| {
        let field = ui.add(egui::TextEdit::singleline(&mut text).hint_text("Type a class name...").desired_width(f32::INFINITY));
        field.request_focus();
        let (enter, escape) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if enter {
            chosen = matches.first().copied();
        }
        if escape || (enter && chosen.is_none()) {
            keep = false;
        }
        ui.separator();
        for name in &matches {
            let class = registry().class(name);
            let hint = class.map_or("", |c| c.desc.category);
            if ui.selectable_label(false, format!("{name}   ({hint})")).on_hover_text(class.map_or("", |c| c.desc.description)).clicked() {
                chosen = Some(name);
            }
        }
        if matches.is_empty() {
            ui.secondary("Nothing matches.");
        }
    });
    // A click anywhere outside the popup closes it.
    if shown.is_some_and(|s| pressed_outside(ctx, s.response.rect)) {
        keep = false;
    }

    if let Some(name) = chosen {
        ed.insert(name);
    } else if keep {
        ed.quick_insert = Some(text);
    }
}

/// Edits the move or rotate snapping value.
fn increment(ctx: &egui::Context, ed: &mut Editor, vp: &ViewportRect) {
    let Some(kind) = ed.increment_popup else { return };
    let (title, suffix, max, speed) = match kind {
        Increment::Move => ("Move increment", " studs", 1000.0, 0.05),
        Increment::Rotate => ("Rotate increment", " degrees", 180.0, 0.5),
    };
    let mut close = false;
    let mut changed = false;
    let shown = egui::Window::new(title).order(egui::Order::Foreground).fixed_pos(anchor(vp)).collapsible(false).resizable(false).show(ctx, |ui| {
        let value = match kind {
            Increment::Move => &mut ed.settings.move_increment,
            Increment::Rotate => &mut ed.settings.rotate_increment,
        };
        ui.horizontal(|ui| {
            changed = ui.add(egui::DragValue::new(value).range(0.0..=max).speed(speed).suffix(suffix)).changed();
            if ui.button("Done").clicked() {
                close = true;
            }
        });
        ui.secondary("0 turns snapping off. Hold Ctrl while dragging to ignore it.");
        if ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
            close = true;
        }
    });
    if shown.is_some_and(|s| pressed_outside(ctx, s.response.rect)) {
        close = true;
    }
    if changed {
        ed.settings.touch();
    }
    if close {
        ed.increment_popup = None;
    }
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use super::*;

    fn frame(ctx: &egui::Context, ed: &mut Editor, events: Vec<egui::Event>) {
        let raw = egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), ..Default::default() };
        let vp = ViewportRect { min_pt: bevy::math::Vec2::new(250.0, 60.0), max_pt: bevy::math::Vec2::new(1000.0, 700.0), ..Default::default() };
        let _ = ctx.run_frame(raw, |ctx| popups(ctx, ed, &vp));
    }

    fn click(at: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        vec![egui::Event::PointerMoved(at), button(true), button(false)]
    }

    #[test]
    fn the_insert_popup_closes_when_you_click_off_it_but_not_when_you_click_in_it() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        ed.quick_insert = Some(String::new());
        frame(&ctx, &mut ed, vec![]);
        frame(&ctx, &mut ed, vec![]);
        assert!(ed.quick_insert.is_some(), "stays open on its own");

        let popup = ctx.memory(|m| m.area_rect(egui::Id::new("Insert Object"))).expect("the popup is shown");
        // On the text box: still open.
        frame(&ctx, &mut ed, click(popup.left_top() + egui::vec2(30.0, 20.0)));
        assert!(ed.quick_insert.is_some(), "a click inside does not close it");
        // Somewhere else in the editor (the far corner): closed.
        frame(&ctx, &mut ed, click(egui::pos2(1150.0, 780.0)));
        assert!(ed.quick_insert.is_none(), "a click outside closes it");
    }

    #[test]
    fn the_snapping_popup_closes_the_same_way() {
        let ctx = egui::Context::default();
        let mut ed = Editor::new(None);
        ed.increment_popup = Some(Increment::Move);
        frame(&ctx, &mut ed, vec![]);
        frame(&ctx, &mut ed, vec![]);
        assert!(ed.increment_popup.is_some());
        frame(&ctx, &mut ed, click(egui::pos2(1150.0, 780.0)));
        assert!(ed.increment_popup.is_none());
    }
}
//! The control for a property that holds a project file (a model, a picture, a sound, a material): one box that shows the
//! file, takes a file dragged onto it from the Asset Browser, opens the picker when clicked, and clears. The Properties panel
//! and the Material Editor's texture slots use the same control, so a file is chosen the same way everywhere.

use std::hash::Hash;

use bevy_egui::egui::{self, vec2, Align2, Color32, FontId, Rect, Sense};
use forge_engine::project::{file_name, AssetKind};

use super::assets::{drop_tint, field_thumb, kind_style, set_drag_hint};
use crate::{state::{AssetDrag, Editor}, theme::SecondaryText};

/// What the control wants done (the caller applies it, since the value may belong to several objects or to a material).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldAction {
    None,
    /// A file was dropped on the box.
    Set(String),
    /// The cross was clicked.
    Clear,
    /// The box was clicked: open the picker.
    Pick,
    /// The Edit button was clicked (materials).
    Edit,
}

/// The first dragged file that suits a field of this kind.
pub fn compatible(paths: &[String], kind: AssetKind) -> Option<&String> {
    paths.iter().find(|p| AssetKind::of(file_name(p)) == kind && AssetKind::usable_by_engine(file_name(p)))
}

/// Why a dragged file does not suit a field, in words.
fn refusal(paths: &[String], kind: AssetKind) -> String {
    match paths.first() {
        Some(p) if AssetKind::of(file_name(p)) == kind => AssetKind::unusable_reason(file_name(p)),
        Some(p) => format!("{} is {} {}, this takes {} {}", file_name(p), article(AssetKind::of(file_name(p))), AssetKind::of(file_name(p)).label().to_lowercase(), article(kind), kind.label().to_lowercase()),
        None => "Nothing to drop".to_owned(),
    }
}

fn article(kind: AssetKind) -> &'static str {
    if matches!(kind, AssetKind::Audio | AssetKind::Animation | AssetKind::Other) {
        "an"
    } else {
        "a"
    }
}

/// Draws the control for `value` (a project path, empty for none). `edit` makes an Edit button (for materials).
pub fn asset_field(ui: &mut egui::Ui, ed: &mut Editor, salt: impl Hash + std::fmt::Debug, kind: AssetKind, value: &str, edit: bool) -> FieldAction {
    let mut action = FieldAction::None;
    ui.push_id(salt, |ui| {
        ui.horizontal(|ui| {
            let missing = !value.is_empty() && ed.asset_db().is_some_and(|db| db.entry(value).is_none());
            // A small picture of what is chosen.
            let (square, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            paint_thumb(ui, ed, square, kind, value);

            let label = if value.is_empty() { "(none)".to_owned() } else { file_name(value).to_owned() };
            let text = if missing { egui::RichText::new(format!("{label}  (missing)")).color(Color32::from_rgb(235, 100, 100)) } else { egui::RichText::new(label) };
            let resp = ui.add(egui::Button::new(text).min_size(vec2(150.0, 22.0)).sense(Sense::click()));
            if resp.clicked() {
                action = FieldAction::Pick;
            }
            // Files dragged from the Asset Browser: green when this field can take them, red when not.
            if let Some(drag) = resp.dnd_hover_payload::<AssetDrag>() {
                let ok = compatible(&drag.paths, kind).is_some();
                drop_tint(ui, resp.rect, ok);
                set_drag_hint(ui.ctx(), ok, if ok { format!("Use {}", file_name(&drag.paths[0])) } else { refusal(&drag.paths, kind) });
            }
            if let Some(drag) = resp.dnd_release_payload::<AssetDrag>() {
                if let Some(p) = compatible(&drag.paths, kind) {
                    action = FieldAction::Set(p.clone());
                }
            }
            let resp = resp.on_hover_ui(|ui| {
                if value.is_empty() {
                    ui.secondary(format!("Click to choose {} {}, or drop one here.", article(kind), kind.label().to_lowercase()));
                } else {
                    ui.strong(file_name(value));
                    ui.secondary(value);
                    if missing {
                        ui.colored_label(Color32::from_rgb(235, 100, 100), "This file is not in the project any more.");
                    }
                }
            });
            drop(resp);
            if kind == AssetKind::Audio && !value.is_empty() && !missing {
                let playing = ed.audio.previewing(value);
                if ui.small_button(if playing { "Stop" } else { "Play" }).on_hover_text("Listen to the sound").clicked() {
                    ed.preview_sound(value);
                }
            }
            if edit && !value.is_empty() && !missing && ui.small_button("Edit").on_hover_text("Open it in the Material Editor").clicked() {
                action = FieldAction::Edit;
            }
            if !value.is_empty() && ui.small_button("x").on_hover_text("Clear").clicked() {
                action = FieldAction::Clear;
            }
        });
    });
    action
}

/// The square at the left of a field: the picture itself for a texture, else the kind's mark.
fn paint_thumb(ui: &mut egui::Ui, ed: &mut Editor, rect: Rect, kind: AssetKind, value: &str) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    if kind == AssetKind::Texture && !value.is_empty() {
        if let Some(texture) = field_thumb(ui.ctx(), ed, value) {
            painter.image(texture.id(), rect.shrink(1.0), Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            return;
        }
    }
    let (glyph, color) = kind_style(kind);
    painter.rect_stroke(rect, 3.0, egui::Stroke::new(1.0_f32, color.gamma_multiply(if value.is_empty() { 0.4 } else { 1.0 })), egui::StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::monospace(9.0), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_takes_the_first_dragged_file_of_its_kind_that_the_engine_can_use() {
        let paths: Vec<String> = ["A/readme.txt", "A/brick.png", "A/model.fbx", "A/wall.png"].into_iter().map(String::from).collect();
        assert_eq!(compatible(&paths, AssetKind::Texture).map(String::as_str), Some("A/brick.png"));
        assert_eq!(compatible(&paths, AssetKind::Mesh), None, "an fbx is a model, but not one Forge reads");
        assert_eq!(compatible(&paths, AssetKind::Audio), None);
        assert!(refusal(&paths[2..3], AssetKind::Mesh).contains("glTF"));
        assert!(refusal(&paths[..1], AssetKind::Audio).contains("a data"));
        assert!(refusal(&["A/b.wav".to_owned()], AssetKind::Texture).contains("an audio"));
    }
}
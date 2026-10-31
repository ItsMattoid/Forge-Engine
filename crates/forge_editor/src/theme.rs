//! The editor's colours: eight colours the user can change (Editor Settings > Colors), and the egui
//! look built from them.
//!
//! * **Accent** / **Accent Hover**: selected rows, links, the text cursor, outlines of the control under
//!   the mouse or being used.
//! * **Background**: behind text fields and scroll areas (the darkest layer).
//! * **Panel**: panels, windows and groups.
//! * **Raised**: buttons, menus and other things that stand out from a panel.
//! * **Outline**: borders and separators.
//! * **Text** / **Secondary text**: normal and muted text.
//!
//! The palette is part of the saved editor settings (`#RRGGBB` strings), so it is shared by every project.

use bevy_egui::egui::{
    self,
    style::{Selection, WidgetVisuals},
    Color32, Stroke,
};
use serde::{ser::SerializeMap, Serialize, Serializer};

/// A colour as red, green, blue.
pub type Rgb = [u8; 3];

/// One of the editable colours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Accent,
    AccentHover,
    Background,
    Panel,
    Raised,
    Outline,
    Text,
    TextWeak,
}

impl Slot {
    /// In the order the settings window lists them.
    pub const ALL: [Slot; 8] = [Slot::Accent, Slot::AccentHover, Slot::Background, Slot::Panel, Slot::Raised, Slot::Outline, Slot::Text, Slot::TextWeak];

    /// The name in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Slot::Accent => "accent",
            Slot::AccentHover => "accent_hover",
            Slot::Background => "background",
            Slot::Panel => "panel",
            Slot::Raised => "raised",
            Slot::Outline => "outline",
            Slot::Text => "text",
            Slot::TextWeak => "text_secondary",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Slot::Accent => "Accent",
            Slot::AccentHover => "Accent Hover",
            Slot::Background => "Background",
            Slot::Panel => "Panel / Group",
            Slot::Raised => "Raised Surface",
            Slot::Outline => "Outline",
            Slot::Text => "Primary Text",
            Slot::TextWeak => "Secondary Text",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Slot::Accent => "Selected rows, links, the text cursor and the outline of what is being used.",
            Slot::AccentHover => "The outline of the control under the mouse.",
            Slot::Background => "Behind text fields and lists: the darkest layer.",
            Slot::Panel => "Panels, windows and groups.",
            Slot::Raised => "Buttons, drop-downs and other surfaces that stand out from a panel.",
            Slot::Outline => "Borders and separator lines.",
            Slot::Text => "Normal text.",
            Slot::TextWeak => "Descriptions, hints and other muted text.",
        }
    }

    pub fn default_rgb(self) -> Rgb {
        match self {
            Slot::Accent => [0xF5, 0x9E, 0x0B],
            Slot::AccentHover => [0xF9, 0x73, 0x16],
            Slot::Background => [0x18, 0x18, 0x18],
            Slot::Panel => [0x1F, 0x1F, 0x1F],
            Slot::Raised => [0x2A, 0x2A, 0x2A],
            Slot::Outline => [0x3F, 0x3F, 0x46],
            Slot::Text => [0xF8, 0xFA, 0xFC],
            Slot::TextWeak => [0xA0, 0xA0, 0xA0],
        }
    }

    fn index(self) -> usize {
        Slot::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// `#RRGGBB` (the `#` is optional, case does not matter).
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

pub fn to_hex(rgb: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn color32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// `a` moved `t` (0..1) of the way to `b`.
fn mix(a: Rgb, b: Rgb, t: f32) -> Color32 {
    let channel = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(channel(0), channel(1), channel(2))
}

fn luminance(rgb: Rgb) -> f32 {
    (0.2126 * rgb[0] as f32 + 0.7152 * rgb[1] as f32 + 0.0722 * rgb[2] as f32) / 255.0
}

/// All eight colours.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Palette {
    colors: [Rgb; 8],
}

impl Default for Palette {
    fn default() -> Self {
        Palette { colors: Slot::ALL.map(Slot::default_rgb) }
    }
}

impl Palette {
    pub fn get(&self, slot: Slot) -> Rgb {
        self.colors[slot.index()]
    }

    pub fn set(&mut self, slot: Slot, rgb: Rgb) {
        self.colors[slot.index()] = rgb;
    }

    pub fn is_default(&self) -> bool {
        *self == Palette::default()
    }

    /// Reads the colours of a settings file: a colour that is missing or is not `#RRGGBB` stays at its
    /// default, and other keys are ignored.
    pub fn from_file(map: &serde_json::Map<String, serde_json::Value>) -> Palette {
        let mut palette = Palette::default();
        for slot in Slot::ALL {
            if let Some(rgb) = map.get(slot.key()).and_then(|v| v.as_str()).and_then(parse_hex) {
                palette.set(slot, rgb);
            }
        }
        palette
    }

    /// The egui look for these colours.
    pub fn visuals(&self) -> egui::Visuals {
        let rgb = |s: Slot| self.get(s);
        let (accent, accent_hover) = (color32(rgb(Slot::Accent)), color32(rgb(Slot::AccentHover)));
        let (background, panel, raised, outline) = (color32(rgb(Slot::Background)), color32(rgb(Slot::Panel)), color32(rgb(Slot::Raised)), color32(rgb(Slot::Outline)));
        let text = color32(rgb(Slot::Text));
        let lifted = mix(rgb(Slot::Raised), rgb(Slot::Text), 0.1);
        // What is selected or pressed: the accent, softened so that text on it can be read.
        let soft_accent = mix(rgb(Slot::Panel), rgb(Slot::Accent), 0.3);
        let pressed = mix(rgb(Slot::Raised), rgb(Slot::Accent), 0.45);

        let mut v = if luminance(rgb(Slot::Background)) < 0.5 { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.override_text_color = None;
        v.hyperlink_color = accent;
        v.panel_fill = panel;
        v.window_fill = panel;
        v.window_stroke = Stroke::new(1.0_f32, outline);
        v.extreme_bg_color = background;
        v.faint_bg_color = mix(rgb(Slot::Panel), rgb(Slot::Raised), 0.5);
        v.code_bg_color = raised;
        v.slider_trailing_fill = true;
        v.text_cursor.stroke = Stroke::new(2.0_f32, accent);
        v.selection = Selection { bg_fill: soft_accent, stroke: Stroke::new(1.0_f32, accent) };

        let widget = |fill: Color32, edge: Stroke, ink: Color32, base: &WidgetVisuals| WidgetVisuals { bg_fill: fill, weak_bg_fill: fill, bg_stroke: edge, fg_stroke: Stroke::new(base.fg_stroke.width, ink), ..*base };
        let w = &v.widgets;
        let (noninteractive, inactive, hovered, active, open) = (
            widget(panel, Stroke::new(1.0_f32, outline), text, &w.noninteractive),
            widget(raised, Stroke::new(1.0_f32, outline), text, &w.inactive),
            widget(lifted, Stroke::new(1.0_f32, accent_hover), text, &w.hovered),
            widget(pressed, Stroke::new(1.0_f32, accent), text, &w.active),
            widget(raised, Stroke::new(1.0_f32, accent), text, &w.open),
        );
        v.widgets.noninteractive = noninteractive;
        v.widgets.inactive = inactive;
        v.widgets.hovered = hovered;
        v.widgets.active = active;
        v.widgets.open = open;
        v
    }

    /// The colour of muted text. (egui can only derive it from the normal text colour, so labels that
    /// should be muted use this instead: see [`SecondaryText`].)
    pub fn secondary(&self) -> Color32 {
        color32(self.get(Slot::TextWeak))
    }
}

/// Written as `{ "accent": "#F59E0B", ... }`.
impl Serialize for Palette {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(Slot::ALL.len()))?;
        for slot in Slot::ALL {
            map.serialize_entry(slot.key(), &to_hex(self.get(slot)))?;
        }
        map.end()
    }
}

const APPLIED: &str = "forge_palette_applied";

/// Makes `palette` the look of the whole editor. Cheap to call every frame: nothing happens unless the
/// palette changed since the last call.
pub fn apply(ctx: &egui::Context, palette: &Palette) {
    let id = egui::Id::new(APPLIED);
    if ctx.data(|d| d.get_temp::<Palette>(id)).as_ref() == Some(palette) {
        return;
    }
    // Both looks: which one egui picks follows the operating system's setting.
    let visuals = palette.visuals();
    ctx.set_visuals_of(egui::Theme::Dark, visuals.clone());
    ctx.set_visuals_of(egui::Theme::Light, visuals);
    ctx.data_mut(|d| d.insert_temp(id, palette.clone()));
}

/// The muted text colour of the palette in use.
pub fn secondary_color(ui: &egui::Ui) -> Color32 {
    ui.ctx().data(|d| d.get_temp::<Palette>(egui::Id::new(APPLIED))).map_or_else(|| ui.visuals().weak_text_color(), |p| p.secondary())
}

/// Muted text in the palette's Secondary Text colour (what `ui.weak` shows, but exact).
pub trait SecondaryText {
    fn secondary(&mut self, text: impl Into<egui::RichText>) -> egui::Response;
}

impl SecondaryText for egui::Ui {
    fn secondary(&mut self, text: impl Into<egui::RichText>) -> egui::Response {
        let color = secondary_color(self);
        self.label(text.into().color(color))
    }
}

#[cfg(test)]
mod tests {
    use forge_engine::compat::RunFrame as _;
    use super::*;

    #[test]
    fn the_starting_colours_are_the_ones_chosen() {
        let hex: Vec<String> = Slot::ALL.iter().map(|s| to_hex(s.default_rgb())).collect();
        assert_eq!(hex, ["#F59E0B", "#F97316", "#181818", "#1F1F1F", "#2A2A2A", "#3F3F46", "#F8FAFC", "#A0A0A0"]);
        assert!(Palette::default().is_default());
    }

    #[test]
    fn hex_codes_are_read_leniently_and_written_in_capitals() {
        assert_eq!(parse_hex("#f59e0b"), Some([0xF5, 0x9E, 0x0B]));
        assert_eq!(parse_hex("  F59E0B "), Some([0xF5, 0x9E, 0x0B]));
        for bad in ["", "#", "#F59E0", "#F59E0BB", "#GG0000", "#ÄÄÄ"] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        assert_eq!(to_hex([1, 2, 255]), "#0102FF");
    }

    #[test]
    fn the_palette_becomes_the_look_of_the_editor() {
        let v = Palette::default().visuals();
        assert_eq!(v.panel_fill, Color32::from_rgb(0x1F, 0x1F, 0x1F));
        assert_eq!(v.window_fill, v.panel_fill);
        assert_eq!(v.extreme_bg_color, Color32::from_rgb(0x18, 0x18, 0x18));
        assert_eq!(v.widgets.inactive.bg_fill, Color32::from_rgb(0x2A, 0x2A, 0x2A));
        assert_eq!(v.widgets.noninteractive.bg_stroke.color, Color32::from_rgb(0x3F, 0x3F, 0x46));
        assert_eq!(v.text_color(), Color32::from_rgb(0xF8, 0xFA, 0xFC));
        assert_eq!(v.selection.stroke.color, Color32::from_rgb(0xF5, 0x9E, 0x0B));
        assert_eq!(v.widgets.hovered.bg_stroke.color, Color32::from_rgb(0xF9, 0x73, 0x16));
        assert_eq!(v.hyperlink_color, Color32::from_rgb(0xF5, 0x9E, 0x0B));
        assert!(v.dark_mode);
        // A light background gives a light look.
        let mut light = Palette::default();
        light.set(Slot::Background, [250, 250, 250]);
        assert!(!light.visuals().dark_mode);
    }

    #[test]
    fn applying_sets_it_for_both_egui_looks_and_only_when_it_changed() {
        let ctx = egui::Context::default();
        let mut palette = Palette::default();
        apply(&ctx, &palette);
        assert_eq!(ctx.global_style().visuals.panel_fill, color32(Slot::Panel.default_rgb()));
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            assert_eq!(ctx.style_of(theme).visuals.panel_fill, color32(Slot::Panel.default_rgb()));
        }
        // An unchanged palette leaves the context alone (a style changed by hand survives).
        let mut style = (*ctx.global_style()).clone();
        style.visuals.panel_fill = Color32::RED;
        ctx.set_global_style(style);
        apply(&ctx, &palette);
        assert_eq!(ctx.global_style().visuals.panel_fill, Color32::RED);
        palette.set(Slot::Panel, [10, 20, 30]);
        apply(&ctx, &palette);
        assert_eq!(ctx.global_style().visuals.panel_fill, Color32::from_rgb(10, 20, 30));
    }

    #[test]
    fn muted_text_is_exactly_the_secondary_colour() {
        let ctx = egui::Context::default();
        let mut palette = Palette::default();
        palette.set(Slot::TextWeak, [1, 2, 3]);
        apply(&ctx, &palette);
        let _ = ctx.run_frame(Default::default(), |ctx| {
            egui::CentralPanel::default().show(&mut forge_engine::compat::root_ui(ctx), |ui| {
                assert_eq!(secondary_color(ui), Color32::from_rgb(1, 2, 3));
                ui.secondary("hint");
            });
        });
    }

    #[test]
    fn a_settings_file_keeps_good_colours_and_defaults_the_rest() {
        let file: serde_json::Value = serde_json::from_str(r##"{ "accent": "#112233", "panel": "not a colour", "text": 5, "from_the_future": "#000000" }"##).unwrap();
        let palette = Palette::from_file(file.as_object().unwrap());
        assert_eq!(palette.get(Slot::Accent), [0x11, 0x22, 0x33]);
        assert_eq!(palette.get(Slot::Panel), Slot::Panel.default_rgb());
        assert_eq!(palette.get(Slot::Text), Slot::Text.default_rgb());

        let written = serde_json::to_value(&palette).unwrap();
        assert_eq!(written["accent"], "#112233");
        assert_eq!(Palette::from_file(written.as_object().unwrap()), palette, "what is written reads back the same");
    }
}

//! The picture of a material in the Material Editor: a sphere lit by one lamp in front of a chequered backdrop, shaded on
//! the CPU. It follows the numbers the viewport's shader uses - colour times the base-colour map, metallic against rough
//! dielectric, emission, opacity (with glass brighter at its edges) - so what is seen while editing is what the numbers will
//! look like, though it is an approximation: no real lights, shadows or reflections of the scene.

use bevy::math::Vec3;
use bevy_egui::egui::{Color32, ColorImage};
use forge_engine::assets::material::{MaterialDef, MaterialKind};

fn srgb_to_linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_f(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round() as u8
}

/// The base-colour map at `(u, v)`, wrapped; linear colour and alpha.
fn sample(tex: &image::RgbaImage, u: f32, v: f32) -> (Vec3, f32) {
    let (w, h) = (tex.width() as f32, tex.height() as f32);
    let (x, y) = ((u.rem_euclid(1.0) * w) as u32 % tex.width(), (v.rem_euclid(1.0) * h) as u32 % tex.height());
    let p = tex.get_pixel(x, y).0;
    (Vec3::new(srgb_to_linear(p[0]), srgb_to_linear(p[1]), srgb_to_linear(p[2])), f32::from(p[3]) / 255.0)
}

/// The material on a sphere, `edge` pixels on a side. `map` is the material's base-colour map, if it has one and it could be read.
pub fn render(def: &MaterialDef, map: Option<&image::RgbaImage>, edge: usize) -> ColorImage {
    let light = Vec3::new(-0.5, 0.75, 0.6).normalize();
    let view = Vec3::Z;
    let half = (light + view).normalize();
    let radius = 0.9f32;
    let linear = |c: [f32; 3]| Vec3::new(srgb_f(c[0]), srgb_f(c[1]), srgb_f(c[2]));
    let base = linear(def.base_color);
    let emission = linear(def.emission_color) * def.emission_strength;
    let mut pixels = Vec::with_capacity(edge * edge);
    for y in 0..edge {
        for x in 0..edge {
            // The chequered backdrop that shows how see-through the material is.
            let check = ((x * 8 / edge.max(1)) + (y * 8 / edge.max(1))) % 2 == 0;
            let backdrop = if check { 0.62 } else { 0.42 };
            let bg = Vec3::splat(backdrop);
            let px = ((x as f32 + 0.5) / edge as f32) * 2.0 - 1.0;
            let py = 1.0 - ((y as f32 + 0.5) / edge as f32) * 2.0;
            let r2 = (px * px + py * py) / (radius * radius);
            if r2 >= 1.0 {
                let c = bg;
                pixels.push(Color32::from_rgb(to_srgb(c.x), to_srgb(c.y), to_srgb(c.z)));
                continue;
            }
            let n = Vec3::new(px / radius, py / radius, (1.0 - r2).sqrt());
            let (u, v) = (n.x.atan2(n.z) / std::f32::consts::TAU + 0.5, 0.5 - n.y.asin() / std::f32::consts::PI);
            let (tint, tex_alpha) = match map {
                Some(t) => sample(t, u * def.uv.tiling[0] + def.uv.offset[0], v * def.uv.tiling[1] + def.uv.offset[1]),
                None => (Vec3::ONE, 1.0),
            };
            let albedo = base * tint;
            let (rough, metal) = (def.roughness.clamp(0.05, 1.0), def.metallic.clamp(0.0, 1.0));
            let ndl = n.dot(light).max(0.0);
            let sky = 0.5 + 0.5 * n.y;
            let diffuse = albedo * (1.0 - metal) * (ndl * 0.9 + 0.12 + 0.12 * sky);
            let shininess = 6.0 + (1.0 - rough) * (1.0 - rough) * 400.0;
            let spec_color = Vec3::splat(0.04).lerp(albedo, metal);
            let spec = spec_color * n.dot(half).max(0.0).powf(shininess) * (1.0 - rough * 0.5) * (shininess + 8.0) / 40.0;
            // A fake room around the sphere: a bright sky above, a dark floor below, seen in the reflection.
            let reflected = (2.0 * n.dot(view) * n - view).normalize();
            let room = Vec3::new(0.45, 0.6, 0.9).lerp(Vec3::new(0.12, 0.1, 0.09), (0.5 - 0.5 * reflected.y).clamp(0.0, 1.0));
            let facing = n.dot(view).clamp(0.0, 1.0);
            let fresnel = (1.0 - facing).powi(5);
            let reflection = room * (spec_color + (Vec3::ONE - spec_color) * fresnel) * (1.0 - rough * 0.85) * (0.15 + metal);
            let glow = if emission != Vec3::ZERO { emission * if def.emission_from_color { albedo.max(Vec3::splat(0.05)) } else { Vec3::ONE } } else { Vec3::ZERO };
            let mut color = diffuse + spec + reflection + glow;
            let mut alpha = (def.opacity * tex_alpha).clamp(0.0, 1.0);
            if def.kind == MaterialKind::Glass {
                alpha = (alpha + (1.0 - alpha) * fresnel * def.edge_opacity).clamp(0.0, 1.0);
            }
            if def.kind == MaterialKind::Unlit {
                color = albedo;
            }
            // Bright things (emission) roll off instead of clipping.
            color = color / (Vec3::ONE + (color - Vec3::ONE).max(Vec3::ZERO));
            let out = bg.lerp(color, alpha);
            pixels.push(Color32::from_rgb(to_srgb(out.x), to_srgb(out.y), to_srgb(out.z)));
        }
    }
    ColorImage { size: [edge, edge], pixels, source_size: bevy_egui::egui::vec2(edge as f32, edge as f32) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(img: &ColorImage) -> Color32 {
        img.pixels[(img.size[1] / 2) * img.size[0] + img.size[0] / 2]
    }

    #[test]
    fn the_sphere_has_the_materials_colour_and_the_backdrop_is_a_chequerboard() {
        let mut def = MaterialDef::default();
        def.base_color = [1.0, 0.1, 0.1];
        let img = render(&def, None, 64);
        let c = centre(&img);
        assert!(c.r() > 120 && c.g() < 90 && c.b() < 90, "{c:?}");
        let (a, b) = (img.pixels[0], img.pixels[8]);
        assert_ne!(a, b, "neighbouring squares differ");
        assert_eq!(a.r(), a.g());
    }

    #[test]
    fn metal_looks_different_from_plastic_and_roughness_dulls_the_highlight() {
        let plastic = render(&MaterialDef::builtin("Plastic"), None, 64);
        let metal = render(&MaterialDef::builtin("Metal"), None, 64);
        assert_ne!(plastic.pixels, metal.pixels);
        // (Only the sphere counts: the backdrop around it is the same for both.)
        let bright = |img: &ColorImage| {
            let (n, c) = (img.size[0], img.size[0] as f32 / 2.0);
            let inside = |i: &usize| ((i % n) as f32 - c).powi(2) + ((i / n) as f32 - c).powi(2) < (c * 0.8).powi(2);
            (0..img.pixels.len()).filter(inside).map(|i| u32::from(img.pixels[i].r())).max().unwrap()
        };
        // (A dark sphere, so the highlight is not lost in a surface that is already white.)
        let smooth = MaterialDef { roughness: 0.1, base_color: [0.2, 0.2, 0.2], ..MaterialDef::default() };
        let rough = MaterialDef { roughness: 1.0, base_color: [0.2, 0.2, 0.2], ..MaterialDef::default() };
        assert!(bright(&render(&smooth, None, 64)) > bright(&render(&rough, None, 64)), "a smooth sphere has a hotter highlight");
    }

    #[test]
    fn opacity_lets_the_backdrop_through_and_glass_shows_its_edge() {
        let solid = render(&MaterialDef::default(), None, 64);
        let see_through = render(&MaterialDef { opacity: 0.2, ..MaterialDef::default() }, None, 64);
        let (s, t) = (centre(&solid), centre(&see_through));
        assert_ne!(s, t);
        // Through a mostly-transparent sphere, the chequers show: two nearby pixels at the centre differ again.
        let glass = render(&MaterialDef::builtin("Glass"), None, 96);
        let edge_row = &glass.pixels[48 * 96..49 * 96];
        let mid = edge_row[48].r();
        let rim = edge_row[48 - 40].r();
        assert!(mid != rim);
    }

    #[test]
    fn neon_glows_and_a_texture_changes_the_look() {
        let plain = render(&MaterialDef { base_color: [0.2, 0.9, 0.3], ..MaterialDef::default() }, None, 64);
        let neon = render(&MaterialDef { base_color: [0.2, 0.9, 0.3], ..MaterialDef::builtin("Neon") }, None, 64);
        let lum = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
        assert!(lum(centre(&neon)) > lum(centre(&plain)));
        let tex = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        let textured = render(&MaterialDef::default(), Some(&tex), 64);
        let c = centre(&textured);
        assert!(c.r() > c.g() + 40, "the red picture tints the sphere: {c:?}");
    }
}
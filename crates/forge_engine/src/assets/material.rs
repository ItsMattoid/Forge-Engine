//! Material assets: `*.mat` files, JSON, one material each.
//!
//! A material is **data in the project**, not code in the engine. The five starter materials (Plastic, SmoothPlastic,
//! Metal, Neon, Glass) are ordinary `.mat` files that a new project is given; they can be edited, replaced or deleted, and
//! any number of others can be added, without touching the engine. A part names its material with the `Material` enum
//! (the item's name is looked up among the project's materials) or, for any material at all, with its `MaterialVariant`
//! (the path or the name of a `.mat` file). When a name has no file the engine falls back to [`MaterialDef::builtin`],
//! which is what a project without material files has always looked like.
//!
//! The format is versioned and tolerant: fields that are missing take their defaults, unknown fields survive a load and
//! save unchanged (a newer editor's material is not damaged by an older one), and a value out of range is brought back into
//! range by [`MaterialDef::sanitize`] instead of failing the load.
//!
//! How the numbers are used (`render::part_material` turns them into the shader's inputs):
//!
//! * the surface colour is `base_color` x the part's `Color` x the base-colour map (the part's colour *tints* the material);
//! * the opacity is `opacity` x (1 - the part's `Transparency`) x the map's alpha;
//! * `emission_color` x `emission_strength` is light the surface gives off; `emission_from_color` makes it the part's colour
//!   (Neon: the part glows in its own colour);
//! * `kind` picks the shading model: [`MaterialKind`].

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as J};

use super::source::GameSource;

/// The extension of a material file.
pub const MATERIAL_EXT: &str = ".mat";
/// The version of the format this build writes (and the newest it reads).
pub const MATERIAL_FORMAT: u32 = 1;
/// Where a new project keeps its materials, and where new materials are created by default.
pub const MATERIALS_DIR: &str = "Assets/Materials";

/// The shading model of a material. A new kind is a new variant here, a branch in `render::part_material`, and (if it
/// needs inputs of its own) fields in [`MaterialDef`]; nothing else has to know about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MaterialKind {
    /// Physically based shading: colour, roughness, metallic, maps.
    #[default]
    Standard,
    /// See-through: low opacity, bright reflections at grazing angles (`edge_opacity`), casts no shadow.
    Glass,
    /// Gives off light: the surface is its own light source (and glows, where the camera has bloom).
    Neon,
    /// Flat colour, not affected by lights.
    Unlit,
}

impl MaterialKind {
    pub const ALL: [MaterialKind; 4] = [MaterialKind::Standard, MaterialKind::Glass, MaterialKind::Neon, MaterialKind::Unlit];

    pub fn name(self) -> &'static str {
        match self {
            MaterialKind::Standard => "standard",
            MaterialKind::Glass => "glass",
            MaterialKind::Neon => "neon",
            MaterialKind::Unlit => "unlit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MaterialKind::Standard => "Standard",
            MaterialKind::Glass => "Glass",
            MaterialKind::Neon => "Neon",
            MaterialKind::Unlit => "Unlit",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            MaterialKind::Standard => "Lit surface: colour, roughness, metallic and texture maps.",
            MaterialKind::Glass => "See-through surface with strong reflections at the edges; casts no shadow.",
            MaterialKind::Neon => "A surface that gives off its own light and glows.",
            MaterialKind::Unlit => "A flat colour that lights do not change.",
        }
    }

    pub fn from_name(name: &str) -> Option<MaterialKind> {
        MaterialKind::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Whether a part drawn with this kind casts a shadow.
    pub fn casts_shadow(self) -> bool {
        !matches!(self, MaterialKind::Glass)
    }
}

impl Serialize for MaterialKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for MaterialKind {
    /// An unknown name (a kind from a newer version) is read as `Standard`, not an error: the material still loads.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Ok(MaterialKind::from_name(&text).unwrap_or_default())
    }
}

/// How transparency in the colour map and the opacity is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlphaKind {
    /// Alpha is ignored (unless the part or the material is see-through, which blends).
    #[default]
    Opaque,
    /// Alpha blending: see-through, sorted with other see-through surfaces.
    Blend,
    /// Alpha is a cut-out: below `alpha_cutoff` the surface is not drawn (leaves, fences).
    Mask,
}

/// Where a surface's texture coordinates come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UvMode {
    /// `Studs` on parts made from the engine's own shapes (so a texture keeps its size on a part of any size), `Mesh` on
    /// imported meshes (which come with texture coordinates).
    #[default]
    Auto,
    /// The mesh's own coordinates.
    Mesh,
    /// Projected along the part's axes: one tile is `studs_per_tile` studs, whatever the part's size.
    Studs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterKind {
    /// Smooth, with mip-maps (and anisotropic filtering): the normal choice.
    #[default]
    Linear,
    /// Hard pixels, no blending: pixel art.
    Nearest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AddressKind {
    #[default]
    Repeat,
    Clamp,
    Mirror,
}

/// The texture maps of a material (project-relative paths; empty means none).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialMaps {
    /// Colour (sRGB). Its alpha is the surface's opacity.
    pub base_color: String,
    /// Tangent-space normal map (linear; +Y up, see `flip_normal_y`).
    pub normal: String,
    /// Roughness (linear, the red channel).
    pub roughness: String,
    /// Metallic (linear, the red channel).
    pub metallic: String,
    /// Roughness and metallic in one picture (glTF's layout: green is roughness, blue is metallic). Used instead of the
    /// two separate maps when set.
    pub metallic_roughness: String,
    /// Ambient occlusion (linear, the red channel): darkens creases.
    pub occlusion: String,
    /// Emission colour (sRGB).
    pub emission: String,
}

impl MaterialMaps {
    /// `(slot name, path)` of every map that is set.
    pub fn set(&self) -> Vec<(&'static str, &str)> {
        [
            ("base_color", &self.base_color),
            ("normal", &self.normal),
            ("roughness", &self.roughness),
            ("metallic", &self.metallic),
            ("metallic_roughness", &self.metallic_roughness),
            ("occlusion", &self.occlusion),
            ("emission", &self.emission),
        ]
            .into_iter()
            .filter(|(_, p)| !p.is_empty())
            .map(|(n, p)| (n, p.as_str()))
            .collect()
    }

    /// Every path, mutably (to point them at moved files).
    pub fn paths_mut(&mut self) -> [&mut String; 7] {
        [&mut self.base_color, &mut self.normal, &mut self.roughness, &mut self.metallic, &mut self.metallic_roughness, &mut self.occlusion, &mut self.emission]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UvSettings {
    pub mode: UvMode,
    /// How many times the maps repeat over the surface (in `Mesh` mode; in `Studs` mode a multiplier on top).
    pub tiling: [f32; 2],
    pub offset: [f32; 2],
    /// Turns the maps, in degrees.
    pub rotation: f32,
    /// In `Studs` mode: how many studs one repeat of the maps covers.
    pub studs_per_tile: [f32; 2],
}

impl Default for UvSettings {
    fn default() -> Self {
        UvSettings { mode: UvMode::Auto, tiling: [1.0, 1.0], offset: [0.0, 0.0], rotation: 0.0, studs_per_tile: [4.0, 4.0] }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct SamplerSettings {
    pub filter: FilterKind,
    pub address: AddressKind,
    /// Anisotropic filtering samples (1 off, 2 / 4 / 8 / 16).
    pub anisotropy: u8,
}

impl Default for SamplerSettings {
    fn default() -> Self {
        SamplerSettings { filter: FilterKind::Linear, address: AddressKind::Repeat, anisotropy: 8 }
    }
}

/// One material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialDef {
    pub format: u32,
    /// The name shown in the editor. The file name is what identifies the material.
    pub name: String,
    pub kind: MaterialKind,
    /// Multiplies the part's colour (white leaves it alone). sRGB, like the part's Color.
    pub base_color: [f32; 3],
    /// How opaque the surface is, 0 to 1 (multiplies the part's own opacity).
    pub opacity: f32,
    pub roughness: f32,
    pub metallic: f32,
    /// How strongly non-metals reflect, 0 to 1 (0.5 is the usual 4%).
    pub reflectance: f32,
    /// The colour of the light given off (sRGB).
    pub emission_color: [f32; 3],
    /// Brightness of the emission (0: none).
    pub emission_strength: f32,
    /// The emission is the part's colour (times `emission_color`) instead of just `emission_color`.
    pub emission_from_color: bool,
    /// How deep a normal map's bumps look (1 is as authored, 0 flat).
    pub normal_strength: f32,
    /// The normal map's green channel points down (DirectX style) instead of up (OpenGL / glTF style).
    pub flip_normal_y: bool,
    /// How much the occlusion map darkens (1 fully, 0 not at all).
    pub occlusion_strength: f32,
    /// Glass: how much more opaque the surface looks at grazing angles (0 none, 1 mirror-like edges).
    pub edge_opacity: f32,
    /// Real refraction of what is behind the surface (0 off; costs a copy of the scene), for glass and water.
    pub transmission: f32,
    pub ior: f32,
    pub alpha_mode: AlphaKind,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
    pub maps: MaterialMaps,
    pub uv: UvSettings,
    pub sampler: SamplerSettings,
    /// Anything a newer version wrote that this one does not know: kept as it is.
    #[serde(flatten)]
    pub extra: Map<String, J>,
}

impl Default for MaterialDef {
    fn default() -> Self {
        MaterialDef {
            format: MATERIAL_FORMAT,
            name: String::new(),
            kind: MaterialKind::Standard,
            base_color: [1.0, 1.0, 1.0],
            opacity: 1.0,
            roughness: 0.8,
            metallic: 0.0,
            reflectance: 0.5,
            emission_color: [1.0, 1.0, 1.0],
            emission_strength: 0.0,
            emission_from_color: true,
            normal_strength: 1.0,
            flip_normal_y: false,
            occlusion_strength: 1.0,
            edge_opacity: 0.0,
            transmission: 0.0,
            ior: 1.5,
            alpha_mode: AlphaKind::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            maps: MaterialMaps::default(),
            uv: UvSettings::default(),
            sampler: SamplerSettings::default(),
            extra: Map::new(),
        }
    }
}

fn finite_or(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        fallback
    }
}

impl MaterialDef {
    /// A plain material of this kind with the look the kind has by default.
    pub fn of_kind(name: &str, kind: MaterialKind) -> MaterialDef {
        let base = MaterialDef { name: name.to_owned(), kind, ..MaterialDef::default() };
        match kind {
            MaterialKind::Standard => base,
            MaterialKind::Glass => MaterialDef { opacity: 0.3, roughness: 0.05, reflectance: 0.9, edge_opacity: 0.8, alpha_mode: AlphaKind::Blend, ior: 1.5, ..base },
            MaterialKind::Neon => MaterialDef { roughness: 0.6, emission_strength: 3.0, emission_from_color: true, ..base },
            MaterialKind::Unlit => base,
        }
    }

    /// What a project without a material file for `name` shows. `name` is an item of the `Material` enum; those the
    /// engine has no look of its own for take the nearest of its few (wood, concrete ... are rough dielectrics).
    pub fn builtin(name: &str) -> MaterialDef {
        let m = |roughness: f32, metallic: f32| MaterialDef { name: name.to_owned(), roughness, metallic, ..MaterialDef::default() };
        match name {
            "SmoothPlastic" | "Ice" | "Glacier" | "Marble" | "Foil" => m(0.35, 0.0),
            "Metal" | "CorrodedMetal" | "DiamondPlate" => m(0.3, 1.0),
            "Neon" | "CrackedLava" | "ForceField" => MaterialDef { name: name.to_owned(), ..MaterialDef::of_kind(name, MaterialKind::Neon) },
            "Glass" | "Water" => MaterialDef { name: name.to_owned(), ..MaterialDef::of_kind(name, MaterialKind::Glass) },
            "Wood" | "WoodPlanks" => m(0.85, 0.0),
            "Concrete" | "Slate" | "Basalt" | "Limestone" | "Granite" | "Pavement" | "Brick" | "Pebble" | "Cobblestone" | "Rock" | "Sandstone" | "Asphalt" | "Salt" => m(1.0, 0.0),
            _ => m(0.8, 0.0),
        }
    }

    /// The five materials a new project starts with.
    pub fn starters() -> Vec<MaterialDef> {
        ["Plastic", "SmoothPlastic", "Metal", "Neon", "Glass"].into_iter().map(MaterialDef::builtin).collect()
    }

    /// Brings every number into its range (and replaces NaN), so whatever a file contains is safe to render.
    /// Returns what was changed, for a warning.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let mut fix = |label: &str, v: &mut f32, lo: f32, hi: f32, fallback: f32| {
            let fixed = if v.is_finite() { v.clamp(lo, hi) } else { fallback };
            if fixed != *v {
                notes.push(format!("{label} {v} is outside {lo}..{hi}; using {fixed}"));
                *v = fixed;
            }
        };
        fix("opacity", &mut self.opacity, 0.0, 1.0, 1.0);
        fix("roughness", &mut self.roughness, 0.0, 1.0, 0.8);
        fix("metallic", &mut self.metallic, 0.0, 1.0, 0.0);
        fix("reflectance", &mut self.reflectance, 0.0, 1.0, 0.5);
        fix("emission_strength", &mut self.emission_strength, 0.0, 1000.0, 0.0);
        fix("normal_strength", &mut self.normal_strength, 0.0, 8.0, 1.0);
        fix("occlusion_strength", &mut self.occlusion_strength, 0.0, 1.0, 1.0);
        fix("edge_opacity", &mut self.edge_opacity, 0.0, 1.0, 0.0);
        fix("transmission", &mut self.transmission, 0.0, 1.0, 0.0);
        fix("ior", &mut self.ior, 1.0, 3.0, 1.5);
        fix("alpha_cutoff", &mut self.alpha_cutoff, 0.0, 1.0, 0.5);
        fix("uv.rotation", &mut self.uv.rotation, -3600.0, 3600.0, 0.0);
        for (label, c) in [("base_color", &mut self.base_color), ("emission_color", &mut self.emission_color)] {
            for v in c.iter_mut() {
                let fixed = if v.is_finite() { v.clamp(0.0, 16.0) } else { 1.0 };
                if fixed != *v {
                    notes.push(format!("{label} has a value outside 0..16"));
                    *v = fixed;
                }
            }
        }
        for (label, pair, lo) in [("uv.tiling", &mut self.uv.tiling, -1000.0), ("uv.studs_per_tile", &mut self.uv.studs_per_tile, 0.001)] {
            for v in pair.iter_mut() {
                let fixed = if v.is_finite() { v.clamp(lo, 1.0e5) } else { 1.0 };
                if fixed != *v {
                    notes.push(format!("{label} has an unusable value"));
                    *v = fixed;
                }
            }
        }
        for v in self.uv.offset.iter_mut() {
            *v = finite_or(*v, 0.0);
        }
        self.sampler.anisotropy = self.sampler.anisotropy.clamp(1, 16);
        notes
    }

    /// Parses a material file. A newer format is refused; anything else that is wrong with the numbers is repaired
    /// (see [`MaterialDef::sanitize`]) and reported in the second value.
    pub fn parse(bytes: &[u8]) -> Result<(MaterialDef, Vec<String>), String> {
        // (Many Windows editors put a byte order mark at the start of a UTF-8 file; it is not part of the JSON.)
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        let mut def: MaterialDef = serde_json::from_slice(bytes).map_err(|e| format!("not a valid material file: {e}"))?;
        if def.format > MATERIAL_FORMAT {
            return Err(format!("this material was saved by a newer version of Forge (format {}, this one reads up to {MATERIAL_FORMAT})", def.format));
        }
        let notes = def.sanitize();
        Ok((def, notes))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a material always serializes")
    }

    /// Every file the material uses.
    pub fn texture_paths(&self) -> Vec<&str> {
        self.maps.set().into_iter().map(|(_, p)| p).collect()
    }

    /// Points the maps at moved files (`old -> new`). Returns how many changed.
    pub fn remap_paths(&mut self, moved: &HashMap<String, String>) -> usize {
        let mut changed = 0;
        for p in self.maps.paths_mut() {
            if let Some(new) = moved.get(p.as_str()) {
                *p = new.clone();
                changed += 1;
            }
        }
        changed
    }

    /// Whether the surface can be seen through (the renderer then blends it and draws it after the solid things).
    pub fn is_see_through(&self) -> bool {
        self.opacity < 1.0 || self.alpha_mode == AlphaKind::Blend || self.kind == MaterialKind::Glass
    }

    /// Whether the emission contributes anything.
    pub fn emits(&self) -> bool {
        self.emission_strength > 0.0 || !self.maps.emission.is_empty()
    }
}

/// The name a material file is known by: its file name without the extension.
pub fn material_name_of(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(MATERIAL_EXT).or_else(|| file.strip_suffix(".material")).unwrap_or(file)
}

/// Whether a string names a material file (rather than a material's name).
pub fn looks_like_path(spec: &str) -> bool {
    spec.contains('/') || spec.contains('\\') || spec.to_ascii_lowercase().ends_with(MATERIAL_EXT)
}

/// A material found in the project.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryEntry {
    pub path: String,
    pub def: MaterialDef,
    /// Repairs made while reading it.
    pub notes: Vec<String>,
}

/// Every material of a project, by name and by path. Built by reading the `.mat` files of a [`GameSource`]; a file that
/// cannot be read is skipped and listed in `problems` (one broken material never takes the others down).
#[derive(Clone, Debug, Default)]
pub struct MaterialLibrary {
    by_path: BTreeMap<String, LibraryEntry>,
    /// Name (case-insensitive) -> path. When two files share a name, the one under `Assets/Materials` wins, then the
    /// first by path.
    by_name: HashMap<String, String>,
    pub problems: Vec<(String, String)>,
}

impl MaterialLibrary {
    pub fn load(source: &GameSource) -> MaterialLibrary {
        let mut lib = MaterialLibrary::default();
        let mut paths = source.list_files(|p| p.to_ascii_lowercase().ends_with(MATERIAL_EXT));
        paths.sort();
        for path in paths {
            match source.read(&path).map_err(|e| format!("{e:#}")).and_then(|b| MaterialDef::parse(&b)) {
                Ok((def, notes)) => {
                    lib.insert(path, def, notes);
                }
                Err(e) => lib.problems.push((path, e)),
            }
        }
        lib
    }

    pub fn insert(&mut self, path: String, def: MaterialDef, notes: Vec<String>) {
        let key = material_name_of(&path).to_ascii_lowercase();
        let preferred = path.starts_with(MATERIALS_DIR);
        match self.by_name.get(&key) {
            Some(existing) if !preferred || existing.starts_with(MATERIALS_DIR) => {}
            _ => {
                self.by_name.insert(key, path.clone());
            }
        }
        self.by_path.insert(path.clone(), LibraryEntry { path, def, notes });
    }

    pub fn len(&self) -> usize {
        self.by_path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_path.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &LibraryEntry> {
        self.by_path.values()
    }

    pub fn by_path(&self, path: &str) -> Option<&LibraryEntry> {
        self.by_path.get(path)
    }

    pub fn by_name(&self, name: &str) -> Option<&LibraryEntry> {
        self.by_name.get(&name.to_ascii_lowercase()).and_then(|p| self.by_path.get(p))
    }

    /// The material a part asks for. `variant` is the part's `MaterialVariant` (a path or a name; empty: none) and
    /// `enum_name` its `Material` item. The result says where it came from, so the renderer can keep one Bevy material
    /// per source. `None` path: the engine's built-in look for the item.
    pub fn resolve(&self, variant: &str, enum_name: &str) -> (Option<String>, MaterialDef) {
        let variant = variant.trim();
        if !variant.is_empty() {
            let found = if looks_like_path(variant) { self.by_path(&variant.replace('\\', "/")) } else { self.by_name(variant) };
            if let Some(entry) = found {
                return (Some(entry.path.clone()), entry.def.clone());
            }
        }
        if let Some(entry) = self.by_name(enum_name) {
            return (Some(entry.path.clone()), entry.def.clone());
        }
        (None, MaterialDef::builtin(enum_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_material_round_trips_and_unknown_fields_survive() {
        let mut m = MaterialDef::of_kind("Shiny", MaterialKind::Glass);
        m.maps.normal = "Assets/Textures/n.png".into();
        m.uv.mode = UvMode::Studs;
        let json = m.to_json();
        let (back, notes) = MaterialDef::parse(json.as_bytes()).unwrap();
        assert_eq!(back, m);
        assert!(notes.is_empty());

        let newer = r#"{ "format": 1, "kind": "glass", "future_feature": { "x": [1, 2] }, "roughness": 0.2 }"#;
        let (def, _) = MaterialDef::parse(newer.as_bytes()).unwrap();
        assert_eq!(def.roughness, 0.2);
        assert!(def.to_json().contains("future_feature"), "what this version does not know is written back");
    }

    #[test]
    fn missing_fields_take_their_defaults_and_an_unknown_kind_is_standard() {
        let (def, _) = MaterialDef::parse(br#"{ "kind": "hologram" }"#).unwrap();
        assert_eq!(def, MaterialDef { extra: def.extra.clone(), ..MaterialDef::default() });
        assert_eq!(def.kind, MaterialKind::Standard);
        let (def, _) = MaterialDef::parse(b"{}").unwrap();
        assert_eq!(def.opacity, 1.0);
    }

    #[test]
    fn bad_numbers_are_repaired_and_reported_and_a_newer_format_or_garbage_is_refused() {
        let (def, notes) = MaterialDef::parse(br#"{ "roughness": 7, "opacity": -1, "ior": 0, "base_color": [1, 2, 99] }"#).unwrap();
        assert_eq!((def.roughness, def.opacity, def.ior, def.base_color[2]), (1.0, 0.0, 1.0, 16.0));
        assert_eq!(notes.len(), 4, "{notes:?}");
        assert!(MaterialDef::parse(br#"{ "format": 99 }"#).unwrap_err().contains("newer"));
        assert!(MaterialDef::parse(b"not json").is_err());
        assert!(MaterialDef::parse(br#"{ "roughness": "rough" }"#).is_err());
        let mut m = MaterialDef { roughness: f32::NAN, metallic: f32::INFINITY, ..MaterialDef::default() };
        m.sanitize();
        assert_eq!((m.roughness, m.metallic), (0.8, 0.0), "not a number: back to the default");
    }

    #[test]
    fn a_byte_order_mark_at_the_start_of_the_file_is_ignored() {
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(MaterialDef::of_kind("Marked", MaterialKind::Neon).to_json().as_bytes());
        let (def, _) = MaterialDef::parse(&bytes).unwrap();
        assert_eq!((def.name.as_str(), def.kind), ("Marked", MaterialKind::Neon));
    }

    #[test]
    fn the_starter_materials_have_the_looks_their_names_promise() {
        let all = MaterialDef::starters();
        let by = |n: &str| all.iter().find(|m| m.name == n).unwrap();
        assert_eq!(all.len(), 5);
        assert!(by("Metal").metallic > 0.9 && by("Plastic").metallic == 0.0);
        assert!(by("SmoothPlastic").roughness < by("Plastic").roughness);
        assert!(by("Neon").kind == MaterialKind::Neon && by("Neon").emits() && !by("Plastic").emits());
        assert!(by("Glass").kind == MaterialKind::Glass && by("Glass").opacity < 0.5 && by("Glass").is_see_through() && !MaterialKind::Glass.casts_shadow());
        assert!(!by("Plastic").is_see_through());
        assert_eq!(MaterialDef::builtin("Marble").roughness, MaterialDef::builtin("SmoothPlastic").roughness, "items without a look of their own take the nearest");
    }

    #[test]
    fn maps_are_listed_and_follow_moved_files() {
        let mut m = MaterialDef::default();
        m.maps.base_color = "a/b.png".into();
        m.maps.normal = "a/n.png".into();
        assert_eq!(m.texture_paths(), vec!["a/b.png", "a/n.png"]);
        let moved = HashMap::from([("a/b.png".to_owned(), "c/b.png".to_owned())]);
        assert_eq!(m.remap_paths(&moved), 1);
        assert_eq!(m.maps.base_color, "c/b.png");
        assert_eq!(m.maps.normal, "a/n.png");
    }

    fn library() -> MaterialLibrary {
        let mut lib = MaterialLibrary::default();
        lib.insert("Assets/Materials/Brick.mat".into(), MaterialDef { name: "Brick".into(), roughness: 0.9, ..MaterialDef::default() }, vec![]);
        lib.insert("Other/Brick.mat".into(), MaterialDef { name: "Other".into(), roughness: 0.1, ..MaterialDef::default() }, vec![]);
        lib.insert("Assets/Materials/Plastic.mat".into(), MaterialDef { name: "Plastic".into(), roughness: 0.55, ..MaterialDef::default() }, vec![]);
        lib
    }

    #[test]
    fn a_part_finds_its_material_by_path_by_name_by_enum_item_or_the_built_in_look() {
        let lib = library();
        // By path.
        let (path, def) = lib.resolve("Other/Brick.mat", "Plastic");
        assert_eq!((path.as_deref(), def.roughness), (Some("Other/Brick.mat"), 0.1));
        // By name: the one in Assets/Materials wins.
        assert_eq!(lib.resolve("brick", "Plastic").0.as_deref(), Some("Assets/Materials/Brick.mat"));
        // By the Material enum when there is no variant: the project's file is the material, not the engine's.
        let (path, def) = lib.resolve("", "Plastic");
        assert_eq!((path.as_deref(), def.roughness), (Some("Assets/Materials/Plastic.mat"), 0.55));
        // A variant that does not exist falls back to the enum item; an item without a file to the built-in look.
        assert_eq!(lib.resolve("Missing.mat", "Plastic").0.as_deref(), Some("Assets/Materials/Plastic.mat"));
        let (path, def) = lib.resolve("", "Metal");
        assert!(path.is_none() && def.metallic > 0.9);
    }

    #[test]
    fn a_library_is_read_from_the_files_of_a_source_and_a_broken_file_does_not_stop_the_rest() {
        let dir = std::env::temp_dir().join(format!("forge_matlib_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Assets/Materials")).unwrap();
        std::fs::write(dir.join("Assets/Materials/Good.mat"), MaterialDef::of_kind("Good", MaterialKind::Neon).to_json()).unwrap();
        std::fs::write(dir.join("Assets/Materials/Bad.mat"), "{ nope").unwrap();
        let lib = MaterialLibrary::load(&GameSource::Dir(dir.clone()));
        assert_eq!(lib.len(), 1);
        assert_eq!(lib.by_name("good").unwrap().def.kind, MaterialKind::Neon);
        assert_eq!(lib.problems.len(), 1);
        assert!(lib.problems[0].0.ends_with("Bad.mat"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn material_names_come_from_file_names() {
        assert_eq!(material_name_of("Assets/Materials/Wet Stone.mat"), "Wet Stone");
        assert!(looks_like_path("A/b.mat") && looks_like_path("b.mat") && !looks_like_path("b"));
    }
}

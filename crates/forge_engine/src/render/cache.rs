//! Mesh, texture and material caches shared by all render hooks, so all parts of one shape and material
//! share one GPU mesh and one material (and are drawn together, instanced: see part_material).
//!
//! * **Models** (mesh files) and **textures** are cached through *weak* handles: the cache lets parts that look
//!   alike share an asset, but it does not keep the asset alive. Once the last part using one is gone Bevy frees it.
//! * **Materials** are few and kept: one Bevy material per (material asset, part colour map, see-through, texture
//!   coordinate mode). Editing a `.mat` file or a picture makes the editor call [`AssetCache::invalidate`]; the next
//!   sync rebuilds what depends on it and redraws the parts.
//! * Everything that can fail (a missing file, a damaged picture, a model Forge cannot read) fails *softly*: the problem is
//!   logged once per file, counted in [`AssetStats`], and the part is drawn with what could be loaded (a box for a model, no
//!   map for a texture) instead of disappearing.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use bevy::{asset::RenderAssetUsages, camera::primitives::Aabb, ecs::system::SystemParam, mesh::PrimitiveTopology, pbr::StandardMaterial, prelude::*};

use super::part_material::{make_material, MaterialImages, PartMaterial};
use crate::assets::{
    material::{AlphaKind, MaterialDef, MaterialKind, MaterialLibrary, SamplerSettings, UvMode},
    mesh::{self, ImportedMaterial, TextureRef},
    texture, GameSource,
};

/// How many of the latest loads are kept for the debug overlay.
const RECENT_LOADS: usize = 8;

/// One file that was read (for the debug overlay).
#[derive(Clone, Debug)]
pub struct LoadRecord {
    pub path: String,
    pub millis: f32,
    pub ok: bool,
}

/// What loading project files cost. Files are read synchronously when a part first needs them, so
/// "loading" is a list of finished loads, not of pending ones.
#[derive(Clone, Default, Debug)]
pub struct AssetStats {
    pub loads: u64,
    pub failures: u64,
    pub total_ms: f64,
    pub slowest_ms: f32,
    pub slowest: String,
    /// The latest loads, newest last.
    pub recent: Vec<LoadRecord>,
    /// Bumped on every load: the overlay can tell when something happened.
    pub generation: u64,
}

impl AssetStats {
    fn record(&mut self, path: &str, millis: f32, ok: bool) {
        self.loads += 1;
        self.generation += 1;
        if !ok {
            self.failures += 1;
        }
        self.total_ms += millis as f64;
        if millis > self.slowest_ms {
            self.slowest_ms = millis;
            self.slowest = path.to_owned();
        }
        if self.recent.len() >= RECENT_LOADS {
            self.recent.remove(0);
        }
        self.recent.push(LoadRecord { path: path.to_owned(), millis, ok });
    }
}

/// Where a part's material comes from. Together with the other fields of [`MatKey`] it identifies one Bevy material.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum MatSource {
    /// The engine's look for a `Material` enum item that has no file in the project.
    Builtin(String),
    /// A `.mat` file of the project.
    File(String),
    /// A material that came with an imported model: `(model path, index)`.
    Imported(String, usize),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct MatKey {
    pub source: MatSource,
    /// A picture the *part* wants as its colour map (it replaces the material's), or empty.
    pub color_map: String,
    pub see_through: bool,
    /// Texture coordinates are projected in studs (a shape) rather than read from the mesh (a model).
    pub studs_uv: bool,
    /// The part's `Reflectance` in whole percent: 0 leaves the material as it is.
    pub reflectance: u8,
}

/// Makes a part's material more mirror-like by its `Reflectance` (0 to 1): more metallic, smoother. The key says so, so parts that reflect
/// by the same amount share one Bevy material.
pub fn with_reflectance(key: &mut MatKey, def: &mut MaterialDef, reflectance: f32) {
    let percent = (reflectance.clamp(0.0, 1.0) * 100.0).round() as u8;
    key.reflectance = percent;
    if percent > 0 {
        let k = f32::from(percent) / 100.0;
        def.metallic = def.metallic.max(k);
        def.roughness = def.roughness * (1.0 - k) + 0.05 * k;
    }
}

/// A model that is loaded: its pieces (one mesh per material) and size.
struct ModelEntry {
    size: Vec3,
    pieces: Vec<Piece>,
    materials: Vec<ImportedMaterial>,
}

#[derive(Clone)]
struct Piece {
    mesh: AssetId<Mesh>,
    aabb: Aabb,
    material: Option<usize>,
}

/// One drawable piece of a model.
#[derive(Clone)]
pub struct ModelPiece {
    pub mesh: Handle<Mesh>,
    pub aabb: Option<Aabb>,
    /// The index of the model's own material this piece uses, if the file gave one.
    pub material: Option<usize>,
}

/// What a part draws: one or several pieces, and the size the model has before the part's `Size` scales it.
#[derive(Clone)]
pub struct Model {
    pub pieces: Vec<ModelPiece>,
    /// The model's natural size in studs (1 x 1 x 1 for the built-in shapes, which `Size` scales directly).
    pub native: Vec3,
    /// How many of the model's own materials there are (0: the file has none).
    pub has_materials: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct TexKey {
    path: String,
    srgb: bool,
    sampler: SamplerSettings,
}

#[derive(Resource, Default)]
pub struct AssetCache {
    /// The built-in shapes: always kept.
    primitives: HashMap<u8, Handle<Mesh>>,
    /// Weak: see the module comment.
    models: HashMap<String, Arc<ModelEntry>>,
    textures: HashMap<TexKey, AssetId<Image>>,
    /// The Bevy materials, and the number each was given (a part remembers the number, not the key).
    materials: HashMap<MatKey, Handle<PartMaterial>>,
    material_ids: HashMap<MatKey, u32>,
    /// Pictures that came inside model files, by `model#imageN`.
    embedded: HashMap<String, Arc<Vec<u8>>>,
    /// The project's materials (read on first use).
    library: Option<MaterialLibrary>,
    /// Files that failed, so the warning is printed once instead of on every refresh.
    warned: HashSet<String>,
    /// Studs per unit of a model file (a project setting).
    pub mesh_scale: f32,
    /// Something the parts depend on changed: the next sync redraws every part.
    stale: bool,
    /// What the cameras were last given (`camera_fx`).
    fx: Option<super::camera_fx::CameraEffects>,
    /// What the effects that move on their own keep per instance (particles, trails ...): see `RenderCtx::fx_state`.
    pub(crate) fx_states: HashMap<crate::dm::Id, Box<dyn std::any::Any + Send + Sync>>,
    /// The entities those effects made, despawned with the instance.
    pub(crate) fx_entities: HashMap<crate::dm::Id, Vec<Entity>>,
    /// Changes whenever files are read again, so effects that hold on to a picture look it up again.
    pub(crate) fx_epoch: u32,
    /// The pictures effects draw when they have none of their own (made on first use).
    pub(crate) fx_textures: HashMap<&'static str, Handle<Image>>,
    /// Loading statistics (see [`AssetStats`]).
    pub stats: AssetStats,
}

impl AssetCache {
    /// Forgets every file read so far (models, pictures, materials), so the next time a part needs one it is read again: a
    /// reimported model, an edited picture or a changed `.mat` shows its new look. The next sync redraws all parts.
    pub fn invalidate(&mut self) {
        self.models.clear();
        self.textures.clear();
        self.materials.clear();
        self.material_ids.clear();
        self.embedded.clear();
        self.library = None;
        self.warned.clear();
        self.stale = true;
        // (Effects that hold on to pictures look them up again.)
        self.fx_epoch = self.fx_epoch.wrapping_add(1);
    }

    /// Older name of [`AssetCache::invalidate`].
    pub fn forget_files(&mut self) {
        self.invalidate();
    }

    /// The state an effect keeps for an instance (made with T::default() the first time).
    pub fn fx_state<T: Default + Send + Sync + 'static>(&mut self, id: crate::dm::Id) -> &mut T {
        self.fx_states.entry(id).or_insert_with(|| Box::new(T::default())).downcast_mut::<T>().expect("an effect keeps one kind of state per instance")
    }

    /// Whether the scene's camera effects differ from what the cameras were last given (and remembers the new ones).
    pub fn look_changed(&mut self, fx: &super::camera_fx::CameraEffects) -> bool {
        if self.fx.as_ref() == Some(fx) {
            return false;
        }
        self.fx = Some(fx.clone());
        true
    }

    /// The cameras are given their look again at the next sync (the sky's pictures may have changed).
    pub fn forget_look(&mut self) {
        self.fx = None;
    }

    /// Whether the parts have to be redrawn (and forgets that it was asked).
    pub fn take_stale(&mut self) -> bool {
        std::mem::take(&mut self.stale)
    }

    /// Cached models and Bevy materials (a model whose asset is already gone still counts).
    pub fn counts(&self) -> (usize, usize) {
        (self.models.len(), self.materials.len())
    }

    /// Cached pictures.
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// The materials of the project, read now if they have not been.
    pub fn library(&mut self, source: &GameSource) -> &MaterialLibrary {
        if self.library.is_none() {
            let library = MaterialLibrary::load(source);
            // A material that cannot be read, or that had numbers brought back into range, says so once.
            for (path, problem) in &library.problems {
                warn!("material {path} was not loaded: {problem}");
            }
            for entry in library.entries() {
                for note in &entry.notes {
                    warn!("material {}: {note}", entry.path);
                }
            }
            self.library = Some(library);
        }
        self.library.as_ref().expect("the library was just made")
    }

    fn warn_once(&mut self, path: &str, message: String) {
        if self.warned.insert(path.to_owned()) {
            warn!("{message}");
        }
    }

    /// The number a part remembers for a material (so a refresh can tell whether it changed).
    pub fn material_id(&mut self, key: &MatKey) -> u32 {
        let next = self.material_ids.len() as u32 + 1;
        *self.material_ids.entry(key.clone()).or_insert(next)
    }
}

// ---- flat-sided shapes -----------------------------------------------------------------------------

/// A mesh from the faces of a convex shape (each face a flat polygon, in any winding). Every face has
/// its own vertices with its own normal, so edges stay sharp. The faces are wound to face outwards.
fn faceted_mesh(faces: &[&[Vec3]]) -> Mesh {
    let centre = faces.iter().flat_map(|f| f.iter()).copied().sum::<Vec3>() / faces.iter().map(|f| f.len()).sum::<usize>() as f32;
    let (mut positions, mut normals, mut uvs): (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>) = (Vec::new(), Vec::new(), Vec::new());
    for face in faces {
        let mut n = (face[1] - face[0]).cross(face[2] - face[0]).normalize_or_zero();
        let flip = n.dot(face[0] - centre) < 0.0;
        if flip {
            n = -n;
        }
        for k in 1..face.len() - 1 {
            let tri = if flip { [face[0], face[k + 1], face[k]] } else { [face[0], face[k], face[k + 1]] };
            for v in tri {
                positions.push(v.to_array());
                normals.push(n.to_array());
                // Texture coordinates from the box the shape sits in, projected along the face's main axis.
                let m = n.abs();
                let (a, b) = if m.x >= m.y && m.x >= m.z {
                    (v.z, v.y)
                } else if m.y >= m.z {
                    (v.x, v.z)
                } else {
                    (v.x, v.y)
                };
                uvs.push([a + 0.5, b + 0.5]);
            }
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh
}

/// A box cut along a slope: the low edge is at the front (-Z) bottom and the slope rises to the top of
/// the back (+Z) wall. The corners are those of the unit box (-0.5..0.5).
fn wedge_mesh() -> Mesh {
    let (a, b, c, d) = (Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, -0.5, -0.5), Vec3::new(0.5, -0.5, 0.5), Vec3::new(-0.5, -0.5, 0.5));
    let (e, f) = (Vec3::new(0.5, 0.5, 0.5), Vec3::new(-0.5, 0.5, 0.5));
    faceted_mesh(&[&[a, b, c, d], &[d, c, e, f], &[a, b, e, f], &[b, c, e], &[a, d, f]])
}

/// A pyramid standing on the unit box's floor with its tip above the back-right bottom corner: half a
/// wedge, cut along the diagonal.
fn corner_wedge_mesh() -> Mesh {
    let (a, b, c, d) = (Vec3::new(-0.5, -0.5, -0.5), Vec3::new(0.5, -0.5, -0.5), Vec3::new(0.5, -0.5, 0.5), Vec3::new(-0.5, -0.5, 0.5));
    let tip = Vec3::new(0.5, 0.5, 0.5);
    faceted_mesh(&[&[a, b, c, d], &[b, c, tip], &[c, d, tip], &[a, b, tip], &[a, d, tip]])
}

#[derive(SystemParam)]
pub struct SpawnParams<'w> {
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<PartMaterial>>,
    pub images: ResMut<'w, Assets<Image>>,
    /// The materials of decals (Bevy's forward decals have a material type of their own).
    pub decal_materials: ResMut<'w, Assets<bevy::pbr::decal::ForwardDecalMaterial<StandardMaterial>>>,
    /// The plain materials of effects (particles, beams, trails, highlights).
    pub fx_materials: ResMut<'w, Assets<StandardMaterial>>,
    /// The materials of highlights.
    pub highlight_materials: ResMut<'w, Assets<super::on_top::HighlightMaterial>>,
    pub cache: ResMut<'w, AssetCache>,
}

/// The `Material` enum item's name, as the engine's built-in look knows it. Kept for callers that only want to know
/// whether an item is one of the five with a look of its own.
pub fn material_index(name: &str) -> u8 {
    match name {
        "SmoothPlastic" | "Ice" | "Glacier" | "Marble" | "Foil" => 1,
        "Metal" | "CorrodedMetal" | "DiamondPlate" => 2,
        "Neon" | "CrackedLava" | "ForceField" => 3,
        "Glass" | "Water" => 4,
        "Wood" | "WoodPlanks" => 5,
        "Concrete" | "Slate" | "Basalt" | "Limestone" | "Granite" | "Pavement" | "Brick" | "Pebble" | "Cobblestone" | "Rock" | "Sandstone" | "Asphalt" | "Salt" => 6,
        _ => 0,
    }
}

/// A material as a model file describes it, in the project's own terms.
pub fn material_from_import(imp: &ImportedMaterial, model: &str) -> MaterialDef {
    let key = |t: &Option<TextureRef>| t.as_ref().map(|t| t.key(model)).unwrap_or_default();
    let mut def = MaterialDef {
        name: imp.name.clone(),
        base_color: [imp.base_color[0], imp.base_color[1], imp.base_color[2]],
        opacity: imp.base_color[3],
        roughness: imp.roughness,
        metallic: imp.metallic,
        emission_color: imp.emissive,
        emission_strength: if imp.emissive.iter().any(|v| *v > 0.0) || imp.emissive_map.is_some() { 1.0 } else { 0.0 },
        emission_from_color: false,
        alpha_mode: imp.alpha,
        alpha_cutoff: imp.alpha_cutoff,
        double_sided: imp.double_sided,
        kind: if imp.unlit { MaterialKind::Unlit } else { MaterialKind::Standard },
        ..MaterialDef::default()
    };
    if imp.emissive_map.is_some() && imp.emissive == [0.0; 3] {
        def.emission_color = [1.0; 3];
    }
    def.maps.base_color = key(&imp.base_color_map);
    def.maps.normal = key(&imp.normal_map);
    def.maps.metallic_roughness = key(&imp.metallic_roughness_map);
    def.maps.roughness = key(&imp.roughness_map);
    def.maps.metallic = key(&imp.metallic_map);
    def.maps.occlusion = key(&imp.occlusion_map);
    def.maps.emission = key(&imp.emissive_map);
    def.uv.mode = UvMode::Mesh;
    def.sanitize();
    def
}

impl SpawnParams<'_> {
    /// Unit-sized primitive: a 1x1x1 box, a sphere of diameter 1, a cylinder of diameter/height 1,
    /// or a wedge / corner wedge that fills a 1x1x1 box (see [`wedge_mesh`], [`corner_wedge_mesh`]).
    pub fn primitive(&mut self, shape: &str) -> Handle<Mesh> {
        let key = match shape {
            "Ball" => 1,
            "Cylinder" => 2,
            "Wedge" => 3,
            "CornerWedge" => 4,
            _ => 0,
        };
        if let Some(h) = self.cache.primitives.get(&key) {
            return h.clone();
        }
        let mesh: Mesh = match key {
            1 => Sphere::new(0.5).into(),
            2 => Cylinder::new(0.5, 1.0).into(),
            3 => wedge_mesh(),
            4 => corner_wedge_mesh(),
            _ => Cuboid::new(1.0, 1.0, 1.0).into(),
        };
        // (Tangents are what a normal map needs to light correctly; the shapes carry texture coordinates and normals already.)
        let mesh = mesh.clone().with_generated_tangents().unwrap_or(mesh);
        let h = self.meshes.add(mesh);
        self.cache.primitives.insert(key, h.clone());
        h
    }

    /// The bounding box of a mesh whose shape is known without reading its vertices: the built-in
    /// primitives, which are unit-sized and centred. A part spawned with it skips the pass Bevy
    /// would otherwise make over the whole mesh to compute one (a ball has hundreds of vertices).
    pub fn known_aabb(&self, mesh: AssetId<Mesh>) -> Option<Aabb> {
        self.cache.primitives.values().any(|h| h.id() == mesh).then(|| Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5)))
    }

    /// The model of a shape: one piece, unit sized.
    pub fn shape_model(&mut self, shape: &str) -> Model {
        let mesh = self.primitive(shape);
        Model { pieces: vec![ModelPiece { mesh, aabb: Some(Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5))), material: None }], native: Vec3::ONE, has_materials: false }
    }

    /// A model from a project file, at its natural size (see `assets::mesh`), centred on its middle. Falls back to a box
    /// (and says why once) when the file is missing or cannot be read.
    pub fn file_model(&mut self, path: &str, source: &GameSource) -> Model {
        if let Some(entry) = self.cache.models.get(path).cloned() {
            // (The pieces are weak: a model whose meshes were freed is read again.)
            let pieces: Option<Vec<ModelPiece>> = entry.pieces.iter().map(|p| self.meshes.get_strong_handle(p.mesh).map(|mesh| ModelPiece { mesh, aabb: Some(p.aabb), material: p.material })).collect();
            if let Some(pieces) = pieces {
                return Model { pieces, native: entry.size, has_materials: !entry.materials.is_empty() };
            }
        }
        if path.is_empty() {
            return self.shape_model("Block");
        }
        let started = std::time::Instant::now();
        let scale = if self.cache.mesh_scale > 0.0 { self.cache.mesh_scale } else { 1.0 };
        let loaded = mesh::load(source, path, scale);
        self.cache.stats.record(path, started.elapsed().as_secs_f32() * 1000.0, loaded.is_ok());
        match loaded {
            Ok(data) => {
                self.cache.warned.remove(path);
                let mut pieces = Vec::new();
                let mut handles = Vec::new();
                for sub in &data.submeshes {
                    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                    for p in &sub.positions {
                        lo = lo.min(Vec3::from(*p));
                        hi = hi.max(Vec3::from(*p));
                    }
                    let handle = self.meshes.add(sub.to_bevy());
                    let aabb = Aabb::from_min_max(lo, hi);
                    pieces.push(Piece { mesh: handle.id(), aabb, material: sub.material });
                    handles.push(ModelPiece { mesh: handle, aabb: Some(aabb), material: sub.material });
                }
                for m in &data.materials {
                    for t in [&m.base_color_map, &m.normal_map, &m.metallic_roughness_map, &m.roughness_map, &m.metallic_map, &m.occlusion_map, &m.emissive_map].into_iter().flatten() {
                        if let TextureRef::Embedded { bytes, .. } = t {
                            self.cache.embedded.insert(t.key(path), bytes.clone());
                        }
                    }
                }
                let used = data.has_materials();
                self.cache.models.insert(path.to_owned(), Arc::new(ModelEntry { size: data.size, pieces, materials: data.materials.clone() }));
                // (A model whose faces name no material is drawn with the part's own.)
                Model { pieces: handles, native: data.size, has_materials: used }
            }
            Err(e) => {
                self.cache.warn_once(path, format!("could not load model '{path}': {e} (a box is shown instead)"));
                self.shape_model("Block")
            }
        }
    }

    /// The natural size of a model that was already loaded (without loading it).
    pub fn model_size(&self, path: &str) -> Option<Vec3> {
        self.cache.models.get(path).map(|m| m.size)
    }

    /// A picture of the project as a texture. `srgb`: colour (the GPU converts to linear) rather than data. `None`
    /// (and a warning, once) when it cannot be read.
    pub fn texture(&mut self, path: &str, srgb: bool, sampler: &SamplerSettings, source: &GameSource) -> Option<Handle<Image>> {
        if path.is_empty() {
            return None;
        }
        let key = TexKey { path: path.to_owned(), srgb, sampler: sampler.clone() };
        if let Some(live) = self.cache.textures.get(&key).and_then(|id| self.images.get_strong_handle(*id)) {
            return Some(live);
        }
        let started = std::time::Instant::now();
        let result = self.read_picture(path, source).map(|d| texture::to_image(&d, srgb, sampler));
        self.cache.stats.record(path, started.elapsed().as_secs_f32() * 1000.0, result.is_ok());
        match result {
            Ok(image) => {
                let handle = self.images.add(image);
                self.cache.textures.insert(key, handle.id());
                self.cache.warned.remove(path);
                Some(handle)
            }
            Err(e) => {
                self.cache.warn_once(path, format!("could not load texture '{path}': {e}"));
                None
            }
        }
    }

    fn read_picture(&mut self, path: &str, source: &GameSource) -> Result<texture::Decoded, String> {
        let bytes: Arc<Vec<u8>> = match self.cache.embedded.get(path) {
            Some(b) => b.clone(),
            None => Arc::new(source.read(path).map_err(|e| format!("{e:#}"))?),
        };
        let ext = path.rsplit('.').next().unwrap_or("");
        texture::decode(&bytes, ext)
    }

    /// The roughness and metallic maps of a material as one picture (green and blue), or the packed map it names.
    fn metallic_roughness_texture(&mut self, def: &MaterialDef, source: &GameSource) -> Option<Handle<Image>> {
        let maps = &def.maps;
        if !maps.metallic_roughness.is_empty() {
            return self.texture(&maps.metallic_roughness, false, &def.sampler, source);
        }
        if maps.roughness.is_empty() && maps.metallic.is_empty() {
            return None;
        }
        let key = TexKey { path: format!("roughness:{}|metallic:{}", maps.roughness, maps.metallic), srgb: false, sampler: def.sampler.clone() };
        if let Some(live) = self.cache.textures.get(&key).and_then(|id| self.images.get_strong_handle(*id)) {
            return Some(live);
        }
        let started = std::time::Instant::now();
        let read = |this: &mut Self, p: &str| -> Result<Option<texture::Decoded>, String> { if p.is_empty() { Ok(None) } else { this.read_picture(p, source).map(Some) } };
        let combined = read(self, &maps.roughness).and_then(|r| read(self, &maps.metallic).map(|m| (r, m))).map(|(r, m)| texture::metallic_roughness(r.as_ref(), m.as_ref()));
        let label = key.path.clone();
        self.cache.stats.record(&label, started.elapsed().as_secs_f32() * 1000.0, matches!(combined, Ok(Some(_))));
        match combined {
            Ok(Some(d)) => {
                let handle = self.images.add(texture::to_image(&d, false, &def.sampler));
                self.cache.textures.insert(key, handle.id());
                Some(handle)
            }
            Ok(None) => None,
            Err(e) => {
                self.cache.warn_once(&label, format!("could not load the roughness / metallic maps of material '{}': {e}", def.name));
                None
            }
        }
    }

    /// Everything a part's material needs, decided: which material file or built-in look, whether the blended variant is
    /// wanted, and the key under which the Bevy material is cached.
    pub fn resolve_material(&mut self, variant: &str, enum_name: &str, color_map: &str, part_see_through: bool, studs_uv: bool, source: &GameSource) -> (MatKey, MaterialDef) {
        let (path, def) = self.cache.library(source).resolve(variant, enum_name);
        let source_key = match path {
            Some(p) => MatSource::File(p),
            None => MatSource::Builtin(enum_name.to_owned()),
        };
        let key = MatKey { source: source_key, color_map: color_map.to_owned(), see_through: part_see_through || def.is_see_through(), studs_uv, reflectance: 0 };
        (key, def)
    }

    /// The Bevy material for a key, built (and its pictures loaded) the first time.
    pub fn part_material(&mut self, key: &MatKey, def: &MaterialDef, source: &GameSource) -> Handle<PartMaterial> {
        if let Some(h) = self.cache.materials.get(key) {
            return h.clone();
        }
        let mut images = MaterialImages {
            base_color: self.texture(if key.color_map.is_empty() { &def.maps.base_color } else { &key.color_map }, true, &def.sampler, source),
            normal: self.texture(&def.maps.normal, false, &def.sampler, source),
            occlusion: self.texture(&def.maps.occlusion, false, &def.sampler, source),
            emission: self.texture(&def.maps.emission, true, &def.sampler, source),
            ..MaterialImages::default()
        };
        images.metallic_roughness = self.metallic_roughness_texture(def, source);
        images.has_roughness_map = !def.maps.roughness.is_empty() || !def.maps.metallic_roughness.is_empty();
        images.has_metallic_map = !def.maps.metallic.is_empty() || !def.maps.metallic_roughness.is_empty();
        let handle = self.materials.add(make_material(def, &images, key.see_through, key.studs_uv));
        self.cache.materials.insert(key.clone(), handle.clone());
        handle
    }

    /// The material definition one of a model's own materials stands for.
    pub fn imported_material(&self, model: &str, index: usize) -> Option<MaterialDef> {
        self.cache.models.get(model).and_then(|m| m.materials.get(index)).map(|m| material_from_import(m, model))
    }

    /// The key for one of a model's own materials.
    pub fn imported_key(&self, model: &str, index: usize, part_see_through: bool, def: &MaterialDef) -> MatKey {
        MatKey { source: MatSource::Imported(model.to_owned(), index), color_map: String::new(), see_through: part_see_through || def.is_see_through() || def.alpha_mode == AlphaKind::Blend, studs_uv: false, reflectance: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle has the winding and normal of a face of a convex shape: facing away from its inside.
    fn assert_outward(mesh: &Mesh, triangles: usize) {
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()).unwrap();
        let normals = mesh.attribute(Mesh::ATTRIBUTE_NORMAL).and_then(|a| a.as_float3()).unwrap();
        assert_eq!(positions.len(), triangles * 3);
        let inside = Vec3::new(0.1, -0.3, 0.1);
        for (tri, ns) in positions.chunks(3).zip(normals.chunks(3)) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(Vec3::from);
            let geometric = (b - a).cross(c - a).normalize();
            let n = Vec3::from(ns[0]);
            assert!(geometric.dot(n) > 0.999, "the winding agrees with the normal: {geometric} vs {n}");
            assert!(n.dot(a - inside) > 0.0, "the face points away from the inside: {n}");
            assert!(tri.iter().all(|p| p.iter().all(|v| v.abs() <= 0.5 + 1e-6)), "inside the unit box");
        }
    }

    #[test]
    fn the_wedge_meshes_are_closed_and_face_outwards() {
        assert_outward(&wedge_mesh(), 8);
        assert_outward(&corner_wedge_mesh(), 6);
    }

    #[test]
    fn a_models_own_material_is_expressed_in_the_projects_terms() {
        let imp = ImportedMaterial {
            name: "Wet".into(),
            base_color: [0.5, 0.25, 0.125, 0.5],
            metallic: 0.3,
            roughness: 0.2,
            emissive: [0.0, 2.0, 0.0],
            base_color_map: Some(TextureRef::File("Assets/Models/wet.png".into())),
            normal_map: Some(TextureRef::Embedded { name: "image3".into(), bytes: Arc::new(vec![1]) }),
            alpha: AlphaKind::Blend,
            double_sided: true,
            ..ImportedMaterial::default()
        };
        let def = material_from_import(&imp, "Assets/Models/car.glb");
        assert_eq!((def.base_color, def.opacity, def.metallic, def.roughness), ([0.5, 0.25, 0.125], 0.5, 0.3, 0.2));
        assert_eq!((def.emission_color, def.emission_strength, def.emission_from_color), ([0.0, 2.0, 0.0], 1.0, false));
        assert_eq!(def.maps.base_color, "Assets/Models/wet.png");
        assert_eq!(def.maps.normal, "Assets/Models/car.glb#image3", "an embedded picture is found by model and name");
        assert!(def.double_sided && def.alpha_mode == AlphaKind::Blend && def.uv.mode == UvMode::Mesh);
        assert!(def.is_see_through());
    }

    #[test]
    fn material_numbers_are_stable_for_a_key_and_distinct_between_keys() {
        let mut cache = AssetCache::default();
        let key = |name: &str, glass: bool| MatKey { source: MatSource::Builtin(name.into()), color_map: String::new(), see_through: glass, studs_uv: true, reflectance: 0 };
        let (a, b, c) = (cache.material_id(&key("Plastic", false)), cache.material_id(&key("Plastic", true)), cache.material_id(&key("Metal", false)));
        assert!(a != b && b != c && a != c);
        assert_eq!(cache.material_id(&key("Plastic", false)), a);
        cache.invalidate();
        assert!(cache.take_stale() && !cache.take_stale(), "the parts are asked to redraw once");
    }
}

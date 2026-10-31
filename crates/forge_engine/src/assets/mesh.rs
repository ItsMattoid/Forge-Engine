//! Reading 3D models: Wavefront OBJ (+ MTL), glTF 2.0 / GLB, STL and PLY.
//!
//! [`load`] turns a file of the project into a [`MeshData`]: triangles grouped by material, the materials the file
//! describes, and the model's size. What it guarantees, whatever the format:
//!
//! * **Real size.** Positions are multiplied by the *unit scale* (studs per unit of the file; 1 by default, a project
//!   setting) and then the model is **centred on the middle of its bounding box**. `size` is the box's size in studs,
//!   which is what a new `MeshPart` is given for its `Size`, so a model appears at the dimensions it was made with. (The
//!   part's collision box and the mesh then agree, and resizing the part scales the mesh to fit.)
//! * **Normals.** Taken from the file; where it has none they are computed: flat per face, or smooth across the faces
//!   that share a vertex and a smoothing group (OBJ `s`), never averaged over a hard edge.
//! * **Texture coordinates.** Taken from the file (flipped to the renderer's convention, v down); a model without any gets a
//!   box projection, so a texture still lands on it.
//! * **Tangents.** Generated for every submesh that has texture coordinates ([`SubMesh::to_bevy`]), which is what normal
//!   maps need to light correctly.
//! * **Materials.** MTL files and glTF materials come with the model as [`ImportedMaterial`]s (colour, metallic, roughness,
//!   emission, and the pictures they use), so a model brought in with its textures looks as its author made it.
//!
//! Formats the engine cannot read (FBX, Collada, ...) are refused with a message that says what to export instead.
//! Parsing is separate from Bevy (only [`SubMesh::to_bevy`] builds a `Mesh`) and works on bytes, so it is tested without a
//! window or files.

use std::{collections::HashMap, sync::Arc};

use bevy::{
    asset::RenderAssetUsages,
    math::{Mat3, Mat4, Vec2, Vec3, Vec4},
    mesh::{Indices, Mesh, PrimitiveTopology},
};

use super::{
    material::AlphaKind,
    source::{clean_path, GameSource},
};

/// The biggest model file that is read.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// Where an imported material's picture is.
#[derive(Clone, Debug, PartialEq)]
pub enum TextureRef {
    /// A file of the project (path relative to the project).
    File(String),
    /// A picture stored inside the model file (GLB, data URIs).
    Embedded { name: String, bytes: Arc<Vec<u8>> },
}

impl TextureRef {
    /// A key for caches: the path, or a name unique to the file and picture.
    pub fn key(&self, model: &str) -> String {
        match self {
            TextureRef::File(p) => p.clone(),
            TextureRef::Embedded { name, .. } => format!("{model}#{name}"),
        }
    }

    /// The file extension, as a hint for decoding.
    pub fn ext(&self) -> String {
        match self {
            TextureRef::File(p) => p.rsplit('.').next().unwrap_or("").to_ascii_lowercase(),
            TextureRef::Embedded { .. } => String::new(),
        }
    }
}

/// A material as the model file describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedMaterial {
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub base_color_map: Option<TextureRef>,
    pub normal_map: Option<TextureRef>,
    /// glTF's packed picture: green is roughness, blue is metallic.
    pub metallic_roughness_map: Option<TextureRef>,
    /// Separate maps (MTL's `map_Pr` / `map_Pm`): the red channel.
    pub roughness_map: Option<TextureRef>,
    pub metallic_map: Option<TextureRef>,
    pub occlusion_map: Option<TextureRef>,
    pub emissive_map: Option<TextureRef>,
    pub alpha: AlphaKind,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
    pub unlit: bool,
}

impl Default for ImportedMaterial {
    fn default() -> Self {
        ImportedMaterial {
            name: String::new(),
            base_color: [1.0; 4],
            metallic: 0.0,
            roughness: 0.8,
            emissive: [0.0; 3],
            base_color_map: None,
            normal_map: None,
            metallic_roughness_map: None,
            roughness_map: None,
            metallic_map: None,
            occlusion_map: None,
            emissive_map: None,
            alpha: AlphaKind::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            unlit: false,
        }
    }
}

/// The triangles that share one material.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Tangents the file brought (glTF); empty when they are to be generated.
    pub tangents: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Index into [`MeshData::materials`].
    pub material: Option<usize>,
    /// The texture coordinates were made up (box projection), not read.
    pub generated_uvs: bool,
}

impl SubMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The Bevy mesh: positions, normals, texture coordinates and (when there are coordinates to derive them from)
    /// tangents.
    pub fn to_bevy(&self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone());
        mesh.insert_indices(Indices::U32(self.indices.clone()));
        if self.tangents.len() == self.positions.len() && !self.tangents.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, self.tangents.clone());
        } else if !self.indices.is_empty() {
            // (Fails only for a mesh without the attributes it needs, which this one has.)
            let _ = mesh.generate_tangents();
        }
        mesh
    }
}

/// A model, ready to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub submeshes: Vec<SubMesh>,
    pub materials: Vec<ImportedMaterial>,
    /// The size of the bounding box in studs (after the unit scale).
    pub size: Vec3,
    /// Where the middle of the box was in the file's own coordinates, times the unit scale (subtracted from every position).
    pub center: Vec3,
}

impl MeshData {
    pub fn triangle_count(&self) -> usize {
        self.submeshes.iter().map(SubMesh::triangle_count).sum()
    }

    pub fn vertex_count(&self) -> usize {
        self.submeshes.iter().map(|s| s.positions.len()).sum()
    }

    /// Whether the file gave the model materials of its own.
    pub fn has_materials(&self) -> bool {
        !self.materials.is_empty() && self.submeshes.iter().any(|s| s.material.is_some())
    }
}

// ---- the formats ------------------------------------------------------------------------------------------

/// The file extensions [`load`] reads.
pub const MESH_EXTENSIONS: &[&str] = &["obj", "gltf", "glb", "stl", "ply"];

/// Whether a file name is a model Forge can read.
pub fn is_loadable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    MESH_EXTENSIONS.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

/// A model format the editor lists but the engine cannot read, with what to do about it.
pub fn unsupported_reason(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    let advice = match ext {
        "fbx" => "FBX is a closed format",
        "dae" => "Collada is not supported",
        _ => return None,
    };
    Some(format!("{advice}: export the model from your 3D program as glTF (.glb) or OBJ and import that."))
}

/// Reads a model from the project and gives it its real size. `unit_scale` is studs per unit of the file.
pub fn load(source: &GameSource, path: &str, unit_scale: f32) -> Result<MeshData, String> {
    let path = clean_path(path).map_err(|e| e.to_string())?;
    if let Some(why) = unsupported_reason(&path) {
        return Err(why);
    }
    if let GameSource::Dir(root) = source {
        if let Ok(meta) = std::fs::metadata(root.join(&path)) {
            if meta.len() > MAX_FILE_BYTES {
                return Err(format!("{path} is {} MB, more than the {} MB Forge reads", meta.len() / (1 << 20), MAX_FILE_BYTES / (1 << 20)));
            }
        }
    }
    let bytes = source.read(&path).map_err(|e| format!("{e:#}"))?;
    let dir = path.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let read_sibling = |rel: &str| -> Option<Vec<u8>> {
        let joined = join_relative(&dir, rel)?;
        source.read(&joined).ok()
    };
    parse(&ext, &bytes, &dir, &read_sibling, unit_scale).map_err(|e| format!("{path}: {e}"))
}

/// Parses a model. `dir` is the folder the file is in and `read_sibling` reads another file named relative to it (an MTL,
/// a `.bin`, a picture).
pub fn parse(ext: &str, bytes: &[u8], dir: &str, read_sibling: &dyn Fn(&str) -> Option<Vec<u8>>, unit_scale: f32) -> Result<MeshData, String> {
    if !(unit_scale.is_finite() && unit_scale > 0.0) {
        return Err(format!("the import scale {unit_scale} must be a number above zero"));
    }
    let mut raw = match ext {
        "obj" => parse_obj(bytes, dir, read_sibling)?,
        "gltf" | "glb" => parse_gltf(bytes, dir, read_sibling)?,
        "stl" => parse_stl(bytes)?,
        "ply" => parse_ply(bytes)?,
        other => return Err(format!("'.{other}' is not a model format Forge reads (it reads {})", MESH_EXTENSIONS.join(", "))),
    };
    raw.submeshes.retain(|s| !s.indices.is_empty());
    if raw.submeshes.is_empty() {
        return Err("the file has no triangles".into());
    }
    finish(&mut raw, unit_scale)?;
    Ok(raw)
}

/// Scales, centres and completes (texture coordinates) a freshly parsed model.
fn finish(data: &mut MeshData, unit_scale: f32) -> Result<(), String> {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for s in &mut data.submeshes {
        for p in &mut s.positions {
            for v in p.iter_mut() {
                if !v.is_finite() {
                    return Err("the file has positions that are not numbers".into());
                }
                *v *= unit_scale;
            }
            let v = Vec3::from(*p);
            min = min.min(v);
            max = max.max(v);
        }
    }
    let center = (min + max) * 0.5;
    let size = (max - min).max(Vec3::splat(1e-4));
    for s in &mut data.submeshes {
        for p in &mut s.positions {
            *p = (Vec3::from(*p) - center).to_array();
        }
        // A model without texture coordinates gets a box projection, over its whole size once.
        if s.uvs.len() != s.positions.len() {
            s.generated_uvs = true;
            s.uvs = s
                .positions
                .iter()
                .zip(&s.normals)
                .map(|(p, n)| {
                    let (p, n) = (Vec3::from(*p) / size + Vec3::splat(0.5), Vec3::from(*n).abs());
                    if n.x >= n.y && n.x >= n.z {
                        [p.z, 1.0 - p.y]
                    } else if n.y >= n.z {
                        [p.x, p.z]
                    } else {
                        [p.x, 1.0 - p.y]
                    }
                })
                .collect();
        }
    }
    data.center = center;
    data.size = size;
    Ok(())
}

/// `rel` as a path relative to `dir`, cleaned (and refused if it leaves the project).
fn join_relative(dir: &str, rel: &str) -> Option<String> {
    let rel = percent_decode(rel.trim()).replace('\\', "/");
    if rel.starts_with('/') || rel.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for piece in rel.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    clean_path(&parts.join("/")).ok()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---- normals ----------------------------------------------------------------------------------------------

fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    (b - a).cross(c - a)
}

/// Flat shading: every triangle gets its own three vertices and its face normal.
fn flat_submesh(triangles: &[[(Vec3, Vec2); 3]], material: Option<usize>, with_uvs: bool) -> SubMesh {
    let mut s = SubMesh { material, ..SubMesh::default() };
    for t in triangles {
        let n = face_normal(t[0].0, t[1].0, t[2].0).normalize_or_zero();
        for (p, uv) in t {
            s.indices.push(s.positions.len() as u32);
            s.positions.push(p.to_array());
            s.normals.push(n.to_array());
            if with_uvs {
                s.uvs.push(uv.to_array());
            }
        }
    }
    s
}

// ---- OBJ --------------------------------------------------------------------------------------------------

fn floats<const N: usize>(mut it: std::str::SplitWhitespace) -> Option<[f32; N]> {
    let mut out = [0.0; N];
    for slot in &mut out {
        let v: f32 = it.next()?.parse().ok()?;
        if !v.is_finite() {
            return None;
        }
        *slot = v;
    }
    Some(out)
}

/// A negative index counts back from the end; indices start at 1.
fn obj_index(s: &str, len: usize) -> Option<usize> {
    let i: i64 = s.parse().ok()?;
    let i = if i < 0 { len as i64 + i } else { i - 1 };
    (i >= 0 && (i as usize) < len).then_some(i as usize)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Shade {
    /// The `vn` the file names.
    Explicit(usize),
    /// Averaged over the faces that share the position inside one smoothing group.
    Smooth(u32),
    /// The face's own normal.
    Flat(usize),
}

struct Corner {
    pos: usize,
    uv: Option<usize>,
    shade: Shade,
}

fn parse_obj(bytes: &[u8], dir: &str, read_sibling: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<MeshData, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut positions: Vec<Vec3> = Vec::new();
    let mut uvs: Vec<Vec2> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    // Triangles per material (None: before any `usemtl`), in the order materials first appear.
    let mut order: Vec<Option<String>> = vec![None];
    let mut groups: HashMap<Option<String>, Vec<[Corner; 3]>> = HashMap::new();
    let mut current: Option<String> = None;
    let mut smoothing = 0u32;
    let mut libraries: Vec<String> = Vec::new();
    let mut face_counter = 0usize;

    // Lines that end in a backslash continue on the next one.
    let mut joined = String::new();
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        if let Some(stripped) = raw.strip_suffix('\\') {
            joined.push_str(stripped);
            joined.push(' ');
        } else {
            joined.push_str(raw);
            lines.push(std::mem::take(&mut joined));
        }
    }
    for line in &lines {
        let line = line.split('#').next().unwrap_or("");
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                if let Some([x, y, z]) = floats::<3>(it) {
                    positions.push(Vec3::new(x, y, z));
                }
            }
            Some("vt") => {
                if let Some([u, v]) = floats::<2>(it) {
                    uvs.push(Vec2::new(u, 1.0 - v));
                }
            }
            Some("vn") => {
                if let Some([x, y, z]) = floats::<3>(it) {
                    normals.push(Vec3::new(x, y, z).normalize_or_zero());
                }
            }
            Some("s") => {
                smoothing = match it.next() {
                    Some("off") | Some("0") | None => 0,
                    Some(n) => n.parse().unwrap_or(0),
                };
            }
            Some("mtllib") => libraries.extend(line.trim_start()[6..].split_whitespace().map(str::to_owned)),
            Some("usemtl") => {
                let name = line.trim_start()[6..].trim().to_owned();
                current = Some(name);
                if !order.contains(&current) {
                    order.push(current.clone());
                }
            }
            Some("f") => {
                let corners: Vec<Corner> = it
                    .filter_map(|tok| {
                        let mut parts = tok.split('/');
                        let pos = obj_index(parts.next()?, positions.len())?;
                        let uv = parts.next().filter(|s| !s.is_empty()).and_then(|s| obj_index(s, uvs.len()));
                        let normal = parts.next().filter(|s| !s.is_empty()).and_then(|s| obj_index(s, normals.len()));
                        let shade = match normal {
                            Some(n) => Shade::Explicit(n),
                            None if smoothing != 0 => Shade::Smooth(smoothing),
                            None => Shade::Flat(face_counter),
                        };
                        Some(Corner { pos, uv, shade })
                    })
                    .collect();
                face_counter += 1;
                for k in 1..corners.len().saturating_sub(1) {
                    let c = |i: usize| Corner { pos: corners[i].pos, uv: corners[i].uv, shade: match corners[i].shade { Shade::Flat(_) => Shade::Flat(face_counter * 1_000_000 + k), s => s } };
                    // (A polygon's triangles each get their own flat normal, so a bent polygon is not smeared.)
                    groups.entry(current.clone()).or_default().push([c(0), c(k), c(k + 1)]);
                }
            }
            _ => {}
        }
    }
    if groups.is_empty() {
        return Err("the file has no faces".into());
    }

    // Area-weighted normals for the smooth corners.
    let mut smooth: HashMap<(usize, u32), Vec3> = HashMap::new();
    for tris in groups.values() {
        for t in tris {
            let n = face_normal(positions[t[0].pos], positions[t[1].pos], positions[t[2].pos]);
            for corner in t {
                if let Shade::Smooth(g) = corner.shade {
                    *smooth.entry((corner.pos, g)).or_insert(Vec3::ZERO) += n;
                }
            }
        }
    }

    // Materials from the MTL files, by name.
    let mut materials: Vec<ImportedMaterial> = Vec::new();
    let mut material_index: HashMap<String, usize> = HashMap::new();
    for lib in &libraries {
        let Some(bytes) = join_relative(dir, lib).and_then(|_| read_sibling(lib)) else { continue };
        for m in parse_mtl(&String::from_utf8_lossy(&bytes), dir) {
            material_index.entry(m.name.clone()).or_insert_with(|| {
                materials.push(m);
                materials.len() - 1
            });
        }
    }

    let mut submeshes = Vec::new();
    for key in order {
        let Some(tris) = groups.get(&key) else { continue };
        let material = key.as_ref().and_then(|n| material_index.get(n).copied());
        let mut s = SubMesh { material, ..SubMesh::default() };
        let mut seen: HashMap<(usize, Option<usize>, Shade), u32> = HashMap::new();
        let has_uvs = tris.iter().all(|t| t.iter().all(|c| c.uv.is_some()));
        for t in tris {
            let flat = face_normal(positions[t[0].pos], positions[t[1].pos], positions[t[2].pos]).normalize_or_zero();
            for corner in t {
                let id = *seen.entry((corner.pos, corner.uv, corner.shade)).or_insert_with(|| {
                    let n = match corner.shade {
                        Shade::Explicit(i) => normals[i],
                        Shade::Smooth(g) => smooth.get(&(corner.pos, g)).copied().unwrap_or(flat).normalize_or_zero(),
                        Shade::Flat(_) => flat,
                    };
                    let n = if n == Vec3::ZERO { flat } else { n };
                    s.positions.push(positions[corner.pos].to_array());
                    s.normals.push(n.to_array());
                    if has_uvs {
                        s.uvs.push(corner.uv.map_or([0.0, 0.0], |i| uvs[i].to_array()));
                    }
                    (s.positions.len() - 1) as u32
                });
                s.indices.push(id);
            }
        }
        submeshes.push(s);
    }
    Ok(MeshData { submeshes, materials, ..MeshData::default() })
}

/// The pictures and numbers of the materials in an MTL file.
pub fn parse_mtl(text: &str, dir: &str) -> Vec<ImportedMaterial> {
    let mut out: Vec<ImportedMaterial> = Vec::new();
    // MTL says "how shiny" as a Phong exponent; the renderer wants roughness (the usual conversion).
    let rough_from_exponent = |ns: f32| (2.0 / (ns.clamp(0.0, 1000.0) + 2.0)).sqrt().clamp(0.04, 1.0);
    let map_path = |rest: &str| -> Option<TextureRef> {
        // Options (`-bm 1.0`, `-s 1 1 1`, `-clamp on`) come first; the name is the rest, and may hold spaces.
        let mut tokens: Vec<&str> = rest.split_whitespace().collect();
        let mut i = 0;
        while i < tokens.len() && tokens[i].starts_with('-') {
            let args = match tokens[i] {
                "-s" | "-o" | "-t" => 3,
                "-mm" => 2,
                "-bm" | "-clamp" | "-blendu" | "-blendv" | "-cc" | "-boost" | "-texres" | "-imfchan" | "-type" => 1,
                _ => 0,
            };
            i += 1 + args;
        }
        tokens.drain(..i.min(tokens.len()));
        let name = tokens.join(" ");
        (!name.is_empty()).then(|| join_relative(dir, &name)).flatten().map(TextureRef::File)
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let (key, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(k, r)| (k, r.trim()));
        if key == "newmtl" {
            out.push(ImportedMaterial { name: rest.to_owned(), ..ImportedMaterial::default() });
            continue;
        }
        let Some(m) = out.last_mut() else { continue };
        let nums = |n: usize| -> Option<Vec<f32>> {
            let v: Vec<f32> = rest.split_whitespace().take(n).filter_map(|t| t.parse().ok()).collect();
            (v.len() == n && v.iter().all(|x| x.is_finite())).then_some(v)
        };
        match key.to_ascii_lowercase().as_str() {
            "kd" => {
                if let Some(v) = nums(3) {
                    m.base_color[..3].copy_from_slice(&v);
                }
            }
            "ke" => {
                if let Some(v) = nums(3) {
                    m.emissive.copy_from_slice(&v);
                }
            }
            "ns" => {
                if let Some(v) = nums(1) {
                    m.roughness = rough_from_exponent(v[0]);
                }
            }
            "pr" => {
                if let Some(v) = nums(1) {
                    m.roughness = v[0].clamp(0.0, 1.0);
                }
            }
            "pm" => {
                if let Some(v) = nums(1) {
                    m.metallic = v[0].clamp(0.0, 1.0);
                }
            }
            "d" => {
                if let Some(v) = nums(1) {
                    m.base_color[3] = v[0].clamp(0.0, 1.0);
                }
            }
            "tr" => {
                if let Some(v) = nums(1) {
                    m.base_color[3] = (1.0 - v[0]).clamp(0.0, 1.0);
                }
            }
            "map_kd" => m.base_color_map = map_path(rest),
            "map_bump" | "bump" | "norm" | "map_kn" => m.normal_map = map_path(rest),
            "map_ke" => m.emissive_map = map_path(rest),
            "map_pr" => m.roughness_map = map_path(rest),
            "map_pm" => m.metallic_map = map_path(rest),
            "map_ao" => m.occlusion_map = map_path(rest),
            _ => {}
        }
    }
    for m in &mut out {
        if m.base_color[3] < 1.0 {
            m.alpha = AlphaKind::Blend;
        }
        // A material with an emission colour but no brightness stays dark; one with a colour glows.
        if m.emissive.iter().any(|v| *v > 0.0) && m.emissive_map.is_none() {
            m.emissive = m.emissive.map(|v| v.clamp(0.0, 16.0));
        }
    }
    out
}

// ---- STL --------------------------------------------------------------------------------------------------

fn parse_stl(bytes: &[u8]) -> Result<MeshData, String> {
    let binary = bytes.len() >= 84 && {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        bytes.len() == 84 + count * 50
    };
    let mut triangles: Vec<[(Vec3, Vec2); 3]> = Vec::new();
    if binary {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        for i in 0..count {
            let base = 84 + i * 50 + 12;
            let v = |k: usize| (Vec3::new(f(base + k * 12), f(base + k * 12 + 4), f(base + k * 12 + 8)), Vec2::ZERO);
            triangles.push([v(0), v(1), v(2)]);
        }
    } else {
        let text = String::from_utf8_lossy(bytes);
        if !text.trim_start().to_ascii_lowercase().starts_with("solid") {
            return Err("not an STL file (neither binary nor a text 'solid')".into());
        }
        let mut current: Vec<Vec3> = Vec::new();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            if it.next().is_some_and(|t| t.eq_ignore_ascii_case("vertex")) {
                if let Some([x, y, z]) = floats::<3>(it) {
                    current.push(Vec3::new(x, y, z));
                }
                if current.len() == 3 {
                    triangles.push([(current[0], Vec2::ZERO), (current[1], Vec2::ZERO), (current[2], Vec2::ZERO)]);
                    current.clear();
                }
            }
        }
    }
    Ok(MeshData { submeshes: vec![flat_submesh(&triangles, None, false)], ..MeshData::default() })
}

// ---- PLY --------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum PlyType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl PlyType {
    fn parse(s: &str) -> Option<PlyType> {
        Some(match s {
            "char" | "int8" => PlyType::I8,
            "uchar" | "uint8" => PlyType::U8,
            "short" | "int16" => PlyType::I16,
            "ushort" | "uint16" => PlyType::U16,
            "int" | "int32" => PlyType::I32,
            "uint" | "uint32" => PlyType::U32,
            "float" | "float32" => PlyType::F32,
            "double" | "float64" => PlyType::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            PlyType::I8 | PlyType::U8 => 1,
            PlyType::I16 | PlyType::U16 => 2,
            PlyType::I32 | PlyType::U32 | PlyType::F32 => 4,
            PlyType::F64 => 8,
        }
    }
}

struct PlyProp {
    name: String,
    ty: PlyType,
    /// For a list: the type of its length.
    list: Option<PlyType>,
}

struct PlyElement {
    name: String,
    count: usize,
    props: Vec<PlyProp>,
}

fn parse_ply(bytes: &[u8]) -> Result<MeshData, String> {
    let header_end = bytes.windows(10).position(|w| w == b"end_header").ok_or("not a PLY file (no end_header)")?;
    let after = &bytes[header_end..];
    let newline = after.iter().position(|b| *b == b'\n').ok_or("the PLY header is cut off")?;
    let body = &after[newline + 1..];
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    if !header.trim_start().starts_with("ply") {
        return Err("not a PLY file".into());
    }
    let (mut format, mut elements): (String, Vec<PlyElement>) = (String::new(), Vec::new());
    for line in header.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        match t.as_slice() {
            ["format", f, ..] => format = (*f).to_owned(),
            ["element", name, count] => elements.push(PlyElement { name: (*name).to_owned(), count: count.parse().map_err(|_| "a PLY element has a bad count")?, props: Vec::new() }),
            ["property", "list", count_ty, item_ty, name] => {
                let e = elements.last_mut().ok_or("a PLY property comes before its element")?;
                e.props.push(PlyProp { name: (*name).to_owned(), ty: PlyType::parse(item_ty).ok_or("unknown PLY type")?, list: Some(PlyType::parse(count_ty).ok_or("unknown PLY type")?) });
            }
            ["property", ty, name] => {
                let e = elements.last_mut().ok_or("a PLY property comes before its element")?;
                e.props.push(PlyProp { name: (*name).to_owned(), ty: PlyType::parse(ty).ok_or("unknown PLY type")?, list: None });
            }
            _ => {}
        }
    }
    let ascii = format == "ascii";
    let big = format == "binary_big_endian";
    if !ascii && format != "binary_little_endian" && !big {
        return Err(format!("unknown PLY format '{format}'"));
    }

    // One value at a time, from the text or the bytes.
    struct Reader<'a> {
        ascii: bool,
        big: bool,
        tokens: std::str::SplitWhitespace<'a>,
        bytes: &'a [u8],
        pos: usize,
    }
    impl Reader<'_> {
        fn next(&mut self, ty: PlyType) -> Result<f64, String> {
            if self.ascii {
                return self.tokens.next().and_then(|t| t.parse::<f64>().ok()).ok_or_else(|| "the PLY data ends too soon".to_owned());
            }
            let n = ty.size();
            let raw = self.bytes.get(self.pos..self.pos + n).ok_or("the PLY data ends too soon")?;
            self.pos += n;
            let mut b = [0u8; 8];
            b[..n].copy_from_slice(raw);
            if self.big {
                b[..n].reverse();
            }
            Ok(match ty {
                PlyType::I8 => f64::from(b[0] as i8),
                PlyType::U8 => f64::from(b[0]),
                PlyType::I16 => f64::from(i16::from_le_bytes([b[0], b[1]])),
                PlyType::U16 => f64::from(u16::from_le_bytes([b[0], b[1]])),
                PlyType::I32 => f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                PlyType::U32 => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                PlyType::F32 => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                PlyType::F64 => f64::from_le_bytes(b),
            })
        }
    }
    let text = if ascii { String::from_utf8_lossy(body).into_owned() } else { String::new() };
    let mut r = Reader { ascii, big, tokens: text.split_whitespace(), bytes: body, pos: 0 };

    let mut verts: Vec<(Vec3, Option<Vec3>, Option<Vec2>)> = Vec::new();
    let mut faces: Vec<Vec<u32>> = Vec::new();
    for e in &elements {
        for _ in 0..e.count {
            let (mut p, mut n, mut uv) = ([0f32; 3], [0f32; 3], [0f32; 2]);
            let (mut has_n, mut has_uv) = (false, false);
            let mut face: Vec<u32> = Vec::new();
            for prop in &e.props {
                if let Some(count_ty) = prop.list {
                    let count = r.next(count_ty)? as usize;
                    if count > 1_000_000 {
                        return Err("a PLY face has too many corners".into());
                    }
                    let items: Vec<u32> = (0..count).map(|_| r.next(prop.ty).map(|v| v as u32)).collect::<Result<_, _>>()?;
                    if e.name == "face" && (prop.name == "vertex_indices" || prop.name == "vertex_index") {
                        face = items;
                    }
                    continue;
                }
                let v = r.next(prop.ty)? as f32;
                if e.name == "vertex" {
                    match prop.name.as_str() {
                        "x" => p[0] = v,
                        "y" => p[1] = v,
                        "z" => p[2] = v,
                        "nx" => (n[0], has_n) = (v, true),
                        "ny" => n[1] = v,
                        "nz" => n[2] = v,
                        "u" | "s" | "texture_u" | "texture_s" => (uv[0], has_uv) = (v, true),
                        "v" | "t" | "texture_v" | "texture_t" => uv[1] = 1.0 - v,
                        _ => {}
                    }
                }
            }
            match e.name.as_str() {
                "vertex" => verts.push((Vec3::from(p), has_n.then(|| Vec3::from(n).normalize_or_zero()), has_uv.then(|| Vec2::from(uv)))),
                "face" => faces.push(face),
                _ => {}
            }
        }
    }
    if verts.is_empty() || faces.is_empty() {
        return Err("the PLY file has no vertices or no faces".into());
    }
    let with_normals = verts.iter().all(|v| v.1.is_some_and(|n| n != Vec3::ZERO));
    let with_uvs = verts.iter().all(|v| v.2.is_some());
    let mut s = SubMesh::default();
    if with_normals {
        for (p, n, uv) in &verts {
            s.positions.push(p.to_array());
            s.normals.push(n.unwrap_or(Vec3::Y).to_array());
            if with_uvs {
                s.uvs.push(uv.unwrap_or(Vec2::ZERO).to_array());
            }
        }
        for f in &faces {
            if f.iter().any(|i| *i as usize >= verts.len()) {
                continue;
            }
            for k in 1..f.len().saturating_sub(1) {
                s.indices.extend([f[0], f[k], f[k + 1]]);
            }
        }
        Ok(MeshData { submeshes: vec![s], ..MeshData::default() })
    } else {
        let mut triangles: Vec<[(Vec3, Vec2); 3]> = Vec::new();
        for f in &faces {
            if f.iter().any(|i| *i as usize >= verts.len()) {
                continue;
            }
            let at = |i: u32| (verts[i as usize].0, verts[i as usize].2.unwrap_or(Vec2::ZERO));
            for k in 1..f.len().saturating_sub(1) {
                triangles.push([at(f[0]), at(f[k]), at(f[k + 1])]);
            }
        }
        Ok(MeshData { submeshes: vec![flat_submesh(&triangles, None, with_uvs)], ..MeshData::default() })
    }
}

// ---- glTF / GLB -------------------------------------------------------------------------------------------

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The bytes a URI names: a `data:` URI, or a file next to the model.
fn gltf_uri(uri: &str, dir: &str, read_sibling: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (_, data) = rest.split_once(";base64,")?;
        return base64_decode(data);
    }
    join_relative(dir, uri)?;
    read_sibling(uri)
}

fn parse_gltf(bytes: &[u8], dir: &str, read_sibling: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<MeshData, String> {
    let glb = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not a valid glTF file: {e}"))?;
    let doc = &glb.document;
    // The buffers: the GLB's own chunk, data URIs, or files next to the model.
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for b in doc.buffers() {
        let data = match b.source() {
            gltf::buffer::Source::Bin => glb.blob.clone().ok_or("the glTF names its binary chunk but the file has none")?,
            gltf::buffer::Source::Uri(uri) => gltf_uri(uri, dir, read_sibling).ok_or_else(|| format!("the glTF buffer '{uri}' cannot be read"))?,
        };
        buffers.push(data);
    }

    let texture_ref = |tex: gltf::Texture| -> Option<TextureRef> {
        let image = tex.source();
        match image.source() {
            gltf::image::Source::Uri { uri, .. } => {
                if uri.starts_with("data:") {
                    gltf_uri(uri, dir, read_sibling).map(|bytes| TextureRef::Embedded { name: format!("image{}", image.index()), bytes: Arc::new(bytes) })
                } else {
                    join_relative(dir, uri).map(TextureRef::File)
                }
            }
            gltf::image::Source::View { view, .. } => {
                let data = buffers.get(view.buffer().index())?;
                let slice = data.get(view.offset()..view.offset() + view.length())?;
                Some(TextureRef::Embedded { name: format!("image{}", image.index()), bytes: Arc::new(slice.to_vec()) })
            }
        }
    };

    let mut materials: Vec<ImportedMaterial> = Vec::new();
    for m in doc.materials() {
        let pbr = m.pbr_metallic_roughness();
        let mut out = ImportedMaterial {
            name: m.name().unwrap_or("").to_owned(),
            base_color: pbr.base_color_factor(),
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            emissive: m.emissive_factor(),
            base_color_map: pbr.base_color_texture().and_then(|t| texture_ref(t.texture())),
            metallic_roughness_map: pbr.metallic_roughness_texture().and_then(|t| texture_ref(t.texture())),
            normal_map: m.normal_texture().and_then(|t| texture_ref(t.texture())),
            occlusion_map: m.occlusion_texture().and_then(|t| texture_ref(t.texture())),
            emissive_map: m.emissive_texture().and_then(|t| texture_ref(t.texture())),
            alpha: match m.alpha_mode() {
                gltf::material::AlphaMode::Opaque => AlphaKind::Opaque,
                gltf::material::AlphaMode::Mask => AlphaKind::Mask,
                gltf::material::AlphaMode::Blend => AlphaKind::Blend,
            },
            alpha_cutoff: m.alpha_cutoff().unwrap_or(0.5),
            double_sided: m.double_sided(),
            unlit: m.unlit(),
            ..ImportedMaterial::default()
        };
        if let Some(strength) = m.emissive_strength() {
            out.emissive = out.emissive.map(|v| v * strength);
        }
        materials.push(out);
    }

    // Walk the scene: every mesh instance, placed by its node's transform.
    let mut placed: Vec<(gltf::Mesh, Mat4)> = Vec::new();
    fn walk<'a>(node: gltf::Node<'a>, parent: Mat4, out: &mut Vec<(gltf::Mesh<'a>, Mat4)>, depth: usize) {
        if depth > 64 {
            return;
        }
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            out.push((mesh, world));
        }
        for child in node.children() {
            walk(child, world, out, depth + 1);
        }
    }
    match doc.default_scene().or_else(|| doc.scenes().next()) {
        Some(scene) => scene.nodes().for_each(|n| walk(n, Mat4::IDENTITY, &mut placed, 0)),
        None => doc.meshes().for_each(|m| placed.push((m, Mat4::IDENTITY))),
    }

    // One submesh per material: primitives that share one are merged.
    let mut by_material: Vec<(Option<usize>, SubMesh)> = Vec::new();
    for (mesh, world) in placed {
        let normal_matrix = Mat3::from_mat4(world).inverse().transpose();
        let flip = Mat3::from_mat4(world).determinant() < 0.0;
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
            let Some(pos) = reader.read_positions() else { continue };
            let positions: Vec<Vec3> = pos.map(|p| world.transform_point3(Vec3::from(p))).collect();
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            if indices.iter().any(|i| *i as usize >= positions.len()) {
                return Err("a glTF mesh has an index past its last vertex".into());
            }
            let mat = prim.material().index();
            let slot = match by_material.iter().position(|(m, _)| *m == mat) {
                Some(i) => i,
                None => {
                    by_material.push((mat, SubMesh { material: mat, ..SubMesh::default() }));
                    by_material.len() - 1
                }
            };
            let s = &mut by_material[slot].1;
            let base = s.positions.len() as u32;
            let normals: Vec<Vec3> = match reader.read_normals() {
                Some(n) => n.map(|n| (normal_matrix * Vec3::from(n)).normalize_or_zero()).collect(),
                None => {
                    // No normals: flat ones from the triangles.
                    let mut n = vec![Vec3::ZERO; positions.len()];
                    for t in indices.chunks_exact(3) {
                        let f = face_normal(positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]);
                        for i in t {
                            n[*i as usize] += f;
                        }
                    }
                    n.into_iter().map(Vec3::normalize_or_zero).collect()
                }
            };
            let uvs: Option<Vec<[f32; 2]>> = reader.read_tex_coords(0).map(|c| c.into_f32().collect());
            let tangents: Option<Vec<[f32; 4]>> = reader.read_tangents().map(|t| t.map(|t| (world.transform_vector3(Vec3::new(t[0], t[1], t[2])).normalize_or_zero().extend(if flip { -t[3] } else { t[3] })).to_array()).collect());
            s.positions.extend(positions.iter().map(|p| p.to_array()));
            s.normals.extend(normals.iter().map(|n| n.to_array()));
            // (A merged submesh keeps its coordinates and tangents only if every piece has them.)
            let had = s.positions.len() as u32 - positions.len() as u32;
            match &uvs {
                Some(u) if s.uvs.len() as u32 == had => s.uvs.extend(u.iter().copied()),
                _ => s.uvs.clear(),
            }
            match &tangents {
                Some(t) if s.tangents.len() as u32 == had && !s.uvs.is_empty() => s.tangents.extend(t.iter().copied()),
                _ => s.tangents.clear(),
            }
            for t in indices.chunks_exact(3) {
                if flip {
                    s.indices.extend([base + t[0], base + t[2], base + t[1]]);
                } else {
                    s.indices.extend([base + t[0], base + t[1], base + t[2]]);
                }
            }
        }
    }
    let _ = (Vec4::ZERO, Vec2::ZERO);
    Ok(MeshData { submeshes: by_material.into_iter().map(|(_, s)| s).collect(), materials, ..MeshData::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_files(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn obj(text: &str) -> MeshData {
        parse("obj", text.as_bytes(), "", &no_files, 1.0).unwrap()
    }

    const CUBE_QUADS: &str = "v 0 0 0\nv 2 0 0\nv 2 3 0\nv 0 3 0\nv 0 0 4\nv 2 0 4\nv 2 3 4\nv 0 3 4\n\
        f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

    #[test]
    fn a_model_has_the_size_it_was_made_with_and_is_centred_on_its_middle() {
        let m = obj(CUBE_QUADS);
        assert_eq!(m.size, Vec3::new(2.0, 3.0, 4.0), "not squeezed into a unit box");
        assert_eq!(m.center, Vec3::new(1.0, 1.5, 2.0));
        assert_eq!(m.triangle_count(), 12, "six quads");
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in &m.submeshes[0].positions {
            lo = lo.min(Vec3::from(*p));
            hi = hi.max(Vec3::from(*p));
        }
        assert_eq!((lo, hi), (Vec3::new(-1.0, -1.5, -2.0), Vec3::new(1.0, 1.5, 2.0)), "the box is centred on the origin");
    }

    #[test]
    fn the_unit_scale_turns_file_units_into_studs() {
        let m = parse("obj", CUBE_QUADS.as_bytes(), "", &no_files, 3.5).unwrap();
        assert_eq!(m.size, Vec3::new(7.0, 10.5, 14.0));
        assert!(parse("obj", CUBE_QUADS.as_bytes(), "", &no_files, 0.0).is_err());
        assert!(parse("obj", CUBE_QUADS.as_bytes(), "", &no_files, f32::NAN).is_err());
    }

    #[test]
    fn a_model_without_normals_is_shaded_flat_and_its_faces_point_outward() {
        let m = obj(CUBE_QUADS);
        let s = &m.submeshes[0];
        for t in s.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(s.positions[i as usize]));
            let n = Vec3::from(s.normals[t[0] as usize]);
            assert!((n.length() - 1.0).abs() < 1e-4);
            assert!((b - a).cross(c - a).normalize().dot(n) > 0.99, "the normal agrees with the winding");
            assert!(n.dot(a) > 0.0, "and points away from the middle (the cube is wound outward)");
        }
    }

    #[test]
    fn explicit_normals_and_texture_coordinates_are_read_with_every_index_style() {
        let m = obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nvn 0 0 1\nf 1/1/1 2/2/1 3/3/1\n");
        let s = &m.submeshes[0];
        assert_eq!(s.triangle_count(), 1);
        assert_eq!(s.normals[0], [0.0, 0.0, 1.0]);
        assert_eq!(&s.uvs[..3], &[[0.0, 1.0], [1.0, 1.0], [0.0, 0.0]], "v is flipped to the renderer's convention");
        assert!(!s.generated_uvs);
        // Negative indices count back from the end.
        let m = obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nf -3//-1 -2//-1 -1//-1\n");
        assert_eq!(m.submeshes[0].normals[2], [0.0, 0.0, 1.0]);
        // No texture coordinates at all: a box projection, so a texture still lands.
        assert!(m.submeshes[0].generated_uvs && m.submeshes[0].uvs.len() == 3);
    }

    #[test]
    fn smoothing_groups_average_normals_and_hard_edges_stay_hard() {
        // Two triangles meeting at a right angle along the edge v1-v2.
        let faces = "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\n";
        let hard = obj(&format!("{faces}s off\nf 1 2 3\nf 1 4 2\n"));
        let soft = obj(&format!("{faces}s 1\nf 1 2 3\nf 1 4 2\n"));
        assert_eq!(hard.submeshes[0].positions.len(), 6, "flat: nothing is shared");
        assert_eq!(soft.submeshes[0].positions.len(), 4, "smooth: the shared corners are one vertex");
        let shared = soft.submeshes[0].normals.iter().map(|n| Vec3::from(*n)).find(|n| n.y.abs() > 0.1 && n.z.abs() > 0.1);
        assert!(shared.is_some_and(|n| (n.length() - 1.0).abs() < 1e-4), "an averaged, unit normal");
    }

    #[test]
    fn bad_lines_are_dropped_without_shifting_the_others_and_nothing_to_draw_is_an_error() {
        let m = obj("v 0 0 0\nv 1 x 3 4\nv 1 1 0\nv 0 1 0\nf 1 2 3\nf 1 2 3 4\n");
        assert_eq!(m.triangle_count(), 2, "three valid vertices: one triangle, and a quad that lost its bad corner");
        assert!(parse("obj", b"# nothing", "", &no_files, 1.0).is_err());
        assert!(parse("obj", b"v nan 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n", "", &no_files, 1.0).is_err(), "positions must be numbers");
        assert!(parse("obj", b"v 0 0 0\nf 1 2 3\n", "", &no_files, 1.0).is_err(), "indices past the end are dropped, leaving no triangles");
    }

    #[test]
    fn faces_are_grouped_by_material_and_the_mtl_is_read() {
        let mtl = "newmtl Red\nKd 1 0 0\nNs 100\nmap_Kd textures/red tile.png\nnewmtl Glass\nKd 0.5 0.5 0.5\nd 0.25\nmap_Bump -bm 0.5 n.png\nPm 1\nKe 1 1 0\n";
        let files = move |name: &str| (name == "scene.mtl").then(|| mtl.as_bytes().to_vec());
        let text = "mtllib scene.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 0 0 1\nusemtl Red\nf 1 2 3\nusemtl Glass\nf 1 3 4\nusemtl Red\nf 1 2 4\n";
        let m = parse("obj", text.as_bytes(), "Assets/Models", &files, 1.0).unwrap();
        assert_eq!(m.submeshes.len(), 2, "one submesh per material, however the faces are interleaved");
        assert_eq!(m.materials.len(), 2);
        let red = &m.materials[m.submeshes[0].material.unwrap()];
        assert_eq!(red.name, "Red");
        assert_eq!(red.base_color, [1.0, 0.0, 0.0, 1.0]);
        assert!(red.roughness > 0.1 && red.roughness < 0.2, "a Phong exponent of 100 is fairly shiny: {}", red.roughness);
        assert_eq!(red.base_color_map, Some(TextureRef::File("Assets/Models/textures/red tile.png".into())), "paths are relative to the model, spaces allowed");
        assert_eq!(m.submeshes[0].triangle_count(), 2);
        let glass = &m.materials[m.submeshes[1].material.unwrap()];
        assert_eq!((glass.base_color[3], glass.alpha, glass.metallic, glass.emissive), (0.25, AlphaKind::Blend, 1.0, [1.0, 1.0, 0.0]));
        assert_eq!(glass.normal_map, Some(TextureRef::File("Assets/Models/n.png".into())), "map options are skipped");
        assert!(m.has_materials());
    }

    #[test]
    fn a_texture_path_cannot_leave_the_project() {
        let mtl = "newmtl A\nmap_Kd ../../../../secret.png\nnewmtl B\nmap_Kd C:/secret.png\nnewmtl C\nmap_Kd /etc/passwd\n";
        let found = parse_mtl(mtl, "Assets");
        assert!(found.iter().all(|m| m.base_color_map.is_none()), "{found:?}");
        assert_eq!(join_relative("Assets/Models", "../Textures/a.png").as_deref(), Some("Assets/Textures/a.png"));
        assert_eq!(join_relative("A", "x%20y.png").as_deref(), Some("A/x y.png"));
    }

    #[test]
    fn stl_binary_and_text_give_flat_triangles() {
        let text = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 4 0 0\nvertex 0 2 0\nendloop\nendfacet\nendsolid t\n";
        let m = parse("stl", text.as_bytes(), "", &no_files, 1.0).unwrap();
        assert_eq!((m.triangle_count(), m.size.x, m.size.y), (1, 4.0, 2.0));
        let mut bin = vec![0u8; 80];
        bin.extend_from_slice(&1u32.to_le_bytes());
        bin.extend_from_slice(&[0u8; 12]);
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 5.0]] {
            for c in v {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        bin.extend_from_slice(&[0u8; 2]);
        let m = parse("stl", &bin, "", &no_files, 1.0).unwrap();
        assert_eq!((m.triangle_count(), m.size.z), (1, 5.0));
        assert!(parse("stl", b"garbage", "", &no_files, 1.0).is_err());
    }

    #[test]
    fn ply_ascii_and_binary_are_read_including_quads_and_normals() {
        let ascii = "ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n2 0 0\n2 2 0\n0 2 0\n4 0 1 2 3\n";
        let m = parse("ply", ascii.as_bytes(), "", &no_files, 1.0).unwrap();
        assert_eq!((m.triangle_count(), m.size), (2, Vec3::new(2.0, 2.0, 1e-4)));
        let mut bin = b"ply\nformat binary_little_endian 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nelement face 1\nproperty list uchar uint vertex_indices\nend_header\n".to_vec();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in v.iter().chain([0.0, 0.0, 1.0].iter()) {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        bin.push(3);
        for i in [0u32, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let m = parse("ply", &bin, "", &no_files, 1.0).unwrap();
        assert_eq!(m.submeshes[0].normals[0], [0.0, 0.0, 1.0]);
        assert!(parse("ply", b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nend_header\n", "", &no_files, 1.0).is_err());
    }

    fn b64(bytes: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | u32::from(*b) << (16 - 8 * i));
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn base64_round_trips() {
        for sample in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 7, 8, 9]] {
            assert_eq!(base64_decode(&b64(sample)).unwrap(), sample);
        }
        assert!(base64_decode("@@@").is_none());
    }

    /// A one-triangle glTF in a data URI: positions, normals, texture coordinates, a material with a colour factor, and a
    /// node that moves and scales it.
    fn gltf_text() -> String {
        let mut data: Vec<u8> = Vec::new();
        for v in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in &v[..if data.len() >= 72 { 2 } else { 3 }] {
                data.extend_from_slice(&c.to_le_bytes());
            }
        }
        format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"mesh":0,"translation":[10,0,0],"scale":[2,2,2]}}],
            "materials":[{{"name":"Paint","pbrMetallicRoughness":{{"baseColorFactor":[0.2,0.4,0.6,1],"metallicFactor":0.7,"roughnessFactor":0.3}},"emissiveFactor":[0,1,0]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2}},"material":0}}]}}],
            "accessors":[
              {{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
              {{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"}},
              {{"bufferView":2,"componentType":5126,"count":3,"type":"VEC2"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":36}},{{"buffer":0,"byteOffset":72,"byteLength":24}}],
            "buffers":[{{"byteLength":96,"uri":"data:application/octet-stream;base64,{}"}}]}}"#,
            b64(&data)
        )
    }

    #[test]
    fn gltf_is_read_with_its_node_transform_its_material_and_its_texture_coordinates() {
        let m = parse("gltf", gltf_text().as_bytes(), "", &no_files, 1.0).unwrap();
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.size, Vec3::new(2.0, 2.0, 1e-4), "scaled by the node");
        assert_eq!(m.center, Vec3::new(11.0, 1.0, 0.0), "moved by the node: the model is centred afterwards");
        let mat = &m.materials[0];
        assert_eq!((mat.name.as_str(), mat.base_color, mat.metallic, mat.roughness, mat.emissive), ("Paint", [0.2, 0.4, 0.6, 1.0], 0.7, 0.3, [0.0, 1.0, 0.0]));
        assert_eq!(m.submeshes[0].material, Some(0));
        assert_eq!(m.submeshes[0].uvs.len(), 3);
        assert!(!m.submeshes[0].generated_uvs);
        assert!(parse("gltf", b"{ not gltf", "", &no_files, 1.0).is_err());
    }

    #[test]
    fn a_mesh_becomes_a_bevy_mesh_with_tangents_for_normal_maps() {
        let m = obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nf 1/1 2/2 3/3\n");
        let mesh = m.submeshes[0].to_bevy();
        assert_eq!(mesh.count_vertices(), 3);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some(), "normal maps need tangents");
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some() && mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
    }

    #[test]
    fn unsupported_and_unknown_formats_say_what_to_do() {
        assert!(unsupported_reason("a.fbx").unwrap().contains("glTF"));
        assert!(unsupported_reason("a.obj").is_none());
        assert!(is_loadable("A.GLB") && is_loadable("x.obj") && !is_loadable("x.fbx") && !is_loadable("x.png"));
        let err = load(&GameSource::Dir(std::env::temp_dir()), "nothing/a.fbx", 1.0).unwrap_err();
        assert!(err.contains("export"));
        assert!(load(&GameSource::Dir(std::env::temp_dir()), "../a.obj", 1.0).is_err(), "paths cannot leave the project");
    }
}

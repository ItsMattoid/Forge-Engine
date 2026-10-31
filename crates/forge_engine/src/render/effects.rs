//! Effects that move on their own: particles (`ParticleEmitter`, `Fire`, `Smoke`, `Sparkles`), `Beam` and `Trail`.
//!
//! Each of them is one entity per instance with a mesh that is rebuilt every frame from the effect's state (quads that face
//! the camera, or a ribbon). The classes only say *what* they want (an [`EmitterSpec`], a [`BeamSpec`], a [`TrailSpec`]) from
//! their properties; this module does the simulation and the drawing, so all of them look and behave alike. The state lives
//! in the asset cache next to the instance (`RenderCtx::fx_state`) and goes with it.
//!
//! The mesh is drawn unlit with vertex colours. Blending: `LightEmission` of half or more adds the light to what is behind
//! (flames, sparks, beams of light); less than that blends it over what is behind (smoke, dust).

use std::collections::VecDeque;

use bevy::{
    asset::RenderAssetUsages,
    camera::primitives::Aabb,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::RenderCtx;
use crate::{
    assets::material::SamplerSettings,
    compat,
    core::{Id, Value},
    dm::DataModel,
};

/// The most particles one emitter keeps alive.
pub const MAX_PARTICLES: usize = 4000;
/// A frame longer than this (a stall, a breakpoint) is simulated as this long.
const MAX_STEP: f32 = 0.1;

// ---- random numbers --------------------------------------------------------------------------------

/// A small, fast, repeatable random number generator (xorshift64*): effects do not need more.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// A number in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let bits = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
        bits as f32 / (1u64 << 24) as f32
    }

    /// A number between the two (either order).
    pub fn between(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }

    pub fn range(&mut self, (a, b): (f32, f32)) -> f32 {
        self.between(a, b)
    }

    /// A direction, evenly spread over the sphere.
    pub fn direction(&mut self) -> Vec3 {
        let z = self.between(-1.0, 1.0);
        let a = self.between(0.0, std::f32::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), r * a.sin(), z)
    }
}

// ---- sequences -------------------------------------------------------------------------------------

/// A `NumberSequence`: a value that changes over a life (0 at birth, 1 at the end).
#[derive(Clone, Debug, PartialEq)]
pub struct NumberSeq(Vec<(f32, f32)>);

impl NumberSeq {
    pub fn constant(v: f32) -> NumberSeq {
        NumberSeq(vec![(0.0, v)])
    }

    /// From `(time, value)` points in order of time.
    pub fn from_points(points: &[(f32, f32)]) -> NumberSeq {
        NumberSeq(points.to_vec())
    }

    /// Reads a property made of `time, value, envelope` triples. A bad or missing one reads as `default`.
    pub fn read(dm: &DataModel, id: Id, name: &str, default: f32) -> NumberSeq {
        let mut points: Vec<(f32, f32)> = numbers(dm, id, name).chunks_exact(3).map(|c| (c[0], c[1])).collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        if points.is_empty() {
            points.push((0.0, default));
        }
        NumberSeq(points)
    }

    pub fn at(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let p = &self.0;
        if t <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            if t <= w[1].0 {
                let span = (w[1].0 - w[0].0).max(1e-6);
                return w[0].1 + (w[1].1 - w[0].1) * ((t - w[0].0) / span);
            }
        }
        p[p.len() - 1].1
    }
}

/// A `ColorSequence`: a colour (sRGB) that changes over a life.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorSeq(Vec<(f32, Vec3)>);

impl ColorSeq {
    pub fn constant(c: Vec3) -> ColorSeq {
        ColorSeq(vec![(0.0, c)])
    }

    pub fn from_points(points: &[(f32, Vec3)]) -> ColorSeq {
        ColorSeq(points.to_vec())
    }

    /// Reads a property made of `time, r, g, b` quadruples. A bad or missing one reads as white.
    pub fn read(dm: &DataModel, id: Id, name: &str) -> ColorSeq {
        let mut points: Vec<(f32, Vec3)> = numbers(dm, id, name).chunks_exact(4).map(|c| (c[0], Vec3::new(c[1], c[2], c[3]))).collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        if points.is_empty() {
            points.push((0.0, Vec3::ONE));
        }
        ColorSeq(points)
    }

    pub fn at(&self, t: f32) -> Vec3 {
        let t = t.clamp(0.0, 1.0);
        let p = &self.0;
        if t <= p[0].0 {
            return p[0].1;
        }
        for w in p.windows(2) {
            if t <= w[1].0 {
                let span = (w[1].0 - w[0].0).max(1e-6);
                return w[0].1.lerp(w[1].1, (t - w[0].0) / span);
            }
        }
        p[p.len() - 1].1
    }
}

fn numbers(dm: &DataModel, id: Id, name: &str) -> Vec<f32> {
    match dm.get_prop(id, name) {
        Some(Value::Data(_, n)) => n,
        _ => Vec::new(),
    }
}

/// A `NumberRange` property as `(smallest, largest)`.
pub fn read_range(dm: &DataModel, id: Id, name: &str, default: (f32, f32)) -> (f32, f32) {
    match numbers(dm, id, name).as_slice() {
        [a, b, ..] => (a.min(*b), a.max(*b)),
        _ => default,
    }
}

// ---- geometry --------------------------------------------------------------------------------------

/// The triangles of one frame of an effect, with a colour (linear, with alpha) per corner.
#[derive(Default)]
pub struct Batch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// A colour in sRGB and an opacity, as a vertex colour.
pub fn vertex_color(srgb: Vec3, alpha: f32) -> [f32; 4] {
    Color::srgba(srgb.x.clamp(0.0, 8.0), srgb.y.clamp(0.0, 8.0), srgb.z.clamp(0.0, 8.0), alpha.clamp(0.0, 1.0)).to_linear().to_f32_array()
}

impl Batch {
    pub fn clear(&mut self) {
        self.positions.clear();
        self.normals.clear();
        self.uvs.clear();
        self.colors.clear();
        self.indices.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    fn corner(&mut self, p: Vec3, n: Vec3, uv: Vec2, color: [f32; 4]) {
        self.positions.push(p.to_array());
        self.normals.push(n.to_array());
        self.uvs.push(uv.to_array());
        self.colors.push(color);
    }

    /// A quad: corners in order around it (so `uv` and `colors` follow the same order).
    pub fn quad(&mut self, p: [Vec3; 4], uv: [Vec2; 4], colors: [[f32; 4]; 4]) {
        let first = self.positions.len() as u32;
        let n = (p[1] - p[0]).cross(p[3] - p[0]).normalize_or_zero();
        for i in 0..4 {
            self.corner(p[i], n, uv[i], colors[i]);
        }
        self.indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
    }

    /// The bounds of everything in the batch.
    fn bounds(&self) -> Option<Aabb> {
        let mut it = self.positions.iter().map(|p| Vec3::from_array(*p));
        let first = it.next()?;
        let (min, max) = it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
        Some(Aabb::from_min_max(min, max))
    }

    fn write(&self, mesh: &mut Mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.clone());
        mesh.insert_indices(Indices::U32(self.indices.clone()));
    }
}

// ---- pictures --------------------------------------------------------------------------------------

/// The pictures effects draw when they have none of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Procedural {
    /// A round puff with soft edges.
    Soft,
    /// A four-pointed glint.
    Star,
    /// Solid white (beams and trails).
    Plain,
}

impl Procedural {
    fn name(self) -> &'static str {
        match self {
            Procedural::Soft => "soft",
            Procedural::Star => "star",
            Procedural::Plain => "plain",
        }
    }

    fn pixel(self, x: f32, y: f32) -> f32 {
        let (x, y) = (x * 2.0 - 1.0, y * 2.0 - 1.0);
        match self {
            Procedural::Plain => 1.0,
            Procedural::Soft => {
                let r = (x * x + y * y).sqrt();
                let t = (1.0 - r).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            }
            Procedural::Star => {
                let (ax, ay) = (x.abs(), y.abs());
                let star = (1.0 - (ax.sqrt() + ay.sqrt())).clamp(0.0, 1.0);
                let r = (x * x + y * y).sqrt();
                let core = (1.0 - r * 2.5).clamp(0.0, 1.0);
                (star * star + core).clamp(0.0, 1.0)
            }
        }
    }

    fn image(self) -> Image {
        const EDGE: usize = 64;
        let mut data = Vec::with_capacity(EDGE * EDGE * 4);
        for j in 0..EDGE {
            for i in 0..EDGE {
                let a = self.pixel((i as f32 + 0.5) / EDGE as f32, (j as f32 + 0.5) / EDGE as f32);
                data.extend_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
            }
        }
        Image::new(Extent3d { width: EDGE as u32, height: EDGE as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
    }
}

/// What an effect's pictures and blending are.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    /// A project picture ("" for the effect's own).
    pub texture: String,
    pub procedural: Procedural,
    /// Light is added to what is behind instead of blended over it.
    pub additive: bool,
    /// A picture the effect made itself (it is used instead of the two above).
    pub custom: Option<Handle<Image>>,
}

impl Look {
    /// `emission` is the effect's `LightEmission`.
    pub fn new(texture: &str, procedural: Procedural, emission: f32) -> Look {
        Look { texture: texture.to_owned(), procedural, additive: emission >= 0.5, custom: None }
    }
}

/// The entity an effect draws into, and what it was last given.
pub struct Layer {
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    look: Look,
    epoch: u32,
}

fn material_for(ctx: &mut RenderCtx, look: &Look) -> StandardMaterial {
    let picture = if let Some(own) = &look.custom {
        Some(own.clone())
    } else if look.texture.is_empty() {
        None
    } else {
        ctx.assets.texture(&look.texture, true, &SamplerSettings::default(), ctx.source)
    };
    let picture = picture.unwrap_or_else(|| {
        let cache = &mut ctx.assets.cache;
        let images = &mut ctx.assets.images;
        cache.fx_textures.entry(look.procedural.name()).or_insert_with(|| images.add(look.procedural.image())).clone()
    });
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(picture),
        unlit: true,
        alpha_mode: if look.additive { AlphaMode::Add } else { AlphaMode::Blend },
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// Shows `batch` as the effect of instance `id` (an empty batch hides it).
pub fn publish(ctx: &mut RenderCtx, id: Id, slot: &mut Option<Layer>, name: &str, batch: &Batch, look: &Look) {
    let epoch = ctx.assets.cache.fx_epoch;
    if batch.is_empty() {
        if let Some(layer) = slot.as_ref() {
            compat::insert(ctx.commands, layer.entity, Visibility::Hidden);
        }
        return;
    }
    if slot.as_ref().is_some_and(|l| l.look != *look || l.epoch != epoch) {
        let material = material_for(ctx, look);
        if let Some(layer) = slot.as_mut() {
            if let Some(mut current) = ctx.assets.fx_materials.get_mut(&layer.material) {
                *current = material;
            }
            layer.look = look.clone();
            layer.epoch = epoch;
        }
    }
    match slot {
        Some(layer) => {
            if let Some(mut mesh) = ctx.assets.meshes.get_mut(&layer.mesh) {
                batch.write(&mut mesh);
            }
            if let Some(aabb) = batch.bounds() {
                compat::insert(ctx.commands, layer.entity, (aabb, Visibility::Visible));
            }
        }
        None => {
            let material = material_for(ctx, look);
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            batch.write(&mut mesh);
            let (mesh, material) = (ctx.assets.meshes.add(mesh), ctx.assets.fx_materials.add(material));
            let aabb = batch.bounds().unwrap_or(Aabb::from_min_max(Vec3::ZERO, Vec3::ZERO));
            let entity = ctx
                .commands
                .spawn((Name::new(name.to_owned()), Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::IDENTITY, Visibility::Visible, NotShadowCaster, NotShadowReceiver, aabb))
                .id();
            ctx.fx_own(id, entity);
            *slot = Some(Layer { entity, mesh, material, look: look.clone(), epoch });
        }
    }
}

/// Where the part or attachment an effect belongs to is, if it is in the world: the effect shows only then.
pub fn parent_frame(dm: &DataModel, parent: Id) -> Option<Frame> {
    if !dm.in_workspace(parent) {
        return None;
    }
    let on_part = dm.is_a(parent, "BasePart");
    if !on_part && !dm.is_a(parent, "Attachment") {
        return None;
    }
    let (position, rotation) = dm.world_cframe(parent)?;
    Some(Frame { position, rotation, size: if on_part { dm.size(parent) } else { Vec3::ZERO } })
}

// ---- particles -------------------------------------------------------------------------------------

/// How the quad of a particle is turned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    Camera,
    CameraWorldUp,
    VelocityParallel,
    VelocityPerpendicular,
}

impl Facing {
    pub fn from_name(name: &str) -> Facing {
        match name {
            "FacingCameraWorldUp" => Facing::CameraWorldUp,
            "VelocityParallel" => Facing::VelocityParallel,
            "VelocityPerpendicular" => Facing::VelocityPerpendicular,
            _ => Facing::Camera,
        }
    }
}

/// Where in the part (or at the attachment) particles are born.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Area {
    /// At the origin (an attachment).
    Point,
    /// Over the face with this outward normal (in the part's own axes).
    Face(Vec3),
    /// Anywhere inside the part, scaled about its centre by this much (0: the centre; 1: the whole part).
    Volume(f32),
    /// Anywhere on the part's surface, up to this many studs outside it.
    Surface(f32),
}

/// Which way particles leave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Heading {
    /// In the part's own axes.
    Local(Vec3),
    /// In the world's.
    World(Vec3),
}

/// Everything about one emitter that the class decides from its properties.
#[derive(Clone, Debug)]
pub struct EmitterSpec {
    pub enabled: bool,
    pub rate: f32,
    pub lifetime: (f32, f32),
    pub speed: (f32, f32),
    pub heading: Heading,
    /// The most a particle's direction may differ from the heading, in degrees, either way (x: one axis, y: the other).
    pub spread: Vec2,
    pub acceleration: Vec3,
    pub drag: f32,
    pub size: NumberSeq,
    pub color: ColorSeq,
    pub transparency: NumberSeq,
    pub rotation: (f32, f32),
    pub rot_speed: (f32, f32),
    pub z_offset: f32,
    pub facing: Facing,
    pub locked_to_part: bool,
    pub time_scale: f32,
    pub inherit_velocity: f32,
    pub area: Area,
    pub look: Look,
}

/// The part (or attachment) the particles come from: where it is and how big.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub position: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
}

#[derive(Clone, Debug)]
struct Particle {
    /// World position (local to the frame when the emitter is locked to its part).
    position: Vec3,
    velocity: Vec3,
    age: f32,
    life: f32,
    roll: f32,
    spin: f32,
}

/// What an emitter keeps between frames.
#[derive(Default)]
pub struct EmitterState {
    particles: Vec<Particle>,
    carry: f32,
    emitted_seen: f32,
    cleared_seen: f32,
    rng: Option<Rng>,
    last_position: Option<Vec3>,
    velocity: Vec3,
    batch: Batch,
    layer: Option<Layer>,
}

impl EmitterState {
    pub fn alive(&self) -> usize {
        self.particles.len()
    }
}

/// A direction that differs from `heading` by up to `spread` degrees on each of two axes (half the sphere or more: any direction).
pub fn spread_direction(heading: Vec3, spread: Vec2, rng: &mut Rng) -> Vec3 {
    let heading = heading.normalize_or(Vec3::Y);
    if spread.max_element() >= 179.0 {
        return rng.direction();
    }
    if spread.max_element() <= 0.0 {
        return heading;
    }
    let (u, v) = heading.any_orthonormal_pair();
    let (a, b) = (rng.between(-spread.x, spread.x).to_radians(), rng.between(-spread.y, spread.y).to_radians());
    (Quat::from_axis_angle(u, a) * Quat::from_axis_angle(v, b) * heading).normalize()
}

fn birth_point(area: Area, size: Vec3, rng: &mut Rng) -> Vec3 {
    match area {
        Area::Point => Vec3::ZERO,
        Area::Volume(f) => Vec3::new(rng.between(-0.5, 0.5), rng.between(-0.5, 0.5), rng.between(-0.5, 0.5)) * size * f,
        Area::Surface(margin) => {
            // A face, more likely the bigger it is; then a point on it, and a little way out.
            let areas = Vec3::new(size.y * size.z, size.x * size.z, size.x * size.y);
            let pick = rng.unit() * (areas.x + areas.y + areas.z).max(1e-6);
            let axis = if pick < areas.x {
                0
            } else if pick < areas.x + areas.y {
                1
            } else {
                2
            };
            let mut normal = Vec3::ZERO;
            normal[axis] = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
            let mut p = birth_point(Area::Face(normal), size, rng);
            p += normal * rng.between(0.0, margin.max(0.0));
            p
        }
        Area::Face(normal) => {
            let mut p = Vec3::ZERO;
            for axis in 0..3 {
                p[axis] = if normal[axis].abs() > 0.5 { normal[axis].signum() * size[axis] * 0.5 } else { rng.between(-0.5, 0.5) * size[axis] };
            }
            p
        }
    }
}

/// The two edges of a particle's quad and the way it faces.
fn axes_for(facing: Facing, position: Vec3, velocity: Vec3, camera: Vec3, roll: f32) -> (Vec3, Vec3) {
    let to_camera = (camera - position).normalize_or(Vec3::Z);
    let (right, up) = match facing {
        Facing::Camera => {
            let right = Vec3::Y.cross(to_camera).try_normalize().unwrap_or(Vec3::X);
            (right, to_camera.cross(right))
        }
        Facing::CameraWorldUp => {
            let flat = Vec3::new(to_camera.x, 0.0, to_camera.z).try_normalize().unwrap_or(Vec3::Z);
            (Vec3::Y.cross(flat).try_normalize().unwrap_or(Vec3::X), Vec3::Y)
        }
        Facing::VelocityParallel => {
            let along = velocity.try_normalize().unwrap_or(Vec3::Y);
            let across = to_camera.cross(along).try_normalize().unwrap_or(Vec3::X);
            return (along, across);
        }
        Facing::VelocityPerpendicular => {
            let normal = velocity.try_normalize().unwrap_or(Vec3::Y);
            let (a, b) = normal.any_orthonormal_pair();
            (a, b)
        }
    };
    let (sin, cos) = roll.sin_cos();
    (right * cos + up * sin, up * cos - right * sin)
}

/// Advances an emitter by the frame's time and draws it. `emitted` and `cleared` count the calls of `Emit` and `Clear` so far.
pub fn run_emitter(ctx: &mut RenderCtx, id: Id, name: &str, spec: &EmitterSpec, frame: Frame, emitted: f32, cleared: f32) {
    let mut st: EmitterState = std::mem::take(ctx.fx_state(id));
    let step = ctx.opts.dt.min(MAX_STEP);
    let dt = step * spec.time_scale.max(0.0);
    let camera = ctx.opts.camera.unwrap_or(frame.position + Vec3::Z * 64.0);
    let rng = st.rng.get_or_insert_with(|| Rng::new(u64::from(id.slot) << 32 | u64::from(id.uid)));
    let mut rng = rng.clone();

    if cleared != st.cleared_seen {
        st.cleared_seen = cleared;
        st.particles.clear();
    }
    if ctx.opts.dt > 0.0 {
        if let Some(before) = st.last_position {
            st.velocity = (frame.position - before) / ctx.opts.dt;
        }
        st.last_position = Some(frame.position);
    }

    // Birth.
    let mut born = 0usize;
    if spec.enabled && dt > 0.0 {
        st.carry += spec.rate.max(0.0) * dt;
        let whole = st.carry.floor();
        st.carry -= whole;
        born += whole as usize;
    }
    if emitted > st.emitted_seen {
        born += (emitted - st.emitted_seen) as usize;
    }
    st.emitted_seen = emitted;
    let room = MAX_PARTICLES.saturating_sub(st.particles.len());
    for _ in 0..born.min(room) {
        let local = birth_point(spec.area, frame.size, &mut rng);
        let heading = match spec.heading {
            Heading::Local(h) => h,
            Heading::World(h) => frame.rotation.inverse() * h,
        };
        let direction = spread_direction(heading, spec.spread, &mut rng);
        let speed = rng.range(spec.speed);
        let (mut position, velocity) =
            if spec.locked_to_part { (local, direction * speed) } else { (frame.position + frame.rotation * local, frame.rotation * direction * speed + st.velocity * spec.inherit_velocity) };
        // Born somewhere inside the frame, not all at its start.
        let head_start = rng.unit() * dt;
        position += velocity * head_start;
        st.particles.push(Particle {
            position,
            velocity,
            age: head_start,
            life: rng.range(spec.lifetime).max(0.01),
            roll: rng.range(spec.rotation).to_radians(),
            spin: rng.range(spec.rot_speed).to_radians(),
        });
    }

    // Life.
    let push = if spec.locked_to_part { frame.rotation.inverse() * spec.acceleration } else { spec.acceleration };
    let slow = (-spec.drag.max(0.0) * dt).exp();
    for p in &mut st.particles {
        if dt > 0.0 {
            p.age += dt;
            p.velocity = (p.velocity + push * dt) * slow;
            p.position += p.velocity * dt;
            p.roll += p.spin * dt;
        }
    }
    st.particles.retain(|p| p.age < p.life);

    // Drawing: the far ones first, so the near ones blend over them.
    let world = |p: &Particle| if spec.locked_to_part { frame.position + frame.rotation * p.position } else { p.position };
    let mut order: Vec<(f32, usize)> = st.particles.iter().enumerate().map(|(i, p)| (world(p).distance_squared(camera), i)).collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut batch = std::mem::take(&mut st.batch);
    batch.clear();
    for (_, i) in order {
        let p = &st.particles[i];
        let t = p.age / p.life;
        let size = spec.size.at(t);
        let alpha = 1.0 - spec.transparency.at(t);
        if size <= 0.0 || alpha <= 0.0 {
            continue;
        }
        let at = world(p);
        let velocity = if spec.locked_to_part { frame.rotation * p.velocity } else { p.velocity };
        let at = if spec.z_offset != 0.0 { at + (camera - at).normalize_or_zero() * spec.z_offset } else { at };
        let (right, up) = axes_for(spec.facing, at, velocity, camera, p.roll);
        let (r, u) = (right * size * 0.5, up * size * 0.5);
        let c = vertex_color(spec.color.at(t), alpha);
        batch.quad([at - r - u, at + r - u, at + r + u, at - r + u], [Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 0.0)], [c; 4]);
    }
    publish(ctx, id, &mut st.layer, name, &batch, &spec.look);
    st.batch = batch;
    st.rng = Some(rng);
    *ctx.fx_state(id) = st;
}

// ---- beams -----------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureMode {
    Stretch,
    Wrap,
    Static,
}

impl TextureMode {
    pub fn from_name(name: &str) -> TextureMode {
        match name {
            "Wrap" => TextureMode::Wrap,
            "Static" => TextureMode::Static,
            _ => TextureMode::Stretch,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BeamSpec {
    pub start: (Vec3, Quat),
    pub end: (Vec3, Quat),
    pub width: (f32, f32),
    pub segments: usize,
    pub curve: (f32, f32),
    pub color: ColorSeq,
    pub transparency: NumberSeq,
    pub face_camera: bool,
    pub z_offset: f32,
    pub texture_length: f32,
    pub texture_mode: TextureMode,
    pub texture_speed: f32,
    pub look: Look,
}

#[derive(Default)]
pub struct BeamState {
    time: f32,
    batch: Batch,
    layer: Option<Layer>,
}

/// The points along a beam: a curve from one attachment to the other that leaves each along its X axis.
pub fn beam_points(spec: &BeamSpec) -> Vec<Vec3> {
    let (p0, p3) = (spec.start.0, spec.end.0);
    let p1 = p0 + spec.start.1 * Vec3::X * spec.curve.0;
    let p2 = p3 + spec.end.1 * Vec3::X * spec.curve.1;
    let n = spec.segments.clamp(1, 300);
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
        })
        .collect()
}

/// Draws a beam as a ribbon.
pub fn run_beam(ctx: &mut RenderCtx, id: Id, name: &str, spec: &BeamSpec) {
    let mut st: BeamState = std::mem::take(ctx.fx_state(id));
    st.time += ctx.opts.dt.min(MAX_STEP);
    let camera = ctx.opts.camera.unwrap_or((spec.start.0 + spec.end.0) * 0.5 + Vec3::Z * 64.0);
    let points = beam_points(spec);
    let n = points.len() - 1;
    // How far along each point is, for a picture that repeats.
    let mut along = vec![0.0f32; points.len()];
    for i in 1..points.len() {
        along[i] = along[i - 1] + points[i].distance(points[i - 1]);
    }
    st.batch.clear();
    let mut edges: Vec<(Vec3, Vec3)> = Vec::with_capacity(points.len());
    for i in 0..points.len() {
        let t = i as f32 / n as f32;
        let tangent = (points[(i + 1).min(n)] - points[i.saturating_sub(1)]).try_normalize().unwrap_or(Vec3::X);
        let to_camera = (camera - points[i]).normalize_or(Vec3::Z);
        // Facing the camera: the band is across the view. Otherwise it is as wide along the attachments' Y axes.
        let across_view = tangent.cross(to_camera).try_normalize();
        let flat = spec.start.1.mul_vec3(Vec3::Y).lerp(spec.end.1.mul_vec3(Vec3::Y), t);
        let along_y = (flat - tangent * flat.dot(tangent)).try_normalize();
        let side = if spec.face_camera { across_view.or(along_y) } else { along_y.or(across_view) }.unwrap_or(Vec3::Y);
        let half = (spec.width.0 + (spec.width.1 - spec.width.0) * t).max(0.0) * 0.5;
        let lift = if spec.z_offset != 0.0 { to_camera * spec.z_offset } else { Vec3::ZERO };
        edges.push((points[i] + side * half + lift, points[i] - side * half + lift));
    }
    let scroll = st.time * spec.texture_speed;
    for i in 0..n {
        let (t0, t1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let u = |k: usize, t: f32| match spec.texture_mode {
            TextureMode::Stretch => t + scroll,
            TextureMode::Wrap | TextureMode::Static => along[k] / spec.texture_length.max(1e-3) + scroll,
        };
        let (c0, c1) = (vertex_color(spec.color.at(t0), 1.0 - spec.transparency.at(t0)), vertex_color(spec.color.at(t1), 1.0 - spec.transparency.at(t1)));
        st.batch.quad(
            [edges[i].0, edges[i + 1].0, edges[i + 1].1, edges[i].1],
            [Vec2::new(u(i, t0), 0.0), Vec2::new(u(i + 1, t1), 0.0), Vec2::new(u(i + 1, t1), 1.0), Vec2::new(u(i, t0), 1.0)],
            [c0, c1, c1, c0],
        );
    }
    let batch = std::mem::take(&mut st.batch);
    publish(ctx, id, &mut st.layer, name, &batch, &spec.look);
    st.batch = batch;
    *ctx.fx_state(id) = st;
}

// ---- trails ----------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TrailSpec {
    /// Where the two attachments are now.
    pub edges: (Vec3, Vec3),
    pub lifetime: f32,
    pub color: ColorSeq,
    pub transparency: NumberSeq,
    pub width_scale: NumberSeq,
    pub min_length: f32,
    pub max_length: f32,
    pub face_camera: bool,
    pub look: Look,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    edges: (Vec3, Vec3),
    age: f32,
}

#[derive(Default)]
pub struct TrailState {
    samples: VecDeque<Sample>,
    cleared_seen: f32,
    batch: Batch,
    layer: Option<Layer>,
}

impl TrailState {
    pub fn samples(&self) -> usize {
        self.samples.len()
    }
}

/// Keeps the trail up to date with where its edges are now and draws it.
pub fn run_trail(ctx: &mut RenderCtx, id: Id, name: &str, spec: &TrailSpec, enabled: bool, cleared: f32) {
    let mut st: TrailState = std::mem::take(ctx.fx_state(id));
    let dt = ctx.opts.dt.min(MAX_STEP);
    if cleared != st.cleared_seen {
        st.cleared_seen = cleared;
        st.samples.clear();
    }
    update_trail(&mut st, spec, enabled, dt);
    let camera = ctx.opts.camera.unwrap_or((spec.edges.0 + spec.edges.1) * 0.5 + Vec3::Z * 64.0);
    let mut batch = std::mem::take(&mut st.batch);
    build_trail(&st, spec, enabled, camera, &mut batch);
    publish(ctx, id, &mut st.layer, name, &batch, &spec.look);
    st.batch = batch;
    *ctx.fx_state(id) = st;
}

fn mid(e: (Vec3, Vec3)) -> Vec3 {
    (e.0 + e.1) * 0.5
}

fn update_trail(st: &mut TrailState, spec: &TrailSpec, enabled: bool, dt: f32) {
    for s in &mut st.samples {
        s.age += dt;
    }
    while st.samples.back().is_some_and(|s| s.age >= spec.lifetime) {
        st.samples.pop_back();
    }
    if !enabled {
        // A trail that is switched off fades away where it is and starts afresh when it comes back.
        return;
    }
    // A new piece once the edges have moved far enough from the last one.
    let far = st.samples.front().is_none_or(|s| mid(s.edges).distance(mid(spec.edges)) >= spec.min_length.max(1e-3));
    if far && spec.lifetime > 0.0 {
        st.samples.push_front(Sample { edges: spec.edges, age: 0.0 });
    }
}

fn build_trail(st: &TrailState, spec: &TrailSpec, enabled: bool, camera: Vec3, batch: &mut Batch) {
    batch.clear();
    // The live end first (where the edges are right now), then the pieces from newest to oldest.
    let mut points: Vec<((Vec3, Vec3), f32)> = Vec::with_capacity(st.samples.len() + 1);
    if enabled {
        points.push((spec.edges, 0.0));
    }
    points.extend(st.samples.iter().map(|s| (s.edges, s.age)));
    if points.len() < 2 || spec.lifetime <= 0.0 {
        return;
    }
    // No longer than allowed.
    if spec.max_length > 0.0 {
        let mut total = 0.0;
        let mut keep = 1;
        for i in 1..points.len() {
            total += mid(points[i].0).distance(mid(points[i - 1].0));
            if total > spec.max_length {
                break;
            }
            keep = i + 1;
        }
        points.truncate(keep);
    }
    let mut run = Vec::with_capacity(points.len());
    let mut walked = 0.0;
    for (i, (edges, age)) in points.iter().enumerate() {
        if i > 0 {
            walked += mid(*edges).distance(mid(points[i - 1].0));
        }
        let t = (age / spec.lifetime).clamp(0.0, 1.0);
        let (centre, mut half) = (mid(*edges), (edges.0 - edges.1) * 0.5);
        if spec.face_camera {
            let next = points[(i + 1).min(points.len() - 1)].0;
            let prev = points[i.saturating_sub(1)].0;
            let tangent = (mid(prev) - mid(next)).try_normalize().unwrap_or(Vec3::X);
            if let Some(side) = tangent.cross(camera - centre).try_normalize() {
                half = side * half.length();
            }
        }
        half *= spec.width_scale.at(t);
        run.push((centre + half, centre - half, t, walked));
    }
    let total = walked.max(1e-4);
    for w in run.windows(2) {
        let (a, b) = (w[0], w[1]);
        let color = |t: f32| vertex_color(spec.color.at(t), 1.0 - spec.transparency.at(t));
        let (ca, cb) = (color(a.2), color(b.2));
        batch.quad([a.0, b.0, b.1, a.1], [Vec2::new(a.3 / total, 0.0), Vec2::new(b.3 / total, 0.0), Vec2::new(b.3 / total, 1.0), Vec2::new(a.3 / total, 1.0)], [ca, cb, cb, ca]);
    }
}

// ---- clouds ----------------------------------------------------------------------------------------

/// How high the clouds are, and how far they reach, in studs. The camera's far plane is 10 000.
const CLOUD_HEIGHT: f32 = 350.0;
const CLOUD_REACH: f32 = 7000.0;
/// How far one copy of the cloud picture reaches, and how fast the wind moves it, in studs.
const CLOUD_TILE: f32 = 2500.0;
const CLOUD_WIND: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudSpec {
    /// How much of the sky is covered (0 to 1).
    pub cover: f32,
    /// How thick the clouds are (0 to 1).
    pub density: f32,
    pub color: Vec3,
}

#[derive(Default)]
pub struct CloudState {
    time: f32,
    picture: Option<((i32, i32), Handle<Image>)>,
    batch: Batch,
    layer: Option<Layer>,
}

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x85EB_CA6B) ^ (y as u32).wrapping_mul(0xC2B2_AE35) ^ 0x27D4_EB2F;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth noise that repeats every `period` cells.
fn value_noise(x: f32, y: f32, period: i32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |dx: i32, dy: i32| hash((ix + dx).rem_euclid(period), (iy + dy).rem_euclid(period));
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// The opacity of the cloud picture at `(u, v)` in `[0, 1)` (it repeats): more of it is cloud the higher `cover` is.
pub fn cloud_alpha(u: f32, v: f32, cover: f32, density: f32) -> f32 {
    let mut sum = 0.0;
    let mut amplitude = 0.5;
    let mut total = 0.0;
    for octave in 0..5 {
        let period = 4 << octave;
        sum += value_noise(u * period as f32, v * period as f32, period) * amplitude;
        total += amplitude;
        amplitude *= 0.5;
    }
    let n = sum / total;
    let threshold = 1.0 - cover.clamp(0.0, 1.0);
    let t = ((n - (threshold - 0.1)) / 0.25).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t) * density.clamp(0.0, 1.0)
}

fn cloud_image(cover: f32, density: f32) -> Image {
    use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
    const EDGE: usize = 256;
    let mut data = Vec::with_capacity(EDGE * EDGE * 4);
    for j in 0..EDGE {
        for i in 0..EDGE {
            let a = cloud_alpha(i as f32 / EDGE as f32, j as f32 / EDGE as f32, cover, density);
            data.extend_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
        }
    }
    let mut image = Image::new(Extent3d { width: EDGE as u32, height: EDGE as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat, ..ImageSamplerDescriptor::linear() });
    image
}

/// Draws the cloud layer: a very large sheet high over the camera that fades out toward the horizon and drifts with the wind.
pub fn run_clouds(ctx: &mut RenderCtx, id: Id, name: &str, spec: &CloudSpec) {
    let mut st: CloudState = std::mem::take(ctx.fx_state(id));
    st.time += ctx.opts.dt.min(MAX_STEP);
    let key = ((spec.cover * 100.0).round() as i32, (spec.density * 100.0).round() as i32);
    if st.picture.as_ref().is_none_or(|(k, _)| *k != key) {
        let image = ctx.assets.images.add(cloud_image(key.0 as f32 / 100.0, key.1 as f32 / 100.0));
        st.picture = Some((key, image));
    }
    let picture = st.picture.as_ref().map(|(_, h)| h.clone());
    let centre = ctx.opts.camera.unwrap_or(Vec3::ZERO);
    let wind = st.time * CLOUD_WIND / CLOUD_TILE;
    const CELLS: usize = 12;
    let step = 2.0 * CLOUD_REACH / CELLS as f32;
    // The sheet follows the camera, but the picture is laid on the world, so the clouds stay where they are.
    let corner = |i: usize, j: usize| {
        let (x, z) = (centre.x - CLOUD_REACH + i as f32 * step, centre.z - CLOUD_REACH + j as f32 * step);
        let r = ((x - centre.x).powi(2) + (z - centre.z).powi(2)).sqrt() / CLOUD_REACH;
        let fade = 1.0 - ((r - 0.35) / 0.65).clamp(0.0, 1.0);
        let fade = fade * fade * (3.0 - 2.0 * fade);
        (Vec3::new(x, CLOUD_HEIGHT, z), Vec2::new(x / CLOUD_TILE + wind, z / CLOUD_TILE), vertex_color(spec.color, fade))
    };
    st.batch.clear();
    for j in 0..CELLS {
        for i in 0..CELLS {
            let c = [corner(i, j), corner(i + 1, j), corner(i + 1, j + 1), corner(i, j + 1)];
            st.batch.quad([c[0].0, c[1].0, c[2].0, c[3].0], [c[0].1, c[1].1, c[2].1, c[3].1], [c[0].2, c[1].2, c[2].2, c[3].2]);
        }
    }
    let look = Look { texture: String::new(), procedural: Procedural::Plain, additive: false, custom: picture };
    let batch = std::mem::take(&mut st.batch);
    publish(ctx, id, &mut st.layer, name, &batch, &look);
    st.batch = batch;
    *ctx.fx_state(id) = st;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_numbers_are_in_range_and_repeatable() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        for _ in 0..1000 {
            let x = a.unit();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.unit());
        }
        let mut c = Rng::new(8);
        assert_ne!(Rng::new(7).unit(), c.unit());
        let d = Rng::new(3).direction();
        assert!((d.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn sequences_blend_between_their_points_and_hold_at_the_ends() {
        let n = NumberSeq(vec![(0.0, 1.0), (0.5, 3.0), (1.0, 0.0)]);
        assert_eq!(n.at(-1.0), 1.0);
        assert_eq!(n.at(0.25), 2.0);
        assert_eq!(n.at(0.5), 3.0);
        assert_eq!(n.at(0.75), 1.5);
        assert_eq!(n.at(2.0), 0.0);
        let c = ColorSeq::from_points(&[(0.0, Vec3::ZERO), (1.0, Vec3::ONE)]);
        assert_eq!(c.at(0.5), Vec3::splat(0.5));
        assert_eq!(ColorSeq::constant(Vec3::X).at(0.9), Vec3::X);
    }

    #[test]
    fn a_spread_keeps_directions_inside_the_cone() {
        let mut rng = Rng::new(1);
        let heading = Vec3::Y;
        for _ in 0..500 {
            let d = spread_direction(heading, Vec2::splat(20.0), &mut rng);
            assert!((d.length() - 1.0).abs() < 1e-4);
            assert!(d.angle_between(heading).to_degrees() <= 40.5, "{d:?}");
        }
        assert_eq!(spread_direction(heading, Vec2::ZERO, &mut rng), heading);
        let any: Vec<Vec3> = (0..50).map(|_| spread_direction(heading, Vec2::splat(180.0), &mut rng)).collect();
        assert!(any.iter().any(|d| d.y < 0.0), "a spread of 180 goes everywhere");
    }

    #[test]
    fn particles_are_born_on_the_face_they_leave_from() {
        let mut rng = Rng::new(2);
        let size = Vec3::new(4.0, 2.0, 6.0);
        for _ in 0..200 {
            let p = birth_point(Area::Face(Vec3::Y), size, &mut rng);
            assert_eq!(p.y, 1.0);
            assert!(p.x.abs() <= 2.0 && p.z.abs() <= 3.0);
            let q = birth_point(Area::Volume(0.5), size, &mut rng);
            assert!(q.x.abs() <= 1.0 && q.y.abs() <= 0.5 && q.z.abs() <= 1.5);
            // On the surface, up to half a stud out: on one of the faces, beyond or at it.
            let s = birth_point(Area::Surface(0.5), size, &mut rng);
            assert!(s.x.abs() <= 2.5 && s.y.abs() <= 1.5 && s.z.abs() <= 3.5);
            let beyond = [(s.x.abs() - 2.0), (s.y.abs() - 1.0), (s.z.abs() - 3.0)];
            assert!(beyond.iter().any(|b| *b >= -1e-4), "it is at or past one face: {s:?}");
        }
        assert_eq!(birth_point(Area::Point, size, &mut rng), Vec3::ZERO);
    }

    #[test]
    fn a_particle_quad_faces_the_camera_and_a_velocity_one_lies_along_the_velocity() {
        let (right, up) = axes_for(Facing::Camera, Vec3::ZERO, Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0), 0.0);
        assert!(right.dot(up).abs() < 1e-5 && right.cross(up).dot(Vec3::Z).abs() > 0.99, "{right:?} {up:?}");
        let (r2, u2) = axes_for(Facing::Camera, Vec3::ZERO, Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0), std::f32::consts::FRAC_PI_2);
        assert!((r2 - up).length() < 1e-4 && (u2 + right).length() < 1e-4, "a quarter turn swaps the edges");
        let (along, _) = axes_for(Facing::VelocityParallel, Vec3::ZERO, Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, 0.0, 10.0), 0.0);
        assert!((along - Vec3::Y).length() < 1e-5);
        let (a, b) = axes_for(Facing::VelocityPerpendicular, Vec3::ZERO, Vec3::new(0.0, 5.0, 0.0), Vec3::Z, 0.0);
        assert!(a.dot(Vec3::Y).abs() < 1e-5 && b.dot(Vec3::Y).abs() < 1e-5, "the quad's plane is across the velocity");
    }

    fn beam(curve: f32) -> BeamSpec {
        BeamSpec {
            start: (Vec3::ZERO, Quat::IDENTITY),
            end: (Vec3::new(10.0, 0.0, 0.0), Quat::IDENTITY),
            width: (1.0, 1.0),
            segments: 8,
            curve: (curve, curve),
            color: ColorSeq::constant(Vec3::ONE),
            transparency: NumberSeq::constant(0.0),
            face_camera: false,
            z_offset: 0.0,
            texture_length: 1.0,
            texture_mode: TextureMode::Stretch,
            texture_speed: 0.0,
            look: Look::new("", Procedural::Plain, 1.0),
        }
    }

    #[test]
    fn a_beam_runs_from_one_attachment_to_the_other_and_curves_when_asked() {
        let straight = beam_points(&beam(0.0));
        assert_eq!(straight.len(), 9);
        assert_eq!(straight[0], Vec3::ZERO);
        assert!((straight[8] - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-5);
        assert!(straight.iter().all(|p| p.y.abs() < 1e-5 && p.z.abs() < 1e-5), "no curve: a straight line");
        // Both ends leave along +X; a curve of 5 bends it (the ends lean the same way, the middle is pulled out of the line... along X only here).
        let mut bent = beam(0.0);
        bent.start.1 = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        bent.curve = (6.0, 0.0);
        let curved = beam_points(&bent);
        assert!(curved.iter().any(|p| p.y > 0.5), "the start leaves upward: {curved:?}");
    }

    fn trail_spec(at: f32) -> TrailSpec {
        TrailSpec {
            edges: (Vec3::new(at, 1.0, 0.0), Vec3::new(at, -1.0, 0.0)),
            lifetime: 1.0,
            color: ColorSeq::constant(Vec3::ONE),
            transparency: NumberSeq::constant(0.0),
            width_scale: NumberSeq::constant(1.0),
            min_length: 0.5,
            max_length: 0.0,
            face_camera: false,
            look: Look::new("", Procedural::Plain, 0.0),
        }
    }

    #[test]
    fn a_trail_grows_as_its_edges_move_and_loses_its_old_end_after_its_lifetime() {
        let mut st = TrailState::default();
        let mut batch = Batch::default();
        update_trail(&mut st, &trail_spec(0.0), true, 0.016);
        assert_eq!(st.samples(), 1);
        update_trail(&mut st, &trail_spec(0.1), true, 0.016);
        assert_eq!(st.samples(), 1, "it has not moved far enough for a new piece");
        for i in 1..=10 {
            update_trail(&mut st, &trail_spec(i as f32), true, 0.016);
        }
        assert_eq!(st.samples(), 11);
        build_trail(&st, &trail_spec(10.0), true, Vec3::new(0.0, 0.0, 50.0), &mut batch);
        assert_eq!(batch.triangle_count(), 2 * 11, "a quad per piece between the live end and the oldest piece");
        // Everything is older than the lifetime a second later.
        update_trail(&mut st, &trail_spec(10.0), false, 2.0);
        assert_eq!(st.samples(), 0);
        build_trail(&st, &trail_spec(10.0), false, Vec3::Z, &mut batch);
        assert!(batch.is_empty());
    }

    #[test]
    fn a_trail_is_never_longer_than_its_maximum() {
        let mut st = TrailState::default();
        for i in 0..10 {
            update_trail(&mut st, &trail_spec(i as f32), true, 0.01);
        }
        let mut limited = trail_spec(9.0);
        limited.max_length = 3.5;
        let mut batch = Batch::default();
        build_trail(&st, &limited, true, Vec3::Z, &mut batch);
        let farthest_back = batch.positions.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        assert!(farthest_back >= 9.0 - 3.5 - 1e-4, "{farthest_back}");
    }

    #[test]
    fn more_of_the_sky_is_cloud_the_higher_the_cover_and_the_thicker_the_density() {
        let covered = |cover: f32, density: f32| {
            let n = 64;
            (0..n * n).map(|i| cloud_alpha((i % n) as f32 / n as f32, (i / n) as f32 / n as f32, cover, density)).sum::<f32>() / (n * n) as f32
        };
        assert_eq!(covered(0.0, 1.0), 0.0, "no cover, no clouds");
        assert!(covered(0.3, 1.0) < covered(0.6, 1.0) && covered(0.6, 1.0) < covered(1.0, 1.0));
        assert!(covered(1.0, 1.0) > 0.9, "full cover is nearly all cloud");
        assert!(covered(0.6, 0.3) < covered(0.6, 1.0) * 0.5, "thin clouds let the sky through");
        // The picture repeats: its edges meet.
        assert!((cloud_alpha(0.0, 0.3, 0.5, 1.0) - cloud_alpha(1.0, 0.3, 0.5, 1.0)).abs() < 1e-4);
        assert!((cloud_alpha(0.3, 0.0, 0.5, 1.0) - cloud_alpha(0.3, 1.0, 0.5, 1.0)).abs() < 1e-4);
    }

    #[test]
    fn the_procedural_pictures_are_solid_in_the_middle_and_clear_at_the_corners() {
        for (which, corner_clear) in [(Procedural::Soft, true), (Procedural::Star, true), (Procedural::Plain, false)] {
            assert!(which.pixel(0.5, 0.5) > 0.9, "{which:?}");
            assert_eq!(which.pixel(0.0, 0.0) < 0.01, corner_clear, "{which:?}");
        }
        assert!(Procedural::Star.pixel(0.5, 0.05) > Procedural::Star.pixel(0.2, 0.2), "the points reach further than the diagonals");
        assert_eq!(Procedural::Soft.image().width(), 64);
    }
}

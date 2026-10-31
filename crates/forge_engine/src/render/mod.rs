//! Mirrors the data model into Bevy entities.
//!
//! The renderer itself is class-agnostic: for every instance that changed since
//! the last call it runs the `render` hook of each component the instance's class
//! uses (for example `PointLightEmitter` in `classes/world/lights/point_light.rs`
//! or `PrimitiveRenderer` in `classes/world/parts/part.rs`). Cost is proportional
//! to what actually changed.

pub mod cache;
pub mod camera_fx;
pub mod culling;
pub mod effects;
pub mod on_top;
pub mod part_material;
pub mod wireframe;

use bevy::prelude::*;

use crate::{assets::GameSource, compat, dm::DataModel, dm::Id};

pub use cache::{material_index, AssetCache, AssetStats, LoadRecord, MatKey, MatSource, Model, ModelPiece, SpawnParams};
pub use camera_fx::{CameraEffectFn, CameraEffects, CameraLook};
pub use culling::{Culling, CullingPlugin};
pub use on_top::{HighlightMaterial, OnTop};
pub use part_material::{make_material, MaterialImages, PartLook, PartMaterial, PartMaterialPlugin};
pub use wireframe::{WireframeView, WireframeViewPlugin};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// The real game: cameras are created, fully transparent parts are hidden.
    Game,
    /// The editor: no cameras (the editor has its own), transparent parts stay faintly visible.
    Editor,
    /// The game running inside the editor's viewport: it looks like the real game (fully
    /// transparent parts are hidden) but the editor keeps owning the camera.
    Play,
}

impl Mode {
    /// Whether fully transparent parts are hidden (as in the real game) instead of faintly drawn.
    pub fn hides_transparent(self) -> bool {
        matches!(self, Mode::Game | Mode::Play)
    }
}

#[derive(Clone, Copy)]
pub struct SyncOptions {
    pub mode: Mode,
    pub msaa: Msaa,
    /// Seconds since the last sync (what effects that move on their own - particles, trails - advance by).
    pub dt: f32,
    /// Where the camera is, for effects that face it (particles); None when it is not known.
    pub camera: Option<Vec3>,
}

impl SyncOptions {
    /// Options for a sync that is not part of a running frame (no time passes, no camera is known).
    pub fn still(mode: Mode, msaa: Msaa) -> SyncOptions {
        SyncOptions { mode, msaa, dt: 0.0, camera: None }
    }
}

pub fn msaa_from_samples(samples: u8) -> Msaa {
    match samples {
        0 | 1 => Msaa::Off,
        2 => Msaa::Sample2,
        8 => Msaa::Sample8,
        _ => Msaa::Sample4,
    }
}

/// What a component's render hook can use.
pub struct RenderCtx<'a, 'w, 's, 'x> {
    pub commands: &'a mut Commands<'w, 's>,
    pub assets: &'a mut SpawnParams<'x>,
    pub opts: SyncOptions,
    /// Where project files (meshes ...) are read from.
    pub source: &'a GameSource,
    /// New transforms of entities that already exist, applied together at the end of the sync.
    moves: Vec<(Entity, Transform)>,
}

impl RenderCtx<'_, '_, '_, '_> {
    /// Entities and state an effect keeps for an instance (see [AssetCache::fx_state]); gone with the instance.
    pub fn fx_state<T: Default + Send + Sync + 'static>(&mut self, id: Id) -> &mut T {
        self.assets.cache.fx_state(id)
    }

    /// Remembers an entity the effect of id made, so it is despawned when the instance goes (or the effect ends).
    pub fn fx_own(&mut self, id: Id, entity: Entity) {
        self.assets.cache.fx_entities.entry(id).or_default().push(entity);
    }

    /// Despawns one of the entities the effect of id made.
    pub fn fx_release(&mut self, id: Id, entity: Entity) {
        if let Some(list) = self.assets.cache.fx_entities.get_mut(&id) {
            list.retain(|e| *e != entity);
        }
        compat::despawn(self.commands, entity);
    }

    /// Despawns every entity the effect of id made, and forgets its state.
    pub fn fx_clear(&mut self, id: Id) {
        for e in self.assets.cache.fx_entities.remove(&id).unwrap_or_default() {
            compat::despawn(self.commands, e);
        }
        self.assets.cache.fx_states.remove(&id);
    }

    /// Clears the effects of instances that no longer exist.
    fn prune_effects(&mut self, dm: &DataModel) {
        let gone: Vec<Id> = self.assets.cache.fx_entities.keys().chain(self.assets.cache.fx_states.keys()).filter(|id| !dm.exists(**id)).copied().collect();
        for id in gone {
            self.fx_clear(id);
        }
    }


    /// Gives an existing entity a new transform. Moving things is by far the most common update, so
    /// these are collected and written in one command (a query-style write per entity, which only
    /// marks the transform changed when it really is different) instead of one `insert` command each.
    pub fn set_transform(&mut self, entity: Entity, transform: Transform) {
        self.moves.push((entity, transform));
    }
}

/// A component's render hook: (context, data model, instance, dirty bits).
pub type RenderFn = fn(&mut RenderCtx, &mut DataModel, Id, u8);

/// A component's animate hook: called every frame for each instance that has the component (effects that move on their own:
/// particles, trails, highlights that follow what they mark), with the frame's time and camera in ctx.opts.
pub type AnimateFn = fn(&mut RenderCtx, &mut DataModel, Id);

pub fn sync(dm: &mut DataModel, commands: &mut Commands, assets: &mut SpawnParams, opts: SyncOptions, source: &GameSource) {
    for e in dm.dead_entities.drain(..) {
        compat::despawn(commands, e);
    }
    // A material file, a picture or a model changed: every part looks at its look again (see AssetCache::invalidate).
    if assets.cache.take_stale() {
        dm.mark_all(crate::dm::D_X | crate::dm::D_V);
        assets.cache.forget_look();
    }
    // What the scene says about the cameras (screen effects, the sky): applied to them when it changes.
    let fx = camera_fx::collect(dm);
    if assets.cache.look_changed(&fx) {
        let look = camera_fx::resolve(&fx, assets, source);
        commands.insert_resource(look);
    }
    let dirty: Vec<Id> = std::mem::take(&mut dm.dirty);
    let mut ctx = RenderCtx { commands, assets, opts, source, moves: Vec::new() };
    for id in dirty {
        let Some((bits, class)) = dm.get_mut(id).map(|i| (std::mem::take(&mut i.dirty), i.class)) else { continue };
        for comp in &class.components {
            if let Some(render) = comp.render {
                render(&mut ctx, dm, id, bits);
            }
        }
    }
    // Effects that move on their own.
    let animated = dm.with_component("Animated");
    for id in animated {
        let Some(class) = dm.get(id).map(|i| i.class) else { continue };
        for comp in &class.components {
            if let Some(animate) = comp.animate {
                animate(&mut ctx, dm, id);
            }
        }
    }
    ctx.prune_effects(dm);
    if !ctx.moves.is_empty() {
        let moves = std::mem::take(&mut ctx.moves);
        ctx.commands.queue(move |world: &mut World| apply_moves(world, moves));
    }
}

/// Writes the transforms collected by [`RenderCtx::set_transform`]. An entity that is gone by now
/// (despawned by an earlier command) is skipped.
fn apply_moves(world: &mut World, moves: Vec<(Entity, Transform)>) {
    for (entity, transform) in moves {
        let Ok(mut entity) = world.get_entity_mut(entity) else { continue };
        match entity.get_mut::<Transform>() {
            Some(mut current) => {
                current.set_if_neq(transform);
            }
            None => {
                entity.insert(transform);
            }
        }
    }
}

// ---- helpers for render hooks ----------------------------------------------------------------

pub fn entity_of(dm: &DataModel, id: Id) -> Option<Entity> {
    dm.get(id).and_then(|i| i.entity)
}

pub fn set_entity(dm: &mut DataModel, id: Id, e: Option<Entity>) {
    if let Some(i) = dm.get_mut(id) {
        i.entity = e;
        if e.is_none() {
            i.drawn = None;
        }
    }
}

/// What a part's entity was last given. A refresh compares against this and only touches the
/// components that really changed: re-inserting the mesh or material handle, even an equal one,
/// makes Bevy treat the entity as changed and redo its render-side work.
#[derive(Clone, Copy)]
pub struct Drawn {
    pub transform: Transform,
    /// The mesh of the first piece (a model of several pieces: the first one's).
    pub mesh: AssetId<Mesh>,
    /// The size the mesh has before the part's `Size` scales it.
    pub native: Vec3,
    /// The number of the shared material (`AssetCache::material_id`); a model of several pieces: all of theirs combined.
    pub material: u32,
    /// The packed colour and opacity (the entity's `MeshTag`).
    pub tag: u32,
    pub casts_shadow: bool,
    /// The part is an entity with one child per piece (a model with materials of its own) rather than one entity.
    pub multi: bool,
}

/// What the instance's entity was last given, if the renderer recorded it.
pub fn drawn(dm: &DataModel, id: Id) -> Option<Drawn> {
    dm.get(id)?.drawn.as_ref()?.downcast_ref::<Drawn>().copied()
}

/// Records what the instance's entity now has.
pub fn set_drawn(dm: &mut DataModel, id: Id, now: Drawn) {
    let Some(inst) = dm.get_mut(id) else { return };
    match inst.drawn.as_mut().and_then(|b| b.downcast_mut::<Drawn>()) {
        Some(slot) => *slot = now,
        None => inst.drawn = Some(Box::new(now)),
    }
}

/// Removes the instance's entity, if it has one.
pub fn despawn_entity(ctx: &mut RenderCtx, dm: &mut DataModel, id: Id) {
    if let Some(e) = entity_of(dm, id) {
        compat::despawn(ctx.commands, e);
        set_entity(dm, id, None);
    }
}

pub fn srgb(v: Vec3) -> Color {
    Color::srgb(v.x, v.y, v.z)
}

#[cfg(test)]
mod tests {
    //! The render mirror against a real data model and a headless Bevy `World` (no window, no GPU):
    //! what an entity is given, and, as important, what it is *not* given again.

    use bevy::{
        camera::primitives::Aabb,
        ecs::{change_detection::Tick, system::SystemState},
        light::NotShadowCaster,
        mesh::MeshTag,
    };

    use super::*;
    use crate::{
        core::Value,
        dm::{D_V, D_X},
        render::part_material::unpack_tag,
    };

    struct Scene {
        world: World,
        dm: DataModel,
        source: GameSource,
        mode: Mode,
        /// The world's change tick when the latest sync began: what counts as "written by it".
        before_sync: Tick,
        /// Entities there are before any part exists.
        base: usize,
    }

    impl Scene {
        fn new() -> Scene {
            let mut world = World::new();
            world.init_resource::<Assets<Mesh>>();
            world.init_resource::<Assets<PartMaterial>>();
            world.init_resource::<Assets<Image>>();
            world.init_resource::<Assets<bevy::pbr::decal::ForwardDecalMaterial<bevy::pbr::StandardMaterial>>>();
            world.init_resource::<Assets<bevy::pbr::StandardMaterial>>();
            world.init_resource::<Assets<HighlightMaterial>>();
            world.init_resource::<AssetCache>();
            let before_sync = world.read_change_tick();
            let mut scene = Scene { world, dm: DataModel::new(), source: GameSource::Dir(std::env::temp_dir()), mode: Mode::Game, before_sync, base: 0 };
            scene.sync();
            scene.base = scene.world.iter_entities().count();
            scene
        }

        fn sync(&mut self) {
            self.before_sync = self.world.read_change_tick();
            let mut state = SystemState::<(Commands, SpawnParams)>::new(&mut self.world);
            {
                let (mut commands, mut spawn) = state.get_mut(&mut self.world).unwrap();
                sync(&mut self.dm, &mut commands, &mut spawn, SyncOptions::still(self.mode, Msaa::Off), &self.source);
            }
            state.apply(&mut self.world);
        }

        /// A sync that is one frame of a running game: `dt` seconds have passed and the camera looks from a little way off.
        fn frame(&mut self, dt: f32) {
            self.before_sync = self.world.read_change_tick();
            let mut state = SystemState::<(Commands, SpawnParams)>::new(&mut self.world);
            {
                let (mut commands, mut spawn) = state.get_mut(&mut self.world).unwrap();
                let opts = SyncOptions { mode: self.mode, msaa: Msaa::Off, dt, camera: Some(Vec3::new(0.0, 5.0, 30.0)) };
                sync(&mut self.dm, &mut commands, &mut spawn, opts, &self.source);
            }
            state.apply(&mut self.world);
        }

        fn frames(&mut self, count: usize) {
            for _ in 0..count {
                self.frame(1.0 / 60.0);
            }
        }

        /// The vertices of the mesh the effect entity of the given number (in the order they were made) draws; `None` when there is no such entity.
        fn effect_vertices(&mut self, n: usize) -> Option<usize> {
            let mut meshes: Vec<(Entity, AssetId<Mesh>)> = self.world.query_filtered::<(Entity, &Mesh3d), With<bevy::light::NotShadowReceiver>>().iter(&self.world).map(|(e, m)| (e, m.0.id())).collect();
            meshes.sort_by_key(|(e, _)| *e);
            let (_, mesh) = meshes.get(n)?;
            self.world.resource::<Assets<Mesh>>().get(*mesh).map(|m| m.count_vertices())
        }

        fn effect_visibility(&mut self, n: usize) -> Option<Visibility> {
            let mut found: Vec<(Entity, Visibility)> = self.world.query_filtered::<(Entity, &Visibility), With<bevy::light::NotShadowReceiver>>().iter(&self.world).map(|(e, v)| (e, *v)).collect();
            found.sort_by_key(|(e, _)| *e);
            found.get(n).map(|(_, v)| *v)
        }

        fn child(&mut self, parent: Id, class: &str) -> Id {
            let id = self.dm.create(class).unwrap();
            self.dm.set_parent(id, Some(parent)).unwrap();
            id
        }

        fn part(&mut self) -> Id {
            let ws = self.dm.workspace;
            let p = self.dm.create("Part").unwrap();
            self.dm.set_parent(p, Some(ws)).unwrap();
            p
        }

        fn set(&mut self, id: Id, prop: &str, v: Value) {
            self.dm.set_prop(id, prop, v).unwrap();
        }

        fn entity(&self, id: Id) -> Entity {
            entity_of(&self.dm, id).expect("the part has an entity")
        }

        /// Which of the part's render components (transform, mesh, material, tag, bounds, shadow marker)
        /// the latest sync wrote.
        fn written(&self, id: Id) -> [bool; 6] {
            let (e, now) = (self.world.entity(self.entity(id)), self.world.read_change_tick());
            let since = self.before_sync;
            fn wrote<C: Component>(e: &EntityRef, since: Tick, now: Tick) -> bool {
                e.get_change_ticks::<C>().is_some_and(|t| t.is_changed(since, now))
            }
            [
                wrote::<Transform>(&e, since, now),
                wrote::<Mesh3d>(&e, since, now),
                wrote::<MeshMaterial3d<PartMaterial>>(&e, since, now),
                wrote::<MeshTag>(&e, since, now),
                wrote::<Aabb>(&e, since, now),
                wrote::<NotShadowCaster>(&e, since, now),
            ]
        }

        /// Entities that are not the workspace's camera (a new data model has one, and it is drawn through too).
        fn entities(&self) -> usize {
            self.world.iter_entities().count() - self.base
        }
    }

    const TRANSFORM: usize = 0;
    const MESH: usize = 1;
    const TAG: usize = 3;

    #[test]
    fn a_part_is_spawned_once_with_its_look_and_its_bounds() {
        let mut s = Scene::new();
        let p = s.part();
        s.set(p, "Position", Value::V3(Vec3::new(1.0, 2.0, 3.0)));
        s.set(p, "Size", Value::V3(Vec3::new(4.0, 5.0, 6.0)));
        s.set(p, "Color", Value::Color(Vec3::new(1.0, 0.0, 0.0)));
        s.sync();
        s.sync();
        assert_eq!(s.entities(), 1, "one entity, however often the scene is mirrored");
        let e = s.world.entity(s.entity(p));
        let t = e.get::<Transform>().unwrap();
        assert_eq!((t.translation, t.scale), (Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0)));
        assert_eq!(unpack_tag(e.get::<MeshTag>().unwrap().0), (Vec3::new(1.0, 0.0, 0.0), 1.0));
        assert!(e.get::<Mesh3d>().is_some() && e.get::<MeshMaterial3d<PartMaterial>>().is_some());
        assert_eq!(e.get::<Aabb>().map(|a| (a.center, a.half_extents)), Some((Vec3A::ZERO, Vec3A::splat(0.5))), "a primitive's bounds are known, not computed");
        assert!(e.get::<NotShadowCaster>().is_none(), "parts cast shadows unless told not to");
    }

    #[test]
    fn moving_a_part_writes_only_its_transform() {
        let mut s = Scene::new();
        let p = s.part();
        s.sync();
        assert_eq!(s.written(p)[..5], [true; 5], "the first sync wrote everything");
        s.set(p, "Position", Value::V3(Vec3::new(0.0, 9.0, 0.0)));
        s.sync();
        assert_eq!(s.written(p), [true, false, false, false, false, false]);
        assert_eq!(s.world.entity(s.entity(p)).get::<Transform>().unwrap().translation.y, 9.0);

        // A resize is a transform change too.
        s.set(p, "Size", Value::V3(Vec3::splat(2.0)));
        s.sync();
        assert_eq!(s.world.entity(s.entity(p)).get::<Transform>().unwrap().scale, Vec3::splat(2.0));
        assert_eq!(s.written(p), [true, false, false, false, false, false]);

        // And a sync with nothing to do writes nothing.
        s.sync();
        assert_eq!(s.written(p), [false; 6]);
    }

    #[test]
    fn recolouring_a_part_writes_its_tag_and_makes_bevy_look_at_the_instance_again() {
        let mut s = Scene::new();
        let p = s.part();
        s.sync();
        s.set(p, "Color", Value::Color(Vec3::new(0.0, 1.0, 0.0)));
        s.sync();
        // The tag is written, and the instance's `Mesh3d` is touched (the same mesh, not a new one):
        // Bevy's GPU mesh building only re-reads an instance when a component on its list changed,
        // and `MeshTag` is not on it. Without the touch a colour change stays unseen until the part is
        // moved or resized. The material, the bounds and the transform are left alone.
        assert_eq!(s.written(p), [false, true, false, true, false, false]);
        assert_eq!(unpack_tag(s.world.entity(s.entity(p)).get::<MeshTag>().unwrap().0).0, Vec3::new(0.0, 1.0, 0.0));
        let mesh_before = s.world.entity(s.entity(p)).get::<Mesh3d>().unwrap().0.id();
        s.set(p, "Color", Value::Color(Vec3::new(0.0, 0.0, 1.0)));
        s.sync();
        assert_eq!(s.world.entity(s.entity(p)).get::<Mesh3d>().unwrap().0.id(), mesh_before, "no new mesh is made for a colour");
        assert_eq!(s.world.resource::<Assets<Mesh>>().len(), 1);
    }

    #[test]
    fn every_look_change_of_one_part_is_seen_by_the_renderer_in_the_frame_it_is_made() {
        // Anything that only changes the per-instance data must reach Bevy's extraction (see above).
        for (prop, value) in [
            ("Transparency", Value::Num(0.3)),
            ("CastShadow", Value::Bool(false)),
            ("Material", Value::Enum("Metal".into())),
            ("Color", Value::Color(Vec3::new(0.2, 0.4, 0.6))),
        ] {
            let mut s = Scene::new();
            let p = s.part();
            s.sync();
            s.set(p, prop, value);
            s.sync();
            assert!(s.written(p)[MESH], "{prop}: the instance was not put back on Bevy's list of changed meshes");
            assert!(!s.written(p)[TRANSFORM], "{prop}: and nothing was moved");
        }
        // Switching the shadow back on removes a marker, which Bevy does not notice either.
        let mut s = Scene::new();
        let p = s.part();
        s.set(p, "CastShadow", Value::Bool(false));
        s.sync();
        s.set(p, "CastShadow", Value::Bool(true));
        s.sync();
        assert!(s.written(p)[MESH]);
        assert!(s.world.entity(s.entity(p)).get::<NotShadowCaster>().is_none());
    }

    #[test]
    fn asking_for_a_refresh_that_changes_nothing_writes_nothing() {
        let mut s = Scene::new();
        let p = s.part();
        s.sync();
        // What re-parenting inside the workspace, or switching the render mode back and forth, asks for.
        s.dm.mark(p, D_X | D_V);
        s.sync();
        assert_eq!(s.written(p), [false; 6]);
    }

    #[test]
    fn parts_of_any_colour_share_one_mesh_and_one_material_so_they_can_be_drawn_together() {
        let mut s = Scene::new();
        let parts: Vec<Id> = (0..4).map(|_| s.part()).collect();
        for (i, p) in parts.iter().enumerate() {
            s.set(*p, "Color", Value::Color(Vec3::new(i as f32 / 4.0, 0.5, 0.2)));
        }
        let metal = s.part();
        s.set(metal, "Material", Value::Enum("Metal".into()));
        s.sync();
        let look = |s: &Scene, p: Id| {
            let e = s.world.entity(s.entity(p));
            (e.get::<Mesh3d>().unwrap().0.id(), e.get::<MeshMaterial3d<PartMaterial>>().unwrap().0.id())
        };
        let first = look(&s, parts[0]);
        assert!(parts.iter().all(|p| look(&s, *p) == first), "colour is per entity (MeshTag), not part of the material");
        assert_eq!(look(&s, metal).0, first.0, "same shape, same mesh");
        assert_ne!(look(&s, metal).1, first.1, "a different surface kind is a different material");
        assert_eq!(s.world.resource::<Assets<Mesh>>().len(), 1);
        assert_eq!(s.world.resource::<Assets<PartMaterial>>().len(), 2);
    }

    #[test]
    fn see_through_parts_use_the_blended_material_and_stay_visible_in_the_editor() {
        let mut s = Scene::new();
        let (solid, glassy) = (s.part(), s.part());
        s.set(glassy, "Transparency", Value::Num(0.5));
        s.sync();
        let material = |s: &Scene, p: Id| s.world.entity(s.entity(p)).get::<MeshMaterial3d<PartMaterial>>().unwrap().0.id();
        assert_ne!(material(&s, solid), material(&s, glassy));
        let alpha = |s: &Scene, p: Id| unpack_tag(s.world.entity(s.entity(p)).get::<MeshTag>().unwrap().0).1;
        assert!((alpha(&s, glassy) - 0.5).abs() < 0.01 && alpha(&s, solid) == 1.0);

        // Fully transparent: gone in the game, a faint ghost in the editor.
        s.set(glassy, "Transparency", Value::Num(1.0));
        s.sync();
        assert!(entity_of(&s.dm, glassy).is_none());
        assert_eq!(s.entities(), 1);
        s.mode = Mode::Editor;
        s.dm.mark_all(D_X | D_V);
        s.sync();
        assert!((alpha(&s, glassy) - 0.12).abs() < 0.01, "ghost");
        assert_eq!(s.entities(), 2);
        s.mode = Mode::Game;
        s.dm.mark_all(D_X | D_V);
        s.sync();
        assert!(entity_of(&s.dm, glassy).is_none());
        assert_eq!(s.entities(), 1);
    }

    #[test]
    fn the_shadow_marker_follows_castshadow() {
        let mut s = Scene::new();
        let p = s.part();
        s.set(p, "CastShadow", Value::Bool(false));
        s.sync();
        assert!(s.world.entity(s.entity(p)).get::<NotShadowCaster>().is_some());
        s.set(p, "CastShadow", Value::Bool(true));
        s.sync();
        assert!(s.world.entity(s.entity(p)).get::<NotShadowCaster>().is_none());
        // The marker was *removed*, which Bevy's mesh extraction does not notice as a change: the mesh is touched
        // (same mesh, no new data) so the instance is read again and the part really casts its shadow again.
        assert_eq!(s.written(p), [false, true, false, false, false, false], "only the marker was removed, and the instance was put back on Bevy's list");
        s.set(p, "CastShadow", Value::Bool(false));
        s.sync();
        assert!(s.world.entity(s.entity(p)).get::<NotShadowCaster>().is_some());
    }

    #[test]
    fn leaving_the_workspace_removes_the_entity_and_coming_back_makes_a_new_one() {
        let mut s = Scene::new();
        let p = s.part();
        s.sync();
        assert_eq!(s.entities(), 1);
        s.dm.set_parent(p, None).unwrap();
        s.sync();
        assert_eq!((s.entities(), entity_of(&s.dm, p)), (0, None));
        assert!(s.dm.get(p).unwrap().drawn.is_none(), "what was drawn is forgotten with the entity");
        let ws = s.dm.workspace;
        s.dm.set_parent(p, Some(ws)).unwrap();
        s.sync();
        assert_eq!(s.entities(), 1);
        assert!(s.world.entity(s.entity(p)).get::<MeshTag>().is_some(), "complete again");

        s.dm.destroy(p);
        s.sync();
        assert_eq!(s.entities(), 0);
    }

    #[test]
    fn changing_the_shape_swaps_the_mesh_and_the_orientation_of_cylinders() {
        let mut s = Scene::new();
        let p = s.part();
        s.sync();
        let block = s.world.entity(s.entity(p)).get::<Mesh3d>().unwrap().0.id();
        s.set(p, "Shape", Value::Enum("Cylinder".into()));
        s.sync();
        let e = s.world.entity(s.entity(p));
        assert_ne!(e.get::<Mesh3d>().unwrap().0.id(), block);
        assert!(e.get::<Aabb>().is_some(), "the new mesh has bounds too");
        assert_eq!(e.get::<Transform>().unwrap().scale, Vec3::new(1.0, 4.0, 2.0), "a cylinder lies along X: its mesh's height axis takes the part's X size");
        assert_eq!([s.written(p)[TRANSFORM], s.written(p)[MESH], s.written(p)[TAG]], [true, true, false], "a new mesh and orientation, but the colour did not change");
    }

    #[test]
    fn an_emitter_draws_the_particles_it_makes_and_hides_when_none_are_left() {
        let mut s = Scene::new();
        let part = s.part();
        let emitter = s.child(part, "ParticleEmitter");
        s.set(emitter, "Rate", Value::Num(120.0));
        s.set(emitter, "Lifetime", Value::Data("NumberRange", vec![0.5, 0.5]));
        s.frames(20);
        assert_eq!(s.entities(), 2, "the part and one entity for all the particles");
        let vertices = s.effect_vertices(0).unwrap();
        assert!(vertices >= 4 * 10 && vertices % 4 == 0, "a quad per particle, got {vertices} vertices");
        assert_eq!(s.effect_visibility(0), Some(Visibility::Visible));
        s.set(emitter, "Enabled", Value::Bool(false));
        s.frames(40);
        assert_eq!(s.effect_visibility(0), Some(Visibility::Hidden), "every particle has lived its half second");
        s.dm.destroy(emitter);
        s.frames(1);
        assert_eq!(s.entities(), 1, "the particles go with the emitter");
    }

    #[test]
    fn emit_makes_particles_at_once_whether_or_not_the_emitter_is_enabled_and_clear_removes_them() {
        let mut s = Scene::new();
        let part = s.part();
        let emitter = s.child(part, "ParticleEmitter");
        s.set(emitter, "Enabled", Value::Bool(false));
        s.frames(3);
        assert!(s.effect_vertices(0).is_none() || s.effect_visibility(0) == Some(Visibility::Hidden), "nothing yet");
        s.set(emitter, "EmitCount", Value::Num(5.0));
        s.frames(1);
        assert_eq!(s.effect_vertices(0), Some(20), "five particles");
        s.frames(5);
        assert_eq!(s.effect_vertices(0), Some(20), "and no more");
        s.set(emitter, "ClearCount", Value::Num(1.0));
        s.frames(1);
        assert_eq!(s.effect_visibility(0), Some(Visibility::Hidden));
    }

    #[test]
    fn an_emitter_outside_the_world_draws_nothing() {
        let mut s = Scene::new();
        let part = s.part();
        let emitter = s.child(part, "ParticleEmitter");
        s.frames(5);
        assert_eq!(s.entities(), 2);
        s.dm.set_parent(part, None).unwrap();
        s.frames(1);
        assert_eq!(s.entities(), 0, "the part and its particles are gone");
        let _ = emitter;
    }

    #[test]
    fn fire_smoke_and_sparkles_make_particles_of_their_own() {
        for class in ["Fire", "Smoke", "Sparkles"] {
            let mut s = Scene::new();
            let part = s.part();
            s.child(part, class);
            s.frames(30);
            assert!(s.effect_vertices(0).is_some_and(|v| v >= 4), "{class} drew something");
        }
    }

    #[test]
    fn a_beam_is_a_ribbon_between_its_attachments_and_goes_when_it_is_switched_off() {
        let mut s = Scene::new();
        let (a, b) = (s.part(), s.part());
        s.set(b, "Position", Value::V3(Vec3::new(20.0, 0.0, 0.0)));
        let (att0, att1) = (s.child(a, "Attachment"), s.child(b, "Attachment"));
        let ws = s.dm.workspace;
        let beam = s.child(ws, "Beam");
        s.set(beam, "Attachment0", Value::Ref(Some(att0)));
        s.set(beam, "Attachment1", Value::Ref(Some(att1)));
        s.set(beam, "Segments", Value::Num(6.0));
        s.frames(2);
        assert_eq!(s.entities(), 3);
        assert_eq!(s.effect_vertices(0), Some(6 * 4), "a quad for each of the six pieces");
        s.set(beam, "Enabled", Value::Bool(false));
        s.frames(1);
        assert_eq!(s.entities(), 2);
    }

    #[test]
    fn a_trail_follows_its_attachments_and_is_removed_by_clear() {
        let mut s = Scene::new();
        let part = s.part();
        let (att0, att1) = (s.child(part, "Attachment"), s.child(part, "Attachment"));
        s.set(att0, "Position", Value::V3(Vec3::new(0.0, 1.0, 0.0)));
        s.set(att1, "Position", Value::V3(Vec3::new(0.0, -1.0, 0.0)));
        let ws = s.dm.workspace;
        let trail = s.child(ws, "Trail");
        s.set(trail, "Attachment0", Value::Ref(Some(att0)));
        s.set(trail, "Attachment1", Value::Ref(Some(att1)));
        for i in 0..20 {
            s.set(part, "Position", Value::V3(Vec3::new(i as f32, 5.0, 0.0)));
            s.frames(1);
        }
        assert!(s.effect_vertices(0).is_some_and(|v| v >= 8), "the trail has several pieces: {:?}", s.effect_vertices(0));
        s.set(trail, "ClearCount", Value::Num(1.0));
        s.set(part, "Position", Value::V3(Vec3::new(500.0, 5.0, 0.0)));
        s.frames(1);
        assert!(s.effect_vertices(0).is_some_and(|v| v <= 4), "only the live piece is left: {:?}", s.effect_vertices(0));
    }

    #[test]
    fn a_highlight_draws_a_tint_and_an_outline_for_every_part_it_marks_and_follows_them() {
        let mut s = Scene::new();
        let ws = s.dm.workspace;
        let model = s.child(ws, "Model");
        let (a, b) = (s.part(), s.part());
        s.dm.set_parent(a, Some(model)).unwrap();
        s.dm.set_parent(b, Some(model)).unwrap();
        let highlight = s.child(model, "Highlight");
        s.frames(2);
        assert_eq!(s.entities(), 2 + 4, "two parts, and a tint and an outline for each");
        let fill = s.world.query_filtered::<(&Transform, &Name), With<bevy::light::NotShadowReceiver>>().iter(&s.world).filter(|(_, n)| n.as_str() == "Highlight fill").map(|(t, _)| t.translation).collect::<Vec<_>>();
        assert_eq!(fill.len(), 2);
        s.set(a, "Position", Value::V3(Vec3::new(50.0, 0.0, 0.0)));
        s.frames(1);
        let xs: Vec<f32> = s.world.query_filtered::<(&Transform, &Name), With<bevy::light::NotShadowReceiver>>().iter(&s.world).filter(|(_, n)| n.as_str() == "Highlight fill").map(|(t, _)| t.translation.x).collect();
        assert!(xs.iter().any(|x| (*x - 50.0).abs() < 1e-3), "the tint moved with the part: {xs:?}");
        // No outline when it is see-through, and nothing when the highlight is off or destroyed.
        s.set(highlight, "OutlineTransparency", Value::Num(1.0));
        s.frames(1);
        assert_eq!(s.entities(), 2 + 2);
        s.set(highlight, "Enabled", Value::Bool(false));
        s.frames(1);
        assert_eq!(s.entities(), 2);
        s.set(highlight, "Enabled", Value::Bool(true));
        s.frames(1);
        assert_eq!(s.entities(), 2 + 2);
        s.dm.destroy(highlight);
        s.frames(1);
        assert_eq!(s.entities(), 2);
    }

    #[test]
    fn clouds_in_the_lighting_settings_are_one_sheet_and_go_when_switched_off() {
        let mut s = Scene::new();
        let lighting = s.dm.service("Lighting").unwrap();
        let clouds = s.child(lighting, "Clouds");
        s.frames(2);
        assert_eq!(s.entities(), 1);
        assert_eq!(s.effect_vertices(0), Some(12 * 12 * 4));
        s.set(clouds, "Enabled", Value::Bool(false));
        s.frames(1);
        assert_eq!(s.entities(), 0);
    }

    #[test]
    fn an_explosion_shows_a_fireball_that_has_faded_a_second_later() {
        let mut s = Scene::new();
        let ws = s.dm.workspace;
        let explosion = s.dm.create("Explosion").unwrap();
        s.set(explosion, "BlastRadius", Value::Num(8.0));
        s.set(explosion, "Position", Value::V3(Vec3::new(0.0, 5.0, 0.0)));
        s.dm.set_parent(explosion, Some(ws)).unwrap();
        s.frames(3);
        assert_eq!(s.entities(), 1);
        assert!(s.effect_vertices(0).is_some_and(|v| v >= 4 * 40), "the puffs: {:?}", s.effect_vertices(0));
        assert_eq!(s.effect_visibility(0), Some(Visibility::Visible));
        s.frames(80);
        assert_eq!(s.effect_visibility(0), Some(Visibility::Hidden), "nothing is left of it");
        // Not visible: nothing is drawn (the blast itself still happens).
        let quiet = s.dm.create("Explosion").unwrap();
        s.set(quiet, "Visible", Value::Bool(false));
        s.dm.set_parent(quiet, Some(ws)).unwrap();
        s.frames(3);
        assert_eq!(s.entities(), 1, "only the first one has a fireball");
    }

    #[test]
    fn reflectance_gives_a_part_a_shinier_material_of_its_own_that_parts_with_the_same_amount_share() {
        let mut s = Scene::new();
        let (plain, shiny, shiny_too) = (s.part(), s.part(), s.part());
        s.set(shiny, "Reflectance", Value::Num(0.8));
        s.set(shiny_too, "Reflectance", Value::Num(0.8));
        s.frames(1);
        let material = |s: &Scene, p: Id| s.world.entity(s.entity(p)).get::<MeshMaterial3d<PartMaterial>>().unwrap().0.id();
        assert_ne!(material(&s, plain), material(&s, shiny), "a reflective part is drawn with another material");
        assert_eq!(material(&s, shiny), material(&s, shiny_too), "the same reflectance is the same material");
        let handle = s.world.entity(s.entity(shiny)).get::<MeshMaterial3d<PartMaterial>>().unwrap().0.clone();
        let base = s.world.resource::<Assets<PartMaterial>>().get(&handle).unwrap().base.clone();
        assert!(base.metallic >= 0.79 && base.perceptual_roughness < 0.3, "{} {}", base.metallic, base.perceptual_roughness);
        // Taking it away puts the part back on the plain material.
        s.set(shiny, "Reflectance", Value::Num(0.0));
        s.frames(1);
        assert_eq!(material(&s, plain), material(&s, shiny));
    }

    #[test]
    fn moves_of_entities_that_are_already_gone_are_skipped() {
        let mut world = World::new();
        let alive = world.spawn(Transform::default()).id();
        let gone = world.spawn(Transform::default()).id();
        world.despawn(gone);
        let target = Transform::from_xyz(1.0, 2.0, 3.0);
        apply_moves(&mut world, vec![(gone, target), (alive, target)]);
        assert_eq!(world.entity(alive).get::<Transform>(), Some(&target));
    }
}


//! The data model: a tree of typed instances (like Roblox's DataModel).
//!
//! This module is *generic*: it stores instances and their property bags, keeps
//! the tree consistent, and routes property reads/writes through the class
//! registry (`crate::core`). It knows nothing about any particular class - all of
//! that lives in `crate::classes` and `crate::components`.
//!
//! * this file  - storage, dirty tracking and events
//! * [`tree`]    - hierarchy: lookup, parenting, create / destroy / clone
//! * [`props`]   - property and attribute access
//! * [`spatial`] - CFrames, pivots and collision volumes
//! * [`file`]    - scene (place) file serialization
//! * [`journal`] - the change journal behind command-based undo

pub mod file;
pub mod journal;

mod props;
mod spatial;
mod tags;
mod tree;

use std::{
    any::Any,
    collections::{BTreeSet, HashMap},
};

use bevy::ecs::entity::Entity;

pub use crate::core::{enum_names, euler_deg, to_euler_deg, value_matches, FastMap, FastSet, Id, PropMeta, PropType, ResolvedClass, Value, D};
use crate::core::{registry, ScriptValue};

#[derive(Clone, Debug)]
pub enum Event {
    /// A property changed. For a value object's `Value` the new value travels with the event (handlers get the value the
    /// property had when it changed, not the one it has when they run).
    Changed(Id, &'static str, Option<(PropType, Value)>),
    ChildAdded(Id, Id),
    ChildRemoved(Id, Id),
    Destroying(Id),
    Touched(Id, Id),
    TouchEnded(Id, Id),
    /// A signal of any class: the instance, the signal's name and what its handlers are called with. This is how
    /// the engine tells scripts about anything that happens to an object (`Part.Touched`, `AncestryChanged`,
    /// `AttributeChanged`, a `BindableEvent` firing ...) without the scripting layer knowing about the class.
    Signal(Id, String, Vec<ScriptValue>),
}

/// Dirty bits (what the renderer has to refresh).
pub const D_X: u8 = 1;
pub const D_V: u8 = 2;

pub struct Instance {
    pub uid: u32,
    pub class: &'static ResolvedClass,
    pub name: String,
    pub parent: Option<Id>,
    pub children: Vec<Id>,
    pub props: Vec<(&'static str, Value)>,
    pub attrs: Vec<(String, Value)>,
    /// Bevy entity mirroring this instance (managed by the renderer's components).
    pub entity: Option<Entity>,
    /// What the renderer last gave the entity, so a refresh only touches what changed. Opaque to the
    /// data model (it is the renderer's own bookkeeping, type-erased to keep `dm` independent of it);
    /// it is cleared together with `entity`.
    pub drawn: Option<Box<dyn Any>>,
    pub dirty: u8,
}

impl Instance {
    pub fn raw(&self, name: &str) -> Option<&Value> {
        self.props.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }
}

/// A set of instance slots, one bit each. Adding and removing an instance (which every
/// `Instance.new` and `Destroy` does once per component its class has) is a bit flip, where a tree
/// set would walk and rebalance. At most one live instance occupies a slot, so iterating the bits
/// in order visits instances in the order of their ids.
#[derive(Default)]
struct SlotSet {
    words: Vec<u64>,
    len: usize,
}

impl SlotSet {
    fn insert(&mut self, slot: u32) {
        let (word, bit) = (slot as usize / 64, 1u64 << (slot % 64));
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        if self.words[word] & bit == 0 {
            self.words[word] |= bit;
            self.len += 1;
        }
    }

    fn remove(&mut self, slot: u32) {
        let (word, bit) = (slot as usize / 64, 1u64 << (slot % 64));
        if let Some(w) = self.words.get_mut(word) {
            if *w & bit != 0 {
                *w &= !bit;
                self.len -= 1;
            }
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    /// The slots in the set, lowest first.
    fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.words.iter().enumerate().flat_map(|(i, &w)| {
            let mut bits = w;
            std::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let bit = bits.trailing_zeros();
                bits &= bits - 1;
                Some(i as u32 * 64 + bit)
            })
        })
    }
}

/// The data model's side of the physics simulation: the list of instances whose physical state has to be
/// brought up to date. Scripts and the editor write properties; the simulation writes positions and velocities
/// back. Writes the simulation makes are `muted`, so they are never mistaken for edits and sent back to it.
#[derive(Default)]
pub struct PhysFeed {
    /// Whether a simulation is attached. While it is not, nothing is recorded.
    pub enabled: bool,
    /// Set while the simulation writes its results into the data model.
    pub muted: bool,
    dirty: FastSet<Id>,
}

impl PhysFeed {
    fn mark(&mut self, id: Id) {
        if self.enabled && !self.muted {
            self.dirty.insert(id);
        }
    }

    /// Stops recording (the simulation that listened is gone) and forgets what was recorded.
    pub fn detach(&mut self) {
        self.enabled = false;
        self.muted = false;
        self.dirty.clear();
    }

    /// Marks an instance for the simulation's own reasons (the collision groups changed), whoever is writing.
    pub fn mark_internal(&mut self, id: Id) {
        if self.enabled {
            self.dirty.insert(id);
        }
    }

    /// The instances to look at, in id order (so the simulation behaves the same on every run), and forgets them.
    pub fn take(&mut self) -> Vec<Id> {
        let mut ids: Vec<Id> = self.dirty.drain().collect();
        ids.sort_unstable();
        ids
    }

    pub fn pending(&self) -> usize {
        self.dirty.len()
    }
}

pub struct DataModel {
    slots: Vec<Option<Instance>>,
    free: Vec<u32>,
    next_uid: u32,
    pub game: Id,
    /// The service marked `world_root` (Workspace).
    pub workspace: Id,

    /// Instances the renderer has to look at.
    pub dirty: Vec<Id>,
    /// Entities of destroyed instances, to be despawned by the renderer.
    pub dead_entities: Vec<Entity>,
    pub events: Vec<Event>,
    /// Scripts that may need to be started.
    pub pending_scripts: Vec<Id>,
    /// Instances somebody listens to (only these produce events).
    pub watched: FastSet<u32>,
    pub touch_watch: FastSet<Id>,
    /// uids of destroyed instances that had listeners; the scripting host drops their signals.
    pub dead_watched: Vec<u32>,
    /// uids of destroyed script instances; the scripting host stops what they started.
    pub dead_scripts: Vec<u32>,
    /// The class and name of instances destroyed since the host last cleaned up (only kept while somebody listens), so a
    /// handler that runs after the destruction can still say what it was: uid -> (class, name).
    pub graves: HashMap<u32, (&'static str, String)>,
    /// Problems found by the last `load_place` (unknown classes, bad property values ...).
    pub load_warnings: Vec<String>,
    /// What can be asked about the project's files (a model's size, a sound's length), when there is a project.
    asset_info: Option<std::rc::Rc<crate::assets::info::AssetInfo>>,
    /// `CollectionService` tags: tag -> tagged instances.
    pub tags: HashMap<String, BTreeSet<Id>>,
    /// Reversible record of the edits made since it was last taken (see [`journal`]);
    /// `None` (the default, and what a running game uses) records nothing.
    journal: Option<journal::Journal>,
    /// Set while undo / redo replays changes, so replaying is not itself recorded.
    journal_paused: bool,
    /// Which instances have which component (the "archetype index" systems iterate).
    /// Ordered (by slot, which is the order of ids), so systems visit instances in the same order on every run.
    comp_index: FastMap<&'static str, SlotSet>,
    /// What the physics simulation has to look at (see [`PhysFeed`]).
    pub phys: PhysFeed,
    /// Bumped when instances are created/destroyed/re-parented.
    pub struct_rev: u64,
}

impl Default for DataModel {
    fn default() -> Self {
        Self::new()
    }
}

impl DataModel {
    /// What the data model can ask about the project's files (see [crate::assets::info::AssetInfo]); None without a project.
    pub fn assets(&self) -> Option<&crate::assets::info::AssetInfo> {
        self.asset_info.as_deref()
    }

    /// The shared handle to the same (so the editor and the audio engine use one cache).
    pub fn asset_info(&self) -> Option<std::rc::Rc<crate::assets::info::AssetInfo>> {
        self.asset_info.clone()
    }

    pub fn set_asset_info(&mut self, info: Option<std::rc::Rc<crate::assets::info::AssetInfo>>) {
        self.asset_info = info;
    }

    pub fn new() -> Self {
        let reg = registry();
        let none = Id { slot: 0, uid: 0 };
        let mut dm = DataModel {
            slots: Vec::new(),
            free: Vec::new(),
            next_uid: 0,
            game: none,
            workspace: none,
            dirty: Vec::new(),
            dead_entities: Vec::new(),
            events: Vec::new(),
            pending_scripts: Vec::new(),
            watched: FastSet::default(),
            touch_watch: FastSet::default(),
            dead_watched: Vec::new(),
            dead_scripts: Vec::new(),
            graves: HashMap::new(),
            load_warnings: Vec::new(),
            asset_info: None,
            tags: HashMap::new(),
            journal: None,
            journal_paused: false,
            comp_index: FastMap::default(),
            phys: PhysFeed::default(),
            struct_rev: 1,
        };
        let game = dm.alloc(reg.root());
        dm.game = game;
        dm.set_name_raw(game, "game");

        let mut services = Vec::new();
        for svc in reg.services() {
            let id = dm.alloc(svc);
            dm.link(game, id);
            if svc.desc.world_root {
                dm.workspace = id;
            }
            services.push((id, svc));
        }
        for (id, svc) in &services {
            dm.mark(*id, D_X | D_V);
            svc.desc.behavior.on_created(&mut dm, *id);
        }
        for (id, svc) in &services {
            svc.desc.behavior.on_loaded(&mut dm, *id);
        }
        dm
    }

    // ---- storage ------------------------------------------------------------------

    fn alloc(&mut self, c: &'static ResolvedClass) -> Id {
        self.next_uid += 1;
        let uid = self.next_uid;
        // (Sized up front: a part has ~17 stored properties and `filter` hides the length from `collect`,
        // which would otherwise grow the vector four times.)
        let mut props = Vec::with_capacity(c.props.len());
        props.extend(c.props.iter().filter(|p| p.meta.getter.is_none()).map(|p| (p.meta.name, p.meta.default.to_value())));
        let inst = Instance { uid, class: c, name: c.name.to_owned(), parent: None, children: Vec::new(), props, attrs: Vec::new(), entity: None, drawn: None, dirty: 0 };
        let slot = match self.free.pop() {
            Some(s) => {
                self.slots[s as usize] = Some(inst);
                s
            }
            None => {
                self.slots.push(Some(inst));
                (self.slots.len() - 1) as u32
            }
        };
        let id = Id { slot, uid };
        for comp in &c.components {
            self.comp_index.entry(comp.name).or_default().insert(slot);
        }
        id
    }

    pub fn get(&self, id: Id) -> Option<&Instance> {
        self.slots.get(id.slot as usize).and_then(|s| s.as_ref()).filter(|i| i.uid == id.uid)
    }

    pub fn get_mut(&mut self, id: Id) -> Option<&mut Instance> {
        self.slots.get_mut(id.slot as usize).and_then(|s| s.as_mut()).filter(|i| i.uid == id.uid)
    }

    pub fn exists(&self, id: Id) -> bool {
        self.get(id).is_some()
    }

    /// Every live instance.
    pub fn all_ids(&self) -> Vec<Id> {
        self.slots.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|inst| Id { slot: i as u32, uid: inst.uid })).collect()
    }

    /// The live instances with these uids, found in one pass over the data model however many are asked for.
    pub fn ids_of_uids(&self, uids: &FastSet<u32>) -> FastMap<u32, Id> {
        let mut found = FastMap::default();
        for (slot, inst) in self.slots.iter().enumerate() {
            if let Some(inst) = inst.as_ref().filter(|i| uids.contains(&i.uid)) {
                found.insert(inst.uid, Id { slot: slot as u32, uid: inst.uid });
            }
        }
        found
    }

    /// How many instances exist (without building a list of them).
    pub fn instance_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// How many instances have a component.
    pub fn component_count(&self, name: &str) -> usize {
        self.comp_index.get(name).map_or(0, |s| s.len())
    }

    /// All instances that have a component (the basis of every system).
    pub fn with_component(&self, name: &str) -> Vec<Id> {
        let Some(set) = self.comp_index.get(name) else { return Vec::new() };
        let mut out = Vec::with_capacity(set.len());
        out.extend(set.iter().filter_map(|slot| self.slots.get(slot as usize)?.as_ref().map(|i| Id { slot, uid: i.uid })));
        out
    }

    pub fn has_component(&self, id: Id, name: &str) -> bool {
        self.get(id).is_some_and(|i| i.class.has_component(name))
    }

    pub fn is_a(&self, id: Id, class_name: &str) -> bool {
        self.get(id).is_some_and(|i| i.class.is_a(class_name))
    }

    fn link(&mut self, parent: Id, child: Id) {
        if let Some(p) = self.get_mut(parent) {
            p.children.push(child);
        }
        if let Some(c) = self.get_mut(child) {
            c.parent = Some(parent);
        }
    }

    fn set_name_raw(&mut self, id: Id, name: &str) {
        if let Some(i) = self.get_mut(id) {
            i.name = name.to_owned();
        }
    }

    // ---- dirty tracking & events ------------------------------------------------------

    /// Asks the renderer to look at every instance again (e.g. after the render mode changed).
    pub fn mark_all(&mut self, bits: u8) {
        for id in self.all_ids() {
            self.mark(id, bits);
        }
    }

    /// Puts every runtime-only property (`F_RUNTIME`: mouse lock, current camera, ...) back to
    /// its default, so a finished play session leaves nothing behind.
    pub fn reset_runtime_props(&mut self) {
        for id in self.all_ids() {
            let Some(class) = self.get(id).map(|i| i.class) else { continue };
            for rp in &class.props {
                if rp.meta.flags & crate::core::F_RUNTIME != 0 && rp.meta.getter.is_none() {
                    let _ = self.set_prop(id, rp.meta.name, rp.meta.default.to_value());
                }
            }
        }
    }

    pub fn mark(&mut self, id: Id, bits: u8) {
        let was = match self.get_mut(id) {
            Some(i) => {
                let was = i.dirty;
                i.dirty |= bits;
                was
            }
            None => return,
        };
        if was == 0 {
            self.dirty.push(id);
        }
    }

    /// Tells the physics simulation that this instance (a part, a constraint ...) may no longer match it.
    /// Costs nothing while no simulation is attached (an editor that is not playing).
    pub fn phys_mark(&mut self, id: Id) {
        self.phys.mark(id);
    }

    /// Marks `id` and everything inside it (the whole subtree moved, appeared or went away).
    pub fn phys_mark_subtree(&mut self, id: Id) {
        if !self.phys.enabled {
            return;
        }
        for n in self.subtree(id) {
            self.phys.mark(n);
        }
    }

    /// Starts listening for `Touched` / `TouchEnded` on a part (the physics simulation only produces contact
    /// events for parts something listens to).
    pub fn watch_touch(&mut self, id: Id) {
        if self.touch_watch.insert(id) {
            self.phys.mark(id);
        }
    }

    fn emit(&mut self, uid: u32, ev: Event) {
        if self.watched.contains(&uid) {
            self.events.push(ev);
        }
    }

    /// Fires the signal `name` of an instance with `args` - if a script listens to the instance (otherwise
    /// `args` is never built). The scripting host delivers it with the frame's other events.
    pub fn fire(&mut self, id: Id, name: &str, args: impl FnOnce() -> Vec<ScriptValue>) {
        let Some(uid) = self.get(id).map(|i| i.uid) else { return };
        if self.watched.contains(&uid) {
            self.events.push(Event::Signal(id, name.to_owned(), args()));
        }
    }

    /// Whether anything is listening to any instance (when nothing is, event bookkeeping can be skipped).
    pub fn has_listeners(&self) -> bool {
        !self.watched.is_empty()
    }

    /// Hook for the host: marks a uid as having listeners.
    pub fn watch(&mut self, uid: u32) {
        self.watched.insert(uid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_component_index_follows_creation_destruction_and_slot_reuse_in_id_order() {
        let mut dm = DataModel::new();
        let before = dm.component_count("Collider");
        let parts: Vec<Id> = (0..200).map(|_| dm.create("Part").unwrap()).collect();
        assert_eq!(dm.component_count("Collider"), before + 200);
        let all = dm.with_component("Collider");
        assert!(all.windows(2).all(|w| w[0] < w[1]), "ids come out in order");
        assert!(parts.iter().all(|p| all.contains(p)));

        // Destroy every third; the next parts reuse those slots but are new instances.
        let gone: Vec<Id> = parts.iter().step_by(3).copied().collect();
        for p in &gone {
            dm.destroy(*p);
        }
        let all = dm.with_component("Collider");
        assert_eq!(all.len(), before + 200 - gone.len());
        assert!(gone.iter().all(|p| !all.contains(p)), "destroyed instances are gone from the index");
        let fresh: Vec<Id> = (0..gone.len()).map(|_| dm.create("Part").unwrap()).collect();
        assert!(fresh.iter().all(|f| gone.iter().any(|g| g.slot == f.slot) && gone.iter().all(|g| g != f)));
        let all = dm.with_component("Collider");
        assert_eq!(all.len(), before + 200);
        assert!(all.windows(2).all(|w| w[0] < w[1]));
        assert!(fresh.iter().all(|f| all.contains(f)));

        // Clones, and components further down the chain, are indexed too.
        let model = dm.create("Model").unwrap();
        dm.set_parent(parts[1], Some(model)).unwrap();
        let copy = dm.clone_tree(model).unwrap();
        let copied_part = dm.children(copy)[0];
        assert!(dm.with_component("Collider").contains(&copied_part));
        assert!(dm.with_component("NoSuchComponent").is_empty());
    }

    #[test]
    fn slot_sets_hold_slots_across_word_boundaries() {
        let mut s = SlotSet::default();
        for slot in [0u32, 1, 63, 64, 65, 127, 128, 1000] {
            s.insert(slot);
            s.insert(slot);
        }
        assert_eq!(s.len(), 8);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0, 1, 63, 64, 65, 127, 128, 1000]);
        s.remove(64);
        s.remove(64);
        s.remove(5000);
        assert_eq!(s.len(), 7);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0, 1, 63, 65, 127, 128, 1000]);
    }
}
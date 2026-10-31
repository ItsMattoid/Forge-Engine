//! Scene files: the instance tree of the persisted services as JSON.
//!
//! ```json
//! { "format": 2, "name": "Main", "services": [
//!   { "class": "Workspace", "children": [
//!       { "class": "Part", "name": "Baseplate",
//!         "props": { "Position": [0,-10,0], "Size": [512,20,512], "Anchored": true } } ] } ] }
//! ```
//! CFrames are stored as `Position` + `Orientation` (degrees); references to
//! other instances are stored as the 1-based depth-first index of the target.

use std::collections::HashMap;

use bevy::math::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as J};

use super::*;
use crate::core::{class, data_type};

pub const FORMAT: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct InstanceDef {
    pub class: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub props: Map<String, J>,
    /// Custom values set with `SetAttribute` (bool, number, string or `[x, y, z]`).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attrs: Map<String, J>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<InstanceDef>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlaceFile {
    #[serde(default)]
    pub format: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub services: Vec<InstanceDef>,
}

impl PlaceFile {
    pub fn empty(name: &str) -> Self {
        Self { format: FORMAT, name: name.to_owned(), services: Vec::new() }
    }

    /// Every script path referenced by this file (properties of type `ScriptPath`).
    pub fn script_paths(&self) -> Vec<(String, String)> {
        self.paths_where(|ty| ty == PropType::ScriptPath)
    }

    /// Every asset path referenced by this file (properties of type `Asset`, e.g. a `MeshId`).
    pub fn asset_paths(&self) -> Vec<(String, String)> {
        self.paths_where(|ty| matches!(ty, PropType::Asset(_)))
    }

    /// Every file this scene refers to (scripts and assets), once each.
    pub fn referenced_files(&self) -> Vec<String> {
        let mut all: Vec<String> = self.paths_where(|ty| ty == PropType::ScriptPath || matches!(ty, PropType::Asset(_))).into_iter().map(|(_, path)| path).collect();
        all.sort();
        all.dedup();
        all
    }

    /// Points every script / asset reference found in `moved` (`old path -> new path`) at its new
    /// place. Returns how many references changed.
    pub fn remap_paths(&mut self, moved: &HashMap<String, String>) -> usize {
        fn walk(d: &mut InstanceDef, moved: &HashMap<String, String>) -> usize {
            let mut changed = 0;
            if let Some(c) = class(&d.class) {
                for rp in c.props.iter().filter(|p| p.meta.ty == PropType::ScriptPath || matches!(p.meta.ty, PropType::Asset(_))) {
                    if let Some(J::String(s)) = d.props.get_mut(rp.meta.name) {
                        if let Some(new) = moved.get(s.as_str()) {
                            *s = new.clone();
                            changed += 1;
                        }
                    }
                }
            }
            changed + d.children.iter_mut().map(|c| walk(c, moved)).sum::<usize>()
        }
        self.services.iter_mut().map(|s| walk(s, moved)).sum()
    }

    /// `(instance name, path)` for every non-empty string property whose type passes `wanted`.
    fn paths_where(&self, wanted: impl Fn(PropType) -> bool + Copy) -> Vec<(String, String)> {
        fn walk(d: &InstanceDef, wanted: impl Fn(PropType) -> bool + Copy, out: &mut Vec<(String, String)>) {
            if let Some(c) = class(&d.class) {
                for rp in c.props.iter().filter(|p| wanted(p.meta.ty)) {
                    if let Some(J::String(s)) = d.props.get(rp.meta.name) {
                        if !s.is_empty() {
                            out.push((d.name.clone(), s.clone()));
                        }
                    }
                }
            }
            for c in &d.children {
                walk(c, wanted, out);
            }
        }
        let mut out = Vec::new();
        for s in &self.services {
            walk(s, wanted, &mut out);
        }
        out
    }
}

fn r5(f: f32) -> f64 {
    ((f as f64) * 1e5).round() / 1e5
}

fn arr3(v: Vec3) -> J {
    J::Array(vec![J::from(r5(v.x)), J::from(r5(v.y)), J::from(r5(v.z))])
}

fn parse3(j: &J) -> Option<Vec3> {
    let a = j.as_array()?;
    Some(Vec3::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32))
}

fn numbers(j: &J) -> Option<Vec<f32>> {
    j.as_array()?.iter().map(|n| n.as_f64().map(|f| f as f32)).collect()
}

fn arr_n(n: &[f32]) -> J {
    J::Array(n.iter().map(|f| J::from(r5(*f))).collect())
}

fn encode_value(v: &Value, index: &HashMap<Id, usize>) -> J {
    match v {
        Value::Bool(b) => J::Bool(*b),
        Value::Num(n) => J::from(r5(*n)),
        Value::Str(s) | Value::Enum(s) => J::String(s.clone()),
        Value::V2(v) => arr_n(&[v.x, v.y]),
        Value::V3(v) | Value::Color(v) => arr3(*v),
        Value::CF(p, _) => arr3(*p),
        Value::Ref(r) => r.and_then(|id| index.get(&id)).map_or(J::Null, |n| J::from(*n)),
        Value::Data(_, n) => arr_n(n),
    }
}

fn decode_value(ty: PropType, j: &J) -> Option<Value> {
    match ty {
        PropType::Bool => j.as_bool().map(Value::Bool),
        PropType::Number => j.as_f64().map(|f| Value::Num(f as f32)),
        PropType::String | PropType::ScriptPath | PropType::Asset(_) => j.as_str().map(|s| Value::Str(s.to_owned())),
        PropType::Vector2 => numbers(j).filter(|n| n.len() == 2).map(|n| Value::V2(Vec2::new(n[0], n[1]))),
        PropType::Vector3 => parse3(j).map(Value::V3),
        PropType::Color3 => parse3(j).map(Value::Color),
        PropType::Enum(_) => j.as_str().map(|s| Value::Enum(s.to_owned())),
        PropType::Data(tag) => numbers(j).map(|n| Value::Data(tag, n)),
        PropType::CFrame | PropType::Ref(_) => None,
    }
}

/// Attributes carry no property type, so the kinds that are not plain JSON are stored as `{"Kind": [numbers]}`.
fn encode_attr(v: &Value) -> Option<J> {
    let tagged = |kind: &str, n: &[f32]| Some(J::Object(Map::from_iter([(kind.to_owned(), arr_n(n))])));
    match v {
        Value::Bool(b) => Some(J::Bool(*b)),
        Value::Num(n) => Some(J::from(r5(*n))),
        Value::Str(s) => Some(J::String(s.clone())),
        Value::V3(v) => Some(arr3(*v)),
        Value::V2(v) => tagged("Vector2", &[v.x, v.y]),
        Value::Color(c) => tagged("Color3", &c.to_array()),
        Value::CF(p, q) => tagged("CFrame", &[p.x, p.y, p.z, q.x, q.y, q.z, q.w]),
        Value::Data(tag, n) => tagged(tag, n),
        Value::Enum(_) | Value::Ref(_) => None,
    }
}

fn decode_attr(j: &J) -> Option<Value> {
    match j {
        J::Bool(b) => Some(Value::Bool(*b)),
        J::Number(n) => n.as_f64().map(|f| Value::Num(f as f32)),
        J::String(s) => Some(Value::Str(s.clone())),
        J::Array(_) => parse3(j).map(Value::V3),
        J::Object(o) if o.len() == 1 => {
            let (kind, body) = o.iter().next()?;
            let n = numbers(body)?;
            match (kind.as_str(), n.as_slice()) {
                ("Vector2", [x, y]) => Some(Value::V2(Vec2::new(*x, *y))),
                ("Color3", [r, g, b]) => Some(Value::Color(Vec3::new(*r, *g, *b))),
                ("CFrame", [x, y, z, qx, qy, qz, qw]) => Some(Value::CF(Vec3::new(*x, *y, *z), bevy::math::Quat::from_xyzw(*qx, *qy, *qz, *qw).normalize())),
                _ => data_type(kind).map(|tag| Value::Data(tag, n)),
            }
        }
        _ => None,
    }
}

/// Classes that older scenes may still contain and that the engine no longer has (the built-in player architecture: player
/// controllers are scripts now). They are left out of a loaded scene without a warning, so an old project opens cleanly.
const RETIRED_CLASSES: &[&str] = &["Humanoid", "Accessory", "Backpack", "Team", "Teams", "Tool", "StarterGear", "StarterPack", "ForceField"];

/// Properties that went away with those classes; an old scene that still has them loads without a warning.
const RETIRED_PROPS: &[&str] = &[
    "CharacterAutoLoads", "RespawnTime", "Team", "TeamColor", "Neutral", "RespawnLocation", "Duration", "AllowTeamChangeOnTouch", "CharacterWalkSpeed", "CharacterJumpPower", "CharacterJumpHeight",
    "CharacterUseJumpPower", "CharacterMaxHealth", "HealthDisplayDistance", "NameDisplayDistance", "AutoJumpEnabled", "LoadCharacterAppearance",
];

/// Number of instances in a definition and everything below it (they all take a reference index).
fn skip_nodes(def: &InstanceDef, created: &mut Vec<Option<Id>>) {
    created.push(None);
    for c in &def.children {
        skip_nodes(c, created);
    }
}

impl DataModel {
    pub fn to_place(&self, name: &str) -> PlaceFile {
        let persisted = self.persisted_services();
        let order: Vec<Id> = persisted.iter().flat_map(|sid| self.subtree(*sid)).collect();
        let index: HashMap<Id, usize> = order.iter().enumerate().map(|(i, id)| (*id, i + 1)).collect();
        let services = persisted.iter().map(|sid| self.encode(*sid, &index)).collect();
        PlaceFile { format: FORMAT, name: name.to_owned(), services }
    }

    /// A prefab: the given objects (each with everything inside it) in the scene file format, as the
    /// children of a `Workspace` entry. Objects inside another listed object are not listed twice.
    pub fn to_prefab(&self, roots: &[Id], name: &str) -> PlaceFile {
        let roots: Vec<Id> = roots.iter().copied().filter(|r| self.exists(*r) && !roots.iter().any(|o| o != r && self.is_descendant_of(*r, *o))).collect();
        // Position 1 is the Workspace entry, so the objects are numbered from 2 (as `load_place` numbers them).
        let order: Vec<Id> = roots.iter().flat_map(|r| self.subtree(*r)).collect();
        let index: HashMap<Id, usize> = order.iter().enumerate().map(|(i, id)| (*id, i + 2)).collect();
        let wrapper = InstanceDef { class: "Workspace".into(), children: roots.iter().map(|r| self.encode(*r, &index)).collect(), ..InstanceDef::default() };
        PlaceFile { format: FORMAT, name: name.to_owned(), services: vec![wrapper] }
    }

    /// Creates the objects of a prefab (or the contents of a scene's first service) under `parent`.
    /// Returns the new top-level objects and what had to be skipped. Nothing outside `parent` changes.
    pub fn instantiate_place(&mut self, place: &PlaceFile, parent: Id) -> Result<(Vec<Id>, Vec<String>), String> {
        if place.format != FORMAT {
            return Err(format!("unsupported file format {} (this version of Forge uses format {FORMAT})", place.format));
        }
        let Some(wrapper) = place.services.first() else { return Err("the prefab is empty".into()) };
        if !self.exists(parent) {
            return Err("the target no longer exists".into());
        }
        let (mut created, mut refs, mut warns): (Vec<Option<Id>>, Vec<(Id, &'static str, usize)>, Vec<String>) = (vec![None], Vec::new(), Vec::new());
        let made: Vec<Id> = wrapper.children.iter().filter_map(|def| self.decode(def, Some(parent), &mut created, &mut refs, &mut warns)).collect();
        self.resolve_refs(refs, &created, &mut warns);
        if made.is_empty() {
            return Err(warns.into_iter().next().unwrap_or_else(|| "the prefab has nothing to place".into()));
        }
        Ok((made, warns))
    }

    /// Every instance of the persisted services in the order `to_place` writes them (which is
    /// also the order `load_place` creates them in): the position of an instance in this list
    /// identifies it across a save / load round trip.
    pub fn persisted_order(&self) -> Vec<Id> {
        self.persisted_services().iter().flat_map(|sid| self.subtree(*sid)).collect()
    }

    pub(super) fn encode(&self, id: Id, index: &HashMap<Id, usize>) -> InstanceDef {
        let Some(inst) = self.get(id) else { return InstanceDef::default() };
        let mut props = Map::new();
        for rp in &inst.class.props {
            let meta = &rp.meta;
            if !meta.saved() {
                continue;
            }
            let Some(v) = self.get_prop(id, meta.name) else { continue };
            if v == meta.default.to_value() {
                continue;
            }
            props.insert(meta.name.to_owned(), encode_value(&v, index));
        }
        let mut attrs = Map::new();
        let mut names: Vec<&(String, Value)> = inst.attrs.iter().collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, v) in names {
            if let Some(j) = encode_attr(v) {
                attrs.insert(name.clone(), j);
            }
        }
        InstanceDef {
            class: inst.class.name.to_owned(),
            name: if inst.class.desc.service_order.is_some() { String::new() } else { inst.name.clone() },
            props,
            attrs,
            children: inst.children.iter().map(|c| self.encode(*c, index)).collect(),
        }
    }

    /// Replaces the contents of all persisted services with the contents of `place`.
    ///
    /// Loading never leaves the data model half-loaded: an unsupported format is rejected before
    /// anything is touched, and content the engine does not understand (an unknown class, a bad
    /// property value) is skipped and reported in [`DataModel::load_warnings`] instead of aborting.
    pub fn load_place(&mut self, place: &PlaceFile) -> Result<(), String> {
        if place.format != FORMAT {
            return Err(format!(
                "unsupported scene format {} (this version of Forge uses format {FORMAT}); recreate the scene or the project",
                place.format
            ));
        }
        let mut warns: Vec<String> = Vec::new();
        // Loading is not an edit: it is not recorded for undo (the caller decides what the
        // history does with a whole new scene).
        let journaling = self.journal.take().is_some();
        for sid in self.persisted_services() {
            for c in self.children(sid) {
                self.destroy(c);
            }
            // The file only lists values that differ from the default, so anything it omits
            // has to go back to the default (otherwise e.g. an undo would keep the edited value).
            self.reset_saved_props(sid);
        }
        // One entry per instance in depth-first order; `None` for skipped ones so the
        // 1-based reference indexes stored in the file still line up.
        let mut created: Vec<Option<Id>> = Vec::new();
        let mut refs: Vec<(Id, &'static str, usize)> = Vec::new();
        for sdef in &place.services {
            let Some(sid) = self.service(&sdef.class) else {
                warns.push(format!("skipped unknown service '{}'", sdef.class));
                skip_nodes(sdef, &mut created);
                continue;
            };
            created.push(Some(sid));
            self.apply_props(sid, &sdef.props, &mut refs, &mut warns);
            self.apply_attrs(sid, &sdef.attrs);
            for c in &sdef.children {
                self.decode(c, Some(sid), &mut created, &mut refs, &mut warns);
            }
        }
        self.resolve_refs(refs, &created, &mut warns);
        for sid in self.persisted_services() {
            if let Some(class) = self.get(sid).map(|i| i.class) {
                class.desc.behavior.on_loaded(self, sid);
            }
        }
        self.load_warnings = warns;
        if journaling {
            self.journal = Some(super::journal::Journal::default());
        }
        Ok(())
    }

    /// Sets the reference properties collected while decoding: `n` is the 1-based position of the
    /// target in `created` (depth-first order).
    pub(super) fn resolve_refs(&mut self, refs: Vec<(Id, &'static str, usize)>, created: &[Option<Id>], warns: &mut Vec<String>) {
        for (id, name, n) in refs {
            let target = n.checked_sub(1).and_then(|i| created.get(i)).copied().flatten();
            if let Some(target) = target {
                if let Err(e) = self.set_prop(id, name, Value::Ref(Some(target))) {
                    warns.push(format!("{}.{name}: {e}", self.name(id)));
                }
            }
        }
    }

    /// Puts the saved (non-runtime, non-computed) properties of an instance back to their defaults.
    fn reset_saved_props(&mut self, id: Id) {
        let Some(class) = self.get(id).map(|i| i.class) else { return };
        for rp in class.props.iter().filter(|rp| rp.meta.saved() && rp.meta.getter.is_none()) {
            let _ = self.set_prop(id, rp.meta.name, rp.meta.default.to_value());
        }
    }

    pub(super) fn apply_attrs(&mut self, id: Id, attrs: &Map<String, J>) {
        for (k, j) in attrs {
            if let Some(v) = decode_attr(j) {
                self.set_attr(id, k, Some(v));
            }
        }
    }

    /// Services whose contents are saved, in creation order.
    pub fn persisted_services(&self) -> Vec<Id> {
        self.services_where(|c| c.desc.persisted)
    }

    /// Creates the instance `def` describes (and everything below it). With a `parent` it is placed
    /// there; without one it is left detached (undo uses that to bring back a deleted subtree).
    pub(super) fn decode(
        &mut self,
        def: &InstanceDef,
        parent: Option<Id>,
        created: &mut Vec<Option<Id>>,
        refs: &mut Vec<(Id, &'static str, usize)>,
        warns: &mut Vec<String>,
    ) -> Option<Id> {
        if RETIRED_CLASSES.contains(&def.class.as_str()) {
            skip_nodes(def, created);
            return None;
        }
        let id = match self.create_for_scene(&def.class) {
            Ok(id) => id,
            Err(e) => {
                warns.push(format!("skipped '{}' ({}): {e}", def.name, def.class));
                skip_nodes(def, created);
                return None;
            }
        };
        if !def.name.is_empty() {
            self.set_name_raw(id, &def.name);
        }
        created.push(Some(id));
        self.apply_props(id, &def.props, refs, warns);
        self.apply_attrs(id, &def.attrs);
        for c in &def.children {
            self.decode(c, Some(id), created, refs, warns);
        }
        if let Some(parent) = parent {
            if let Err(e) = self.set_parent(id, Some(parent)) {
                warns.push(format!("'{}' could not be placed: {e}", def.name));
                self.destroy(id);
                return None;
            }
        }
        Some(id)
    }

    fn apply_props(&mut self, id: Id, props: &Map<String, J>, refs: &mut Vec<(Id, &'static str, usize)>, warns: &mut Vec<String>) {
        let Some(class) = self.get(id).map(|i| i.class) else { return };
        let label = self.name(id);
        for (k, j) in props {
            let Some(rp) = class.prop(k) else {
                if RETIRED_PROPS.contains(&k.as_str()) {
                    continue;
                }
                warns.push(format!("{label} ({}): unknown property '{k}'", class.name));
                continue;
            };
            let meta = &rp.meta;
            if let PropType::Ref(_) = meta.ty {
                if let Some(n) = j.as_u64() {
                    refs.push((id, meta.name, n as usize));
                }
                continue;
            }
            match decode_value(meta.ty, j) {
                Some(v) => {
                    if let Err(e) = self.set_prop(id, meta.name, v) {
                        warns.push(format!("{label}.{}: {e}", meta.name));
                    }
                }
                None => warns.push(format!("{label}.{}: expected a {:?} value", meta.name, meta.ty)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DataModel {
        let mut dm = DataModel::new();
        let ws = dm.workspace;
        let model = dm.create("Model").unwrap();
        dm.set_parent(model, Some(ws)).unwrap();
        let part = dm.create("Part").unwrap();
        dm.set_name(part, "Root");
        dm.set_prop(part, "Position", Value::V3(Vec3::new(1.0, 2.0, 3.0))).unwrap();
        dm.set_prop(part, "Orientation", Value::V3(Vec3::new(0.0, 45.0, 0.0))).unwrap();
        dm.set_prop(part, "Anchored", Value::Bool(true)).unwrap();
        dm.set_parent(part, Some(model)).unwrap();
        dm.set_prop(model, "PrimaryPart", Value::Ref(Some(part))).unwrap();
        dm
    }

    #[test]
    fn place_round_trip() {
        let dm = sample();
        let place = dm.to_place("Test");
        let json = serde_json::to_string(&place).unwrap();
        let back: PlaceFile = serde_json::from_str(&json).unwrap();
        assert_eq!(place, back);

        let mut other = DataModel::new();
        other.load_place(&back).unwrap();
        assert_eq!(other.to_place("Test"), place);
        let root = other.find_first_child(other.workspace, "Root", true).unwrap();
        assert_eq!(other.vec3(root, "Position"), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn reparenting_rules() {
        let mut dm = sample();
        let model = dm.find_first_child(dm.workspace, "Model", false).unwrap();
        let part = dm.find_first_child(model, "Root", false).unwrap();
        assert!(dm.set_parent(model, Some(part)).is_err(), "cycles must be rejected");
        assert!(dm.set_parent(dm.workspace, Some(model)).is_err(), "services cannot move");
        dm.destroy(model);
        assert!(!dm.exists(part), "destroying a model destroys its children");
    }

    #[test]
    fn classes_and_properties_the_engine_retired_are_dropped_silently_from_old_scenes() {
        let dm = sample();
        let mut place = dm.to_place("Old");
        let ws = place.services.iter_mut().find(|s| s.class == "Workspace").unwrap();
        let model = ws.children.iter_mut().find(|c| c.class == "Model").unwrap();
        model.children.push(InstanceDef { class: "Humanoid".into(), name: "Humanoid".into(), ..Default::default() });
        let players = place.services.iter_mut().find(|s| s.class == "Players");
        if let Some(players) = players {
            players.props.insert("CharacterAutoLoads".into(), J::from(true));
        }
        let mut other = DataModel::new();
        other.load_place(&place).expect("loads");
        assert!(other.load_warnings.is_empty(), "an old project opens cleanly: {:?}", other.load_warnings);
        let model = other.find_first_child(other.workspace, "Model", false).unwrap();
        assert!(other.find_child(model, "Humanoid").is_none());
        let root = other.find_child(model, "Root").expect("the model keeps its other parts");
        assert_eq!(other.get_prop(model, "PrimaryPart"), Some(Value::Ref(Some(root))), "references after the dropped node still point at the right instance");
    }

    #[test]
    fn unknown_content_is_skipped_without_breaking_the_load() {
        let dm = sample();
        let mut place = dm.to_place("Test");
        // Insert an unknown class (with a child) in front of the Model: every reference
        // index that follows must still point at the right instance.
        let ws = place.services.iter_mut().find(|s| s.class == "Workspace").unwrap();
        let ghost = InstanceDef { class: "NoSuchClass".into(), name: "Ghost".into(), children: vec![InstanceDef { class: "Part".into(), name: "Inner".into(), ..Default::default() }], ..Default::default() };
        ws.children.insert(0, ghost);
        // ...and one with a property the class does not know and one with a bad enum value.
        let model = ws.children.iter_mut().find(|c| c.class == "Model").unwrap();
        // The file as a newer Forge would have written it: the ghost and its child take two
        // reference indexes, so the PrimaryPart reference moves up by two.
        let n = model.props["PrimaryPart"].as_u64().unwrap();
        model.props.insert("PrimaryPart".into(), J::from(n + 2));
        model.props.insert("Banana".into(), J::from(1));
        model.children[0].props.insert("Shape".into(), J::from("Triangle"));

        let mut other = DataModel::new();
        other.load_place(&place).expect("a scene with unknown content still loads");
        assert!(other.load_warnings.iter().any(|w| w.contains("NoSuchClass")));
        assert!(other.load_warnings.iter().any(|w| w.contains("Banana")));
        assert!(other.load_warnings.iter().any(|w| w.contains("Triangle")));
        let model = other.find_first_child(other.workspace, "Model", false).unwrap();
        let root = other.find_child(model, "Root").unwrap();
        assert_eq!(other.get_prop(model, "PrimaryPart"), Some(Value::Ref(Some(root))), "references survive skipped nodes");
        assert_eq!(other.string(root, "Shape"), "Block", "an invalid enum value is rejected");
    }

    #[test]
    fn unsupported_format_leaves_the_scene_untouched() {
        let mut dm = sample();
        let before = dm.to_place("Test");
        let bad = PlaceFile { format: FORMAT + 1, name: "x".into(), services: Vec::new() };
        assert!(dm.load_place(&bad).is_err());
        assert_eq!(dm.to_place("Test"), before);
    }

    #[test]
    fn loading_resets_service_properties_the_file_omits() {
        let mut dm = sample();
        let saved = dm.to_place("Test");
        let lighting = dm.service("Lighting").unwrap();
        dm.set_prop(lighting, "ClockTime", Value::Num(3.0)).unwrap();
        dm.load_place(&saved).unwrap();
        assert_eq!(dm.num(lighting, "ClockTime", 0.0), 14.0, "undo / stop must restore service settings too");
    }

    #[test]
    fn attributes_round_trip() {
        let mut dm = sample();
        let model = dm.find_first_child(dm.workspace, "Model", false).unwrap();
        dm.set_attr(model, "Health", Some(Value::Num(75.0)));
        dm.set_attr(model, "Team", Some(Value::Str("red".into())));
        let place = dm.to_place("Test");
        let mut other = DataModel::new();
        other.load_place(&place).unwrap();
        let copy = other.find_first_child(other.workspace, "Model", false).unwrap();
        assert_eq!(other.attr(copy, "Health"), Some(Value::Num(75.0)));
        assert_eq!(other.attr(copy, "Team"), Some(Value::Str("red".into())));
    }

    #[test]
    fn non_finite_and_mistyped_values_are_rejected() {
        let mut dm = sample();
        let model = dm.find_first_child(dm.workspace, "Model", false).unwrap();
        let part = dm.find_child(model, "Root").unwrap();
        assert!(dm.set_prop(part, "Size", Value::V3(Vec3::new(f32::NAN, 1.0, 1.0))).is_err());
        assert!(dm.set_prop(part, "Position", Value::V3(Vec3::new(f32::INFINITY, 0.0, 0.0))).is_err());
        assert!(dm.set_prop(part, "Shape", Value::Enum("Banana".into())).is_err());
        assert!(dm.set_prop(model, "PrimaryPart", Value::Ref(Some(model))).is_err(), "a Model is not a BasePart");
    }

    #[test]
    fn clone_remaps_references() {
        let mut dm = sample();
        let model = dm.find_first_child(dm.workspace, "Model", false).unwrap();
        let copy = dm.clone_tree(model).unwrap();
        let copy_root = dm.find_child(copy, "Root").unwrap();
        assert_eq!(dm.get_prop(copy, "PrimaryPart"), Some(Value::Ref(Some(copy_root))));
    }
}

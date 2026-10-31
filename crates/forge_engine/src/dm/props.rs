//! Property and attribute access, routed through the class registry: typed reads,
//! validated writes (type check, clamping, computed properties), and the side
//! effects a write has (dirty bits, `Changed`, component and class hooks).

use bevy::math::Vec3;

use super::{journal::Change, DataModel, Event, D_V, D_X};
use crate::core::{enum_names, value_matches, Id, PropMeta, PropType, Value, F_INT, F_PHYS, F_SCRIPT, F_VISUAL, F_XFORM};

impl DataModel {
    // ---- properties ------------------------------------------------------------------------

    pub fn get_prop(&self, id: Id, name: &str) -> Option<Value> {
        let inst = self.get(id)?;
        let meta = &inst.class.prop(name)?.meta;
        self.read_prop(inst, id, meta)
    }

    /// The value of a property the class declares: computed through its getter, else the stored one.
    /// (`get_prop` and the scripting layer both resolve the property first, so they share this.)
    pub fn read_prop(&self, inst: &super::Instance, id: Id, meta: &PropMeta) -> Option<Value> {
        match meta.getter {
            Some(getter) => getter(self, id),
            None => inst.raw(meta.name).cloned(),
        }
    }

    // The typed readers below sit on the hottest paths (the renderer, physics and `set_prop` read a
    // handful of properties per instance per frame). A property that is stored under its own name is
    // read in place; only computed properties and aliases take the full `get_prop` route. The
    // two give the same answer, the fast one just skips the class lookup and the clone.

    pub fn num(&self, id: Id, name: &str, default: f32) -> f32 {
        match self.get(id).and_then(|i| i.raw(name)) {
            Some(Value::Num(n)) => *n,
            Some(_) => default,
            None => match self.get_prop(id, name) {
                Some(Value::Num(n)) => n,
                _ => default,
            },
        }
    }

    pub fn flag(&self, id: Id, name: &str) -> bool {
        match self.get(id).and_then(|i| i.raw(name)) {
            Some(v) => matches!(v, Value::Bool(true)),
            None => matches!(self.get_prop(id, name), Some(Value::Bool(true))),
        }
    }

    pub fn vec3(&self, id: Id, name: &str) -> Vec3 {
        match self.get(id).and_then(|i| i.raw(name)) {
            Some(Value::V3(v) | Value::Color(v)) => *v,
            Some(_) => Vec3::ZERO,
            None => match self.get_prop(id, name) {
                Some(Value::V3(v) | Value::Color(v)) => v,
                _ => Vec3::ZERO,
            },
        }
    }

    pub fn string(&self, id: Id, name: &str) -> String {
        match self.get_prop(id, name) {
            Some(Value::Str(s) | Value::Enum(s)) => s,
            _ => String::new(),
        }
    }

    /// Like [`string`](Self::string) for a *stored* string or enum property, but borrowed: no
    /// allocation. Empty for anything else (a missing property, a computed one, another type).
    pub fn str_ref(&self, id: Id, name: &str) -> &str {
        match self.get(id).and_then(|i| i.raw(name)) {
            Some(Value::Str(s) | Value::Enum(s)) => s,
            _ => "",
        }
    }

    pub fn reference(&self, id: Id, name: &str) -> Option<Id> {
        match self.get_prop(id, name) {
            Some(Value::Ref(Some(r))) if self.exists(r) => Some(r),
            _ => None,
        }
    }

    /// Stores a value; returns the previous one when it actually changed.
    fn write(&mut self, id: Id, name: &str, v: Value) -> Option<Value> {
        let inst = self.get_mut(id)?;
        match inst.props.iter_mut().find(|(n, _)| *n == name) {
            Some((_, cur)) if *cur != v => Some(std::mem::replace(cur, v)),
            _ => None,
        }
    }

    pub fn set_prop(&mut self, id: Id, name: &str, value: Value) -> Result<(), String> {
        let class = self.get(id).ok_or("instance has been destroyed")?.class;
        let meta = &class.prop(name).ok_or_else(|| format!("{name} is not a valid member of {}", class.name))?.meta;
        self.set_resolved(id, class, meta, value)
    }

    /// [`set_prop`](Self::set_prop) for a property the caller has already looked up in the instance's
    /// class (the scripting layer needs the property's type to convert the value, so it has).
    pub fn set_resolved(&mut self, id: Id, class: &'static crate::core::ResolvedClass, meta: &'static PropMeta, value: Value) -> Result<(), String> {
        let uid = self.get(id).ok_or("instance has been destroyed")?.uid;
        if !value_matches(meta.ty, &value) {
            return Err(format!("invalid value type for {}", meta.name));
        }
        self.check_value(meta, &value)?;
        let value = clamp_value(meta, value);
        if let Some(validate) = meta.validator {
            validate(self, id, &value)?;
        }

        if let Some(setter) = meta.setter {
            setter(self, id, value)?;
            self.emit(uid, Event::Changed(id, meta.name, None));
            return Ok(());
        }
        if meta.getter.is_some() {
            return Err(format!("{} is read-only", meta.name));
        }

        // Writing the value a property already has is not a change: no dirty marks, no hooks, no
        // event, and none of the work below (the scene is not touched, nothing is re-rendered).
        if self.get(id).and_then(|i| i.raw(meta.name)).is_none_or(|cur| *cur == value) {
            return Ok(());
        }

        let new = self.recording().then(|| value.clone());
        let Some(old) = self.write(id, meta.name, value) else {
            return Ok(());
        };
        if let Some(new) = new {
            self.record(Change::Prop { id, name: meta.name, old, new });
        }

        if meta.flags & F_XFORM != 0 {
            self.mark(id, D_X);
        }
        if meta.flags & F_VISUAL != 0 {
            self.mark(id, D_V);
        }
        if meta.flags & F_PHYS != 0 {
            // The simulation brings itself up to date with this instance before its next step.
            self.phys.mark(id);
        }
        if meta.flags & F_SCRIPT != 0 {
            self.pending_scripts.push(id);
        }
        for comp in &class.components {
            if let Some(hook) = comp.on_changed {
                hook(self, id, meta);
            }
        }
        class.desc.behavior.on_prop_changed(self, id, meta);
        let carried = (meta.name == "Value" && class.has_component("ValueObject")).then(|| self.get(id).and_then(|i| i.raw("Value")).cloned().map(|v| (meta.ty, v))).flatten();
        self.emit(uid, Event::Changed(id, meta.name, carried));
        Ok(())
    }

    /// Rejects values that would corrupt the simulation or the scene: NaN / infinite numbers,
    /// enum items that do not exist and references to the wrong class or a destroyed instance.
    /// (The scripting layer checks these too, but scene files and the editor reach `set_prop` directly.)
    fn check_value(&self, meta: &PropMeta, value: &Value) -> Result<(), String> {
        let finite = match value {
            Value::Num(n) => n.is_finite(),
            Value::V2(v) => v.is_finite(),
            Value::V3(v) | Value::Color(v) => v.is_finite(),
            Value::CF(p, q) => p.is_finite() && q.is_finite(),
            Value::Data(_, numbers) => numbers.iter().all(|n| n.is_finite()),
            _ => true,
        };
        if !finite {
            return Err(format!("{} must be a finite number (got NaN or infinity)", meta.name));
        }
        match (meta.ty, value) {
            (PropType::Enum(t), Value::Enum(name)) => {
                let items = enum_names(t);
                if !items.is_empty() && !items.contains(&name.as_str()) {
                    return Err(format!("'{name}' is not a valid {t}"));
                }
            }
            (PropType::Ref(want), Value::Ref(Some(target))) => {
                if !self.exists(*target) {
                    return Err(format!("{} cannot refer to a destroyed instance", meta.name));
                }
                if want != "Instance" && !self.is_a(*target, want) {
                    return Err(format!("{} expects a {want}", meta.name));
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ---- attributes ---------------------------------------------------------------------------

    pub fn attr(&self, id: Id, name: &str) -> Option<Value> {
        self.get(id)?.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone())
    }

    /// All attributes of an instance, by name.
    pub fn attrs(&self, id: Id) -> Vec<(String, Value)> {
        let mut all = self.get(id).map(|i| i.attrs.clone()).unwrap_or_default();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    /// Why `name` cannot be the name of an attribute, or `None` if it can: Roblox allows letters, digits and
    /// underscores (at most 100), and keeps names that start with `RBX` for itself.
    pub fn attribute_name_problem(name: &str) -> Option<String> {
        if name.is_empty() || name.len() > 100 {
            return Some("attribute names must be 1 to 100 characters long".into());
        }
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Some(format!("'{name}' is not a valid attribute name: use only letters, digits and underscores"));
        }
        if name.starts_with("RBX") {
            return Some(format!("'{name}' is not a valid attribute name: names that start with RBX are reserved"));
        }
        None
    }

    /// Sets an attribute (`None` removes it) and tells scripts: `AttributeChanged(name)` and the signal of
    /// `GetAttributeChangedSignal(name)`. Setting the value an attribute already has is not a change.
    pub fn set_attr(&mut self, id: Id, name: &str, v: Option<Value>) {
        let Some(i) = self.get_mut(id) else { return };
        let old = i.attrs.iter().position(|(n, _)| n == name).map(|at| i.attrs.remove(at).1);
        if old == v {
            // (Put back where it was found, order does not matter but nothing may be lost.)
            if let Some(v) = old {
                i.attrs.push((name.to_owned(), v));
            }
            return;
        }
        if let Some(v) = v {
            i.attrs.push((name.to_owned(), v));
        }
        self.fire(id, "AttributeChanged", || vec![crate::core::ScriptValue::Str(name.to_owned())]);
        self.fire(id, &format!("AttributeChanged:{name}"), Vec::new);
    }
}

fn clamp_value(meta: &PropMeta, v: Value) -> Value {
    let v = match v {
        Value::Num(n) if meta.flags & F_INT != 0 => Value::Num(n.round()),
        other => other,
    };
    let Some((lo, hi)) = meta.clamp else { return v };
    match v {
        Value::Num(n) => Value::Num(n.clamp(lo, hi)),
        Value::V3(x) => Value::V3(x.clamp(Vec3::splat(lo), Vec3::splat(hi))),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use crate::{
        core::{Id, Value},
        dm::{DataModel, D_V, D_X},
    };

    fn part_in_workspace(dm: &mut DataModel) -> Id {
        let ws = dm.workspace;
        let p = dm.create("Part").unwrap();
        dm.set_parent(p, Some(ws)).unwrap();
        p
    }

    fn settle(dm: &mut DataModel) {
        for id in std::mem::take(&mut dm.dirty) {
            if let Some(i) = dm.get_mut(id) {
                i.dirty = 0;
            }
        }
        dm.events.clear();
    }

    #[test]
    fn writing_the_value_a_property_already_has_changes_nothing() {
        let mut dm = DataModel::new();
        let part = part_in_workspace(&mut dm);
        dm.set_prop(part, "Anchored", Value::Bool(true)).unwrap();
        dm.set_prop(part, "Position", Value::V3(Vec3::new(1.0, 2.0, 3.0))).unwrap();
        dm.watch(dm.get(part).unwrap().uid);
        dm.phys.enabled = true;
        settle(&mut dm);
        let _ = dm.phys.take();

        for (name, v) in [
            ("CFrame", Value::CF(Vec3::new(1.0, 2.0, 3.0), bevy::math::Quat::IDENTITY)),
            ("Size", Value::V3(Vec3::new(4.0, 1.0, 2.0))),
            ("Color", Value::Color(Vec3::new(0.64, 0.64, 0.64))),
            ("Anchored", Value::Bool(true)),
            ("Material", Value::Enum("Plastic".into())),
        ] {
            dm.set_prop(part, name, v).unwrap();
        }
        assert!(dm.dirty.is_empty(), "nothing for the renderer to look at");
        assert!(dm.events.is_empty(), "no Changed event: {:?}", dm.events);
        assert_eq!(dm.phys.pending(), 0, "and physics has nothing to update");

        dm.set_prop(part, "Color", Value::Color(Vec3::new(1.0, 0.0, 0.0))).unwrap();
        assert_eq!(dm.get(part).unwrap().dirty, D_V, "a real change marks exactly what it affects");
        dm.set_prop(part, "Size", Value::V3(Vec3::new(5.0, 1.0, 2.0))).unwrap();
        assert_eq!(dm.get(part).unwrap().dirty, D_V | D_X);
    }

    #[test]
    fn the_typed_readers_agree_with_get_prop() {
        let mut dm = DataModel::new();
        let part = part_in_workspace(&mut dm);
        dm.set_prop(part, "Color", Value::Color(Vec3::new(0.1, 0.2, 0.3))).unwrap();
        dm.set_prop(part, "Transparency", Value::Num(0.25)).unwrap();
        dm.set_prop(part, "Material", Value::Enum("Metal".into())).unwrap();
        assert_eq!(dm.vec3(part, "Color"), Vec3::new(0.1, 0.2, 0.3));
        assert_eq!(dm.num(part, "Transparency", 9.0), 0.25);
        assert_eq!(dm.string(part, "Material"), "Metal");
        assert_eq!(dm.str_ref(part, "Material"), "Metal");
        assert!(dm.flag(part, "CanCollide") && !dm.flag(part, "Anchored"));
        // Computed properties, a missing property and a value of another type take the general route.
        assert_eq!(dm.string(part, "Name"), "Part");
        assert_eq!(dm.str_ref(part, "Name"), "", "borrowed reads are for stored properties; computed ones are empty");
        assert_eq!(dm.vec3(part, "Position"), Vec3::ZERO);
        assert_eq!(dm.num(part, "NoSuchProperty", 7.0), 7.0);
        assert_eq!(dm.num(part, "Material", 7.0), 7.0, "a string is not a number");
        // Old names still resolve to the property they stand for.
        let light = dm.create("PointLight").unwrap();
        dm.set_prop(light, "Brightness", Value::Num(3.0)).unwrap();
        assert_eq!(dm.num(light, "Intensity", 0.0), 3.0);
        assert_eq!(dm.num(light, "Brightness", 0.0), 3.0);
    }

    #[test]
    fn physical_edits_are_noted_for_the_simulation_only_while_one_is_attached_and_only_when_made_by_someone_else() {
        let mut dm = DataModel::new();
        let part = part_in_workspace(&mut dm);
        dm.set_prop(part, "Anchored", Value::Bool(true)).unwrap();
        assert_eq!(dm.phys.pending(), 0, "nothing is recorded without a simulation (an editor that is not playing)");

        dm.phys.enabled = true;
        dm.set_prop(part, "Color", Value::Color(Vec3::new(1.0, 0.0, 0.0))).unwrap();
        assert_eq!(dm.phys.pending(), 0, "a colour is not physical");
        dm.set_prop(part, "Position", Value::V3(Vec3::X)).unwrap();
        dm.set_prop(part, "Size", Value::V3(Vec3::splat(3.0))).unwrap();
        dm.set_prop(part, "CanCollide", Value::Bool(false)).unwrap();
        assert_eq!(dm.phys.take(), vec![part], "one entry however many edits, in id order");
        assert_eq!(dm.phys.pending(), 0, "taking empties the list");

        dm.phys.muted = true;
        dm.set_prop(part, "Position", Value::V3(Vec3::Y)).unwrap();
        dm.phys.muted = false;
        assert_eq!(dm.phys.pending(), 0, "what the simulation writes back is not an edit");
        assert_eq!(dm.vec3(part, "Position"), Vec3::Y, "but it is written");
    }
}

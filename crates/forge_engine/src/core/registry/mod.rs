//! The class registry.
//!
//! Every class lives in its own file under `src/classes/` and exports a
//! `pub static CLASS: ClassDesc`; components shared by several classes live under
//! `src/components/` (a class file may also export a component of its own). The
//! build script (`build.rs`) discovers those files, so adding a class or a
//! component means adding one file - nothing else has to change.
//!
//! At start-up the registry *resolves* each class: it walks the inheritance chain
//! (root first), adds the properties of the components the classes declare, applies
//! overrides (`hide`, `defaults`) and sorts the result for the editor. Scripting,
//! object creation, serialization, the inspector and the runtime all read this one
//! resolved description.
//!
//! * [`desc`]    - the descriptors classes and components export
//! * [`resolve`] - inheritance + component resolution into [`ResolvedClass`]

mod desc;
mod resolve;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap},
    sync::OnceLock,
};

pub use desc::{ClassDesc, ComponentDesc, EnumDesc};
pub use resolve::{ResolvedClass, ResolvedProp};

use crate::core::{hooks::PhysicsFn, value::FastMap};

// ---- the registry ---------------------------------------------------------------------------------

pub struct Registry {
    classes: Vec<ResolvedClass>,
    by_name: FastMap<&'static str, usize>,
    components: HashMap<&'static str, &'static ComponentDesc>,
    enums: BTreeMap<&'static str, &'static EnumDesc>,
    services: Vec<usize>,
    root: usize,
    collider_components: Vec<&'static str>,
    physics_systems: Vec<(i16, &'static str, PhysicsFn)>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// The global registry (built on first use from the discovered class/component files).
pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(Registry::build)
}

pub fn class(name: &str) -> Option<&'static ResolvedClass> {
    registry().class(name)
}

pub fn enum_names(name: &str) -> &'static [&'static str] {
    registry().enums.get(name).map_or(&[], |e| e.values)
}

impl Registry {
    fn build() -> Registry {
        let mut descs: HashMap<&'static str, &'static ClassDesc> = HashMap::new();
        for d in crate::classes::ALL_CLASSES.iter().copied() {
            assert!(descs.insert(d.name, d).is_none(), "class '{}' is registered twice", d.name);
        }
        // Shared components (src/components) and the ones class files define for themselves.
        let mut components: HashMap<&'static str, &'static ComponentDesc> = HashMap::new();
        for c in crate::components::ALL_COMPONENTS.iter().chain(crate::classes::OWNED_COMPONENTS.iter()).copied() {
            assert!(components.insert(c.name, c).is_none(), "component '{}' is registered twice", c.name);
        }
        let mut enums: BTreeMap<&'static str, &'static EnumDesc> = BTreeMap::new();
        for list in crate::classes::ENUM_LISTS.iter().copied() {
            for e in list.iter() {
                assert!(enums.insert(e.name, e).is_none(), "enum '{}' is registered twice", e.name);
            }
        }

        let mut names: Vec<&'static str> = descs.keys().copied().collect();
        names.sort_unstable();
        let classes: Vec<ResolvedClass> = names.iter().map(|n| resolve::resolve(descs[n], &descs, &components)).collect();
        let by_name: FastMap<&'static str, usize> = classes.iter().enumerate().map(|(i, c)| (c.name, i)).collect();

        let mut services: Vec<usize> = (0..classes.len()).filter(|i| classes[*i].desc.service_order.is_some()).collect();
        services.sort_by_key(|i| (classes[*i].desc.service_order, classes[*i].name));
        let root = classes.iter().position(|c| c.desc.root).expect("no class is marked as the root (ClassDesc::root)");

        let mut collider_components: Vec<&'static str> = components.values().filter(|c| c.collider.is_some()).map(|c| c.name).collect();
        collider_components.sort_unstable();
        let mut physics_systems: Vec<(i16, &'static str, PhysicsFn)> = components.values().filter_map(|c| c.physics.map(|f| (c.physics_order, c.name, f))).collect();
        physics_systems.sort_by_key(|(order, name, _)| (*order, *name));

        Registry { classes, by_name, components, enums, services, root, collider_components, physics_systems }
    }

    pub fn class(&'static self, name: &str) -> Option<&'static ResolvedClass> {
        self.by_name.get(name).map(|i| &self.classes[*i])
    }

    pub fn classes(&'static self) -> &'static [ResolvedClass] {
        &self.classes
    }

    pub fn root(&'static self) -> &'static ResolvedClass {
        &self.classes[self.root]
    }

    /// Services in creation order.
    pub fn services(&'static self) -> Vec<&'static ResolvedClass> {
        self.services.iter().map(|i| &self.classes[*i]).collect()
    }

    /// Classes the editor/scripts can create, sorted by category then name.
    pub fn creatable(&'static self) -> Vec<&'static ResolvedClass> {
        let mut v: Vec<&'static ResolvedClass> = self.classes.iter().filter(|c| c.desc.creatable).collect();
        v.sort_by_key(|c| (c.desc.category, c.name));
        v
    }

    pub fn component(&'static self, name: &str) -> Option<&'static ComponentDesc> {
        self.components.get(name).copied()
    }

    pub fn enums(&'static self) -> impl Iterator<Item = &'static EnumDesc> {
        self.enums.values().copied()
    }

    pub fn collider_components(&'static self) -> &'static [&'static str] {
        &self.collider_components
    }

    pub fn physics_systems(&'static self) -> &'static [(i16, &'static str, PhysicsFn)] {
        &self.physics_systems
    }
}

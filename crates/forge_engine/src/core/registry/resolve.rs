//! Resolution: turns a class's descriptor plus its inheritance chain and components
//! into the single flat description (`ResolvedClass`) every system reads.

use std::collections::HashMap;

use super::desc::{ClassDesc, ComponentDesc};
use crate::core::{
    meta::{MethodDesc, PropMeta, SignalDesc},
    value::FastMap,
};

// ---- resolved classes -----------------------------------------------------------------------

pub struct ResolvedProp {
    pub meta: PropMeta,
    /// The class or component that declared the property.
    pub owner: &'static str,
}

pub struct ResolvedClass {
    pub name: &'static str,
    pub desc: &'static ClassDesc,
    /// This class first, then its parents up to the root.
    pub chain: Vec<&'static ClassDesc>,
    /// All properties (inherited and from components), in inspector order.
    pub props: Vec<ResolvedProp>,
    pub components: Vec<&'static ComponentDesc>,
    pub relative_transform: bool,
    prop_index: FastMap<&'static str, usize>,
    /// Methods and signals by name, nearest class first (what `method` / `signal` find by walking `chain`).
    method_index: FastMap<&'static str, &'static MethodDesc>,
    signal_index: FastMap<&'static str, &'static SignalDesc>,
}

impl ResolvedClass {
    pub fn is_a(&self, name: &str) -> bool {
        self.chain.iter().any(|c| c.name == name)
    }

    /// Looks a property up by name or alias.
    pub fn prop(&self, name: &str) -> Option<&ResolvedProp> {
        self.prop_index.get(name).map(|i| &self.props[*i])
    }

    pub fn has_component(&self, name: &str) -> bool {
        self.components.iter().any(|c| c.name == name)
    }

    pub fn signal(&self, name: &str) -> Option<&'static SignalDesc> {
        self.signal_index.get(name).copied()
    }

    pub fn method(&self, name: &str) -> Option<&'static MethodDesc> {
        self.method_index.get(name).copied()
    }

    /// Whether `name` is a method Forge lists but cannot do (see `ClassDesc::unsupported`).
    pub fn is_unsupported(&self, name: &str) -> bool {
        self.chain.iter().any(|c| c.unsupported.contains(&name))
    }

    /// Whether scripts assign a function to the member `name` (see `ClassDesc::callbacks`).
    pub fn is_callback(&self, name: &str) -> bool {
        self.chain.iter().any(|c| c.callbacks.contains(&name))
    }

    /// Distinct inspector groups in display order.
    pub fn groups(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for p in &self.props {
            let g = p.meta.display_group();
            if !out.contains(&g) {
                out.push(g);
            }
        }
        out
    }
}

fn upsert(props: &mut Vec<ResolvedProp>, p: ResolvedProp) {
    match props.iter_mut().find(|e| e.meta.name == p.meta.name) {
        Some(existing) => *existing = p,
        None => props.push(p),
    }
}

fn add_component(
    name: &'static str,
    owner: &'static str,
    components: &mut Vec<&'static ComponentDesc>,
    props: &mut Vec<ResolvedProp>,
    all: &HashMap<&'static str, &'static ComponentDesc>,
) {
    if components.iter().any(|c| c.name == name) {
        return;
    }
    let comp: &'static ComponentDesc = *all.get(name).unwrap_or_else(|| panic!("class '{owner}' uses unknown component '{name}'"));
    for req in comp.requires {
        add_component(req, owner, components, props, all);
    }
    components.push(comp);
    for p in comp.props {
        upsert(props, ResolvedProp { meta: *p, owner: comp.name });
    }
}

pub(super) fn resolve(desc: &'static ClassDesc, classes: &HashMap<&'static str, &'static ClassDesc>, components: &HashMap<&'static str, &'static ComponentDesc>) -> ResolvedClass {
    // inheritance chain, self first
    let mut chain = vec![desc];
    while let Some(parent) = chain.last().and_then(|c| c.parent) {
        let p: &'static ClassDesc = *classes.get(parent).unwrap_or_else(|| panic!("class '{}' has unknown parent '{parent}'", chain.last().map_or("?", |c| c.name)));
        chain.push(p);
        assert!(chain.len() < 64, "inheritance cycle at class '{}'", desc.name);
    }

    let mut props: Vec<ResolvedProp> = Vec::new();
    let mut comps: Vec<&'static ComponentDesc> = Vec::new();
    let mut groups: Vec<&'static str> = Vec::new();
    let mut relative = false;

    for class in chain.iter().rev() {
        for g in class.groups {
            if !groups.contains(g) {
                groups.push(g);
            }
        }
        relative |= class.relative_transform;
        for c in class.components {
            add_component(c, class.name, &mut comps, &mut props, components);
        }
        for p in class.props {
            upsert(&mut props, ResolvedProp { meta: *p, owner: class.name });
        }
        for h in class.hide {
            if let Some(p) = props.iter_mut().find(|p| p.meta.name == *h) {
                p.meta.visible = false;
            }
        }
        for (name, default) in class.defaults {
            if let Some(p) = props.iter_mut().find(|p| p.meta.name == *name) {
                p.meta.default = *default;
            }
        }
    }

    // Stable sort: group order first, then `order`, then declaration order.
    let rank = |g: &str| groups.iter().position(|x| *x == g).unwrap_or(groups.len());
    props.sort_by_key(|p| (rank(p.meta.display_group()), p.meta.order));

    let mut prop_index = FastMap::default();
    for (i, p) in props.iter().enumerate() {
        prop_index.insert(p.meta.name, i);
        for a in p.meta.aliases {
            prop_index.insert(*a, i);
        }
    }

    // The nearest class wins, as it does when walking the chain.
    let mut method_index: FastMap<&'static str, &'static MethodDesc> = FastMap::default();
    let mut signal_index: FastMap<&'static str, &'static SignalDesc> = FastMap::default();
    for class in &chain {
        for m in class.methods {
            method_index.entry(m.name).or_insert(m);
        }
        for s in class.signals {
            signal_index.entry(s.name).or_insert(s);
        }
    }

    ResolvedClass { name: desc.name, desc, chain, props, components: comps, relative_transform: relative, prop_index, method_index, signal_index }
}

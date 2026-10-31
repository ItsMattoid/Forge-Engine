//! Class, component and enum descriptors: the declarative data every class and
//! component file exports (`pub static CLASS` / `COMPONENT` / `ENUMS`).

use crate::core::{
    behavior::{ClassBehavior, NO_BEHAVIOR},
    hooks::{AnimateFn, CameraEffectFn, ColliderFn, EffectFn, GizmoFn, OnChangedFn, PhysicsFn, RenderFn},
    meta::{MethodDesc, ParentRule, PropMeta, SignalDesc},
    value::D,
};

// ---- class descriptor ---------------------------------------------------------------------

pub struct ClassDesc {
    pub name: &'static str,
    pub parent: Option<&'static str>,
    pub description: &'static str,
    /// Editor "Insert" menu category.
    pub category: &'static str,
    pub creatable: bool,
    /// Services exist exactly once per game, created in this order.
    pub service_order: Option<i16>,
    /// The Explorer does not list this service: it exists for scripts (`GetService`) and has nothing to edit.
    pub hidden_in_explorer: bool,
    /// The contents of this service are saved in scene files.
    pub persisted: bool,
    /// Scripts under this service run.
    pub runs_scripts: bool,
    /// The editor inserts new objects into instances of this class.
    pub container: bool,
    /// The service that holds the 3D world (physics settings, "in the world" checks).
    pub world_root: bool,
    /// The root of the instance tree (`game`).
    pub root: bool,
    /// May appear in scene files although scripts cannot create it (an object a service makes for itself and saves, such as
    /// the Workspace's `Terrain`).
    pub scene_object: bool,
    /// The object's transform is relative to a spatial parent (like attachments/lights).
    pub relative_transform: bool,
    /// The editor places new instances where the camera looks.
    pub spawn_at_focus: bool,
    pub parent_rule: ParentRule,
    /// Values the editor sets on newly inserted instances.
    pub insert_props: &'static [(&'static str, D)],
    /// Creating this class in the editor also creates a source file with this text.
    pub script_stub: Option<&'static str>,
    pub props: &'static [PropMeta],
    /// Inherited properties to hide in the inspector for this class.
    pub hide: &'static [&'static str],
    /// Overrides of inherited default values.
    pub defaults: &'static [(&'static str, D)],
    /// Inspector group order contributed by this class.
    pub groups: &'static [&'static str],
    pub components: &'static [&'static str],
    pub methods: &'static [MethodDesc],
    pub signals: &'static [SignalDesc],
    /// Members scripts assign a function to (`BindableFunction.OnInvoke`). The host keeps the function; the
    /// class's Luau snippet calls it.
    pub callbacks: &'static [&'static str],
    /// Methods Roblox has that Forge cannot do (they need its online services, or an engine feature Forge does not
    /// have): scripts find them, and calling one raises an error that says it is not supported.
    pub unsupported: &'static [&'static str],
    /// Luau source that adds methods implemented in Luau (see `scripting`).
    pub luau: &'static str,
    pub behavior: &'static dyn ClassBehavior,
}

impl ClassDesc {
    pub const fn new(name: &'static str, parent: Option<&'static str>) -> Self {
        ClassDesc {
            name,
            parent,
            description: "",
            category: "Other",
            creatable: false,
            service_order: None,
            hidden_in_explorer: false,
            persisted: false,
            runs_scripts: false,
            container: false,
            world_root: false,
            root: false,
            relative_transform: false,
            scene_object: false,
            spawn_at_focus: false,
            parent_rule: ParentRule::Auto,
            insert_props: &[],
            script_stub: None,
            props: &[],
            hide: &[],
            defaults: &[],
            groups: &[],
            components: &[],
            methods: &[],
            signals: &[],
            callbacks: &[],
            unsupported: &[],
            luau: "",
            behavior: NO_BEHAVIOR,
        }
    }
    pub const fn describe(mut self, d: &'static str) -> Self {
        self.description = d;
        self
    }
    pub const fn category(mut self, c: &'static str) -> Self {
        self.category = c;
        self
    }
    pub const fn creatable(mut self) -> Self {
        self.creatable = true;
        self
    }
    pub const fn service(mut self, order: i16) -> Self {
        self.service_order = Some(order);
        self
    }
    pub const fn hidden_in_explorer(mut self) -> Self {
        self.hidden_in_explorer = true;
        self
    }
    pub const fn persisted(mut self) -> Self {
        self.persisted = true;
        self
    }
    pub const fn runs_scripts(mut self) -> Self {
        self.runs_scripts = true;
        self
    }
    pub const fn container(mut self) -> Self {
        self.container = true;
        self
    }
    pub const fn world_root(mut self) -> Self {
        self.world_root = true;
        self
    }
    pub const fn root(mut self) -> Self {
        self.root = true;
        self
    }
    pub const fn scene_object(mut self) -> Self {
        self.scene_object = true;
        self
    }
    pub const fn relative_transform(mut self) -> Self {
        self.relative_transform = true;
        self
    }
    pub const fn spawn_at_focus(mut self) -> Self {
        self.spawn_at_focus = true;
        self
    }
    pub const fn parent_rule(mut self, r: ParentRule) -> Self {
        self.parent_rule = r;
        self
    }
    pub const fn insert_props(mut self, p: &'static [(&'static str, D)]) -> Self {
        self.insert_props = p;
        self
    }
    pub const fn script_stub(mut self, text: &'static str) -> Self {
        self.script_stub = Some(text);
        self
    }
    pub const fn props(mut self, p: &'static [PropMeta]) -> Self {
        self.props = p;
        self
    }
    pub const fn hide(mut self, h: &'static [&'static str]) -> Self {
        self.hide = h;
        self
    }
    pub const fn defaults(mut self, d: &'static [(&'static str, D)]) -> Self {
        self.defaults = d;
        self
    }
    pub const fn groups(mut self, g: &'static [&'static str]) -> Self {
        self.groups = g;
        self
    }
    pub const fn components(mut self, c: &'static [&'static str]) -> Self {
        self.components = c;
        self
    }
    pub const fn methods(mut self, m: &'static [MethodDesc]) -> Self {
        self.methods = m;
        self
    }
    pub const fn signals(mut self, s: &'static [SignalDesc]) -> Self {
        self.signals = s;
        self
    }
    pub const fn callbacks(mut self, names: &'static [&'static str]) -> Self {
        self.callbacks = names;
        self
    }
    pub const fn unsupported(mut self, names: &'static [&'static str]) -> Self {
        self.unsupported = names;
        self
    }
    pub const fn luau(mut self, source: &'static str) -> Self {
        self.luau = source;
        self
    }
    pub const fn behavior(mut self, b: &'static dyn ClassBehavior) -> Self {
        self.behavior = b;
        self
    }
}

// ---- component descriptor ------------------------------------------------------------------

/// A reusable bundle of properties and systems. Classes list the components they
/// use; the engine's systems (renderer, physics, scripting, editor gizmos) run the
/// hooks of every instance that has the component.
pub struct ComponentDesc {
    pub name: &'static str,
    pub description: &'static str,
    /// Properties every class using the component gets.
    pub props: &'static [PropMeta],
    /// Other components this one needs.
    pub requires: &'static [&'static str],
    /// Mirrors the instance into Bevy (called for changed instances).
    pub render: Option<RenderFn>,
    /// Per-frame physics system; lower `physics_order` runs first.
    pub physics: Option<PhysicsFn>,
    pub physics_order: i16,
    /// Describes the instance's collision volume (used by physics, ray casts, picking).
    pub collider: Option<ColliderFn>,
    /// Reacts to property changes.
    pub on_changed: Option<OnChangedFn>,
    /// Shapes drawn in the editor viewport while the instance is selected.
    pub gizmo: Option<GizmoFn>,
    /// The audio processor the instance stands for (a sound effect inside a Sound or SoundGroup).
    pub audio_effect: Option<EffectFn>,
    /// What the instance does to every camera (a screen effect, the sky): see `render::camera_fx`.
    pub camera_effect: Option<CameraEffectFn>,
    /// Called every frame (effects that move on their own: particles, trails ...).
    pub animate: Option<AnimateFn>,
}

impl ComponentDesc {
    pub const fn new(name: &'static str) -> Self {
        ComponentDesc {
            name,
            description: "",
            props: &[],
            requires: &[],
            render: None,
            physics: None,
            physics_order: 0,
            collider: None,
            on_changed: None,
            gizmo: None,
            audio_effect: None,
            camera_effect: None,
            animate: None,
        }
    }
    pub const fn describe(mut self, d: &'static str) -> Self {
        self.description = d;
        self
    }
    pub const fn props(mut self, p: &'static [PropMeta]) -> Self {
        self.props = p;
        self
    }
    pub const fn requires(mut self, r: &'static [&'static str]) -> Self {
        self.requires = r;
        self
    }
    pub const fn render(mut self, f: RenderFn) -> Self {
        self.render = Some(f);
        self
    }
    pub const fn physics(mut self, order: i16, f: PhysicsFn) -> Self {
        self.physics = Some(f);
        self.physics_order = order;
        self
    }
    pub const fn collider(mut self, f: ColliderFn) -> Self {
        self.collider = Some(f);
        self
    }
    pub const fn on_changed(mut self, f: OnChangedFn) -> Self {
        self.on_changed = Some(f);
        self
    }
    pub const fn gizmo(mut self, f: GizmoFn) -> Self {
        self.gizmo = Some(f);
        self
    }
    pub const fn audio_effect(mut self, f: EffectFn) -> Self {
        self.audio_effect = Some(f);
        self
    }
    pub const fn camera_effect(mut self, f: CameraEffectFn) -> Self {
        self.camera_effect = Some(f);
        self
    }
    pub const fn animate(mut self, f: AnimateFn) -> Self {
        self.animate = Some(f);
        self
    }
}

// ---- enums -------------------------------------------------------------------------------------

/// A named set of values exposed to scripts as `Enum.<Name>.<Value>`.
pub struct EnumDesc {
    pub name: &'static str,
    pub values: &'static [&'static str],
    /// The `Value` of each item (`Enum.Material.Plastic.Value`); empty means 0, 1, 2 ... in order.
    pub numbers: &'static [i32],
    /// Other names that stand for an item: `(alias, item)`. The item is what a script gets for the alias
    /// (`Enum.RaycastFilterType.Blacklist` is `Exclude`), so old scripts keep working without listing it twice.
    pub aliases: &'static [(&'static str, &'static str)],
}

impl EnumDesc {
    pub const fn new(name: &'static str, values: &'static [&'static str]) -> Self {
        EnumDesc { name, values, numbers: &[], aliases: &[] }
    }

    /// An enum whose items have the numbers Roblox gives them (`numbers[i]` is the value of `values[i]`).
    pub const fn numbered(name: &'static str, values: &'static [&'static str], numbers: &'static [i32]) -> Self {
        assert!(values.len() == numbers.len(), "an enum needs one number per item");
        EnumDesc { name, values, numbers, aliases: &[] }
    }

    pub const fn aliases(mut self, aliases: &'static [(&'static str, &'static str)]) -> Self {
        self.aliases = aliases;
        self
    }

    /// The `Value` of item `i`.
    pub fn number(&self, i: usize) -> i32 {
        self.numbers.get(i).copied().unwrap_or(i as i32)
    }
}

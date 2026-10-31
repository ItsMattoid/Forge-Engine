//! Declarative metadata: how a property looks to scripts, files and the editor.
//!
//! Everything here is `const`-constructible so classes can describe themselves in
//! `static` items:
//!
//! ```ignore
//! PropMeta::number("Range", 16.0).group("Light").desc("How far the light reaches.").slider(0.0, 200.0)
//! ```

use bevy::math::{Quat, Vec3};

use super::{
    script::MethodFn,
    value::{Id, PropType, Value, D},
};
use crate::dm::DataModel;

/// Changing the property moves/resizes the object (the renderer refreshes its transform).
pub const F_XFORM: u8 = 1;
/// Changing the property changes how the object looks.
pub const F_VISUAL: u8 = 2;
/// Changing the property may change collision.
pub const F_PHYS: u8 = 4;
/// Runtime-only state: never saved to scene files.
pub const F_RUNTIME: u8 = 8;
/// Changing the property affects whether a script runs.
pub const F_SCRIPT: u8 = 16;
/// Not saved to scene files (derived, stored elsewhere, or saved by another property).
pub const F_NOSAVE: u8 = 32;
/// A whole number: writes are rounded (Roblox's `int` properties, such as `IntValue.Value`).
pub const F_INT: u8 = 64;

/// Which editor control to use for a property.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Widget {
    /// Pick by property type.
    Auto,
    Slider { min: f32, max: f32 },
    Drag { speed: f32 },
    Multiline,
}

/// Show a property only while another property has a given value.
#[derive(Clone, Copy, Debug)]
pub enum Cond {
    Eq(&'static str, D),
    Ne(&'static str, D),
    True(&'static str),
    False(&'static str),
}

impl Cond {
    pub fn evaluate(&self, dm: &DataModel, id: Id) -> bool {
        match *self {
            Cond::Eq(p, d) => dm.get_prop(id, p).is_some_and(|v| v == d.to_value()),
            Cond::Ne(p, d) => dm.get_prop(id, p).is_none_or(|v| v != d.to_value()),
            Cond::True(p) => matches!(dm.get_prop(id, p), Some(Value::Bool(true))),
            Cond::False(p) => !matches!(dm.get_prop(id, p), Some(Value::Bool(true))),
        }
    }
}

pub type Getter = fn(&DataModel, Id) -> Option<Value>;
pub type Setter = fn(&mut DataModel, Id, Value) -> Result<(), String>;
/// Checks a value for a property of one particular instance (what the type alone cannot say: a Model's
/// `PrimaryPart` must be inside that model). Runs for every write, whoever makes it.
pub type Validator = fn(&DataModel, Id, &Value) -> Result<(), String>;

/// Everything the engine knows about one property.
#[derive(Clone, Copy)]
pub struct PropMeta {
    pub name: &'static str,
    /// Display name in the editor (falls back to `name`).
    pub label: &'static str,
    /// Tooltip text.
    pub description: &'static str,
    /// Inspector category (falls back to "Data").
    pub group: &'static str,
    pub ty: PropType,
    pub default: D,
    /// Sort key inside the group (lower first).
    pub order: i16,
    /// Shown in the inspector.
    pub visible: bool,
    /// Shown but not editable in the inspector.
    pub readonly: bool,
    pub widget: Widget,
    pub visible_if: Option<Cond>,
    pub flags: u8,
    /// Older names that still work in scripts and scene files.
    pub aliases: &'static [&'static str],
    /// Value limits (numbers; applied per component for vectors).
    pub clamp: Option<(f32, f32)>,
    /// Computed property: read through this function instead of the stored value.
    pub getter: Option<Getter>,
    /// Computed property: writes go through this function.
    pub setter: Option<Setter>,
    /// Rejects values that do not suit the instance the property belongs to (see [`Validator`]).
    pub validator: Option<Validator>,
}

impl PropMeta {
    pub const fn new(name: &'static str, ty: PropType, default: D) -> Self {
        PropMeta {
            name,
            label: "",
            description: "",
            group: "",
            ty,
            default,
            order: 0,
            visible: true,
            readonly: false,
            widget: Widget::Auto,
            visible_if: None,
            flags: 0,
            aliases: &[],
            clamp: None,
            getter: None,
            setter: None,
            validator: None,
        }
    }

    // ---- typed constructors ----
    pub const fn boolean(name: &'static str, default: bool) -> Self {
        Self::new(name, PropType::Bool, D::B(default))
    }
    pub const fn number(name: &'static str, default: f32) -> Self {
        Self::new(name, PropType::Number, D::N(default))
    }
    pub const fn string(name: &'static str, default: &'static str) -> Self {
        Self::new(name, PropType::String, D::S(default))
    }
    /// A whole number (see [`F_INT`]).
    pub const fn integer(name: &'static str, default: i32) -> Self {
        let mut p = Self::new(name, PropType::Number, D::N(default as f32));
        p.flags |= F_INT;
        p
    }
    pub const fn vector2(name: &'static str, x: f32, y: f32) -> Self {
        Self::new(name, PropType::Vector2, D::V2(x, y))
    }
    pub const fn vector3(name: &'static str, x: f32, y: f32, z: f32) -> Self {
        Self::new(name, PropType::Vector3, D::V(x, y, z))
    }
    /// A value type made of numbers (one of [`DATA_TYPES`](super::value::DATA_TYPES)); `default` are its numbers.
    pub const fn data(name: &'static str, tag: &'static str, default: &'static [f32]) -> Self {
        Self::new(name, PropType::Data(tag), D::Dn(tag, default))
    }
    pub const fn color3(name: &'static str, r: f32, g: f32, b: f32) -> Self {
        Self::new(name, PropType::Color3, D::C(r, g, b))
    }
    pub const fn cframe(name: &'static str) -> Self {
        Self::new(name, PropType::CFrame, D::Cf)
    }
    pub const fn enumeration(name: &'static str, enum_name: &'static str, default: &'static str) -> Self {
        Self::new(name, PropType::Enum(enum_name), D::E(default))
    }
    pub const fn reference(name: &'static str, class: &'static str) -> Self {
        Self::new(name, PropType::Ref(class), D::R)
    }
    pub const fn script_path(name: &'static str) -> Self {
        Self::new(name, PropType::ScriptPath, D::S(""))
    }
    pub const fn asset(name: &'static str, extension: &'static str) -> Self {
        Self::new(name, PropType::Asset(extension), D::S(""))
    }

    // ---- presentation ----
    pub const fn label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }
    pub const fn desc(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }
    pub const fn group(mut self, group: &'static str) -> Self {
        self.group = group;
        self
    }
    pub const fn order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }
    pub const fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }
    pub const fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }
    pub const fn widget(mut self, widget: Widget) -> Self {
        self.widget = widget;
        self
    }
    /// Slider editor that also limits the value to the range.
    pub const fn slider(mut self, min: f32, max: f32) -> Self {
        self.widget = Widget::Slider { min, max };
        self.clamp = Some((min, max));
        self
    }
    pub const fn drag(mut self, speed: f32) -> Self {
        self.widget = Widget::Drag { speed };
        self
    }
    pub const fn multiline(mut self) -> Self {
        self.widget = Widget::Multiline;
        self
    }

    // ---- conditional visibility ----
    pub const fn when_eq(mut self, prop: &'static str, value: D) -> Self {
        self.visible_if = Some(Cond::Eq(prop, value));
        self
    }
    pub const fn when_ne(mut self, prop: &'static str, value: D) -> Self {
        self.visible_if = Some(Cond::Ne(prop, value));
        self
    }
    pub const fn when_true(mut self, prop: &'static str) -> Self {
        self.visible_if = Some(Cond::True(prop));
        self
    }
    pub const fn when_false(mut self, prop: &'static str) -> Self {
        self.visible_if = Some(Cond::False(prop));
        self
    }

    // ---- behavior ----
    pub const fn flags(mut self, flags: u8) -> Self {
        self.flags |= flags;
        self
    }
    /// Runtime-only state (not saved).
    pub const fn runtime(mut self) -> Self {
        self.flags |= F_RUNTIME;
        self
    }
    pub const fn no_save(mut self) -> Self {
        self.flags |= F_NOSAVE;
        self
    }
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }
    pub const fn clamp(mut self, min: f32, max: f32) -> Self {
        self.clamp = Some((min, max));
        self
    }
    /// Computed, writable property.
    pub const fn computed(mut self, getter: Getter, setter: Setter) -> Self {
        self.getter = Some(getter);
        self.setter = Some(setter);
        self
    }
    /// Every write is checked against the instance by `validator` first.
    pub const fn validate(mut self, validator: Validator) -> Self {
        self.validator = Some(validator);
        self
    }
    /// Computed, read-only property.
    pub const fn computed_ro(mut self, getter: Getter) -> Self {
        self.getter = Some(getter);
        self.readonly = true;
        self
    }

    pub fn display_label(&self) -> &'static str {
        if self.label.is_empty() {
            self.name
        } else {
            self.label
        }
    }

    pub fn display_group(&self) -> &'static str {
        if self.group.is_empty() {
            "Data"
        } else {
            self.group
        }
    }

    /// Whether the property is written to scene files.
    pub fn saved(&self) -> bool {
        self.flags & (F_RUNTIME | F_NOSAVE) == 0 && (self.getter.is_none() || self.setter.is_some())
    }

    /// The editor may edit it (visible, writable).
    pub fn editable(&self) -> bool {
        !self.readonly && (self.getter.is_none() || self.setter.is_some())
    }
}

// ---- signals & methods ---------------------------------------------------------------

pub struct SignalDesc {
    pub name: &'static str,
    pub description: &'static str,
    /// Fires every frame: handlers are called directly instead of in a new thread each.
    pub hot: bool,
    /// Listening makes the physics system report touches for the instance.
    pub touch: bool,
}

impl SignalDesc {
    pub const fn new(name: &'static str) -> Self {
        SignalDesc { name, description: "", hot: false, touch: false }
    }
    pub const fn desc(mut self, d: &'static str) -> Self {
        self.description = d;
        self
    }
    pub const fn hot(mut self) -> Self {
        self.hot = true;
        self
    }
    pub const fn touch(mut self) -> Self {
        self.touch = true;
        self
    }
}

pub struct MethodDesc {
    pub name: &'static str,
    pub description: &'static str,
    pub func: MethodFn,
}

impl MethodDesc {
    pub const fn new(name: &'static str, func: MethodFn) -> Self {
        MethodDesc { name, description: "", func }
    }
    pub const fn desc(mut self, d: &'static str) -> Self {
        self.description = d;
        self
    }
}

// ---- editor integration ------------------------------------------------------------------

/// Where the editor inserts a new instance of a class.
#[derive(Clone, Copy, Debug)]
pub enum ParentRule {
    /// Inside the selected container (Model, Folder, Workspace), else the world root.
    Auto,
    /// Inside the selected instance, which must be of `class`.
    Selected { class: &'static str, hint: &'static str },
    /// Always inside the named service.
    Service(&'static str),
    /// Inside the selection if there is one, else the named service.
    SelectedOrService(&'static str),
}

/// A shape a component wants drawn in the editor viewport for the selected instance.
#[derive(Clone, Copy, Debug)]
pub enum GizmoShape {
    Box { center: Vec3, rotation: Quat, size: Vec3 },
    Sphere { center: Vec3, radius: f32 },
    Arrow { from: Vec3, to: Vec3 },
    /// A cone opening along `rotation`'s forward (-Z) axis from `apex`; `length` is measured
    /// along the axis and `radius` is that of the open end.
    Cone { apex: Vec3, rotation: Quat, length: f32, radius: f32 },
}

pub type GizmoFn = fn(&DataModel, Id, &mut Vec<GizmoShape>);

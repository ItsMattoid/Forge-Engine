//! A scripting-language-neutral calling convention for class methods.
//!
//! Classes declare their methods with plain Rust functions that take a
//! [`MethodCall`] and return a [`ScriptValue`]. The Luau binding converts to and
//! from this type, so class files never depend on the scripting library.

use bevy::math::{Quat, Vec2, Vec3};

use super::value::{Id, Value};
use crate::{dm::DataModel, physics::PhysicsBridge};

#[derive(Clone, Debug)]
pub enum ScriptValue {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Vec2(Vec2),
    Vec3(Vec3),
    CFrame(Vec3, Quat),
    Color(Vec3),
    Instance(Id),
    /// The name of an enum item (e.g. `"W"` for `Enum.KeyCode.W`), as it comes in from a script.
    Enum(String),
    /// An enum item going out to a script: the enum's name and the item's (`Enum.Material.Plastic`).
    EnumItem(&'static str, String),
    /// A value type that is only numbers (`core::DATA_TYPES`): its type name and numbers.
    Data(&'static str, Vec<f32>),
    /// A table that is one of Roblox's result types (`RaycastResult`): the type's name and its fields (a `Map`).
    Typed(&'static str, Box<ScriptValue>),
    List(Vec<ScriptValue>),
    Map(Vec<(String, ScriptValue)>),
    /// Several results (a method that returns more than one value).
    Multi(Vec<ScriptValue>),
}

impl ScriptValue {
    pub fn instances(ids: impl IntoIterator<Item = Id>) -> ScriptValue {
        ScriptValue::List(ids.into_iter().map(ScriptValue::Instance).collect())
    }

    pub fn opt_instance(id: Option<Id>) -> ScriptValue {
        id.map_or(ScriptValue::Nil, ScriptValue::Instance)
    }

    pub fn strings<S: Into<String>>(items: impl IntoIterator<Item = S>) -> ScriptValue {
        ScriptValue::List(items.into_iter().map(|s| ScriptValue::Str(s.into())).collect())
    }

    /// A property or attribute value as a script value.
    pub fn from_value(v: &Value) -> ScriptValue {
        match v {
            Value::Bool(b) => ScriptValue::Bool(*b),
            Value::Num(n) => ScriptValue::Num(f64::from(*n)),
            Value::Str(s) => ScriptValue::Str(s.clone()),
            Value::V2(v) => ScriptValue::Vec2(*v),
            Value::V3(v) => ScriptValue::Vec3(*v),
            Value::Color(c) => ScriptValue::Color(*c),
            Value::CF(p, q) => ScriptValue::CFrame(*p, *q),
            Value::Enum(e) => ScriptValue::Enum(e.clone()),
            Value::Ref(r) => r.map_or(ScriptValue::Nil, ScriptValue::Instance),
            Value::Data(tag, numbers) => ScriptValue::Data(tag, numbers.clone()),
        }
    }

    /// The value as something that can be stored (an attribute): `None` for nil, and for kinds that are not values
    /// (instances, lists, maps).
    pub fn to_value(&self) -> Option<Value> {
        Some(match self {
            ScriptValue::Bool(b) => Value::Bool(*b),
            ScriptValue::Num(n) => Value::Num(*n as f32),
            ScriptValue::Str(s) => Value::Str(s.clone()),
            ScriptValue::Vec2(v) => Value::V2(*v),
            ScriptValue::Vec3(v) => Value::V3(*v),
            ScriptValue::Color(c) => Value::Color(*c),
            ScriptValue::CFrame(p, q) => Value::CF(*p, *q),
            ScriptValue::Data(tag, numbers) => Value::Data(tag, numbers.clone()),
            _ => return None,
        })
    }
}

/// What the running game offers to methods (input state, quitting, scene switching).
pub trait HostServices {
    fn key_down(&self, key: &str) -> bool;
    fn mouse_button_down(&self, button: &str) -> bool;
    fn mouse_delta(&self) -> Vec2;
    fn mouse_position(&self) -> Vec2;
    fn quit(&self);
    fn load_scene(&self, name: String);
}

pub struct MethodCall<'a> {
    pub dm: &'a mut DataModel,
    pub this: Id,
    pub args: Vec<ScriptValue>,
    pub host: &'a dyn HostServices,
    /// The physics simulation (impulses, queries): see [`crate::physics`].
    pub physics: &'a mut PhysicsBridge,
}

impl MethodCall<'_> {
    pub fn arg(&self, i: usize) -> &ScriptValue {
        self.args.get(i).unwrap_or(&ScriptValue::Nil)
    }

    pub fn string(&self, i: usize) -> Result<String, String> {
        match self.arg(i) {
            ScriptValue::Str(s) | ScriptValue::Enum(s) => Ok(s.clone()),
            _ => Err(format!("argument {} must be a string", i + 1)),
        }
    }

    pub fn number(&self, i: usize) -> Result<f64, String> {
        match self.arg(i) {
            ScriptValue::Num(n) => Ok(*n),
            _ => Err(format!("argument {} must be a number", i + 1)),
        }
    }

    pub fn opt_bool(&self, i: usize) -> Option<bool> {
        match self.arg(i) {
            ScriptValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn opt_number(&self, i: usize) -> Option<f64> {
        match self.arg(i) {
            ScriptValue::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// A string argument that may be left out (or nil).
    pub fn opt_string(&self, i: usize) -> Result<Option<String>, String> {
        match self.arg(i) {
            ScriptValue::Nil => Ok(None),
            _ => self.string(i).map(Some),
        }
    }

    /// An instance argument that may be left out (or nil).
    pub fn opt_instance(&self, i: usize) -> Result<Option<Id>, String> {
        match self.arg(i) {
            ScriptValue::Nil => Ok(None),
            _ => self.instance(i).map(Some),
        }
    }

    pub fn vec2(&self, i: usize) -> Result<Vec2, String> {
        match self.arg(i) {
            ScriptValue::Vec2(v) => Ok(*v),
            _ => Err(format!("argument {} must be a Vector2", i + 1)),
        }
    }

    pub fn vec3(&self, i: usize) -> Result<Vec3, String> {
        match self.arg(i) {
            ScriptValue::Vec3(v) => Ok(*v),
            _ => Err(format!("argument {} must be a Vector3", i + 1)),
        }
    }

    pub fn cframe(&self, i: usize) -> Result<(Vec3, Quat), String> {
        match self.arg(i) {
            ScriptValue::CFrame(p, q) => Ok((*p, *q)),
            _ => Err(format!("argument {} must be a CFrame", i + 1)),
        }
    }

    pub fn instance(&self, i: usize) -> Result<Id, String> {
        match self.arg(i) {
            ScriptValue::Instance(id) => Ok(*id),
            _ => Err(format!("argument {} must be an Instance", i + 1)),
        }
    }

    /// Looks up a field of a table argument.
    pub fn field(&self, i: usize, key: &str) -> Option<&ScriptValue> {
        match self.arg(i) {
            ScriptValue::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

pub type MethodFn = fn(&mut MethodCall) -> Result<ScriptValue, String>;

/// The error of a method that exists so that scripts which call it find it, but that Forge cannot do: it needs
/// Roblox's online services (accounts, the marketplace, data stores, teleporting ...) or an engine feature
/// (terrain, pathfinding) that is not part of Forge. The message says so, so a script's author knows why it failed.
pub fn unsupported<T>(what: &str) -> Result<T, String> {
    Err(format!("{what} is not supported by Forge (it needs Roblox's online services or a feature the engine does not have)"))
}

//! The data-driven core: value types, property metadata, the class registry and
//! the scripting-neutral method interface. Nothing in here knows about any
//! specific class.

pub mod behavior;
pub mod brickcolor;
pub mod hooks;
pub mod meta;
pub mod registry;
pub mod script;
pub mod value;

pub use behavior::{ClassBehavior, NoBehavior, NO_BEHAVIOR};
pub use meta::*;
pub use hooks::{ColliderFn, GizmoFn, OnChangedFn, PhysicsFn, RenderFn};
pub use registry::{class, enum_names, registry, ClassDesc, ComponentDesc, EnumDesc, Registry, ResolvedClass, ResolvedProp};
pub use script::{unsupported, HostServices, MethodCall, MethodFn, ScriptValue};
pub use value::*;

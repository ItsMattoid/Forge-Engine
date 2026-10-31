//! The API: one file per class, grouped by domain.
//!
//! ```text
//! classes/
//!   base/        Object, Instance, WorldObject, Folder      the foundation every class builds on
//!   world/       Model, Camera
//!     parts/     BasePart, Part, MeshPart                   solid things
//!     lights/    Light, PointLight, SpotLight, AreaLight, DirectionalLight
//!   characters/  Player (there is no built-in character: controllers are scripts)
//!   scripts/     LuaSourceContainer, BaseScript, Script, LocalScript, ModuleScript
//!   values/      BoolValue, IntValue, NumberValue, StringValue, ObjectValue
//!   services/    DataModel (game) and the services: Workspace, Lighting, Players, RunService, ...
//! ```
//!
//! The folders only organise files; the class hierarchy is declared by each class
//! naming its parent, never by Rust types or folders:
//!
//! ```text
//! Object
//! └─ Instance
//!    ├─ WorldObject ─┬─ BasePart ─┬─ Part
//!    │               │            └─ MeshPart
//!    │               ├─ Model
//!    │               ├─ Light ────┬─ PointLight / SpotLight / AreaLight / DirectionalLight
//!    │               └─ Camera
//!    ├─ Folder, Player, *Value, LuaSourceContainer ─ BaseScript ─ Script/LocalScript, ModuleScript
//!    └─ services (DataModel, Workspace, Lighting, Players, RunService, ...)
//! ```
//!
//! # Adding a class
//!
//! Create `<folder>/<snake_case_name>.rs` that exports a `CLASS` descriptor:
//!
//! ```ignore
//! use crate::core::*;
//!
//! static PROPS: &[PropMeta] = &[
//!     PropMeta::number("Range", 16.0).group("Light").desc("How far the light reaches.").slider(0.0, 200.0),
//! ];
//!
//! pub static CLASS: ClassDesc = ClassDesc::new("PointLight", Some("Light")).creatable().props(PROPS);
//! ```
//!
//! That is all: the build script finds the file (in any subfolder), and the registry
//! makes the class available to `Instance.new`, the scripting API, scene files, the
//! editor's Insert menu and Properties panel, and the runtime.
//!
//! A class file can also export:
//!
//! * `pub static COMPONENT: ComponentDesc` - behaviour only this class needs (a Bevy
//!   renderer, a physics system); list its name in `ClassDesc::components`
//! * `pub static ENUMS: &[EnumDesc]` - enums scripts see as `Enum.<Name>`
//!
//! Files that export none of these (for example `world/lights/light.rs`, the code
//! shared by every light) are plain modules the other class files can `use`.

include!(concat!(env!("OUT_DIR"), "/classes_generated.rs"));

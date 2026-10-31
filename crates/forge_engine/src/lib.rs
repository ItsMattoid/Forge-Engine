//! Forge engine runtime library.
//!
//! The engine is data-driven: classes and components describe themselves, and
//! generic systems act on those descriptions. Reading order for newcomers:
//!
//! | module          | responsibility                                                                 |
//! |-----------------|---------------------------------------------------------------------------------|
//! | [`core`]        | value types, property metadata, the class/component registry                   |
//! | [`classes`]     | the API: one file per class, grouped by domain (`Object -> Instance -> ...`)   |
//! | [`components`]  | reusable behaviour shared by several classes (Transform, Collider, ...)        |
//! | [`dm`]          | the data model: the instance tree, property access, scene serialization        |
//! | [`physics`]     | collision world, character/rigidbody stepping, ray casts                       |
//! | [`render`]      | mirrors the data model into Bevy entities                                      |
//! | [`scripting`]   | the Luau host: exposes whatever the registry describes                         |
//! | [`assets`]      | where game files come from (folder or `game.pak`) and mesh decoding           |
//! | [`project`]     | project folders, manifests, validation and packaging                           |
//! | [`runtime`]     | the Bevy plugin that runs a game                                               |
//! | [`bench`]       | `forge_player --bench`: frame times and screenshots of a running game          |
//! | [`compat`]      | the few Bevy calls that change between releases                                |
//!
//! `classes` and `components` are discovered by `build.rs`; see `ARCHITECTURE.md`.

pub mod assets;
pub mod audio;
pub mod bench;
pub mod classes;
pub mod compat;
pub mod components;
pub mod core;
pub mod diag;
pub mod dm;
pub mod physics;
pub mod project;
pub mod render;
pub mod runtime;
pub mod scripting;

pub mod prelude {
    pub use crate::assets::GameSource;
    pub use crate::core::{class, registry, Id, Value};
    pub use crate::dm::{file::PlaceFile, DataModel};
    pub use crate::project::{Project, ProjectManifest};
    pub use crate::runtime::ForgeRuntimePlugin;
}

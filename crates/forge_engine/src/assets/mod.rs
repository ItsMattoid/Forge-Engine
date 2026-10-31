//! Reading game content.
//!
//! * [`source`]   - where a running game reads files from: a project folder or a packed archive
//! * [`pak`]      - the `game.pak` archive format
//! * [`mesh`]     - reading 3D models (OBJ + MTL, glTF / GLB, STL, PLY): real size, normals, tangents, materials
//! * [`texture`]  - decoding pictures into textures (colour spaces, mip-maps, samplers)
//! * [`material`] - material assets (`.mat` files) and the project's material library
//!
//! GPU-side caches of loaded content live in `render::cache`.

pub mod info;
pub mod material;
pub mod mesh;
pub mod pak;
pub mod source;
pub mod texture;

pub use pak::{Pak, PakBuilder};
pub use source::{clean_path, FileStamp, GameSource, PAK_NAME};
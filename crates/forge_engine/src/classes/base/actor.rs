//! Actor: Roblox's unit of parallel Luau. Forge runs scripts on one thread, so an Actor is a container
//! (scripts inside still run, `task.desynchronize` and `ConnectParallel` run on the one thread).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("Actor", Some("Model")).describe("A model whose scripts could run in parallel (here: on the one thread).").category("Parts").creatable().container();
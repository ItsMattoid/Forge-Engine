//! ReplicatedFirst: objects and LocalScripts that are needed before anything else (a loading screen).

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("ReplicatedFirst", Some("Instance"))
    .describe("objects and LocalScripts that are needed first, before the rest of the game.")
    .service(3)
    .persisted()
    .runs_scripts()
    .container();
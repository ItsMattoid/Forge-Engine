//! Roblox compatibility, tested the way scripts meet it: real Luau, a real data model, the real physics world.
//!
//! Each test is a Roblox script (or the part of one that matters) written as Roblox documents the behaviour; a failing
//! check names what differs. The files group them by what they exercise:
//!
//! * `datatypes`  - Vector2/3, CFrame, Color3, BrickColor, UDim2, sequences, Random, Enums, `typeof`
//! * `instances`  - `Instance.new`, the tree, cloning, attributes, tree events
//! * `signals`    - signals and connections, `task`, Bindable and Remote events, errors and the log
//! * `modules`    - `require`, ModuleScripts, `_G` / `shared`
//! * `classes`    - parts, models, attachments, humanoids, players, tools, lighting, sounds, effects
//! * `services`   - the services: HttpService, DataStoreService, TweenService, ContextActionService ...
//! * `physics`    - constraints by attachment, welds, movers, explosions
//! * `scripts`    - whole scripts of the kind Roblox games are made of

mod classes;
mod datatypes;
mod instances;
mod modules;
mod physics;
mod scripts;
mod services;
mod signals;
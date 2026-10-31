# Forge architecture

Forge is built around one idea: **a class describes itself once, and everything else reads that description.**
Scripting, `Instance.new`, scene files, the editor's Insert menu and Properties panel, and the runtime all consume the
same registry. Adding a class, a light type or a property never means touching those systems.

> Status: the workspace builds and its tests pass (`cargo test -p forge_engine -p forge_editor`). The tests run real
> Luau through the script host, load and save scenes, step the physics and drive a whole Play session.

## Where things live

```
crates/forge_engine/src/
  core/         the object model: values, property metadata, ClassDesc / ComponentDesc, registry, hook types
  classes/      THE API. One file per class, grouped by domain, discovered by build.rs
    base/         Object, Instance, PVInstance, WorldObject, Folder, Configuration, Actor
    world/        Model, Camera, Attachment, Decal/Texture, Terrain, SpawnLocation, parts/ (BasePart, Part, MeshPart),
                  lights/ (Light, Point/Spot/Area/Directional/SurfaceLight)
    characters/   Player, PlayerGui, StarterPlayer, ... (no built-in character: controllers are scripts)
    scripts/      LuaSourceContainer, BaseScript, Script, LocalScript, ModuleScript
    values/       ValueBase and the Bool/Int/Number/String/Object/Color3/Vector3/CFrame/BrickColor values
    events/       BindableEvent/Function, RemoteEvent/Function (one process: both sides see the same signals)
    constraints/  Constraint, JointInstance, Weld, WeldConstraint, Hinge, BallSocket, Prismatic, Rod, Rope, Spring, ...
    movers/       BodyVelocity/Position/Gyro/Force/Thrust, LinearVelocity, VectorForce, Torque, AlignPosition/Orientation
    effects/      ParticleEmitter, Beam, Trail, Fire, Smoke, Sparkles, Highlight, Explosion, ProximityPrompt, ClickDetector
    audio/        Sound, SoundGroup and the sound effects (reverb, echo, filters ...)
    lighting/     Sky, Atmosphere and the post-processing effects
    animation/    Animation, Animator, Tween
    services/     Workspace, Lighting, Players, RunService, TweenService, DataStoreService, ... (and the online ones, which
                  keep the API but refuse to work)
  components/   behaviour shared by several classes (Transform, Appearance, Collider, Rigidbody, markers)
  dm/           the data model: instance tree, property access, events, scene-file serialization
  physics/      Box3D (via bevy_box3d): the bridge to the data model, collision groups, joints, queries
  audio/        decoding, the mixer, effects, 3D placement, the sound card, the mirror of `Sound` objects
  render/       mirrors changed instances into Bevy entities by calling components' render hooks; materials and the asset cache,
                camera effects (`camera_fx`), animated effects (`effects`)
  scripting/    the Luau host: exposes whatever the registry describes
  diag/         debug overlays: frame-time statistics, metrics collectors, the egui panels and Debug menu
  assets/       where game files come from (folder or game.pak), pak format, models (glTF, OBJ, STL, PLY), textures,
                material files, facts about files
  project/      project folders and manifest (Project.toml), what a file is (kinds), the asset index (assetdb),
                safe file operations (files), templates, validation, export, player launcher
  runtime.rs    the Bevy plugin that runs a game
  compat.rs     the few Bevy calls that change between releases
crates/forge_editor/   ui/ (panels) and state/ (editing operations, undo history, project operations)
crates/forge_player/   the standalone player        crates/forge_cli/   the `forge` command
```

Dependencies point one way: `core` knows nothing about classes; `dm`, `physics`, `render` and `scripting` use `core`
and never name a specific class; `classes` and `components` plug into `core` and may use `dm` and `render` helpers.
The only place `core` names types owned by another subsystem is `core/hooks.rs`, which lists the hook signatures
(a render hook takes the renderer's context, a physics hook takes the physics state, ...).

## Inheritance (shared API and properties)

```
Object                      ClassName, IsA, Changed
â””â”€ Instance                 Name, Parent, tree methods, ChildAdded/ChildRemoved/Destroying
   â”œâ”€ WorldObject           PivotTo / GetPivot
   â”‚  â”œâ”€ BasePart           Size, Touched/TouchEnded     + Transform, Appearance, Collider, Rigidbody
   â”‚  â”‚  â”œâ”€ Part            Shape                        + PrimitiveRenderer
   â”‚  â”‚  â””â”€ MeshPart        MeshId                       + MeshRenderer
   â”‚  â”œâ”€ Model              PrimaryPart, MoveTo
   â”‚  â”œâ”€ Light              Color, Intensity, CastShadows, Enabled   + Transform (relative)
   â”‚  â”‚  â”œâ”€ PointLight      Range                        + PointLightEmitter
   â”‚  â”‚  â”œâ”€ SpotLight       Range, InnerAngle, OuterAngle + SpotLightEmitter
   â”‚  â”‚  â”œâ”€ AreaLight       Range, Width, Height         + AreaLightEmitter
   â”‚  â”‚  â””â”€ DirectionalLight                             + DirectionalLightEmitter
   â”‚  â””â”€ Camera             FieldOfView, CameraType      + Transform, CameraView
   â”œâ”€ Folder, Player, BoolValue/IntValue/NumberValue/StringValue/ObjectValue
   â”œâ”€ LuaSourceContainer â”€ BaseScript â”€ Script / LocalScript,   LuaSourceContainer â”€ ModuleScript
   â””â”€ services: DataModel, then Workspace, Players, Lighting, ReplicatedFirst, ReplicatedStorage, ServerScriptService,
      ServerStorage, StarterGui, StarterPlayer, SoundService (listed by the Explorer, in this order), then RunService,
      UserInputService, CollectionService, Debris, TweenService (`hidden_in_explorer`: scripts only)
```

The diagram shows the core of the tree; the Roblox compatibility layer adds `PVInstance` (the base of everything that has a
pivot: `WorldObject` derives from it, so `Model` and `BasePart` are `PVInstance`s as in Roblox), `ValueBase` (the value
objects), `JointInstance` (`Weld`, `ManualWeld`, `Motor6D`), `PostEffect`, `BaseRemoteEvent`, `Attachment`
and the constraint, mover and effect families listed above. See [COMPATIBILITY.md](COMPATIBILITY.md) for what each does.

A child inherits its parent's properties, methods and signals and may override them: redefine a property with the
same name, `hide(&["Name"])` an inherited one, or change an inherited default with `defaults(...)`. Parents are
referenced **by name**, so class files never depend on each other in Rust.

The folder a class sits in is for people; the *inheritance* comes only from `ClassDesc::new("Name", Some("Parent"))`.

## How a class owns its behaviour

Rust has no class inheritance, so Forge splits a class into two layers that mirror what OOP would do:

* **The base class holds the shared logic** as ordinary functions and constants in its own file.
* **Each subclass supplies the part that differs** and registers it as a hook of a component that it owns.

`Light` is the model. Open `classes/world/lights/point_light.rs` and you see the whole of what a point light is:

| In `point_light.rs` | Role |
|---|---|
| `PROPS` | its own property (`Range`, shared definition from `light.rs`) |
| `apply` | creates / updates the Bevy `PointLight` component from the instance's properties |
| `render` | the render hook: `light::sync(ctx, dm, id, bits, apply)` |
| `COMPONENT` (`PointLightEmitter`) | registers `render` and the editor gizmo |
| `CLASS` | name, parent (`Light`), Insert-menu category, parent rule, components |

and `light.rs` holds what *every* light does: the shared properties (`Color`, `Intensity`, `CastShadows`, `Enabled`),
`sync` (enabled? in the workspace? compute the transform; spawn, move or despawn the Bevy entity; read the shared
values into `Common`), the unit constant, and shared gizmos. A new light type is one new file that supplies an
`apply` function; `Light` and the other lights are not modified. `DirectionalLight` was added exactly this way.

Parts follow the same shape: `base_part.rs` holds `Size`, signals, the material/shadow/visibility handling and
`sync`; `part.rs` and `mesh_part.rs` each supply a `Look` (where the mesh comes from and how the transform is built)
and their own renderer component. `camera.rs` and `lighting.rs` define their own components
(`CameraView`, `Environment`) the same way.

## Components (reusable functionality)

Inheritance gives the shared *API*; components give the shared *behaviour*. A component bundles properties and hooks.
Components used by one class live in that class's file; components used by several live in `components/`:

| Component | Where | Properties | Hooks |
|---|---|---|---|
| `Transform` | `components/` | CFrame, Position, Orientation | keeps attached (relative) objects in sync |
| `Appearance` | `components/` | Color, Material, Transparency, CastShadow | â€“ |
| `Collider` | `components/` | CanCollide, CanTouch, CanQuery, CollisionGroup | collision volume, editor gizmo |
| `Rigidbody` | `components/` | Anchored, Kinematic, Density, Mass, Friction, Elasticity, velocities, GravityScale, damping, CanSleep, Sleeping, ContinuousCollision | read by the simulation |
| `Joint` / `JointFrames` | `components/` | Enabled, Part0, Part1, CollideConnected / Anchor0, Rotation0, Anchor1, Rotation1, C0, C1 | read by the simulation (constraints) |
| `LuaSource`, `ScriptRunner`, `ValueObject` | `components/` | â€“ | markers the script host / event system query |
| `PrimitiveRenderer` / `MeshRenderer` | `parts/part.rs`, `parts/mesh_part.rs` | â€“ | render hook |
| `Point/Spot/Area/DirectionalLightEmitter` | `lights/*.rs` | â€“ | render hook, editor gizmo |
| `CameraView` | `world/camera.rs` | â€“ | render hook (the current camera), editor gizmo |
| `Environment` | `services/lighting.rs` | â€“ | render hook (sun, ambient, sky) |
| `Animated` | `components/` | none | marker: the instance has an `animate` hook (particles, beams, trails, highlights, clouds) |
| `CameraEffect` | `components/` | none | marker: the instance says something about every camera (see `camera_fx`) |
| `SoundEmitter`, `*SoundEffect` | `audio` classes | none | `audio_effect`: the processor a sound effect stands for |

Hooks (`core/hooks.rs`):

| Hook | Called by | When |
|---|---|---|
| `render` | `render` | an instance changed: mirror it into Bevy |
| `physics(order, fn)` | `physics` | before every simulation step, in `order` (gets the data model, the bridge, the world and `dt`) |
| `collider` | editor picking, `physics` | describe the collision volume (the simulation builds its shape from it) |
| `on_changed` | `dm` | right after a property was written |
| `gizmo` | editor | helper shapes for the selected instance |
| `audio_effect` | `audio` | describe the audio processor an instance stands for |
| `camera_effect` | `render` | say what an instance does to every camera (bloom, fog, the sky ...) |
| `animate` | `render` | every frame, with the frame's time and the camera: effects that move on their own |

Systems are **driven by component membership**: the renderer runs each dirty instance's component render hooks,
the simulation runs every component's physics function in order before each step, the editor's picking uses whatever has a collider.

## Data flow and lifecycle

The **data model is the single source of truth.** Bevy entities are disposable mirrors; nothing is stored only on
an entity, and an instance has at most one entity (`Instance.entity`).

1. *Create*: `Instance.new` / scene load / editor Insert allocate an instance with its class defaults, then call
   `ClassBehavior::on_created`; after a scene is loaded, `on_loaded` runs for each persisted service (so a service can
   fix up its contents).
2. *Parent*: setting `Parent` goes through one function (`DataModel::set_parent` in `dm/tree.rs`) that refuses
   services and cycles, updates both sides of the link, re-marks the whole subtree dirty, bumps `struct_rev`, and
   emits `ChildAdded`/`ChildRemoved`. (`ParentRule` only decides where the *editor* puts a newly inserted object.)
3. *Change*: `DataModel::set_prop` checks the type, clamps the value, writes it, marks the instance dirty with
   `D_X` (moved) and/or `D_V` (look changed) from the property's `F_XFORM` / `F_VISUAL` flags, bumps
   the physics feed (`DataModel::phys`) for properties flagged `F_PHYS`, runs the components' `on_changed` hooks and the class's
   `on_prop_changed`, and emits `Changed` (only if somebody listens).
4. *Mirror*: once per frame `render::sync` takes the dirty list, and for each instance calls its components' render
   hooks with the dirty bits. A hook spawns, updates or despawns the entity and stores it back in `Instance.entity`.
   The mirror only writes what differs from what it last wrote (`Instance::drawn`, see `render::Drawn`): a recolour
   writes one component, a move writes the transform (all moves of a sync are applied in one command), a refresh that
   changes nothing writes nothing. Every part shares the mesh of its shape and the material of its surface kind, so
   Bevy batches them (the colour travels in the entity's `MeshTag`, which is not part of Bevy's batch key).
   Writing a property to the value it already has is not a change (no dirty bits, no hooks, no `Changed`).
5. *Destroy*: the instance and its subtree are removed from the tree, `Destroying` is emitted, and their entities are
   queued in `dead_entities`, which `render::sync` despawns before anything else.

### Physics: the data model and the Box3D world

Box3D's world (owned and stepped at a fixed 60 Hz by `bevy_box3d`'s plugin in `FixedUpdate`) is a **mirror of the parts
that can be simulated**, kept in step by `physics::PhysicsBridge` (see its module docs for the details):

* **Edits go in.** A property flagged `F_PHYS`, a re-parent or a destroy marks the instance in `DataModel::phys` (the
  *feed*; it records nothing while no simulation is attached). Before each step `PhysicsBridge::sync` reads each marked
  instance, compares what it wants (`read_spec`) with what the world was last given (the part's *applied* state) and changes
  the world only where they differ. Constraints work the same way and are rebuilt when anything about them differs.
* **Results come out.** After the step, `post_step` writes the pose and velocities of the bodies that moved back into the
  data model with the feed *muted*, and records the same values as applied - which is how the next `sync` tells a script's
  teleport (the data model differs from what was applied) from the simulation's own motion (it does not). So nothing can
  feed back on itself.
* **Who leads:** an anchored part is moved only by edits; a kinematic one by its velocity or a `CFrame` set toward; a dynamic
  one by the simulation (an edit to its pose or velocity is a teleport it continues from). A character's limbs are not
  simulated; they follow the body.
* **Frames:** `ScriptHost::physics_pre_step` / `physics_post_step` are the two `FixedUpdate` systems around `Box3dSet::Step`
  (the runtime and the editor's Play add them; the editor finds the host in `Editor::play`). `ScriptHost::frame_with` hands the
  world to the frame's scripts for its length (a scoped pointer, `WorldScope`) so impulses and queries reach the simulation
  at once.
* `box3d`'s bodies and shapes are owning handles; the bridge keeps them with an extended lifetime (one documented `unsafe`
  place per kind: `bridge::extend_body` / `extend_shape`, the joint handles in `joints.rs`), and dropping a part's record
  destroys its body, so nothing outlives its part. The editor does not simulate: picking and placing use the data model's own
  collision volumes (`physics::ray_collider`, `pick_colliders_all`).
Small hot-path maps (class property / method lookup, watched instances, the component index) use
`FastMap` / `FastSet` (`core/value.rs`); the component index is a bit per slot, which keeps ids in order.

### Loading scenes

`DataModel::load_place` never leaves the data model half-loaded. An unsupported `format` is refused before anything
is touched. Everything else the file contains that this build does not understand (an unknown class, a property that
no longer exists, a bad enum value, a reference to the wrong class) is skipped and listed in
`DataModel::load_warnings`; skipped instances still take their reference index, so references after them stay correct.
Properties a file omits (they equal the default) are reset on the persisted services first, which is what makes Undo
and Stop restore service settings such as `Lighting.ClockTime`. `forge check` / the build step report the warnings as
errors. `set_prop` also rejects NaN/infinite numbers, enum items that do not exist and references to the wrong class.

### Scripts and their lifetime

Every script chunk is named `path#uid`, so the prelude can tell which Script instance a function was written in
(`debug.info`). Connections and threads a script starts are remembered under that uid; when the Script is destroyed,
disabled or moved out of a script-running service the host calls `reap(uid)` and they are disconnected / closed
(`DataModel::dead_scripts` carries the uids of destroyed scripts). Signals of destroyed instances are dropped through
`DataModel::dead_watched`. Every signal handler runs in its own coroutine, so any handler may yield. The time budget
(`PHASE_BUDGET`) is re-armed for each phase of the frame. Tags (`CollectionService`) live in `DataModel::tags`.

### The Roblox compatibility layer

The goal is that a Roblox script runs unchanged (the status is in [COMPATIBILITY.md](COMPATIBILITY.md), the proof in
`scripting/compat_tests/`). It is built from the same mechanisms as everything else, not from special cases in the host:

* **Calling convention.** A class method written in Rust takes a `MethodCall` and returns a `ScriptValue` (`core/script.rs`):
  no Luau types, so `dm` and `classes` stay free of the scripting crate. A method that must yield or call back is a Luau
  snippet next to the class (`luau(include_str!(..))`, as `tween_service.luau`, `proximity_prompt.luau`); snippets see
  `methods`, `Signal`, `task`, `Enum` and `game`, and a `__start` method of a service runs once after the game has started.
* **Signals** are declared with `SignalDesc` and fired from Rust by `dm.fire(id, name, args)`, which queues an
  `Event::Signal`; the host delivers the queue after the change that caused it (deferred, like Roblox's default
  `SignalBehavior`). Only instances somebody listens to (`watched`) queue anything. `Event::Changed` carries the new value
  for `ValueBase`. Because delivery is later, an instance destroyed in between leaves a `DataModel::graves` entry (class
  and name) for the frame, so handlers can still print `part.Name`.
* **Data types.** The vector-like types (`Vector2/3`, `CFrame`, `Color3`, `Random`) are Rust userdata (`scripting/lua_types`);
  the data-only ones (`UDim2`, `NumberSequence`, `BrickColor`, `PhysicalProperties`, `Ray`, `TweenInfo` ...) are Luau tables
  with a `__type` metatable in `datatypes.luau`, which also gives each a codec (value <-> list of numbers). A property of
  such a type is a `PropMeta::data(tag, ..)` holding `Value::Data(tag, numbers)`, so it is saved, undone and tweened like
  any other; `core::DATA_TYPES` lists the tags. The prelude's `typeof` reports `__type` for tables. (`datatypes.luau`
  keeps its own local `typeof`: Luau compiles calls to the global `typeof` as a fast path to the built-in one.)
* **Enums** are `EnumDesc`s (`numbered` for Roblox's numbers, `aliases` for renamed items). The prelude builds `Enum.X.Y`
  `EnumItem`s from them; properties of an enum type accept the item or its name.
* **Refusing what cannot work.** `ClassDesc::unsupported(&[..])` makes a member answer with a function that raises
  `X:Y is not supported by Forge`; `callbacks(&[..])` names members a script assigns a function to (`OnInvoke`);
  `scene_object` lets a scene file name a class that scripts cannot create with `Instance.new` (`Terrain`).
* **Behaviour hooks** (`ClassBehavior`): `on_created`, `on_loaded`, `on_prop_changed`, `on_parent_changed` (a `Light`
  joins a part, an `Explosion` goes off), `on_destroying` and `on_started` (after the game
  starts; the `Players` service uses it to add the player).
* **Aliases.** `PropMeta::aliases` keeps old property names (`Velocity`), the host maps lower-case method names, and the
  prelude keeps `wait`/`spawn`/`delay`.
* **Physics.** Constraints are read by `physics/joints.rs` (`read_joint`): the frames come from `Attachment0/1` when they
  are set (`Side`), else from `C0/C1`. Movers are `physics` hooks of components built on `movers/mover.rs` (`approach`,
  `rotation_error`).

### Undo

Undo is command-based (`dm/journal.rs`, `forge_editor/src/state/history.rs`). With journaling on, the data model
records each low-level edit as a reversible `Change`: a property write (old and new value), a rename, a re-parent
(old and new parent and position), creating an instance, destroying a subtree (captured as a scene fragment, plus the
references that pointed into it). The editor calls `commit_edits` at the end of every frame; it turns what was
recorded into one `Command`, except while the mouse is held, so a drag is one step however long it lasts. Typing
or dragging the same properties again within 600 ms extends the previous step. Undo replays a command's changes
backwards and redo forwards, touching nothing else: instances stay the same instances, so the cost does not grow with
the scene. A deleted instance cannot get its id back (ids are never reused), so it returns under a new one and
`IdMap` remembers old -> new; older commands that mention it still reach it. Loading a scene is not an edit and is not
recorded; a running game records nothing. After a Play session rebuilds the scene, the history is re-pointed at the
rebuilt instances by position (`History::remap`), so undo works across Play. Creating a script file when a script
is inserted is not undone (the file stays).

### Shortcuts and editor settings

`forge_editor/src/keymap.rs` holds one table (`DEFS`) of every shortcut: id, name, group, the window it works in
(`Context`: the viewport, the Explorer, the Properties panel, or anywhere), how it fires (press / hold / mouse
wheel) and its starter binding. The Shortcuts tab of the settings window (`ui/settings.rs`) is generated from that
table, so a new shortcut is one line there plus a case in `state/actions.rs`. Each frame `ui_system` works out which
window the mouse is over and `Keymap::poll` returns what fired; a window-specific binding wins over an "anywhere"
one on the same keys, and two shortcuts may share keys only if their windows cannot be active together (Ctrl+R
rotates in the viewport and renames in the Explorer). Only the user's changes are stored
(`%APPDATA%\Forge\editor_settings.json`, `settings.rs`), so starter bindings can still improve in later versions.
The viewport tools (`Select / Move / Scale / Rotate`) keep their maths in `gizmo.rs`.

### Selection, tools and the camera

* **Selection** (`state/selection.rs`): `Editor::selection` is every selected object and `Editor::selected` the active one
  (the gizmo and the lead values of the Properties panel). `normalize_selection` keeps them consistent and treats a
  plain `selected = x` as "select only x". Viewport clicks go through `Editor::click_select`: click selects, Shift
  toggles, Alt walks through everything under the mouse nearest-first (`PickCycle`: same spot within a configurable
  tolerance, same objects), Shift+Alt does the walk while adding. `physics::pick_colliders_all` supplies the sorted hits.
  The Explorer (`state/tree_view.rs`, drawn by `ui/explorer.rs`) lists the services that are not `hidden_in_explorer`, in
  `service_order`; Ctrl (or Alt) + click toggles, Shift + click selects the range of listed rows from the anchor (the row
  last clicked without Shift; the active object stands in when the anchor is not listed), Ctrl + Shift adds a range. Its
  search (`TreeFilter`: text, match case, name / class, type incl. subclasses) lists matches plus the chain of containers
  above them, ignoring collapsed nodes without changing them; it never touches the scene or the selection, and the
  status line counts selected objects that are not listed. Move, rotate, copy, duplicate, delete, group act on the
  top-level selected objects; scale on the active one.
* **Project files and the Asset Browser** (one system, no competing ones). `project::Project::walk` is the only place that
  walks the disk; `scenes()`, `scripts()`, `asset_files()` and `packaged_files()` filter its result by `AssetKind::of`
  (the file name), wherever the files are. `project::AssetDb::scan` builds the index the editor uses (kinds, folders, search,
  what scenes / prefabs / the manifest use which files) and keeps a stable id per file in `.forge/assets.json`; a file
  that vanishes from one path while a file with the same size and first / last 32 KB appears at another is reported as a
  *move* and keeps its id (empty files never count). `project::files` holds the operations (create folder, rename, move,
  duplicate, trash, import): they refuse paths outside the project, the manifest and `.forge`, never overwrite, and return
  `(old, new)` pairs per file; `Project::remap_references` rewrites scene / prefab files and the start scene from them.
  In the editor (`state/assets.rs`), `sync_assets` scans about once a second, `apply_moves` points the open scene (in
  memory, undoable), the scenes on disk, the script editor and the browser at the new paths - for moves made here and, from a
  scan, for moves made outside - and `plan_asset` is the one decision about what a drop or a double-click does, shared by
  the Explorer (`ui/explorer.rs`), the viewport (`viewport::drop_assets`) and the browser (`ui/assets.rs`). A script object
  stores the *path* of its source (`SourceFile`); its class and place in the scene decide where it runs, so a file can
  live in any folder. File operations are refused while a game plays.
* **Windows and the dock** (`dock.rs`, `state/layout.rs`, `ui/panels.rs`). Every window is a `Panel` (viewport, Explorer,
  Properties, Output, Assets, Script Editor) and the layout is an
  `egui_dock::DockState<Panel>`: tab groups, splits, and floating window surfaces. **A panel is shown exactly when it is
  in the layout** - the View menu's ticks, the tab close buttons, the window shortcuts (`Action::Window`, generated from
  `dock::PANELS` like the Debug ones, `Action::RestoreLayout`) and `Editor::show_panel / hide_panel / toggle_panel` all
  read and write that one thing. What a panel shows lives in the `Editor`, never in the layout, so moving, floating or
  hiding it loses nothing. While the dock is drawn it is moved out of the editor (the panels need `&mut Editor`);
  requests made from inside a panel are queued (`PanelOp`) and applied right after, in the same frame. The viewport panel
  is a hole in the dock (`clear_background` is false): its body rectangle is the camera's viewport (`ViewportRect`), its
  hover is `rect_contains_pointer` (so a window or menu on top of it counts), and when it is not on screen the camera is
  switched off. Which panel the mouse is over (`Editor::panel_hover`) decides which contextual shortcuts can fire.
  **The Editor Settings and Project Settings are not panels**: they are plain `egui::Window`s drawn by `ui/window_memory.rs`
  (`Editor::{open,close,toggle}_{editor,project}_settings`, flags in `SettingsUi` / `ProjectSettingsUi`), so the dock cannot
  take them or tab them. `window_memory::show` puts a window back where `EditorSettings::window_places` (name -> x, y,
  inner width, height) says on the first frame after it opens (`fit` pulls it back on to the screen) and writes its place
  back once the mouse is up.
  The layout is saved in `editor_settings.json` as JSON of panel *names* plus the places of the floating windows (egui
  keeps those, egui_dock does not); unknown panels are dropped, duplicates kept once, anything unreadable gives the default.
* **Transforms by shortcut**: `Ctrl+R` / `Ctrl+T` (`Editor::rotate_selection`) turn the top-level selection as one rigid
  group about `tools::frame_for`'s origin and axes, exactly as dragging the Rotate tool does (`tools::turn_all`).
* **Cursor during a look drag** (`viewport::viewport_cursor`): the camera turns by raw device motion, so the pointer is only
  hidden and held at the spot where the button went down (reset every frame); it is never sent to the middle of the view.
* **Per-instance render data**: `MeshTag` (colour, opacity) is not among the components Bevy's GPU mesh extraction watches,
  so `base_part::sync` also touches the part's `Mesh3d` (same mesh) whenever the tag, the shadow flag or the material
  changes; without it an edited colour would not show until the part was moved.
* **Properties panel** (`ui/inspector.rs`): with nothing selected it keeps showing the last selected object, marked "not
  selected" (`last_inspected`; deleting it falls back to its nearest non-service ancestor, without selecting it). With
  several objects selected it lists the properties all of them have (same name and type) and writes edits to every
  object that allows it; differing values are marked "mixed" (with a button to equalise); the rest are listed as not shared.
* **Tools** (`tools.rs`, maths in `gizmo.rs`): move = six arrows (a gap around the centre, a solid cone at the end) + three
  plane handles on the camera-facing side; scale = six spheres standing off the faces of the bounding box (a part's own box,
  or the world box of what is selected, which stretches the parts inside); rotate = three rings + a trackball; pivot = the
  move handles on the pivot. With several objects selected the gizmo is at the centre of their combined bounds. While a handle
  is dragged the gizmo hides and only the measurement shows. A drag shows a ruler marked every 5 units with the distance (move)
  or the new size (scale, one short value: `X 7.00`, or `x1.75` with Shift), or a ring with marks at the rotation increment
  (longest at every 90 degrees, shorter at every 45, all starting on the ring) and the angle turned filled in (rotate). Lines are
  Bevy gizmos in the `ToolGizmos` group (`depth_bias = -1`, so in front of the scene without touching scene depth); fills and
  text are an egui overlay clipped to the viewport. Mouse priority: tool handle > selection click > right-button look / orbit.
  * **A drag is always measured from where it began**, from the objects' state then and the mouse now, never from the last
    frame's result: nothing adds up error, and pressing or releasing a modifier mid-drag just changes the formula. The two
    exceptions are counting turns (a direction only gives an angle modulo a full turn: `gizmo::unwrap_angle`, so a ring goes on
    past 180 and 360 degrees) and the trackball (a rotation cannot be read off the mouse's position: it adds up the small arcs from
    frame to frame; measured from the start, an arc is never more than half a turn, which was the limit it had).
  * **Scale modifiers** (read live every frame): Shift = one factor for all three sizes; Ctrl = about the centre, so both faces move;
    Ctrl+Shift = both. The grabbed face is under the mouse in every combination, so toggling a key never moves the handle held.
    A model's parts are all scaled about the same anchor (about the centre, or the opposite face's centre); with Shift even a part
    turned between the axes scales, without it such a part can only follow in position (stretching it would shear it). **Alt**
    (Scale tool, a model selected) puts the handles on the part under the mouse and scales only it (`GizmoState::part`); the choice
    sticks while Alt is held, and the model and its hierarchy are untouched. In the Scale tool Ctrl means "about the centre" and
    not "ignore snapping".
  * **Move plane handles** are drawn small (`PLANE_NEAR..PLANE_FAR`) but picked from their square plus a margin that follows the
    gizmo's size on screen (`plane_pick_pad`), judged in screen space so a plane seen at a slant (a thin sliver) is still easy to
    take. Inside a drawn square beats an arrow unless the mouse is right on the arrow's line; planes seen almost edge-on are not
    pickable and a drag on one holds still (`MIN_PLANE_FACING`), since where the ray meets it is far away.
  * **The Select tool picks objects up**: a press selects as before; moving the mouse a few points (`DRAG_START_DISTANCE`) with the
    button down then carries the selection, which is **set down on the surface under the mouse**: the carried objects' world
    bounding box touches that surface (on the ground, on another object's top, against a wall's face), neither sinking in nor
    hovering, and is centred on the point the mouse is at, so the object is always right under the cursor (`set_down`). The other objects' collision volumes are collected once when the object is picked up, so each frame is a ray
    test, not a look at the whole scene; the carried objects themselves are never a surface. The move increment applies along a
    surface that lines up with an axis, never across it (or the box would stop touching). Over empty sky it slides on the level
    through the grabbed point (in a nearly level view, on the plane facing the camera). A press on one of several selected
    objects keeps them all (so a drag carries all) and only narrows the selection if it turns out to be a click. Shift / Alt
    presses only select. The whole carry is one undo step.* **Camera** (`viewport.rs`): the move keys fly whenever the mouse is over the viewport; the right button adds looking and
  captures the mouse (hidden, recentred, restored on release). Alt + right button orbits the point under
  the mouse (`orbit_point`: first ray hit, else the ground, else a point ahead; **never the selection**), with the pivot fixed
  for the drag. The wheel (`zoom_target`, `ZOOM_*`) queues travel in `EditorCam::zoom_pending` - a tenth of the distance
  to what is ahead per notch, clamped, and not closer than `ZOOM_STOP_DISTANCE` to a surface - which is eased out per frame
  (`1 - e^(-rate*dt)`); the direction is the view (`ZoomMode::Camera`) or the ray through the pointer (`ZoomMode::Hover`,
  the view when there is no pointer), or the pivot in orbit mode. The selection is not read. **Input does not follow the
  mouse once started**: `ui::navigation_latch` keeps `Editor::nav_latched` (and so the `Viewport` shortcut context) while
  a movement key or the right button is down, if it began over the picture; `Keymap::modifiers_held` ignores an extra Alt,
  so Alt (speed wheel, orbit) never blocks flying (a freshly pressed Alt+key shortcut such as Alt+S wins over the fly key). Focus frames the
  selection from a distance computed from its bounds and the field of view, glides there and enters orbit mode, which
  ends when the camera is flown by hand or something else is selected. Alt + wheel ("Adjust Fly Camera Speed") changes the fly speed, Shift + wheel
  ("Adjust Secondary Camera Speed") the secondary multiplier (which may be below 1); the shortcut ids are unchanged.
### Pivots

Every `WorldObject` has a `PivotOffset` (a vector in the object's own axes, default zero). `DataModel::get_pivot` is the
object's position (for a Model: its primary part's pivot) shifted by that offset, and `pivot_to`, `PivotTo` / `GetPivot` /
`MoveTo` and the editor's move and rotate tools all work through it, so rotating an object turns it about its pivot and
moving it puts the pivot (not the centre) at the target. `set_pivot_point` changes only the offset. The editor's Pivot
tool (`5`) shows the move handles at the pivot and drags the offset while leaving the object where it is.

### Selection history

Selection is part of the undo history (`state/history.rs`). A step is a set of data-model changes and/or a selection
(before, after). A click that only changes the selection is a step of its own; an action that changes both (a paste
selects what it pasted, a delete clears the selection) is one step that undoes both. During a mouse gesture a selection
change is recorded at once (a click that turns into a drag is two steps) unless edits are already waiting. Undo and redo
restore the complete selection (every object and the active one); ids are followed through the same `IdMap` that
tracks revived objects and, across a Play session, the objects rebuilt on Stop. Selecting while a game plays is not recorded.

### Playing in the editor

`forge_editor/src/state/play.rs`: Play snapshots the scene (`to_place`), attaches a `ScriptHost` to the editor's own
data model and runs the frame loop on it (`play::play_frame`), rendering with `render::Mode::Play` (the real game's
look, but the editor owns the camera). The viewport camera follows `Workspace.CurrentCamera` ("Game camera") or stays
free. While the viewport has focus the keyboard and mouse go to the game (the mouse is captured when a script asks for
`LockCenter`); a click on any panel or Esc hands them back. The Explorer, Properties panel, Insert menu and the move
gizmo all work live. Stop drops the host, resets runtime-only properties and reloads the snapshot, so nothing the game
or the live edits did is kept; the scene on disk is never written while playing. "Play in Window" still runs the
separate player process.

### Debug overlays and performance monitoring

`forge_engine/src/diag/` is shared by the editor and the player; the editor's part is `ui/debug.rs`.

* **What can be shown** is one table, `diag::overlay::OVERLAYS` (id, name, group, description, starting shortcut). The
  Debug menu, the Shortcuts tab (`Action::Debug(Overlay)`, generated into `keymap::DEFS`) and the panels all read it, so a
  new overlay is a variant, a row there, and a drawing function in `diag/ui.rs`. Toggling from the menu or from the
  shortcut changes the same bit (`DebugSettings::enabled`), which is saved with the editor settings.
* **Collection** (`diag/plugin.rs`, `diag/metrics.rs`): the frame time is recorded every frame (`FrameHistory`, a ring buffer;
  one push per frame). Everything else is switched on by the overlay that shows it (`DebugSettings::wants()` -> `Wants`)
  and refreshed at the overlay's update interval (4x per second by default): visible meshes / triangles / batches, lights,
  the GPU's own pass timings, process memory, asset loads, the recorded warnings. A closed Debug menu therefore costs one
  push per frame. The numbers live in one `Metrics` resource; the editor's `debug_feed` system adds what only it can see
  (instance count, the running game's script host and physics).
* **Measured vs estimated.** Clocks, counters and the GPU's timestamps are measured; draw calls (one per distinct
  mesh+material of the opaque meshes, plus one per see-through mesh) and "waiting / other main-thread work" (frame minus what
  was timed) are estimates and are drawn in a different colour with a `~`. What a platform cannot give is `None` and the
  panel says why (no GPU timestamp queries, no networking in the engine, no game running) instead of showing a zero.
* **Frame statistics** (`diag/frames.rs`): FPS is frames / elapsed time over the smoothing window (not an average of
  per-frame rates); the 1% low is the average FPS of the slowest 1% of the window's frames; a frame is a *spike* when it takes
  `spike_factor` times the median (and at least 2 ms more), and *over budget* when it exceeds `1000 / target_fps` by more than 5%.
  Graph points keep each slice's slowest frame, so a one-frame hitch cannot be averaged away.
* **Scripts**: the host times each phase of its frame always; per-script cost is measured only while the Script
  Performance overlay is on (`ScriptHost::set_profiling`): the Luau prelude times every handler, task and script chunk and
  charges it to the script it was written in (inclusive times). **Physics** keeps a few counters per step (`PhysicsStats`).
* **Warnings & errors**: a tracing layer (`diag::log::layer`, installed with `LogPlugin::custom_layer`) keeps WARN and ERROR
  events; the editor's own `warn` / `error` go there too.
* **Placement**: every overlay is a window of its own (`diag/ui.rs`), sized by the "Window size" setting (default 1.45x). A window
  is docked to a viewport corner or free (drag its title; drop near a corner to dock). By default each group has a corner
  (performance top-left, rendering top-right, scripting + physics bottom-right, diagnostics bottom-left); windows in one corner
  stack, and a full column continues in a second one (`Column::place`). A window is as big as what it shows (no scroll bar, nothing
  cropped; it may reach past the viewport over the panels, and only one taller than the whole screen scrolls). Overlays are on
  `diag::ui::WINDOW_ORDER` (`Order::Middle`) and the editor's own windows (settings, dialogs, popups) on `Order::Foreground`, so
  those are always in front of them, whatever was drawn last. Only windows that were moved are stored
  (`DebugSettings::panels`). The Debug menu is a strip above the viewport (`ui/debug.rs`); overlays are drawn before the
  pointer is looked at, so the viewport is not "hovered" while the mouse is over a window.
* **Debug shortcuts** are `General`-style (work over any window) but do not fire while a text field has the keyboard unless
  "Debug shortcuts also work while typing" is on (`Keymap::debug_while_typing`). They also work while a game has the keyboard.
* **Player**: `forge_player --debug [overlay,...]` adds the same overlays to a running game (see the README).

### Performance: what scales, what was measured, what was left alone

Everything here was measured (`tests/perf.rs` for the CPU side without a window, `forge_player --bench` for the whole
engine), on a Ryzen 5 5500 / Radeon RX 6650 XT (Vulkan), release build.

* **Scripts.** A script file is compiled once per host (`Ctx::chunks`, keyed by path and checked against the file's size and
  modification time at most once per frame, since a `stat` costs about as much as a small compile on Windows); each instance
  loads the bytecode under its own chunk name, so ownership and `reap` are unchanged. A file that does not compile takes the
  old path, so its error message is the same. Starting 5,000 instances of a 40-line script: 740 ms -> 125 ms.
* **`task.wait`.** Sleepers are a heap ordered by wake time (`Sleepers`); a frame pops only the due ones and resumes them in
  the order they went to sleep, as the plain list did. 5,000 threads waiting two seconds: 2.5 ms -> 0.2 ms per 100 frames.
* **Physics** (Box3D; `tests/perf.rs`, the `physics_*` workloads, dev profile with optimised dependencies, so a release build
  is faster still). Per frame, scripts and render sync not counted: 3,000 falling parts 0.8 ms; 2,000 parts piled and settling
  2.2 ms; 3,000 parts at rest **0.01 ms** (resting parts sleep and nothing is written back for them); 3,000 `Touched` pads under
  1,000 falling parts 0.7 ms (contact events are only produced for parts something listens to; they were an AABB test of every
  watched part against every mover before); a 200-rod chain 0.25 ms; a Luau `Raycast` about 6 us each (500 a frame: 3 ms).
  The step runs at a fixed 60 Hz in `FixedUpdate`, so a 144 fps game runs it on 5 frames in 12 and costs nothing on the rest.
* **Script profiler.** `ScriptProfile::absorb` and the uid lookup used to scan, and rescanned the whole data model for every
  script below the top 512 on *every frame* the overlay was open: 2.7 s -> 0.32 s per 30 frames with 5,000 scripts.
* **Culling.** Frustum culling needs nothing from us (parts carry an `Aabb`). Occlusion culling is Bevy's GPU two-phase
  implementation (`render/culling.rs`) and is **off by default**: it wins 1.4x in a GPU-bound, high-overdraw scene without MSAA
  and loses in CPU-bound scenes (0.7 ms at 32k parts) and with MSAA (3x slower at 8x), because it runs after the CPU has
  already queued every entity in view. On DirectX 12 Bevy cannot do it at all, and nothing is added to the camera there.
  Rendered images match with it on or off (compared pixel by pixel, including moving parts, a moving camera, glass and
  a camera inside a huge part). **Not on a camera with a viewport**: Bevy 0.16 builds the occlusion pyramid for the viewport
  but sizes the depth texture to the whole window, so a camera drawing into part of the window (the editor's) has visible
  objects culled (reproduced: every cube seen through a doorway vanished). The editor's own viewport therefore never uses it;
  Play in Window and the player do.
* **Wireframe.** `render/wireframe.rs`: Bevy's wireframe pass behind one resource (`WireframeView`), with `hide_solid` for an
  edges-only view (the part shader discards its fragments; the depth prepass is untouched, so culling sees the same occluders).
* **Where a big scene's frame goes** (32,768 static parts in view, 720p): about 1.4 ms of fixed cost, 1.6 ms for the parts
  themselves (visibility, extraction, queueing: ~50 ns each), and **3.3 ms for the sun's shadow cascades**, which queue every
  caster again per cascade. `Lighting.GlobalShadows = false`, or `CastShadow = false` on small / distant parts, is the lever;
  the sun now uses 3 cascades instead of Bevy's 4 (`Lighting` default `num_cascades`).
* **Bevy 0.16 -> 0.19 frame cost** (small scene, release, same machine, `forge_player --bench` / `FORGE_EDITOR_BENCH=<frames>` for
  the editor; p50 frame time): player 1.29 -> 2.29 ms, editor 1.61 -> 3.53 ms before the fixes below. Box3D is not part of it (it
  does nothing while the editor is not playing, and the traces show it as negligible while a game runs). The rise is the render
  thread: Bevy 0.19's render graph is now schedules of many small systems, which adds a fixed cost per frame and per camera. Recovered:
  * **CPU light clustering.** Bevy 0.19 clusters lights on the GPU by default (extra compute passes per view). Forge has one sun and
    few lights, so `PartMaterialPlugin` resets `GlobalClusterSettings::gpu_clustering` to `None` (-0.43 ms).
  * **GPU timings only on demand.** The editor records GPU pass timestamps only if a GPU debug overlay was on when it started
    (`DiagPlugin { gpu_timing }`); Bevy writes two timestamps and a resolve per pass otherwise (-0.5 ms). Restart the editor after
    turning a GPU overlay on.
  * **Shadow cascades 4 -> 3** (-0.15 ms).
  * **egui camera.** bevy_egui 0.42 draws from a camera of its own; it is configured as in bevy_egui's examples (transparent clear,
    alpha-blended output, no MSAA, no render layers) so the 3D image underneath is kept and gizmos do not smear.
  Tried without effect: DX12 vs Vulkan, present mode, frame latency, multi-threaded vs single-threaded executors, dropping the
  deferred-lighting plugin. Without GPU indirect drawing the layouts do not match the part material (it crashes), so it stays on.
* **Left alone, on purpose.** Running Luau on several threads (the VM and the data model are single-threaded by design; scripts
  see one consistent world, in a deterministic order). Pooling the thread each signal handler runs in: measured at 0.27 us vs
  0.33 us per handler, which does not justify changing what `task.cancel` and `reap` see. Merging static parts into combined
  meshes would cut the per-part cost above, but is a feature of its own (parts stay individually scriptable).

## Assets, materials, audio, camera effects and animated effects

Everything a game uses is a **file of the project**; the data model only holds the path. A path is relative to the project
(`Assets/Models/crate.glb`), is read through `GameSource` (the folder, or the packed `game.pak` of an exported game) and goes
through the editor's one file system (`project::files`), so renaming, moving or deleting a file rewrites the references to it
(`Project::remap_references`) and an asset that has gone is reported once, softly, never by a crash.

* **Meshes** (`assets/mesh.rs`): glTF/GLB, OBJ (+MTL), STL and PLY. `load` returns triangles grouped by material, the materials the
  file describes and the model's real size. Positions are multiplied by the *unit scale* (`[assets] mesh_unit_scale` in
  `Project.toml`: studs per unit of the file), the model is centred on its bounding box and that box is the model's `native` size.
  A `MeshPart` is created with `Size = native` and a part's `Size` scales the mesh to `size / native`, so a model appears at the size it
  was made with and resizing the part resizes the mesh (the collision box and the mesh always agree). Normals come from the file or
  are computed (flat, or smooth across a smoothing group), tangents are generated for every submesh with texture coordinates (normal
  maps), a model without texture coordinates gets a box projection. A model whose file has materials draws each piece with its own
  (`UseImportedMaterials`), as one parent entity with a child per piece. FBX and Collada are refused with a message that names what to
  export instead.
* **Textures** (`assets/texture.rs`): PNG, JPEG, BMP, TGA, GIF, WebP, TIFF, HDR. Colour pictures are sRGB, data pictures (normal,
  roughness, metallic, occlusion) are linear; mip-maps are built in linear light; the sampler (filter, wrapping, anisotropy) comes from the
  material that uses the picture.
* **Materials** (`assets/material.rs`): a material is a `.mat` JSON file (versioned, tolerant: missing fields take defaults, unknown
  ones survive a save, out-of-range numbers are brought back). `Plastic`, `SmoothPlastic`, `Metal`, `Neon` and `Glass` are ordinary files a new
  project gets; they can be edited, replaced, deleted or joined by any number of others with no change to the engine. A part's material is, in
  order: its `MaterialVariant` (a path or a name), the project file named like its `Material` enum item, the engine's built-in look for that
  item. `kind` picks the shading model (standard, glass, neon, unlit); colours are sRGB; the part's `Color` tints the material and its
  `Transparency` multiplies the opacity. `render::part_material` turns a `MaterialDef` into Bevy's `StandardMaterial` plus the `PartLook`
  extension (`part.wgsl`: texture coordinates projected in studs or taken from the mesh, tangent-space normals, glass edge opacity, emission
  from the part's colour). Bevy materials are cached per `MatKey` (asset, colour map, see-through, UV mode), so parts that look alike are
  still drawn together.
* **The cache** (`render/cache.rs`): models and pictures are held by weak handles, materials are kept. `AssetCache::invalidate` (the editor
  calls it when files change) makes the next sync rebuild everything that depends on files. A file that cannot be read is logged once, counted in
  `AssetStats` (the Asset Loading overlay) and replaced by what could be loaded (a box for a model, no map for a picture).
* **File facts** (`assets/info.rs`, `dm.asset_info`): the size of a model and the length of a sound, cached per file, so the Properties
  panel can say "3 x 2 x 4 studs" and a `Sound` can report `TimeLength`.
* **In the editor**: the Asset Browser drags an `AssetDrag` payload; `state/assets.rs::plan_asset` is the one decision about what a drop or a
  double-click does (create a `MeshPart`, assign a mesh, texture, material or sound to the object under the mouse, create a `Sound`, open a
  scene, a script or the Material Editor) and says why when it cannot; `ui/asset_field.rs` is the widget every asset property uses (a field that
  accepts drops, a thumbnail, a button that opens the popup asset picker `ui/asset_picker.rs`, and a warning when the file is missing);
  `ui/material_editor.rs` edits the project's `.mat` files with a live preview, writing the file as you drag (and ignoring its own writes when the
  asset scan notices them).

### Audio

`audio/` is a subsystem next to `physics/` and `render/`, a mirror of the data model like them:

```
clip     a decoded sound file (WAV, MP3, OGG, FLAC) as 16-bit stereo     dsp      the effects (reverb, echo, filters ...)
mixer    plays any number of voices together into one stereo stream      spatial  distance, panning and Doppler of 3D sounds
engine   the mixer + the sound card (or none)                            sync     Sound objects -> voices, and back
```

A `Sound` says what it wants (`Playing`, `Volume`, `PlaybackSpeed`, `Looped`, `TimePosition`, the roll-off distances, where it sits) and
`sync::SoundMirror::update` makes the mixer do it once per frame, then writes back what only the mixer knows (`TimePosition`, that a sound
ended or looped: `Ended`, `Looped`). A `Sound` in a part or attachment is positional (equal-power pan, attenuation by the roll-off model, Doppler from
the movement relative to the camera); elsewhere it is heard everywhere. `SoundGroup` volumes multiply, and the sound-effect classes
(`ReverbSoundEffect` ... `PitchShiftSoundEffect`) describe processors through the `audio_effect` hook (an `EffectSpec`) for the `Sound` or the `SoundGroup` they sit in.
The sound card is opened the first time something plays; with no card, or `FORGE_AUDIO=off`, the engine renders the mix into a scratch buffer as time
passes, so sounds still progress, end and report it exactly as they would be heard (this is what the tests use). The editor previews a sound from the
Asset Browser through the same engine. Sounds do not play in the editor's edit mode.

### Camera effects and the sky

`render/camera_fx.rs`: the things that affect *every* camera are objects in the scene (the `Lighting` service and the effects inside it). Each class says
what it contributes with the `camera_effect` hook of its component; `collect` gathers it into one plain `CameraEffects`, `render::sync` turns it into a
`CameraLook` when it changed and `apply_camera_look` puts that on every 3D camera (the editor's too, so the viewport shows what the game will):
`BloomEffect` -> Bevy bloom (on a high-dynamic-range picture; Neon and emissive materials glow through it), `ColorCorrectionEffect` -> colour grading,
`DepthOfFieldEffect` / `BlurEffect` -> depth of field, `Atmosphere` and `Lighting.Fog*` -> distance fog, `Sky` -> a cube-mapped sky that also lights the scene
(metal and glass reflect it), `SunRaysEffect` -> volumetric light shafts, `Lighting` itself -> the hemisphere of ambient light. Not drawn: the sun, moon and
stars of Roblox's sky.

### Effects that move on their own

`render/effects.rs`: `ParticleEmitter`, `Fire`, `Smoke`, `Sparkles`, `Beam`, `Trail`, `Clouds` and (with `render/on_top.rs`) `Highlight` are drawn by the
`animate` hook of a component, which `render::sync` calls every frame for each instance with the `Animated` marker (it is not limited to what changed). The
class turns its properties into a plain spec (`EmitterSpec`, `BeamSpec`, `TrailSpec`, `CloudSpec`); `effects.rs` simulates and draws it. State lives next to the
instance in the asset cache (`RenderCtx::fx_state`); the entities an effect makes are registered with `fx_own` and go when the instance is destroyed, leaves the
world or is switched off (`fx_clear`). Particles, beams and trails are one entity each with a mesh rebuilt every frame (quads that face the camera, or a ribbon),
drawn unlit with vertex colours; a `LightEmission` of half or more adds the light to what is behind it. `Emit` and `Clear` reach the renderer through counters
(`EmitCount`, `ClearCount`, runtime properties), so a script call needs no access to the renderer. A highlight draws, for each part it marks, a tinted copy of the
mesh (always on top, or occluded, through a pipeline that changes the depth test) and a slightly larger back-face copy as the outline. Not done: shading by the
world's lights, particle shapes other than the face, outlines that show through walls, clouds that are lit or cast shadows.

### Characters are scripts

Forge has no built-in character: no `Humanoid`, `Tool`, `Accessory`, `Backpack`, `Team`, `StarterGear` or `ForceField` (old scenes that contain them still load: those objects, with everything inside them, are left out without a warning, and
the properties that went with them are ignored; `dm/file.rs` keeps the list of retired classes and properties). A player is whatever a script makes: the starter
template's `FirstPersonController.luau` moves a body with `Rigidbody` properties (`FixedRotation` keeps it upright), `Raycast` and `UserInputService`, and
`Players` / `Player` only describe who is playing. `Rigidbody.FixedRotation` is the general building block for upright, script-driven bodies.
## Adding things

### A new class (one file)

`crates/forge_engine/src/classes/world/lights/directional_light.rs` is a complete small example; the minimum is:

```rust
use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::number("Radius", 1.0).group("Orb").order(10).flags(F_VISUAL).slider(0.0, 50.0).desc("Size of the orb."),
];

pub static CLASS: ClassDesc = ClassDesc::new("Orb", Some("WorldObject"))
    .describe("A glowing orb.")
    .category("Effects")         // Insert menu category
    .creatable()                 // Instance.new / editor
    .props(PROPS);
```

`build.rs` finds the file wherever you put it under `classes/`; the registry resolves the inheritance chain; and the
class is immediately available in `Instance.new("Orb")`, as `orb.Radius` in Luau, in scene files, in the editor's
Insert menu, and in the Properties panel.

A class can also declare, in the same file: `methods(&[MethodDesc::new("Name", rust_fn)])` (scripting-neutral, see
`core/script.rs`), `signals(...)`, a Luau snippet `luau(include_str!("x.luau"))` for methods that yield, enums
(`pub static ENUMS: &[EnumDesc]`), editor insertion rules (`parent_rule`, `insert_props`, `spawn_at_focus`,
`script_stub`), service flags (`service(order)`, `persisted()`, `runs_scripts()`), `unsupported(..)` / `callbacks(..)`, and an
optional `ClassBehavior` (`on_created`, `on_loaded`, `on_prop_changed`, `on_parent_changed`, `on_destroying`, `on_started`).

### A new light type

Copy `directional_light.rs`, rename, and change `apply` to insert the Bevy light you want (add its own properties
to a `PROPS` list if it has any). `Light` and the existing lights stay untouched.

### A new component (one file)

Export `pub static COMPONENT: ComponentDesc` from the class file that owns it, or from `components/<name>.rs` when
several classes share it, with any of: `props`, `requires`, `render`, `physics(order, fn)`, `collider`,
`on_changed`, `gizmo`. Then list its name in the `components(&[...])` of the classes that use it.

### Property metadata (what the inspector shows)

Every `PropMeta` can set: `label`, `desc` (tooltip), `group`, `order`, `hidden()`, `readonly()`, `widget` /
`slider(min, max)` / `drag(speed)` / `multiline()`, `clamp`, conditional visibility (`when_true("Other")`,
`when_false`, `when_eq("Shape", D::E("Ball"))`, `when_ne`), `aliases(&["OldName"])` (old names keep working in scripts
and scene files), `runtime()` (never saved), `no_save()`, and computed properties
(`computed(getter, setter)` / `computed_ro(getter)`), which is how `Position`, `Name`, `Parent` and
`Players.LocalPlayer` are implemented without any special-casing. Inspector groups are ordered by each class's
`groups(&[...])` (parents first); properties by `order`. A class adds its own sections after the ones it inherits:
`Instance` gives `General` (Name, Parent), `WorldObject` `Transform`, `BasePart` `Appearance`, `Physics`, `Collision`,
`Rendering`, `Editor`, `Light` `Light`, `Shadows`, `Rendering`; `Part` / `MeshPart` / `Model` / `SpotLight` / `AreaLight`
add `Geometry` / `Geometry` / `Model` / `Spotlight` / `Area`. Empty sections are not shown. `label` changes only what the
panel prints (`Orientation` shows as "Rotation"), not the name scripts and scene files use.

### Reordering / hiding inherited properties

```rust
ClassDesc::new("Decal", Some("BasePart")).hide(&["Anchored", "CanTouch"]).defaults(&[("CanCollide", D::B(false))])
```

### Future systems

A new subsystem (networking, ...) follows the pattern `audio/` did: a folder next to `physics/` and `render/`
that depends on `core` and `dm` only; a new hook type in `core/hooks.rs` plus a field and builder on
`ComponentDesc` so classes can opt in (`audio_effect`, `camera_effect` and `animate` were added this way); and classes
that list the component. Nothing in `dm`, `scripting` or the editor needs to learn about it.

## What is data-driven

| Concern | Where it is described |
|---|---|
| Class list, properties, defaults | each class file |
| Services, persisted services, script-running services | flags on the service's own class |
| Enums | `ENUMS` in the file that owns them |
| Signals (`Touched`, `Heartbeat` ...) | `signals(...)` on the owning class |
| Class-specific script methods | `methods(...)` / `luau(...)` on the class |
| Rendering | components' `render` hooks (and `animate` for effects that move) |
| Screen effects, the sky | components' `camera_effect` hooks |
| Sound and its effects | the `Sound` class and components' `audio_effect` hooks |
| Materials | `.mat` files in the project |
| Per-step physics behaviour (a character controller) | components' `physics` hooks |
| Editor Insert menu & placement rules | `category`, `parent_rule`, `insert_props` |
| Property inspector, serialization | `PropMeta` (including `saved()` and aliases) |

## Tests

`cargo test -p forge_engine` covers the registry (every class resolves, parents exist, component names are
defined, every service exists, light types share the `Light` properties), the data model and scene format, the pak
format, and physics. `scripting/compat_tests/` runs real Luau against a real data model and physics world through the
harness in `scripting/testkit.rs` (`Game`, `checks`): data types, instances and the tree, signals and `task`, modules and
`require`, classes, services, physics and whole Roblox-style scripts. A new Roblox-facing feature gets a test there.

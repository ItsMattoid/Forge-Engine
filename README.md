# Forge

A data-driven, Roblox-style game engine in **Rust**, rendered by **Bevy**, scripted in **Luau**.
Create a project, edit it in the built-in editor, and build it into a standalone game.

> **Status:** the workspace builds (Bevy 0.19) and `cargo test -p forge_engine -p forge_editor` passes: the tests cover the registry, the
> data model and scene format, the pak format, physics, real Luau scripts, assets (models, textures, materials), audio, the render mirror and the editor.
> Sound and the screen were checked by tests and by screenshots (`forge_player --screenshot`); nobody has listened to the sound on a real speaker yet.

## Quick start

```bash
cargo build --release                       # forge, forge_editor, forge_player
./target/release/forge new ~/games/demo     # new project (obstacle course + first-person controller)
./target/release/forge edit ~/games/demo    # open the editor
./target/release/forge run ~/games/demo     # play from the project folder
./target/release/forge check ~/games/demo   # validate scenes + Luau syntax
./target/release/forge build ~/games/demo   # -> ~/games/demo/build/demo/{demo, game.pak}
```

Keep the three binaries in one folder. Linux needs the usual Bevy packages
(`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`).

Physics (Box3D) is compiled from source: building also needs **CMake** and **libclang** (LLVM) - set `LIBCLANG_PATH` to the
folder with `libclang.dll` / `libclang.so` if the build cannot find it (see [Physics](#physics)). On Windows,
`powershell -ExecutionPolicy Bypass -File tools\setup-windows-build.ps1` installs/locates both and records them for cargo (in `~/.cargo/config.toml`, so any terminal works). A build error such as `is cmake not installed?` or `Unable to find libclang` means this setup is missing.

> **Breaking change from the first version:** Forge is now a 3D, Roblox-style engine. The 2D mode, the
> old scene format and the old scripting API are gone. Create new projects with `forge new`.

## The data model

Everything is an **Instance** in a tree: a class, a `Name`, a `Parent`, children and properties.
The tree is rooted at `game`, whose children are **services**:

| Service | Purpose |
|---|---|
| `Workspace` | the 3D world (parts, models, lights, the camera); `Gravity`, `CurrentCamera` |
| `Players` | `LocalPlayer` (with `Character`, which your scripts set), `PlayerAdded`, `GetPlayers` |
| `Lighting` | sun (`ClockTime`, `Brightness`), `SkyColor`, `Ambient`, `AmbientBrightness`, `GlobalShadows` |
| `ReplicatedFirst`, `ReplicatedStorage`, `ServerStorage` | containers for objects needed first, shared objects and values, and hidden objects |
| `ServerScriptService`, `StarterPlayer`, `ReplicatedFirst` | where `Script` / `LocalScript` objects run (also inside `Workspace`) |
| `StarterGui`, `SoundService` | containers for the player interface and for sounds |
| `RunService` | `Heartbeat`, `Stepped`, `RenderStepped` |
| `UserInputService` | `InputBegan/Ended/Changed`, `IsKeyDown`, `GetMouseDelta`, `MouseBehavior` |
| `CollectionService`, `Debris`, `TweenService` | tags, timed destruction, tweens |

The Explorer lists the first ten services above (Workspace to SoundService, in the order shown). `RunService`, `UserInputService`,
`CollectionService`, `Debris` and `TweenService` have nothing to edit, so they are not listed there; scripts reach them with `GetService`.

Classes: `Folder`, `Model`, `Part` (`Shape` Block/Ball/Cylinder), `MeshPart` (`MeshId`: a glTF, GLB, OBJ, STL or PLY file anywhere in the project; both are
`BasePart`s with `Material`, `Color`, `Transparency`, `Anchored`, `CanCollide`, `CanTouch`, `CastShadow`, `Size`,
`CFrame`/`Position`/`Orientation`), `Camera`, lights (`PointLight`, `SpotLight`, `AreaLight`, `DirectionalLight`: all `Light`s with `Color`,
`Intensity`, `CastShadows`, `Enabled`; `Brightness` and `Shadows` still work as aliases), `Sound`, `Script`, `LocalScript`, `ModuleScript`, `BoolValue`, `IntValue`,
`NumberValue`, `StringValue`, `ObjectValue`.

Each class lives in its own file under `crates/forge_engine/src/classes/` (grouped into `base`, `world`, `characters`,
`scripts`, `values`, `services`) and describes itself: properties with editor metadata, methods, signals, and the
Bevy renderer or physics behaviour only it needs. Behaviour shared by several classes lives in `src/components/`.
Files are discovered automatically, and the Luau API, `Instance.new`, scene files, the editor's Insert menu and
Properties panel and the runtime are all driven by the resulting registry. See **[ARCHITECTURE.md](ARCHITECTURE.md)**
for the module map, the class hierarchy, the data flow, and how to add a class (one file), a light type or a property.

Scripts and the editor edit the data model; the renderer mirrors only *changed* instances into Bevy
entities, and the physics step reads the data model. Parts use world-space CFrames like Roblox
(a Model has no transform of its own: `model:PivotTo(cf)` moves all its parts).

## Scripting (Luau, Roblox-style)

Script objects reference a `.luau` file through their `SourceFile` property (edit it in the built-in
script editor or any external editor). Each script runs in its own coroutine; `script` is the Script
instance.

```lua
local RunService = game:GetService("RunService")
local part = script.Parent

part.Touched:Connect(function(other)
	print(other.Name, "touched", part.Name)
end)

RunService.Heartbeat:Connect(function(dt)
	part.CFrame = part.CFrame * CFrame.Angles(0, math.rad(60) * dt, 0)
end)

task.wait(2)
local bullet = Instance.new("Part")
bullet.Size = Vector3.new(1, 1, 1)
bullet.Position = Vector3.new(0, 20, 0)
bullet.Parent = workspace            -- unanchored parts fall
```

Available: `game`, `workspace`, `script`, `Instance.new`, `Vector3` (native Luau vectors, `.Magnitude`,
`.Unit`, `:Dot`, `:Cross`, `:Lerp`), `CFrame` (`new`, `Angles`, `fromOrientation`, `lookAt`, `*`, `Inverse`,
`ToWorldSpace`, `LookVector`, ...), `Color3`, `Enum.*`, `task.wait/spawn/delay/defer/cancel`, signals
(`:Connect`, `:Once`, `:Wait`, `:Disconnect`), `require(ModuleScript)`, `RaycastParams`, `OverlapParams`, `TweenInfo`, `time()`,
`tick()`, `typeof()`, `shared`, `print`/`warn`.

Instance members: `Name`, `Parent`, `ClassName`, `Destroy`, `Clone`, `GetChildren`, `GetDescendants`,
`FindFirstChild(name, recursive)`, `FindFirstChildOfClass/WhichIsA`, `FindFirstAncestor/OfClass`, `WaitForChild`,
`IsA`, `IsDescendantOf`, `IsAncestorOf`, `ClearAllChildren`, `GetFullName`, `GetPropertyChangedSignal`,
`SetAttribute/GetAttribute`, `PivotTo/GetPivot`, signals `Changed`, `ChildAdded`, `ChildRemoved`, `Destroying`,
and `Touched`/`TouchEnded` on parts. `workspace:Raycast(origin, direction, params)` (and the other queries and part methods in **Physics** below),
`game:GetService(name)`, `game:Shutdown()`, `game:LoadPlace("level2")` (Forge extension).

The API follows Roblox's Engine API: over a hundred classes (parts, models, attachments, constraints and movers, players,
values, lights, effects, sounds, tweens, about fifty services), Roblox's data types (`Vector2/3`, `CFrame`, `Color3`,
`BrickColor`, `UDim2`, sequences, `Ray`, `PhysicalProperties` ...) and enums, `require` with module caching, `task`, attributes and
tags. **[COMPATIBILITY.md](COMPATIBILITY.md)** lists what is implemented, what only keeps its state (animation
playback) and what is refused with a clear message (online services, GUI classes, CSG, `Script.Source`). The compatibility
tests are in `crates/forge_engine/src/scripting/compat_tests/`.

Deliberate differences from Roblox: there is no replication/server-client split (everything runs locally, one player);
there is no built-in character (no `Humanoid`, `Tool`, `Team`, `Backpack` ...): a game's own scripts make and move its player;
destroying a Script stops its threads and connections; `Camera.CameraType` is informational (the current camera follows its `CFrame`);
signal handlers run after the change that caused them, as with Roblox's deferred signals.

Frame order: the fixed-rate physics step(s) -> input events -> `Stepped` -> `Heartbeat` -> `task.wait` wake-ups -> `RenderStepped`.
(Physics runs first in Bevy's `FixedUpdate`, 60 steps a second whatever the frame rate; the scripts of the frame see its result, and what they change is applied before the next step.)

## Physics

Physics is **Box3D** (the `box3d` crate), through **`bevy_box3d`**: its plugin owns the world and steps it at a fixed 60 Hz in
Bevy's `FixedUpdate` (8 solver sub-steps each, one unit = one stud = 0.28 m). The engine's own part, `crates/forge_engine/src/physics/`,
keeps that world in step with the data model; its module docs say who owns what. In short: **edits go in, results come out**.
A script or the editor changes a property and the data model notes it; before the next step only the parts that changed are
brought up to date. After the step, the bodies that moved write their position and velocity back (muted, so a result is never
mistaken for an edit and sent back). Nothing is polled: a resting scene costs nothing (resting parts sleep), contact events are only
produced for parts something listens to, and a part nobody touched is never looked at.

**Bodies.** An unanchored part is a dynamic body; an anchored one is static; an anchored part with `Kinematic` on moves by its
velocity (or toward a `CFrame` a script sets) and pushes what it meets without being pushed. Setting the `CFrame`/`Position` or the
velocity of a dynamic part teleports it and the simulation goes on from there. Spawning, destroying, re-parenting out of and back into
the workspace, moving a model and loading a scene all create and destroy the world's bodies; nothing is left behind.

**Part properties** (all validated when written - numbers are held in range, NaN/infinity refused - and all changeable while the game runs):

| Property | Default | |
|---|---|---|
| `Anchored`, `Kinematic` | false | fixed in place; anchored + kinematic moves by velocity |
| `CanCollide`, `CanTouch`, `CanQuery` | true | collides / `Touched` fires (both parts need it) / found by queries. A part that does not collide is a *sensor*: it reports what overlaps it |
| `CollisionGroup` | `"Default"` | which collision group (see `PhysicsService`) |
| `Density`, `Mass` (read-only) | 1 | `Mass` = volume x `Density` |
| `Friction`, `Elasticity` | 0.3, 0 | grip (mixed as sqrt(a*b)) and bounce (mixed as max(a, b)) |
| `AssemblyLinearVelocity`, `AssemblyAngularVelocity` | 0 | studs/s and rad/s (aliases `Velocity`, `RotVelocity`); anchored parts always read 0 |
| `GravityScale`, `LinearDamping`, `AngularDamping` | 1, 0, 0 | per-part gravity multiplier and drag |
| `CanSleep`, `Sleeping` (read-only) | true | resting parts sleep; anything touching, pushing or editing one wakes it |
| `ContinuousCollision` | false | swept collision against other moving parts (every part is always swept against anchored ones) |

`Workspace` adds `Gravity` (196.2), `AllowSleeping`, `ContinuousCollision` and `FallenPartsDestroyHeight` (-500).

**Part methods:** `ApplyImpulse(v)`, `ApplyImpulseAtPosition(v, position)`, `ApplyAngularImpulse(v)`, `GetMass()`,
`GetVelocityAtPosition(position)`, `Sleep()`, `WakeUp()`, `GetTouchingParts()`. They take effect at once (a part created a line
earlier can be pushed) and raise a script error with the reason when they cannot (anchored part, not in the workspace, NaN).

**Events:** `part.Touched` / `part.TouchEnded` (pass the other part), also for parts that do not collide (a trigger zone) and
when a touching part is destroyed. `GetPropertyChangedSignal("Sleeping")` reports sleep and wake-up.

**Spatial queries** on `workspace`, answered by the simulation: `Raycast(origin, direction, params?)` (the direction is also the length),
`Spherecast(position, radius, direction, params?)`, `Blockcast(cframe, size, direction, params?)` - each returns
`{Instance, Position, Normal, Distance, Material}` or `nil` - and `GetPartBoundsInBox(cframe, size, params?)`,
`GetPartBoundsInRadius(position, radius, params?)`, `GetPartsInPart(part, params?)`, which return lists. `RaycastParams.new()` /
`OverlapParams.new()` have `FilterDescendantsInstances`, `FilterType` (`Exclude`/`Include`), `CollisionGroup`, `RespectCanCollide` and
(overlap) `MaxParts`. Queries see exactly what `CanQuery`, the filter and the group let them.

**Collision groups:** `game:GetService("PhysicsService")` - `RegisterCollisionGroup(name)`, `UnregisterCollisionGroup`,
`CollisionGroupSetCollidable(a, b, bool)`, `CollisionGroupsAreCollidable`, `IsCollisionGroupRegistered`,
`GetRegisteredCollisionGroups`, `SetPartCollisionGroup(part, name)`. Up to 32 groups, saved with the scene; changes apply to parts
already in the world at once. A part in a group that is not registered behaves as `Default` (and a warning says so).

**Constraints** are ordinary instances (`Instance.new("HingeConstraint")`, set `Part0`/`Part1`, parent it into the workspace;
change a property to reconfigure it, set `Enabled` false, clear a part or `:Destroy()` it to release it): `WeldConstraint`,
`HingeConstraint` (limits, motor), `BallSocketConstraint` (cone and twist limits), `PrismaticConstraint` (limits, motor),
`RodConstraint`, `RopeConstraint`, `SpringConstraint` (`FreeLength`, `Frequency`, `DampingRatio`, limits). Where the parts are joined
is `Anchor0`/`Rotation0` (in `Part0`) and `Anchor1`/`Rotation1` (in `Part1`, or the world when there is no `Part1`), also as the CFrames
`C0`/`C1`; a hinge turns, and a prismatic constraint slides, along the X axis of that frame. `constraint:SetWorldFrame(cframe)` places
the frame in the world, `UseCurrentLength()` sets a rod, rope or spring to the distance it has now. A weld holds the parts as they
are when it is made.

**Characters are scripts.** The engine has no character class. A body that a script steers (the starter project's player) is an ordinary
unanchored `Part` with `FixedRotation` on (so it cannot tip over) whose `AssemblyLinearVelocity` the script sets each frame; it falls, stands on
and is stopped by everything the simulation holds, and `Raycast` finds the ground under it.

**Limits, stated plainly**
* Parts are convex solids. A block, a ball and the wedges are exact; a *stretched* ball (unequal sizes) is the convex hull of 42
  points on the ellipsoid, a little smaller than it. Concave collision (the exact surface of a mesh part) is not simulated.
* A part collides no thinner than about 0.07 studs and not bigger than the world holds; a part whose bounds leave the world (about
  160 000 studs) is left alone, not simulated. Speeds are held to 10 000 studs per second.
* Two anchored parts that both collide never report `Touched` (Box3D has no contact events between bodies that cannot move).
* Moving a welded part by its `CFrame` moves it alone, and the weld pulls the other part after it; use `PivotTo` on a model, or a
  constraint's frames, to move an assembly.
* The data model shows each part where the last step left it (no interpolation between steps: at 60 fps and 60 steps they coincide).

**Building.** Box3D is compiled from source, so building needs **CMake** and **libclang** (LLVM; for the Rust bindings) on the
path - set `LIBCLANG_PATH` to the folder holding `libclang.dll` / `libclang.so` if it is not found - besides a C toolchain
(Visual Studio Build Tools on Windows, `cc` elsewhere).
## Assets, materials, sound and effects

Everything a game uses is a **file of the project**, and objects only hold its path. Drag a file from the Assets tab onto an object, onto a
property field or onto the viewport, or press the small button beside an asset property to pick one from a list; moving or renaming the file in
the editor keeps every reference to it, and a file that goes missing is reported (in the Output, and as a warning on the property) instead of
breaking the scene.

* **Models**: glTF / GLB, OBJ (+ MTL), STL and PLY. A model keeps the size it was made with (`Project Settings`: `[assets] mesh_unit_scale` says how
  many studs one unit of the file is), is centred on its bounding box and a new `MeshPart` is as big as the model; resizing the part scales the
  mesh. Normals, tangents, texture coordinates and the model's own materials and textures are used. FBX and Collada are refused with a message that
  says what to export instead.
* **Textures**: PNG, JPEG, BMP, TGA, GIF, WebP, TIFF, HDR. Colour maps are read as sRGB, normal / roughness / metallic maps as data.
* **Materials** are `.mat` files in `Assets/Materials`. A new project has `Plastic`, `SmoothPlastic`, `Metal`, `Neon` and `Glass`; they are ordinary
  files you can edit, replace or delete, and you can add as many as you like. Open the **Material Editor** (View menu, or double-click a `.mat`
  file): base colour, roughness, metallic, opacity, emission, glass edge, a base-colour / normal / roughness / metallic / occlusion / emission map each
  (drop a texture on the slot), texture coordinates (studs or the mesh's, tiling, offset, rotation), filtering and wrapping, with a live preview and
  changes written to the file as you drag. A part uses a material through its `Material` property (the file named like the item) or its
  `MaterialVariant` (any `.mat`: drop one on the part, or pick it). Neon glows through a `BloomEffect` in `Lighting`; Glass is see-through with a
  brighter edge and reflects the sky. Materials need no engine change: the five names above are only what a new project starts with.
* **Sound**: a `Sound` object plays a wav, mp3, ogg, flac or m4a file (`SoundId`: drop one on it, or drop a file on a part to make a `Sound` that comes
  from it). `Volume`, `PlaybackSpeed`, `Looped`, `Playing`, `TimePosition`, `Play / Pause / Resume / Stop`, `Played / Paused / Resumed / Stopped / Ended /
  DidLoop`. A sound inside a part or attachment is positional (volume falls with distance by `RollOffMode`, `RollOffMinDistance`, `RollOffMaxDistance`;
  it pans between the ears and bends in pitch as it moves); a `SoundGroup` sets the volume of the sounds in it; `ReverbSoundEffect`, `EchoSoundEffect`,
  `DistortionSoundEffect`, `ChorusSoundEffect`, `FlangeSoundEffect`, `TremoloSoundEffect`, `EqualizerSoundEffect`, `CompressorSoundEffect`,
  `PitchShiftSoundEffect` and the filter effects process the `Sound` or `SoundGroup` they are in. `SoundService` has the room reverb, Doppler and
  roll-off scales. Double-click a sound in the Assets tab to hear it. `FORGE_AUDIO=off` plays nothing (the tests use it).
* **Lights and the picture**: `PointLight`, `SpotLight`, `SurfaceLight` / `AreaLight` and the sun cast shadows and light normal-mapped surfaces; Neon and
  any emissive material give off light that blooms; `BloomEffect`, `ColorCorrectionEffect`, `DepthOfFieldEffect`, `BlurEffect`, `SunRaysEffect`,
  `Atmosphere`, `Sky` and `Clouds` are drawn on every camera, the editor's too.
* **Effects**: `ParticleEmitter` (`Emit`, `Clear`), `Fire`, `Smoke`, `Sparkles`, `Beam`, `Trail`, `Highlight`, `Decal` and `Texture` are drawn. They are
  not shaded by lights; see [COMPATIBILITY.md](COMPATIBILITY.md) for what each does and does not do.
## Project files

A new project starts from this layout. It is a **template, not a rule**: every folder is optional, and files and
folders can be created, renamed, moved and nested anywhere inside the project.

```text
MyGame/
  Assets/      Models/ Textures/ Materials/ Audio/ Animations/ Fonts/
  Scenes/      MainScene.scene   Prefabs/
  Client/      Controllers/ UI/ Input/
  Server/      Services/ Systems/
  Shared/      Modules/ Types/ Constants/
  Packages/    Plugins/
  Project.toml README.md
```

* **Luau is the only scripting language.** A script object in a scene (`Script`, `LocalScript`, `ModuleScript`) stores the
  *path* of its `.luau` file; not every object needs a file, and a file needs no object. **Where a file is stored never
  decides where it runs**: that is the object's class and place in the scene (`Script` runs on the server,
  `LocalScript` on the client, `ModuleScript` when it is `require`d). Moving a script file between folders (even out of
  `Client/`, `Server/` and `Shared/`) changes nothing about how it runs.
* Making a script in the editor (Insert menu) creates its file, named after the object, in `Server/`, `Client/` or
  `Shared/Modules/` (made on demand). Renaming the object renames the file with it when the file was named after it and
  nothing else uses it. Deleting the object keeps the file (delete it from the Assets tab, which asks first and lists the
  scenes that still use it). The script editor and the file stay in step: a file changed outside is reloaded, or flagged
  when there are unsaved changes here.
* A project's settings are in `Project.toml`: `[project]` (name, version, start scene), `[game]` (`type = "single_player"`
  or `"multiplayer"`), `[window]` (title, size, `display_mode` = `windowed` / `borderless` / `fullscreen`, V-Sync,
  `fps_limit`, `msaa`, `shadow_quality` = `off` / `low` / `medium` / `high`, occlusion culling) and `[launch]` (the `icon`
  and the `splash` screen, each the *project path* of a PNG or JPEG) and `[assets]` (`mesh_unit_scale`: studs per unit of a model file, 1 by default). Every key has a default and unknown keys are kept, so
  an older or newer manifest loads and is saved back without losing anything (`[multiplayer]` is kept as written even for a
  single-player game: nothing set for multiplayer is lost by switching the type). The **quality preset** (Low / Medium /
  High / Ultra) is not stored: it is read off the anti-aliasing and shadow settings, and any other mixture is *Custom*.
  These are the *defaults of the built game*. A player's own choices (a smaller window, no V-Sync, lower shadows) are kept
  per game and per user in `Forge/Players/<game>/player_settings.json` in the user's config folder and win over them when
  the game starts (see `project::player_settings`); the editor's own viewport settings are separate (Editor Settings >
  Rendering). The icon is set on the game's window when it starts, the splash screen is shown over the window until the
  first scene is loaded and drawn (at least 1.5 s); both are files of the project, so a build packs them, and the editor,
  `forge check` and a build all report a missing or unusable one the same way. Scenes are `.scene` files (the same JSON as the
  older `.scene.json`, which still loads), prefabs are `.prefab`. Projects made earlier (`forge.toml`, `scenes/`,
  `scripts/`, `assets/`) open and run unchanged.
* The editor finds everything by looking at the real folders (about once a second), keeps a stable id for each file in
  `.forge/assets.json`, and recognises a file moved or renamed outside the editor by its contents, so scenes keep
  pointing at it. Deleted files go to `.forge/trash`. A build packs every file of the project.

## First-person controller

`Client/Controllers/FirstPersonController.luau` (a `LocalScript` in `StarterPlayer`) is a plain script you can read and
edit; its settings are at the top. Mouse look, `W A S D`, `Space` jump, `Shift` run (with FOV kick), `C` or `Ctrl`
crouch (the body shrinks and you can only stand where there is headroom), `Escape` releases the mouse and a
click captures it again. It drives the `Body` part of a model named `Player` (an ordinary part with `FixedRotation` on), like the
one in the template; the engine has no character of its own, so a different game just writes a different controller.

## Editor

* **Windows are a dock.** The viewport, Explorer, Properties, Output, Assets and Script Editor are panels you arrange: drag a tab by its title to another place (the dock shows where it will land: beside,
  above, below, or as another tab in a group), reorder tabs by dragging, split a group by dropping at its edge, drag a
  tab out of the editor (or right-click it > move to a new window) to make it a **floating window**: move it by its tab
  bar, resize it by its edges, minimise or close the whole window with the buttons at the ends of its tab bar (hold
  Shift, or right-click them, for those on a window), and drag a tab back into the editor to dock it. The button at the
  left of a group's tab bar collapses the group, the one at the right closes its tabs. Moving, tabbing, floating and
  hiding a panel never resets what is in it (the open script and its unsaved changes, the search box, the selection).
  The **View** menu lists every panel with a tick for the ones that are shown: it, the close buttons on the tabs and the
  shortcuts always agree. **Editor Settings** and **Project Settings** are not part of the dock: they are windows of their own
  (listed at the bottom of the View menu, also in the File menu) that cannot be dragged into a group or tabbed with the panels,
  while the panels still dock, float and tab as before. Each one reopens where you left it, at the size you left it, also
  after the editor was closed (their places are in the editor settings; a place that no longer fits the screen is pulled back onto it). **View > Restore Default Layout** (Alt+0) puts everything back. The layout (docking, tabs,
  floating windows with their positions and sizes, what is hidden) is saved with the editor settings and comes back the
  next time; a panel the saved layout does not know is left out and one it lists twice is kept once.
* **Window shortcuts**: Alt+1 Viewport, Alt+2 Explorer, Alt+3 Properties, Alt+4 Output, Alt+5 Assets, Alt+6 Script Editor
  (Alt+S and Ctrl+Alt+S open and close the two settings windows). One press shows a hidden window, brings a shown one forward, and hides the one
  already in use. They are in **Editor Settings > Shortcuts** (Windows) like every shortcut, so they can be changed, and
  an already used key is refused. They do nothing while a text field has the keyboard, so typing in the script editor,
  the search boxes and the properties is never interrupted.
* **Explorer**: the instance tree. Click selects, Ctrl + click adds / removes one object, Shift + click selects the
  rows from the last one clicked (Ctrl + Shift keeps the current selection). Search filters by name and / or class (the
  **Filter** menu: match case, search in name / class, only one type of object); the matches are shown under the objects
  that contain them (dimmed), and clearing the search brings the tree back as it was. Drag items onto another to re-parent
  them (all selected ones move together); right-click for Insert / Duplicate / Delete / Focus. Selecting in the viewport
  expands and scrolls to the item.
* **Assets** (a panel, along the bottom to start with): the folders inside the project as a tree on the left (the project folder itself is not a row
  there), and the folder you are in as a grid of tiles on the right, under a breadcrumb that starts with the game's name
  (`MyGame > Assets > Models`; click any part, including the game's name for the top, to go there; double-click a folder to
  open it). Images show thumbnails. The search box filters by name, `type:mesh` (or `texture`, `script`, `scene`, ...) and
  `ext:png`, in the folder being browsed and the folders below it. `+ New` makes folders and Luau files, `Import` (or
  dragging files from your computer onto the editor) copies files in; right-click for Open / Preview, Rename (F2),
  Duplicate, Move to, Delete (asks first), Reveal in Containing Folder, Reimport. Dragging moves things between folders.
  Moves and renames made here keep the scenes, the script editor and the Explorer pointing at the right files.
  Double-click: a folder opens; a scene opens; a `.luau` opens in the script editor; a model creates a `MeshPart` (or
  fills in the selected `MeshPart`); a prefab is placed in the scene; an image opens a preview; a `.mat` opens in the Material Editor; a sound plays.
  **Drag** an asset onto the viewport, an Explorer row or a property field (green = it works, red = it does not, with the reason): a model creates a
  `MeshPart` where it lands or assigns the mesh to the `MeshPart` it is dropped on, a picture lies over the part it is dropped on (a `Decal`) or fills a
  `Decal` / texture property, a `.mat` becomes the material of the part it is dropped on, a sound becomes a `Sound` in the part it is dropped on (or the
  `SoundId` of the `Sound` it is dropped on), a `.luau` adds a script object using it (or assigns it to the script it is dropped on), a prefab places its
  objects. Animation files are listed, but nothing in this version of Forge can use them yet, and the editor says so. Right-click an object in the
  Explorer > **Save as Prefab** makes a prefab in `Scenes/Prefabs`.
* **Properties** (right): generated from the class table for whatever is selected. The window's title says what it is
  showing: `Properties - Part "SpawnLocation"` for one selected object, `Properties - Multiple Objects`, or plain
  `Properties`. **Parent** is a button, not a
  drop-down: click it, then click the object to move this into, in the Explorer (services included) or in the viewport
  (Esc cancels; an object cannot go inside itself or something inside it). A Model's **Primary Part** is chosen the
  same way (click the button, then click one of the model's parts; **Clear** unsets it), but only a part inside the model
  is taken - never the model, an object outside it, or something that is not a part; the data layer refuses anything else
  too, whatever way it is set (scripts, files, undo). A part that leaves the model or is deleted stops being its primary
  part. A Model also has a **Scale**: it resizes every part inside (at any depth) and moves them toward or away from the
  model's pivot by the same factor, so rendering, collision, physics, bounds and the saved scene all follow; the pivot
  and the orientations stay. It is uniform (a part turned between the model's axes cannot be stretched without shearing),
  one undo step, kept by Duplicate, and saved as what it is (the parts are stored at their scaled size, so loading does
  not scale them again). Parts added later keep their own size until Scale is changed again; a scale that would make a
  part smaller than 0.001 or bigger than 100000 studs is refused and nothing changes. Dragging the Scale tool's handles
  with Shift on a model keeps its Scale in step.
* **Insert Object** (Shift+Space) and the snapping popups close when you click anywhere outside them, or press Esc.
* **Project Settings** (File menu, or Ctrl+Alt+S, which also closes it): a window like Editor Settings, with tabs on the
  left in groups: *Project* (General: name, version, start scene; **Game Type**: single-player or multiplayer),
  *Game Window* (Display: title, display mode, size, resizable; Graphics: quality preset, anti-aliasing, shadows, V-Sync,
  frame-rate limit, occlusion culling, and where a player's own settings are kept; **Icon & Splash Screen**: each with a
  preview, *Select / Replace*, *From the project* and *Clear*; a picture from elsewhere is copied into `Assets/Textures`,
  one that cannot be used is refused with the reason and nothing changes) and, only for a multiplayer game,
  *Multiplayer*. Forge has no networking yet, so that page offers nothing to set and says so (the options go there as
  the engine gets them); a single-player game hides it and the Network Statistics debug overlay. Every page says
  whether it is about the built game or the editor, has a Reset button, and changes are saved to `Project.toml` as you
  make them.
* **Viewport**: while a game plays in it, the viewport's own group gets a second tab: **Editor** (the viewport as
  it was) and **Game** (the running game); clicking one brings it forward without stopping or restarting anything, the
  editor camera and the scene stay as they were, and Stop removes the Game tab. It is a tab like the others (not a
  toolbar button, and it cannot be closed or pulled out into a window of its own), and is never saved with the layout.
  The viewport grid is off until **Editor Settings > Rendering > Show Viewport Grid** is ticked. Ctrl+R and Ctrl+T turn the whole selection a quarter turn around the Y or X axis *as one group*: it swings
  round one point (the Rotate tool's: the pivot of a single object, otherwise the middle of everything selected; in Local
  space the active object's axes), so how the objects sit relative to each other is kept, nothing inside a selected
  object is turned twice, nothing unselected moves, and one Ctrl+Z puts it all back. Pressed *while dragging* (an arrow,
  a plane, a ring, or an object being carried), the turn becomes part of the drag: the objects stay where the drag has
  put them, turn, and the drag carries on from there under the same mouse, pivot, space and snapping (a drag measures
  from where it began, so turning the objects behind its back used to be undone by its next frame). Taking hold of
  Ctrl for the shortcut does not change the snapping until the mouse next moves, so the objects do not jump. The whole
  drag and its turns are one undo step; a Scale drag cannot take a turn and ignores it. Holding the right mouse button hides the pointer and holds it where it was (it never
  jumps to the middle); the camera turns by the mouse's own movement, so it keeps turning at the edge of the screen, and the
  pointer is shown again, in the same place, when the button is released or the window loses focus. Click selects a part, or the whole group (Model) the part is in; Alt + click selects the part itself inside the group (click empty space to deselect). Tools: Select / Move / Scale / Rotate (1-4), in world or local space (Ctrl + the backtick key); drag a handle (snaps to the increments in Editor Settings, hold Ctrl to move freely). With the Select tool, dragging an object carries it and sets its bounding box down on whatever is under the mouse (the ground, another object, a wall); over empty sky it slides (a plain click still only selects; Shift / Alt clicks select; Ctrl ignores the snapping increment). **Scale**: hold Shift while dragging a handle to scale all three sizes by the same factor, Ctrl to scale about the centre (both sides), Ctrl+Shift for both; with a Model selected, hold Alt over one of its parts to put the handles on that part alone. **Rotate**: rings keep turning past 180 / 360 degrees (the angle turned is filled in, marks every increment, longer at 45 and 90 degrees), and the trackball in the middle turns freely without limit. W A S D E Q flies (Shift is the secondary camera speed; Shift+scroll, "Adjust Secondary Camera Speed", changes it, and Alt+scroll, "Adjust Fly Camera Speed", changes the main speed - the move keys keep working while Alt is held). The **scroll wheel zooms in small, eased steps** that follow the view and never depend on what is selected: **Editor Settings > Camera > Zoom behaviour** chooses **Camera-Based Zoom** (the default: along the direction the camera is looking) or **Hover-Based Zoom** (toward the point under the mouse pointer; with nothing there it goes along the pointer's line of sight, and it never turns into focusing the selection). Zooming in stops half a stud short of a surface. **F is the only thing that moves the camera to the selection**; the wheel then zooms around that focus point until you fly away. **Alt + right mouse drag orbits the point under the mouse** from where the camera and its view are now - with or without a selection. A fly or a look that was started over the viewport is not interrupted when the pointer passes over a menu or a panel; it ends when the keys and the button are let go. Shift+Space inserts an object.
* **Shortcuts are contextual** (they fire in the window the mouse is over) and rebindable: **File > Editor Settings** (Alt+S) > Shortcuts, with search, click-to-rebind, conflict detection and reset. Starters: undo/redo Ctrl+Z / Ctrl+Shift+Z, duplicate Ctrl+D, **repeat duplicate Ctrl+Shift+D**, copy / cut / paste Ctrl+C/X/V, delete Del, save Ctrl+S, play F5; in the Explorer group Ctrl+G, folder Ctrl+Shift+G, ungroup Alt+G, find F, rename Ctrl+R. The Shortcuts tab has an **Order** control: *Common* (the default: the everyday General shortcuts first, related ones side by side, and the settings pages as Input, Viewport, Interface, Debug) or *Alphabetical* (the groups and the entries by the names shown, A to Z; the pages A to Z too). It is remembered, and changes only where things are listed - never a key or a value. A starting key that you already use for something else in your saved settings is never taken from you: the new shortcut starts unbound and the editor says why.
* **Repeat Duplicate** (Ctrl+Shift+D) is not a generic "repeat the last action": it repeats the last *duplication*. Duplicate (Ctrl+D), move and / or turn the copy, then press it again and again: each press copies the newest copies and carries them by the same move and turn that took the first copy from where it was made to where it is, so the copies are evenly spaced (or follow a spiral, if the copy was turned too). Originals and earlier copies stay, the new copies are selected (as after Ctrl+D), several selected objects are repeated each by its own offset, and each press is one undo step. It applies only while what is selected is the copies of the last duplication and they still exist; selecting something else, undo / redo, Play, or opening another scene or project resets it (and the log says so).
* **Play** (`F5`) runs the game inside the viewport on the scene you are editing: the Explorer, Properties and tools keep working live, and Stop puts the scene back. **Play in Window** launches the real player process instead. Either way the log is in the Output tab.
* **Debug menu** (above the viewport, top-left): switches performance and debugging overlays on and off, in groups. Several can be on at once.
  *Performance*: FPS (smoothed, with the instantaneous value beside it), FPS graph, frame time, frame-time graph, frame budget, 1% low,
  frame-time breakdown (scripts / physics / scene sync / editor UI / waiting, plus render-thread and GPU time where the GPU reports them).
  *Rendering*: instance count, draw calls, triangles, vertices, lights, render statistics. *Scripting*: per-script cost, engine timings.
  *Physics*: step time, bodies, touches. *Diagnostics*: memory, network, asset loading, warnings & errors. Each one has a shortcut in
  **Editor Settings > Shortcuts > Debug** (only **Show FPS** is bound at first: `Ctrl+Shift+F5`); they do not fire while you type in a text field
  unless you allow it in **Editor Settings > Debug Overlays**, which also sets the target FPS (30 / 60 / 120 / 144 / custom), the smoothing
  interval, the graph history, the average window, the update interval, spike detection, the window size and opacity, and which corner each
  group's windows start in. **Every overlay is a window of its own**; drag one by its title, and drop it near a corner to dock it (windows in the same
  corner stack). The menu also has *Disable All Debug Overlays*, *Reset Debug Settings* and *Clear Collected Data*.
  Values the engine measures are shown plainly; estimates (draw calls, "waiting") are marked `~`; what a machine cannot report (GPU timings
  without Vulkan / DirectX 12 timestamp queries, GPU memory, networking, which the engine does not have yet) says why instead of showing a number.
  GPU timings cost a little per frame, so the editor only records them if a GPU overlay was enabled when it started (restart after turning one on).
  Only the frame times are recorded all the time; everything else is collected only while an overlay shows it.
* **Wireframe**: the **Wireframe** button in the toolbar (or `Z` over the viewport, rebindable; also in Editor Settings > Rendering) draws the
  triangle edges of every mesh over the scene. The **Solid** button next to it (or `Shift+Z`) hides the shaded parts so only the edges
  remain. In a game: `forge_player --wireframe` or `--wireframe-only`. It needs a graphics API that can draw polygon edges
  (Vulkan, DirectX 12, Metal) and draws every part twice, so a very big scene gets slower while it is on.
* **Run > Build Standalone Game** validates and packages `<slug>[.exe]` + `game.pak`.
* **In a game**: `forge_player --project <folder> --debug` (or `--pak`) shows the same overlays over the game window (`--debug fps,physics,memory`
  or `--debug all` picks them; `Ctrl+Shift+F5` toggles the FPS and a small Debug menu sits in the top-left corner). Without `--debug` the player
  does not load any of it.

## Performance design

* Scripts talk to a plain-Rust data model (no ECS queries from Lua); the renderer only touches dirty instances.
* Native Luau vectors, bytecode compiled at optimisation level 2, hot signals (`Heartbeat` etc.) call handlers
  directly instead of spawning a coroutine each.
* Mesh and material handles are shared so Bevy can batch identical parts.
* Instance events are only produced for instances somebody listens to.
* A script file is compiled once per run; every instance of it (a cloned enemy, a coin) loads the bytecode. Sleeping
  `task.wait` threads sit in a heap, so a frame only looks at the ones that are due. Touch detection only tests the parts
  near each mover.
* Frustum culling is always on (every part carries its bounds). Occlusion culling is available on Vulkan / Metal but **off by
  default**, because it only pays off when the GPU is the bottleneck: `[window] occlusion_culling = true` in `Project.toml`. It
  applies to the player and Play in Window, **not** to the editor's own viewport (Bevy culls visible objects for a camera that
  draws into part of the window). The measurements are in `render/culling.rs`.
* `dev` compiles dependencies at `opt-level = 3`; `release` uses thin LTO; `--profile dist` adds fat LTO,
  `panic = "abort"` and stripping. `--features jit` on `forge_player` enables Luau native codegen.

### Measuring

* `cargo test --release -p forge_engine --test perf -- --ignored --nocapture --test-threads=1` (set `FORGE_BENCH_N` for the
  size) times the scripting host, data model, physics and render mirror without a window.
* `forge_player --project <folder> --bench 300` plays the game for 300 frames after a warm-up (V-Sync off, window focus
  irrelevant) and prints the average, p50, p99 and worst frame time; `--screenshot out.png` saves what the window shows
  (alone or after `--bench`). Compare two settings with the same scene, a few runs each: frame times on a desktop vary by
  several percent from run to run.

## Limitations

* Models: glTF / GLB, OBJ, STL and PLY (no FBX or Collada); collision for a `MeshPart` is its box. No GUI classes, no animation playback, no Roblox sun, moon or stars, effects are not shaded by lights, and collision is not rotation-accurate. See [COMPATIBILITY.md](COMPATIBILITY.md). `AreaLight` is approximated with a soft point light.
* Debug overlays: draw calls and triangle counts are estimated from the visible meshes (shadow passes are not included); GPU timings need Vulkan or DirectX 12; GPU memory and network statistics are not available. In the player only the FPS shortcut exists (no rebinding).
* Instances that are never parented and never destroyed are not garbage collected.
* Cross-compiling: `forge build --player <exe>` bundles whatever player you give it.

## Extending

* New class: add one file anywhere under `src/classes/` (see `ARCHITECTURE.md`); a new light type is one file next to
  `lights/point_light.rs`. A component used by one class goes in that class's file; one shared by several goes in
  `src/components/`.
* `build.rs` discovers these files; nothing else needs editing.
* `cargo test -p forge_engine` also runs registry-level tests (every class resolves, every service exists, ...).

## Upgrading

Pinned: Bevy 0.19, bevy_egui 0.42, mlua 0.10 (Luau), rodio 0.22, gltf 1.4. Version-sensitive spots:
`compat.rs`, the Bevy light structs in `classes/world/lights/`, `EguiPlugin`/`ctx_mut()` in the editor, and in `scripting/` the mlua calls
`set_type_metatable::<Vector>`, `Chunk::set_environment`, `Thread::resume/status`, `set_interrupt`, `sandbox`.

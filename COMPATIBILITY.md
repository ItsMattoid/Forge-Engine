# Roblox API compatibility

Forge's Luau API follows Roblox's Engine API, so that a typical Roblox script runs with little or no change. This page says
what is implemented, what only half is, and what is refused on purpose. It describes what the tests show
(`cargo test -p forge_engine`, in particular `src/scripting/compat_tests/`), not what the Roblox documentation promises: a
statement here is backed by a test, or it is listed under *Not implemented*.

**Forge is not Roblox.** It has no servers, no accounts and no website. "Compatible" here means: the same names, the same
types, the same call and event semantics for the parts of the API that make sense in a local engine. Where a feature cannot
work, the call fails with a message that says so (`Foo:Bar is not supported by Forge`), instead of quietly doing nothing.

## How it is built

* Every class describes itself in one Rust file under `crates/forge_engine/src/classes/` (properties, methods, signals,
  enums, the behaviour that only it needs). Inheritance is declared (`Part -> BasePart -> WorldObject -> PVInstance -> Instance
  -> Object`), and `IsA`, `FindFirstChildWhichIsA`, property lookup and the editor all use that one registry.
* Methods and events that are easier to write in Luau live in a `.luau` file next to the class (`tween_service.luau`,
  `data_store_service.luau` ...). `prelude.luau` owns the generic runtime (signals, `task`, `Enum`, `require`, `typeof`);
  `datatypes.luau` owns the data-only value types.
* A class lists the members it refuses (`unsupported`), so the error is specific, and the members that scripts may assign a
  function to (`OnInvoke` ...).
* Old spellings are kept as aliases: `Velocity`, `RotVelocity`, `Brightness`/`Shadows` of lights, lower-case methods
  (`getChildren`, `isA`, `findFirstChild`), `wait`/`spawn`/`delay`, `Remove`, `BrickColor`.

## Implemented

Verified by tests in `src/scripting/compat_tests/` (and the unit tests beside the code).

**Instances.** `Instance.new(class, parent)`, `ClassName`, `Name`, `Parent` (and the rule that a destroyed instance cannot be
parented again), `Archivable`, `Clone` (deep, references inside the copy are repointed, non-archivable parts are left out),
`Destroy` (also harmless on an instance that is already gone), `ClearAllChildren`, `Remove`, `GetChildren`,
`GetDescendants`, `FindFirstChild[OfClass|WhichIsA]`, `FindFirstAncestor[OfClass|WhichIsA]`, `WaitForChild` (with a timeout),
`IsA`, `IsDescendantOf`, `IsAncestorOf`, `GetFullName`, `GetPropertyChangedSignal`, `Changed` (passes the value for
`ValueBase`), `ChildAdded/Removed`, `DescendantAdded/Removing`, `AncestryChanged`, `Destroying`, attributes
(`SetAttribute`, `GetAttribute`, `GetAttributes`, `GetAttributeChangedSignal`, `AttributeChanged`, name rules and the
allowed value types), tags (`AddTag`, `HasTag`, `GetTags`, `RemoveTag`), `GetPivot`/`PivotTo`.

**Services.** `game:GetService` / `FindService`, `game.Workspace` style access, one object per service: `Workspace`,
`Players`, `Lighting`, `ReplicatedStorage`, `ReplicatedFirst`, `ServerStorage`, `ServerScriptService`, `StarterGui`,
`StarterPlayer`, `SoundService`, `RunService`, `UserInputService`, `ContextActionService`,
`CollectionService`, `Debris`, `TweenService`, `HttpService` (JSON, GUIDs, URL encoding), `DataStoreService` (see below),
`MessagingService` (within the running game), `PhysicsService` (collision groups), `ProximityPromptService`, `TextService`
(sizes), `ScriptContext`, `LogService`, `Stats`, `DataModel` (`BindToClose`, `PlaceId`).

**Signals.** `Connect`, `Once`, `Wait`, `Disconnect`, `Connected`, passing arguments; handlers run in their own thread and
an error in one does not stop the others; connections of a destroyed script are dropped.

**Scripts.** `Script`, `LocalScript` (`LocalPlayer` is set, `RunService:IsClient()` is true), `ModuleScript` with Roblox's
`require` rules: runs once and is cached, exactly one return value, a cycle is an error, a module that failed reports the
failure to later callers, a clone is a different module. `script`, `_G`, `shared`, `task.spawn/defer/delay/wait/cancel`
(a deferred thread runs before the threads that wait), `coroutine.*`, `pcall`/`xpcall`/`error` with tables, `print`/`warn`,
`time()`, `tick()`, `typeof`, `wait`/`spawn`/`delay`. Scripts are sandboxed.

**Data types.** `Vector2`, `Vector3`, `CFrame`, `Color3` (RGB, HSV, hex), `BrickColor` (the palette, `Name`, `Number`,
nearest colour), `UDim`, `UDim2`, `Rect`, `Ray`, `Region3`, `NumberRange`, `NumberSequence`, `ColorSequence`,
`PhysicalProperties`, `Axes`, `Faces`, `Font`, `DateTime`, `TweenInfo`, `RaycastParams`, `OverlapParams`, `RaycastResult`,
`Random`, `Vector2int16`, `Vector3int16`, `Enum` (items, `GetEnumItems`, `FromName`, `FromValue`, Roblox's numbers for the
enums that have them), `typeof` for all of them.

**Parts and models.** `Part` (shapes, wedges, corner wedges), `MeshPart`, `SpawnLocation`; `Size`, `Position`,
`Orientation`/`Rotation` (one thing with `CFrame`), `Color`/`BrickColor`, `Material`, `Transparency`, `Reflectance` (makes the part more metallic and smoother, so it reflects the sky),
`CanCollide`, `CanTouch`, `CanQuery`, `CollisionGroup`, `Anchored`, `Massless` (stored), `CustomPhysicalProperties`,
`Mass`/`GetMass`, `AssemblyLinearVelocity`/`AssemblyAngularVelocity`, `Touched`/`TouchEnded`, `Resize`, `CanCollideWith`,
`GetConnectedParts`, `GetJoints`, `BreakJoints`, `ApplyImpulse` (see `physics_tests`), `Model` (`PrimaryPart`, `GetBoundingBox`,
`GetExtentsSize`, `PivotTo`, `TranslateBy`, `MoveTo`, `ScaleTo`/`GetScale`), `Attachment` (local and world frames),
`Folder`, `Configuration`, `Actor`, `Camera` (`WorldToViewportPoint`, `ViewportPointToRay`, `FieldOfView`, `CameraSubject`).

**Physics.** Gravity, contacts, `Touched`, `Raycast` (`RaycastParams` Include/Exclude, `RaycastResult`) plus the older
`FindPartOnRay*`, region queries (`GetPartBoundsInBox/Radius`, `GetPartsInPart`), `Explosion`, collision groups.
Constraints join parts **by attachments** as in Roblox, or by `Part0`/`Part1`: `WeldConstraint`, `Weld`, `Motor6D`,
`ManualWeld`, `NoCollisionConstraint`, `HingeConstraint`, `BallSocketConstraint`, `PrismaticConstraint`, `RodConstraint`,
`RopeConstraint`, `SpringConstraint` (`CurrentLength`). Movers: `BodyVelocity`, `BodyPosition`, `BodyGyro`, `BodyForce`,
`BodyThrust`, `BodyAngularVelocity`, `LinearVelocity`, `VectorForce`, `Torque`, `AlignPosition`, `AlignOrientation`.

**Players.** `Player` (`Character`, which a script sets, `CharacterAdded/Removing` following it, `leaderstats` pattern, `PlayerGui`,
`PlayerScripts`; account questions are answered as for a new guest), `Players` (`LocalPlayer`, `PlayerAdded`, `GetPlayers`,
`GetPlayerFromCharacter`), `SpawnLocation` (a marker part a script can use). **There is no built-in character**: see *Removed on purpose*.

**Values.** `BoolValue`, `IntValue`, `NumberValue`, `StringValue`, `ObjectValue`, `Color3Value`, `Vector3Value`,
`CFrameValue`, `BrickColorValue` (`ValueBase`), type-checked, `Changed` passes the value.

**Events.** `BindableEvent`, `BindableFunction` (`OnInvoke`), `RemoteEvent`, `RemoteFunction`, `UnreliableRemoteEvent`
(client and server are the same process, so a fire from either side arrives on the other's signals).

**Effects, sound and lighting objects.** `Sound` / `SoundGroup` and the sound effects (`ReverbSoundEffect`, `EchoSoundEffect`,
`DistortionSoundEffect`, `ChorusSoundEffect`, `FlangeSoundEffect`, `TremoloSoundEffect`, `EqualizerSoundEffect`,
`CompressorSoundEffect`, `PitchShiftSoundEffect`, and the filter effects) are heard; `ParticleEmitter` (`Emit`, `Clear`), `Beam`, `Trail` (`Clear`), `Fire`,
`Smoke`, `Sparkles`, `Highlight`, `Decal`, `Texture`, `Clouds`, `PointLight`, `SpotLight`, `SurfaceLight`, `Sky`, `Atmosphere`, `BloomEffect`,
`BlurEffect`, `ColorCorrectionEffect`, `DepthOfFieldEffect` and `SunRaysEffect` are drawn; `Lighting` (`ClockTime`, `TimeOfDay`,
sun and moon direction, fog, exposure, ambient). Typed properties (`NumberSequence`, `ColorSequence`, `NumberRange`) are checked. What each
does and does not do is in *Partly implemented*.

**Input.** `UserInputService` (`InputBegan/Ended/Changed`, `IsKeyDown`, `GetKeysPressed`, `GetMouseLocation`, `JumpRequest`),
`ContextActionService` (`BindAction`, priorities, `UnbindAction`), `ClickDetector`, `ProximityPrompt` (`Triggered`,
`InputHoldBegin/End`, `HoldDuration`) with `ProximityPromptService`.

**Animation / tweens.** `TweenService:Create` for any property whose value can be interpolated (numbers, Vector2/3, CFrame,
Color3, UDim2 ...), `TweenInfo` with `EasingStyle`, `EasingDirection`, `RepeatCount`, `Reverses`, `DelayTime`; `Tween`
`Play/Pause/Cancel`, `Completed`, `PlaybackState`; `TweenService:GetValue`.

## Partly implemented

These exist and keep their state, but the engine does not act on it (yet).

| Area | What is missing |
|---|---|
| Audio | Heard (project files: wav, mp3, ogg, flac, m4a). `Sound` plays, pauses, loops, seeks, changes speed and volume, reports `TimeLength` and fires `Played/Paused/Resumed/Stopped/Ended/DidLoop`; a sound in a part or attachment is positional (roll-off, pan, Doppler); `SoundGroup`s and sound effects apply. Not done: `Loaded` never fires (`IsLoaded` says whether the file can be played), no emitter size or directional sounds, a sound outside `Workspace` / `SoundService` is silent (as in Roblox). Sounds do not play in the editor's edit mode. |
| Particles, beams, trails, fire, smoke, sparkles | Drawn as quads that face the camera, or ribbons, unlit, with `Emit` / `Clear`. Not done: shading by the world's lights (`LightInfluence` is hidden), `ParticleEmitter.Shape*` (particles always come from the face), `Beam` / `Trail` do not cast light, a `LightEmission` between 0 and 0.5 blends and from 0.5 adds light (no values between). |
| `Explosion` | Pushes parts, breaks joints, fires `Hit`, and shows as a fireball (`Visible`). No craters. |
| `Highlight` | A tint and an outline for a part or a model. `DepthMode = AlwaysOnTop` shows the tint through walls but not the outline. |
| `Decal`, `Texture` | Drawn on one face of the part with Bevy's forward decals (a decal on a ball paints what is under the face's square); `Texture` tiles every `StudsPerTile`. `ZIndex` is kept for scripts: decals are drawn in the order they were made. |
| `Sky`, `Atmosphere`, `Clouds`, post-processing effects | `Sky` is a cube map that also lights the scene; `Atmosphere` and `Lighting.Fog*` are distance fog; `BloomEffect`, `ColorCorrectionEffect`, `DepthOfFieldEffect`, `BlurEffect`, `SunRaysEffect` map to Bevy's own effects; `Clouds` is one sheet at a fixed height. Not drawn: the sun, moon and stars, `Lighting.ColorShift_*`, lit or shadow-casting clouds. |
| `Animation` / `Animator` / `AnimationTrack` | `LoadAnimation` returns a track that keeps `IsPlaying`, `Speed`, `Looped`, `Stopped` etc. No keyframes are played (no rig animation). |
| Hidden stored properties | Properties that no part of Forge acts on are hidden in the editor, not offered as if they did something: `Massless`, `RootPriority`, `EnableFluidForces`, the part surfaces, `Lighting.OutdoorAmbient` / `GeographicLatitude` / `Technology` / `ColorShift_*`, `ClickDetector.CursorIcon`, `Explosion.ExplosionType`. Scripts read and write them. No effect (welded groups are not tracked as assemblies: a part is its own assembly). |
| `DataStoreService` | **In memory for the length of the run.** `GetAsync`, `SetAsync`, `UpdateAsync`, `IncrementAsync`, `RemoveAsync`, `GetOrderedDataStore` / `GetSortedAsync` work; nothing is saved to disk or shared. `GetVersionAsync` and `ListKeysAsync` are refused. |
| `MessagingService`, `MemoryStoreService` | Work inside this process only. |
| `PathfindingService` | The service exists; `CreatePath` / `FindPathAsync` are refused (no navigation mesh). |
| `Terrain` | The object exists; voxel reads and writes are refused and there is no water (its water properties are kept for scripts and hidden in the editor). |
| `Camera` | Follows its `CFrame`; `CameraType` is informational. |
| `Player` | One local player (`Player1`); other players cannot join. |
| `Workspace` as a `Model` | `Workspace` is not a `Model`: no `Workspace:PivotTo`. |
| `Model.WorldPivot` | Not a property (use `GetPivot` / `PivotTo`). |

## Not implemented (refused or absent)

* **Online services**: `HttpService:GetAsync/PostAsync/RequestAsync`, `TeleportService`, `MarketplaceService` purchases and
  product info, `BadgeService` awards, `SocialService`, `GroupService`, `Players:GetUserIdFromNameAsync` for anyone but the local player,
  `VoiceChatService`, `PolicyService`, `AvatarEditorService`, `InsertService:LoadAsset`, `ContentProvider` downloads. The
  services exist and answer with defaults, or refuse with `not supported by Forge`. A script that checks for them still runs.
* **Interface classes**: `ScreenGui`, `SurfaceGui`, `BillboardGui`, `Frame`, `TextLabel`, `TextButton`, `ImageLabel`,
  `UIListLayout` and the other UI objects cannot be created (`Instance.new` says so). `StarterGui` and `PlayerGui` exist as
  containers; `StarterGui:SetCore` is accepted and ignored.
* **Parts**: `Seat`, `VehicleSeat`, `TrussPart`, `UnionOperation` / `NegateOperation` (CSG: `SubtractAsync` etc. are refused),
  `SpecialMesh`, `BlockMesh`, `SurfaceAppearance`, `Bone`, `SelectionBox` and other selection adornments.
* **Services**: `TextChatService`, `KeyframeSequenceProvider`, `NetworkClient`, `Selection`, `TestService`, `TimerService`,
  `StarterPlayerScripts` / `StarterCharacterScripts` as services (the containers are children of `StarterPlayer`).
* `Script.Source`: a script's code is its `SourceFile`, and cannot be read or written from Luau.
* Studio-only APIs (`ChangeHistoryService`, plugins, `Selection`), `Instance.Sandboxed` / capabilities.
* Real client/server replication, `FilteringEnabled`, remote players, network ownership
  (`SetNetworkOwner` is accepted and ignored).
* Parallel Luau (`Actor` exists; `ConnectParallel` runs serially, `task.synchronize` / `desynchronize` do nothing).

## Removed on purpose

`Humanoid`, `Tool`, `Accessory`, `Backpack`, `Team` / `Teams`, `StarterGear`, `StarterPack` and `ForceField` are not part of Forge: a game's player controller,
inventory, teams and health are scripts, with no built-in player architecture to fight. `Instance.new` refuses them; an older scene that contains them still
opens (they are left out, silently, with the properties that went with them). `Rigidbody.FixedRotation` keeps a script-driven body upright, which is what a
controller needs from the physics.

## Deliberate differences

* One process: `RunService:IsClient()` is true in a `LocalScript` and `IsServer()` in a `Script`, but both run in the same
  place and see the same data.
* The engine makes no character. `Players.CharacterAutoLoads`, `LoadCharacter` and `RespawnTime` do not exist; a game's own scripts make the
  player's model, move it and set `Player.Character` (the starter project's `FirstPersonController` is one).
* The one player joins on the first frame, so `PlayerAdded` handlers connected in the first run of a script do fire.
* A destroyed instance answers `Parent` (nil), and `Name` / `ClassName` while a script is listening, until the end of the frame; other
  members raise `this instance has been destroyed`. (Roblox keeps every property readable.)
* Signal handlers run after the change that caused them (deferred), as Roblox's default `SignalBehavior` does: a handler for
  two quick changes reads the latest value both times.
* `tostring(Enum.X.Y)` is `Enum.X.Y`; `Color3` and `Vector3` store single-precision floats, so `Vector3.new(0.1, 0, 0).X == 0.1`
  is false (compare with a tolerance, as in Roblox).

## Checking a claim

```
cargo test -p forge_engine --lib compat_tests   # the Roblox-style tests
cargo test -p forge_engine                      # everything
```

`compat_tests/` is grouped like this page: `datatypes`, `instances`, `signals`, `modules`, `classes`, `services`, `physics`,
`scripts` (whole Roblox-style scripts: leaderstats, coin pickup, kill brick, round manager with a module, tween door with a
prompt, tag-driven behaviour, an inventory class, coroutines and `task`).

use bevy::{input::keyboard::KeyCode, input::mouse::MouseButton, math::Vec2};

use crate::scripting::testkit::{checks, checks_for, Game};

#[test]
fn parts_have_the_properties_and_methods_roblox_documents() {
    checks(
        r#"
        local part = Instance.new("Part")
        part.Parent = workspace
        part.Anchored = true
        equal("Size default", part.Size, Vector3.new(4, 1, 2))
        equal("Shape default", part.Shape, Enum.PartType.Block)
        equal("Material default", part.Material, Enum.Material.Plastic)
        equal("CanCollide default", part.CanCollide, true)
        equal("CanTouch default", part.CanTouch, true)
        equal("CanQuery default", part.CanQuery, true)
        equal("Massless", part.Massless, false)
        equal("Reflectance", part.Reflectance, 0)
        equal("Transparency", part.Transparency, 0)
        equal("CastShadow", part.CastShadow, true)
        equal("CollisionGroup", part.CollisionGroup, "Default")
        part.Reflectance = 0.5
        equal("Reflectance set", part.Reflectance, 0.5)

        -- Position, Orientation and CFrame are one thing.
        part.Position = Vector3.new(10, 5, 0)
        equal("Position -> CFrame", part.CFrame.Position, Vector3.new(10, 5, 0))
        part.Orientation = Vector3.new(0, 90, 0)
        check("Orientation", near(part.Orientation.Y, 90, 1e-2))
        check("Rotation is the same", near(part.Rotation.Y, 90, 1e-2))
        part.CFrame = CFrame.new(1, 2, 3) * CFrame.Angles(0, math.pi / 2, 0)
        equal("CFrame -> Position", part.Position, Vector3.new(1, 2, 3))
        check("CFrame -> Orientation", near(part.Orientation.Y, 90, 1e-2))
        part:PivotTo(CFrame.new(0, 0, 0))
        equal("PivotTo", part.Position, Vector3.zero)
        equal("GetPivot", part:GetPivot().Position, Vector3.zero)

        -- Resize keeps the opposite face where it is.
        part.CFrame = CFrame.new()
        part.Size = Vector3.new(4, 1, 2)
        equal("Resize returns true", part:Resize(Enum.NormalId.Right, 2), true)
        equal("Resize size", part.Size, Vector3.new(6, 1, 2))
        equal("Resize moves the centre", part.Position, Vector3.new(1, 0, 0))
        part:Resize(Enum.NormalId.Left, 2)
        equal("Resize left", part.Position, Vector3.zero)
        equal("Resize too small is refused", part:Resize(Enum.NormalId.Top, -100), false)
        equal("and changes nothing", part.Size, Vector3.new(8, 1, 2))

        -- Physical properties
        part.Size = Vector3.new(4, 1, 2)
        equal("no custom properties", part.CustomPhysicalProperties, nil)
        part.CustomPhysicalProperties = PhysicalProperties.new(2, 0.5, 0.25)
        local pp = part.CustomPhysicalProperties
        check("custom properties read back", pp ~= nil and pp.Density == 2 and pp.Friction == 0.5 and pp.Elasticity == 0.25)
        part.CustomPhysicalProperties = nil
        equal("nil restores the defaults", part.CustomPhysicalProperties, nil)
        check("GetMass", part:GetMass() > 0)
        equal("Mass property", part.Mass, part:GetMass())
        equal("AssemblyMass", part.AssemblyMass, part.Mass)
        equal("AssemblyCenterOfMass", part.AssemblyCenterOfMass, part.Position)
        equal("AssemblyRootPart", part.AssemblyRootPart, part)
        equal("GetRootPart", part:GetRootPart(), part)

        -- Assemblies and ownership
        check("no joints", #part:GetJoints() == 0 and #part:GetConnectedParts() == 0)
        equal("network owner", part:GetNetworkOwner(), nil)
        part:BreakJoints()

        -- Colliding
        local other = Instance.new("Part", workspace)
        equal("two solid parts collide", part:CanCollideWith(other), true)
        other.CanCollide = false
        equal("a part that does not collide", part:CanCollideWith(other), false)

        -- Wedges are parts with a shape.
        equal("WedgePart shape", Instance.new("WedgePart").Shape, Enum.PartType.Wedge)
        equal("CornerWedgePart shape", Instance.new("CornerWedgePart").Shape, Enum.PartType.CornerWedge)
        local spawn = Instance.new("SpawnLocation")
        check("SpawnLocation", spawn.Enabled and spawn.Anchored)
    "#,
    );
}

#[test]
fn models_measure_move_and_scale_their_parts() {
    checks(
        r#"
        local model = Instance.new("Model")
        model.Parent = workspace
        local function block(name, x)
            local p = Instance.new("Part", model)
            p.Name = name
            p.Anchored = true
            p.Size = Vector3.new(2, 2, 2)
            p.Position = Vector3.new(x, 0, 0)
            return p
        end
        local a, b = block("A", -5), block("B", 5)
        local cf, size = model:GetBoundingBox()
        equal("bounding box size", size, Vector3.new(12, 2, 2))
        equal("bounding box centre", cf.Position, Vector3.zero)
        equal("GetExtentsSize", model:GetExtentsSize(), Vector3.new(12, 2, 2))

        model.PrimaryPart = a
        equal("PrimaryPart", model.PrimaryPart, a)
        equal("the pivot is the primary part's", model:GetPivot().Position, a.Position)
        model:TranslateBy(Vector3.new(0, 10, 0))
        equal("TranslateBy moves A", a.Position, Vector3.new(-5, 10, 0))
        equal("TranslateBy moves B", b.Position, Vector3.new(5, 10, 0))
        model:PivotTo(CFrame.new(0, 0, 0))
        equal("PivotTo puts the pivot there", a.Position, Vector3.zero)
        equal("and keeps the others around it", b.Position, Vector3.new(10, 0, 0))
        model:MoveTo(Vector3.new(0, 20, 0))
        check("MoveTo moves the model", near(model:GetBoundingBox().Position.Y, 20, 0.5))

        -- Scale
        model:PivotTo(CFrame.new())
        equal("scale starts at 1", model:GetScale(), 1)
        model:ScaleTo(2)
        equal("GetScale", model:GetScale(), 2)
        equal("parts are scaled", a.Size, Vector3.new(4, 4, 4))
        check("and moved about the pivot", near(b.Position.X, 20, 1e-3), tostring(b.Position))
        model:ScaleTo(1)
        equal("back again", a.Size, Vector3.new(2, 2, 2))

        -- A model without a primary part still has a pivot.
        local loose = Instance.new("Model", workspace)
        local only = Instance.new("Part", loose)
        only.Anchored = true
        only.Position = Vector3.new(3, 3, 3)
        equal("pivot of a model with no primary part", loose:GetPivot().Position, Vector3.new(3, 3, 3))
    "#,
    );
}

#[test]
fn attachments_follow_their_part_and_convert_between_local_and_world() {
    checks(
        r#"
        local part = Instance.new("Part", workspace)
        part.Anchored = true
        part.Position = Vector3.new(10, 0, 0)
        part.Orientation = Vector3.new(0, 90, 0)
        local attachment = Instance.new("Attachment", part)
        attachment.Position = Vector3.new(1, 0, 0)
        equal("class", attachment.ClassName, "Attachment")
        check("WorldPosition follows the turned part", (attachment.WorldPosition - Vector3.new(10, 0, -1)).Magnitude < 1e-3, tostring(attachment.WorldPosition))
        equal("Position is local", attachment.Position, Vector3.new(1, 0, 0))
        equal("CFrame is local", attachment.CFrame.Position, Vector3.new(1, 0, 0))
        check("WorldCFrame", (attachment.WorldCFrame.Position - attachment.WorldPosition).Magnitude < 1e-4)

        attachment.WorldPosition = Vector3.new(10, 3, 0)
        check("setting WorldPosition sets the local position", (attachment.Position - Vector3.new(0, 3, 0)).Magnitude < 1e-3, tostring(attachment.Position))
        attachment.WorldCFrame = CFrame.new(10, 0, 0) * CFrame.Angles(0, math.pi / 2, 0)
        check("setting WorldCFrame", attachment.Position.Magnitude < 1e-3, tostring(attachment.Position))
        part.Position = Vector3.zero
        check("moving the part moves the attachment", attachment.WorldPosition.Magnitude < 1e-3)

        attachment.Rotation = Vector3.new(0, 45, 0)
        check("Orientation and Rotation are one", near(attachment.Orientation.Y, 45, 1e-2))
        equal("Visible", attachment.Visible, false)
    "#,
    );
}

#[test]
fn the_classes_that_made_up_the_built_in_player_do_not_exist() {
    checks(
        r#"
        for _, name in { "Humanoid", "Accessory", "Backpack", "Team", "Tool", "StarterGear", "ForceField", "StarterPack", "Teams" } do
            throws(name .. " is not a class", function() Instance.new(name) end)
        end
        throws("there is no Teams service", function() game:GetService("Teams") end)
        throws("there is no StarterPack service", function() game:GetService("StarterPack") end)
        local player = game:GetService("Players").LocalPlayer
        check("a player only has the containers every game needs", player:FindFirstChild("PlayerGui") ~= nil and player:FindFirstChild("PlayerScripts") ~= nil and player:FindFirstChild("Backpack") == nil and player:FindFirstChild("StarterGear") == nil)
        throws("a player has no team", function() return player.Team end)
    "#,
    );
}

#[test]
fn players_join_and_the_character_is_whatever_a_script_assigns() {
    let mut g = Game::new(
        r#"
        local Players = game:GetService("Players")
        local log = {}
        local function note(x) table.insert(log, x) end
        local player = Players.LocalPlayer
        player.CharacterAdded:Connect(function(character) note("added:" .. character.Name) end)
        player.CharacterRemoving:Connect(function(character) note("removing:" .. character.Name) end)
        Players.PlayerAdded:Connect(function(p) note("joined:" .. p.Name) end)

        task.wait(0.2)
        note("count:" .. #Players:GetPlayers())
        note("no character of its own:" .. tostring(player.Character == nil))
        note("nothing was made:" .. tostring(workspace:FindFirstChildOfClass("Model") == nil))

        local hero = Instance.new("Model", workspace)
        hero.Name = "Hero"
        local body = Instance.new("Part", hero)
        body.Position = Vector3.new(3, 4, 5)
        hero.PrimaryPart = body
        player.Character = hero
        task.wait(0.1)
        note("owner:" .. tostring(Players:GetPlayerFromCharacter(hero) == player))
        note("distance:" .. tostring(math.round(player:DistanceFromCharacter(Vector3.new(3, 4, 8)))))
        player.Character = nil
        task.wait(0.1)
        game:GetService("ReplicatedStorage").Out.Value = table.concat(log, "|")
    "#,
    );
    g.frames(30);
    assert_eq!(g.out(), "joined:Player1|count:1|no character of its own:true|nothing was made:true|added:Hero|owner:true|distance:3|removing:Hero", "{}", g.logs());
}

#[test]
fn value_objects_hold_each_kind_of_value() {
    checks(
        r#"
        local function roundtrip(class, value)
            local v = Instance.new(class)
            v.Value = value
            equal(class, v.Value, value)
        end
        roundtrip("BoolValue", true)
        roundtrip("IntValue", 7)
        roundtrip("NumberValue", 1.5)
        roundtrip("StringValue", "hi")
        roundtrip("Color3Value", Color3.new(1, 0, 0))
        roundtrip("Vector3Value", Vector3.new(1, 2, 3))
        roundtrip("CFrameValue", CFrame.new(1, 2, 3))
        roundtrip("BrickColorValue", BrickColor.new("Bright red"))
        roundtrip("ObjectValue", workspace)
        equal("defaults", Instance.new("StringValue").Value, "")
        equal("ObjectValue default", Instance.new("ObjectValue").Value, nil)
        throws("a string is not a number", function() Instance.new("NumberValue").Value = "x" end)
        throws("a part is not a colour", function() Instance.new("Color3Value").Value = workspace end)
        local target = Instance.new("Part", workspace)
        local ref = Instance.new("ObjectValue")
        ref.Value = target
        target:Destroy()
        equal("a reference to a destroyed object is nil", ref.Value, nil)
        check("ValueBase", Instance.new("IntValue"):IsA("ValueBase"))
    "#,
    );
}

#[test]
fn lighting_exposes_roblox_s_time_and_settings() {
    checks(
        r#"
        local lighting = game:GetService("Lighting")
        lighting.ClockTime = 14.5
        equal("TimeOfDay", lighting.TimeOfDay, "14:30:00")
        lighting.TimeOfDay = "06:15:30"
        check("TimeOfDay sets ClockTime", near(lighting.ClockTime, 6.2583, 1e-3), lighting.ClockTime)
        lighting:SetMinutesAfterMidnight(720)
        equal("SetMinutesAfterMidnight", lighting.ClockTime, 12)
        equal("GetMinutesAfterMidnight", lighting:GetMinutesAfterMidnight(), 720)
        check("noon sun is high", lighting:GetSunDirection().Y > 0.9)
        lighting.ClockTime = 0
        check("midnight sun is below", lighting:GetSunDirection().Y < -0.9)
        for name, kind in { Brightness = "number", ClockTime = "number", GlobalShadows = "boolean", Ambient = "Color3", OutdoorAmbient = "Color3", FogColor = "Color3", FogEnd = "number", FogStart = "number", TimeOfDay = "string" } do
            equal("Lighting." .. name, typeof(lighting[name]), kind)
        end
        lighting.FogEnd = 500
        equal("settings are kept", lighting.FogEnd, 500)

        -- The effects and sky objects.
        local sky = Instance.new("Sky", lighting)
        sky.SkyboxBk = "rbxassetid://123"
        equal("Sky texture", sky.SkyboxBk, "rbxassetid://123")
        local atmosphere = Instance.new("Atmosphere", lighting)
        check("Atmosphere", typeof(atmosphere.Color) == "Color3" and atmosphere.Offset == 0)
        local bloom = Instance.new("BloomEffect", lighting)
        check("Bloom", bloom.Enabled)
        bloom.Enabled = false
        equal("PostEffect.Enabled", bloom.Enabled, false)
        check("PostEffect", bloom:IsA("PostEffect"))
        local cc = Instance.new("ColorCorrectionEffect")
        check("ColorCorrection", cc.Brightness == 0 and cc.Contrast == 0 and cc.Saturation == 0)
    "#,
    );
}

#[test]
fn lights_have_roblox_s_names_and_faces() {
    checks(
        r#"
        local point = Instance.new("PointLight")
        check("PointLight", point.Brightness == 1 and point.Range == 16 and point.Enabled and point.Shadows == false, point.Range)
        point.Brightness = 3
        equal("Brightness", point.Brightness, 3)
        local spot = Instance.new("SpotLight")
        spot.Angle = 40
        equal("Angle", spot.Angle, 40)
        equal("Face", spot.Face, Enum.NormalId.Front)
        spot.Face = Enum.NormalId.Top
        equal("Face set", spot.Face, Enum.NormalId.Top)
        check("SurfaceLight", Instance.new("SurfaceLight"):IsA("Light"))
        local part = Instance.new("Part", workspace)
        point.Parent = part
        equal("a light inside a part", point.Parent, part)
    "#,
    );
}

#[test]
fn sounds_keep_playback_state_and_announce_it() {
    checks_for(
        r#"
        local sound = Instance.new("Sound", workspace)
        check("defaults", sound.Volume == 0.5 and sound.PlaybackSpeed == 1 and not sound.Looped and not sound.Playing and sound.TimePosition == 0)
        local log = {}
        for _, name in { "Played", "Paused", "Resumed", "Stopped" } do
            sound[name]:Connect(function() table.insert(log, name) end)
        end
        sound.SoundId = "rbxassetid://1234"
        sound:Play()
        equal("Play", sound.Playing, true)
        sound:Pause()
        equal("Pause", sound.Playing, false)
        sound:Resume()
        equal("Resume", sound.Playing, true)
        sound:Stop()
        equal("Stop", sound.Playing, false)
        task.wait()
        equal("signals", table.concat(log, ","), "Played,Paused,Resumed,Stopped")
        sound.Volume = 2
        equal("Volume", sound.Volume, 2)
    "#,
        6,
    );
}

#[test]
fn effect_objects_keep_their_settings_with_the_right_types() {
    checks(
        r#"
        local emitter = Instance.new("ParticleEmitter")
        check("defaults", emitter.Rate == 20 and emitter.Enabled)
        equal("Lifetime is a NumberRange", typeof(emitter.Lifetime), "NumberRange")
        equal("Size is a NumberSequence", typeof(emitter.Size), "NumberSequence")
        equal("Color is a ColorSequence", typeof(emitter.Color), "ColorSequence")
        emitter.Size = NumberSequence.new(0, 5)
        emitter.Color = ColorSequence.new(Color3.new(1, 0, 0), Color3.new(0, 0, 1))
        emitter.Lifetime = NumberRange.new(1, 2)
        check("NumberSequence kept", emitter.Size.Keypoints[2].Value == 5)
        check("ColorSequence kept", emitter.Color.Keypoints[1].Value == Color3.new(1, 0, 0) and emitter.Color.Keypoints[2].Value == Color3.new(0, 0, 1))
        equal("NumberRange kept", emitter.Lifetime.Max, 2)
        throws("a number is not a range", function() emitter.Lifetime = 5 end)
        throws("a range is not a sequence", function() emitter.Size = NumberRange.new(1, 2) end)
        emitter:Emit(10)
        emitter:Clear()

        local part = Instance.new("Part", workspace)
        local a0, a1 = Instance.new("Attachment", part), Instance.new("Attachment", part)
        local beam = Instance.new("Beam")
        beam.Attachment0 = a0
        beam.Attachment1 = a1
        beam.Parent = part
        check("Beam", beam.Attachment0 == a0 and beam.Attachment1 == a1 and beam.Enabled)
        check("Trail", Instance.new("Trail").Enabled)
        check("Fire", Instance.new("Fire").Enabled)
        check("Smoke", Instance.new("Smoke").Enabled)
        check("Sparkles", Instance.new("Sparkles").Enabled)
        local highlight = Instance.new("Highlight")
        highlight.Adornee = part
        equal("Adornee", highlight.Adornee, part)

        local decal = Instance.new("Decal", part)
        check("Decal", decal.Face == Enum.NormalId.Front and decal.Transparency == 0 and decal.Texture == "")
        decal.Texture = "rbxassetid://99"
        decal.Face = Enum.NormalId.Top
        check("Decal set", decal.Texture == "rbxassetid://99" and decal.Face == Enum.NormalId.Top)
        local texture = Instance.new("Texture", part)
        check("Texture is a Decal", texture:IsA("Decal"))
    "#,
    );
}

#[test]
fn camera_maps_between_the_world_and_the_screen() {
    checks(
        r#"
        local camera = workspace.CurrentCamera
        equal("camera class", camera.ClassName, "Camera")
        equal("ViewportSize is a Vector2", typeof(camera.ViewportSize), "Vector2")
        camera.CameraType = Enum.CameraType.Scriptable
        camera.CFrame = CFrame.new(0, 0, 0)
        camera.FieldOfView = 90
        local size = camera.ViewportSize

        local ahead, on = camera:WorldToViewportPoint(Vector3.new(0, 0, -10))
        check("a point straight ahead is in the middle", on and near(ahead.X, size.X / 2, 0.01) and near(ahead.Y, size.Y / 2, 0.01) and near(ahead.Z, 10, 1e-3), tostring(ahead))
        local _, behind = camera:WorldToViewportPoint(Vector3.new(0, 0, 10))
        equal("a point behind is not on screen", behind, false)
        local right = camera:WorldToScreenPoint(Vector3.new(5, 0, -10))
        check("to the right is larger X", right.X > size.X / 2)
        local above = camera:WorldToViewportPoint(Vector3.new(0, 5, -10))
        check("up is smaller Y (screen Y points down)", above.Y < size.Y / 2)
        local edge = camera:WorldToViewportPoint(Vector3.new(0, 10, -10))
        check("the edge of a 90 degree view", near(edge.Y, 0, 0.01), edge.Y)

        local ray = camera:ViewportPointToRay(size.X / 2, size.Y / 2)
        check("the middle looks ahead", (ray.Direction - Vector3.new(0, 0, -1)).Magnitude < 1e-4 and ray.Origin.Magnitude < 1e-6, tostring(ray.Direction))
        local corner = camera:ScreenPointToRay(0, 0)
        check("the top-left looks up and left", corner.Direction.X < 0 and corner.Direction.Y > 0 and corner.Direction.Z < 0)
        check("rays are unit length", near(corner.Direction.Magnitude, 1, 1e-4))
        -- Round trip: the ray through the projection of a point passes through the point.
        local point = Vector3.new(3, 2, -12)
        local screen = camera:WorldToViewportPoint(point)
        local through = camera:ViewportPointToRay(screen.X, screen.Y)
        check("round trip", near((point - through.Origin).Unit:Dot(through.Direction), 1, 1e-4))
        local part = Instance.new("Part", workspace)
        camera.CameraSubject = part
        equal("CameraSubject set", camera.CameraSubject, part)
    "#,
    );
}

#[test]
fn terrain_exists_but_cannot_be_drawn_on() {
    checks(
        r#"
        local terrain = workspace.Terrain
        equal("class", terrain.ClassName, "Terrain")
        throws("FillBlock", function() terrain:FillBlock(CFrame.new(), Vector3.one, Enum.Material.Grass) end, "not supported")
        throws("Clear", function() terrain:Clear() end, "not supported")
        equal("it is a child of the workspace", terrain.Parent, workspace)
    "#,
    );
}

#[test]
fn animations_load_into_tracks_that_keep_state() {
    checks_for(
        r#"
        local model = Instance.new("Model", workspace)
        local animator = Instance.new("Animator", model)
        local anim = Instance.new("Animation")
        anim.AnimationId = "rbxassetid://1"
        local track = animator:LoadAnimation(anim)
        equal("track type", typeof(track), "AnimationTrack")
        equal("not playing", track.IsPlaying, false)
        track.Looped = true
        track:Play()
        equal("a looped track keeps playing", track.IsPlaying, true)
        track:AdjustSpeed(2)
        equal("speed", track.Speed, 2)
        track:Stop()
        equal("Stop", track.IsPlaying, false)
    "#,
        4,
    );
}

#[test]
fn player_input_reaches_scripts_as_roblox_documents() {
    let mut g = Game::new(
        r#"
        local uis = game:GetService("UserInputService")
        local log = {}
        uis.InputBegan:Connect(function(input, processed)
            table.insert(log, "begin:" .. input.UserInputType.Name .. ":" .. input.KeyCode.Name .. ":" .. typeof(input))
        end)
        uis.InputEnded:Connect(function(input) table.insert(log, "end:" .. input.KeyCode.Name) end)
        local held = false
        game:GetService("RunService").Heartbeat:Connect(function()
            if uis:IsKeyDown(Enum.KeyCode.W) then held = true end
        end)
        task.wait(0.2)
        table.insert(log, "held:" .. tostring(held))
        table.insert(log, "mouse:" .. typeof(uis:GetMouseLocation()))
        game:GetService("ReplicatedStorage").Out.Value = table.concat(log, "|")
    "#,
    );
    g.press_key(KeyCode::KeyW);
    g.frames(2);
    g.release_key(KeyCode::KeyW);
    g.frames(2);
    g.press_mouse(MouseButton::Left);
    g.move_cursor(Vec2::new(10.0, 10.0));
    g.frames(2);
    g.release_mouse(MouseButton::Left);
    g.frames(8);
    let out = g.out();
    assert!(out.contains("begin:Keyboard:W:InputObject"), "{out}\n{}", g.logs());
    assert!(out.contains("end:W"), "{out}");
    assert!(out.contains("begin:MouseButton1"), "{out}");
    assert!(out.contains("held:true"), "{out}");
    assert!(out.contains("mouse:Vector2"), "{out}");
}

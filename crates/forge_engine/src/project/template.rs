//! Starter content for new projects, including a first-person controller written as a script.

use anyhow::Result;
use bevy::math::Vec3;

use super::Project;
use crate::{
    assets::material::{MaterialDef, MATERIALS_DIR},
    audio::clip::wav_bytes,
    dm::{file::PlaceFile, DataModel, Id, Value},
};

/// The starter text for a new script of this class (declared by the class itself).
pub fn script_stub(class: &str) -> String {
    crate::core::class(class).and_then(|c| c.desc.script_stub).unwrap_or("-- Luau script\n").to_owned()
}

pub const SPIN_FILE: &str = "Server/Systems/Spin.luau";
pub const COIN_FILE: &str = "Server/Systems/Coin.luau";
pub const GAME_MANAGER_FILE: &str = "Server/Services/GameManager.luau";
pub const CONTROLLER_FILE: &str = "Client/Controllers/FirstPersonController.luau";
/// The sound a coin makes when it is collected (made up here, so a new project has one to hear).
pub const COIN_SOUND: &str = "Assets/Audio/Coin.wav";

/// A short rising "ding": two sine tones a fifth apart that fade out.
fn coin_wav() -> Vec<u8> {
    let rate = 44_100u32;
    let samples: Vec<i16> = (0..(rate as f32 * 0.4) as usize)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let envelope = (-t * 9.0).exp() * (1.0 - (-t * 400.0).exp());
            let tone = (std::f32::consts::TAU * 988.0 * t).sin() * 0.6 + (std::f32::consts::TAU * 1480.0 * t).sin() * 0.4;
            (tone * envelope * 0.7 * 32767.0) as i16
        })
        .collect();
    wav_bytes(rate, 1, &samples)
}

/// The README a new project starts with: what the folders are for, and that they are only a suggestion.
fn readme(name: &str) -> String {
    format!(
        "# {name}\n\nA Forge project. Open this folder in the Forge editor.\n\n## Folders\n\nThe layout below is a starting point; create, rename and move folders and files however you like.\nEvery folder is optional.\n\n- `Assets/` models, textures, materials (`.mat`), audio, animations, fonts\n- `Scenes/` scenes (`.scene`) and `Prefabs/` (`.prefab`)\n- `Client/`, `Server/`, `Shared/` Luau scripts\n- `Packages/` reusable libraries\n- `Plugins/` editor plugins for this project\n- `Project.toml` the project name, start scene and window settings\n\n## Scripts\n\nLuau is the only scripting language. Where a script file is stored does not decide where it runs: a\n`Script` instance runs on the server, a `LocalScript` on the client, and a `ModuleScript` runs when it is\nrequired, wherever the file is. A script in the scene only stores the path of its file.\n"
    )
}

const SPIN: &str = r#"-- Spins the part this script is attached to.
local RunService = game:GetService("RunService")

local part = script.Parent

RunService.Heartbeat:Connect(function(dt)
	part.CFrame = part.CFrame * CFrame.Angles(0, math.rad(60) * dt, 0)
end)
"#;

const COIN: &str = r#"-- A collectable coin: bobs and spins, and is collected when the player touches it.
local RunService = game:GetService("RunService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local coin = script.Parent
local baseY = coin.Position.Y
local t = math.random() * math.pi * 2

local heartbeat
heartbeat = RunService.Heartbeat:Connect(function(dt)
	if not coin.Parent then
		heartbeat:Disconnect()
		return
	end
	t += dt * 3
	local p = coin.Position
	coin.CFrame = CFrame.new(p.X, baseY + math.sin(t) * 0.5, p.Z) * CFrame.Angles(0, t, 0)
end)

coin.Touched:Connect(function(other)
	local character = other:FindFirstAncestorOfClass("Model")
	if character and character:GetAttribute("IsPlayer") then
		ReplicatedStorage.Coins.Value += 1
		coin:Destroy()
	end
end)
"#;

const GAME_MANAGER: &str = r#"-- Runs once when the game starts (this Script lives in ServerScriptService).
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local coins = ReplicatedStorage:WaitForChild("Coins")
print("Game started. Collect the coins!")

coins.Changed:Connect(function(value)
	print("Coins:", value)
	if value >= 6 then
		print("You collected every coin!")
	end
end)
"#;

const CONTROLLER: &str = r#"-- First-person player controller. The engine has no built-in player: this script is the whole of it. It steers an
-- ordinary Part (the "Body" of the Player model, which has FixedRotation so it cannot tip over) by setting its
-- velocity, finds the ground with rays and drives the camera. Copy it, change it, or replace it with your own.
--   Mouse         look around          Escape   release the mouse
--   W A S D       move                 Click    capture the mouse again
--   Space         jump                 Shift    run
--   C / Ctrl      crouch (you can only stand up where there is room)
local Players = game:GetService("Players")
local RunService = game:GetService("RunService")
local UserInputService = game:GetService("UserInputService")

local CONFIG = {
	WalkSpeed = 16,
	RunSpeed = 28,
	CrouchSpeed = 7,
	JumpPower = 50,
	Acceleration = 12, -- how quickly the body reaches the speed it is asked for (per second, times the speed)
	AirControl = 0.3, -- how much of that it keeps in the air
	StandHeight = 5,
	CrouchHeight = 3,
	CrouchSpeedLerp = 14, -- how fast the body shrinks/grows
	Sensitivity = 0.15, -- degrees of rotation per pixel of mouse movement
	EyeOffset = 0.4, -- how far the eyes are below the top of the head
	MaxPitch = 89,
	FieldOfView = 70,
	RunFieldOfView = 80,
}

local character = workspace:WaitForChild("Player")
local body = character:WaitForChild("Body")
local camera = workspace.CurrentCamera

Players.LocalPlayer.Character = character
body.Transparency = 1 -- you cannot see your own body in first person
camera.CameraType = Enum.CameraType.Scriptable

local yaw, pitch = 0, 0
local height = CONFIG.StandHeight
local crouching = false
local speed = CONFIG.WalkSpeed
local fov = CONFIG.FieldOfView

local function setMouseLocked(locked)
	UserInputService.MouseBehavior = locked and Enum.MouseBehavior.LockCenter or Enum.MouseBehavior.Default
end
setMouseLocked(true)

UserInputService.InputBegan:Connect(function(input)
	if input.KeyCode == Enum.KeyCode.Escape then
		setMouseLocked(false)
	elseif input.UserInputType == Enum.UserInputType.MouseButton1 then
		setMouseLocked(true)
	end
end)

local function isDown(key)
	return UserInputService:IsKeyDown(key)
end

local rayParams = RaycastParams.new()
rayParams.FilterDescendantsInstances = { character }

-- Is there ground just under the feet? Five short rays: the middle and the four corners.
local function isGrounded()
	local half = body.Size / 2
	for _, offset in { Vector3.zero, Vector3.new(0.8, 0, 0.8), Vector3.new(-0.8, 0, 0.8), Vector3.new(0.8, 0, -0.8), Vector3.new(-0.8, 0, -0.8) } do
		local origin = body.Position + Vector3.new(half.X * offset.X, -half.Y + 0.05, half.Z * offset.Z)
		if workspace:Raycast(origin, Vector3.new(0, -0.3, 0), rayParams) then
			return true
		end
	end
	return false
end

-- Is there room above the head to stand up?
local function canStandUp()
	local top = body.Position + Vector3.new(0, height / 2, 0)
	return workspace:Raycast(top, Vector3.new(0, CONFIG.StandHeight - height + 0.3, 0), rayParams) == nil
end

-- Movement, jumping, crouching (every frame; the physics world is stepped at a fixed rate in between)
RunService.Heartbeat:Connect(function(dt)
	local wantsCrouch = isDown(Enum.KeyCode.C) or isDown(Enum.KeyCode.LeftControl)
	if wantsCrouch then
		crouching = true
	elseif crouching and canStandUp() then
		crouching = false
	end

	-- Resize the body around the feet.
	local targetHeight = crouching and CONFIG.CrouchHeight or CONFIG.StandHeight
	local newHeight = height + (targetHeight - height) * math.min(1, CONFIG.CrouchSpeedLerp * dt)
	if math.abs(newHeight - targetHeight) < 0.01 then
		newHeight = targetHeight
	end
	if newHeight ~= height then
		local pos = body.Position
		local feet = pos.Y - height / 2
		body.Size = Vector3.new(body.Size.X, newHeight, body.Size.Z)
		body.Position = Vector3.new(pos.X, feet + newHeight / 2, pos.Z)
		height = newHeight
	end

	-- Walk direction relative to where we are looking (forward is -Z).
	local x = (isDown(Enum.KeyCode.D) and 1 or 0) - (isDown(Enum.KeyCode.A) and 1 or 0)
	local z = (isDown(Enum.KeyCode.S) and 1 or 0) - (isDown(Enum.KeyCode.W) and 1 or 0)
	local move = Vector3.new(x, 0, z)
	local moving = move.Magnitude > 0
	if moving then
		move = CFrame.Angles(0, math.rad(yaw), 0) * move.Unit
	end

	local running = moving and not crouching and (isDown(Enum.KeyCode.LeftShift) or isDown(Enum.KeyCode.RightShift))
	local targetSpeed = crouching and CONFIG.CrouchSpeed or (running and CONFIG.RunSpeed or CONFIG.WalkSpeed)
	speed += (targetSpeed - speed) * math.min(1, 10 * dt)
	fov += ((running and CONFIG.RunFieldOfView or CONFIG.FieldOfView) - fov) * math.min(1, 8 * dt)

	-- Steer the horizontal velocity toward the wanted one; gravity and jumping own the vertical part.
	local grounded = isGrounded()
	local velocity = body.AssemblyLinearVelocity
	local wanted = move * speed
	local horizontal = Vector3.new(velocity.X, 0, velocity.Z)
	local delta = wanted - horizontal
	local reach = CONFIG.Acceleration * math.max(speed, 1) * dt * (grounded and 1 or CONFIG.AirControl)
	if delta.Magnitude <= reach then
		horizontal = wanted
	else
		horizontal += delta.Unit * reach
	end
	local vertical = velocity.Y
	if grounded and isDown(Enum.KeyCode.Space) then
		vertical = CONFIG.JumpPower
	end
	body.AssemblyLinearVelocity = Vector3.new(horizontal.X, vertical, horizontal.Z)
end)

-- Mouse look and camera (once per drawn frame)
RunService.RenderStepped:Connect(function()
	if UserInputService.MouseBehavior == Enum.MouseBehavior.LockCenter then
		local delta = UserInputService:GetMouseDelta()
		yaw -= delta.X * CONFIG.Sensitivity
		pitch = math.clamp(pitch - delta.Y * CONFIG.Sensitivity, -CONFIG.MaxPitch, CONFIG.MaxPitch)
	end
	local eye = body.Position + Vector3.new(0, height / 2 - CONFIG.EyeOffset, 0)
	camera.CFrame = CFrame.new(eye) * CFrame.Angles(0, math.rad(yaw), 0) * CFrame.Angles(math.rad(pitch), 0, 0)
	camera.FieldOfView = fov
end)
"#;

fn v(x: f32, y: f32, z: f32) -> Value {
    Value::V3(Vec3::new(x, y, z))
}

fn color(r: f32, g: f32, b: f32) -> Value {
    Value::Color(Vec3::new(r, g, b))
}

fn make(dm: &mut DataModel, class: &str, name: &str, parent: Id) -> Id {
    let id = dm.create(class).expect("template class");
    dm.set_name(id, name);
    dm.set_parent(id, Some(parent)).expect("template parent");
    id
}

fn part(dm: &mut DataModel, parent: Id, name: &str, pos: (f32, f32, f32), size: (f32, f32, f32), col: (f32, f32, f32)) -> Id {
    let id = make(dm, "Part", name, parent);
    let _ = dm.set_prop(id, "Size", v(size.0, size.1, size.2));
    let _ = dm.set_prop(id, "Position", v(pos.0, pos.1, pos.2));
    let _ = dm.set_prop(id, "Color", color(col.0, col.1, col.2));
    let _ = dm.set_prop(id, "Anchored", Value::Bool(true));
    id
}

fn script(dm: &mut DataModel, parent: Id, class: &str, name: &str, file: &str) -> Id {
    let id = make(dm, class, name, parent);
    let _ = dm.set_prop(id, "SourceFile", Value::Str(file.to_owned()));
    id
}

/// An empty scene: a baseplate, a spawn marker-less world and the default camera/lighting.
pub fn empty_place(name: &str) -> PlaceFile {
    let mut dm = DataModel::new();
    let ws = dm.workspace;
    part(&mut dm, ws, "Baseplate", (0.0, -10.0, 0.0), (512.0, 20.0, 512.0), (0.36, 0.52, 0.30));
    dm.to_place(name)
}

pub fn populate(project: &Project) -> Result<()> {
    let write = |rel: &str, text: &str| -> Result<()> {
        let path = project.root.join(rel);
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(path, text)?;
        Ok(())
    };
    // The folders say what a file is for, not where it runs: a Script runs on the server wherever its
    // file is, and these could be moved without changing anything.
    write(SPIN_FILE, SPIN)?;
    write(COIN_FILE, COIN)?;
    write(GAME_MANAGER_FILE, GAME_MANAGER)?;
    write(CONTROLLER_FILE, CONTROLLER)?;
    write("README.md", &readme(project.name()))?;
    // The starter materials are files like any other: edit them in the Material Editor, replace them, delete them, add more.
    for material in MaterialDef::starters() {
        write(&format!("{MATERIALS_DIR}/{}.mat", material.name), &material.to_json())?;
    }
    let sound = project.root.join(COIN_SOUND);
    if let Some(p) = sound.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(sound, coin_wav())?;

    let mut dm = DataModel::new();
    let ws = dm.workspace;
    part(&mut dm, ws, "Baseplate", (0.0, -10.0, 0.0), (512.0, 20.0, 512.0), (0.36, 0.52, 0.30));

    // A small obstacle course to try the controller on.
    part(&mut dm, ws, "Step 1", (0.0, 1.0, -20.0), (10.0, 2.0, 10.0), (0.85, 0.55, 0.30));
    part(&mut dm, ws, "Step 2", (0.0, 2.0, -34.0), (10.0, 4.0, 10.0), (0.85, 0.65, 0.30));
    part(&mut dm, ws, "Step 3", (0.0, 3.0, -48.0), (10.0, 6.0, 10.0), (0.85, 0.75, 0.30));
    let wall = part(&mut dm, ws, "Wall", (-30.0, 6.0, -10.0), (40.0, 12.0, 2.0), (0.55, 0.55, 0.65));
    let _ = dm.set_prop(wall, "Material", Value::Enum("SmoothPlastic".into()));
    // A low tunnel: you have to crouch to get through it.
    part(&mut dm, ws, "Tunnel Left", (30.0, 3.0, -20.0), (1.0, 6.0, 10.0), (0.6, 0.4, 0.4));
    part(&mut dm, ws, "Tunnel Right", (38.0, 3.0, -20.0), (1.0, 6.0, 10.0), (0.6, 0.4, 0.4));
    let roof = part(&mut dm, ws, "Tunnel Roof", (34.0, 4.2, -20.0), (9.0, 2.0, 10.0), (0.6, 0.4, 0.4));
    let _ = dm.set_prop(roof, "Material", Value::Enum("Glass".into()));
    // A lamp inside the tunnel.
    let lamp = make(&mut dm, "PointLight", "Tunnel Lamp", roof);
    let _ = dm.set_prop(lamp, "Range", Value::Num(22.0));
    let _ = dm.set_prop(lamp, "Color", color(1.0, 0.85, 0.6));
    let _ = dm.set_prop(lamp, "CastShadows", Value::Bool(true));
    let spinner = part(&mut dm, ws, "Spinner", (-20.0, 4.0, -30.0), (8.0, 1.0, 2.0), (0.9, 0.35, 0.35));
    let _ = dm.set_prop(spinner, "Material", Value::Enum("Metal".into()));
    script(&mut dm, spinner, "Script", "Spin", SPIN_FILE);

    for (i, (x, y, z)) in [(14.0, 3.0, -8.0), (-12.0, 3.0, 12.0), (20.0, 3.0, 14.0), (0.0, 9.0, -48.0), (34.0, 3.0, -20.0), (-22.0, 6.0, -30.0)].into_iter().enumerate() {
        let coin = part(&mut dm, ws, &format!("Coin {}", i + 1), (x, y, z), (2.0, 2.0, 2.0), (1.0, 0.85, 0.2));
        let _ = dm.set_prop(coin, "Shape", Value::Enum("Ball".into()));
        let _ = dm.set_prop(coin, "Material", Value::Enum("Neon".into()));
        let _ = dm.set_prop(coin, "CanCollide", Value::Bool(false));
        script(&mut dm, coin, "Script", "Collect", COIN_FILE);
        // Heard once more where the coin was when it is collected (the script destroys it).
        let ding = make(&mut dm, "Sound", "Pickup", coin);
        let _ = dm.set_prop(ding, "SoundId", Value::Str(COIN_SOUND.to_owned()));
        let _ = dm.set_prop(ding, "Volume", Value::Num(0.8));
        let _ = dm.set_prop(ding, "PlayOnRemove", Value::Bool(true));
    }

    // The player: a Model with one body part. The engine knows nothing about players: the controller script
    // (Client/Controllers/FirstPersonController.luau) moves this part by its velocity. FixedRotation keeps it upright.
    let model = make(&mut dm, "Model", "Player", ws);
    let body = make(&mut dm, "Part", "Body", model);
    let _ = dm.set_prop(body, "Size", v(2.0, 5.0, 2.0));
    let _ = dm.set_prop(body, "Position", v(0.0, 6.0, 0.0));
    let _ = dm.set_prop(body, "Color", color(0.3, 0.5, 0.9));
    let _ = dm.set_prop(body, "Transparency", Value::Num(0.5));
    let _ = dm.set_prop(body, "FixedRotation", Value::Bool(true));
    let _ = dm.set_prop(body, "Friction", Value::Num(0.0));
    let _ = dm.set_prop(model, "PrimaryPart", Value::Ref(Some(body)));
    dm.set_attr(model, "IsPlayer", Some(Value::Bool(true)));
    let sps = dm.service("ServerScriptService").expect("service");
    script(&mut dm, sps, "Script", "GameManager", GAME_MANAGER_FILE);
    let starter = dm.service("StarterPlayer").expect("service");
    script(&mut dm, starter, "LocalScript", "FirstPersonController", CONTROLLER_FILE);
    let rs = dm.service("ReplicatedStorage").expect("service");
    make(&mut dm, "NumberValue", "Coins", rs);
    // Neon (the coins) glows on every camera.
    let lighting = dm.service("Lighting").expect("service");
    make(&mut dm, "BloomEffect", "Bloom", lighting);

    project.write_place(&project.manifest.project.start_scene, &dm.to_place("Main"))
}

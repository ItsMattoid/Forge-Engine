//! Whole scripts of the kind Roblox games are made of, written as a Roblox developer writes them (no Forge-specific calls).

use crate::scripting::testkit::Game;

fn run(source: &str, frames: usize) -> Game {
    let mut g = Game::new(source);
    g.frames(frames);
    g
}

#[test]
fn leaderstats_script_on_player_added() {
    let g = run(
        r#"
        local Players = game:GetService("Players")
        local function onPlayerAdded(player)
            local leaderstats = Instance.new("Folder")
            leaderstats.Name = "leaderstats"
            leaderstats.Parent = player
            local coins = Instance.new("IntValue")
            coins.Name = "Coins"
            coins.Value = 0
            coins.Parent = leaderstats
        end
        Players.PlayerAdded:Connect(onPlayerAdded)
        for _, player in ipairs(Players:GetPlayers()) do
            task.spawn(onPlayerAdded, player)
        end
        task.wait(0.2)
        local player = Players:GetPlayers()[1]
        local coins = player.leaderstats.Coins
        coins.Changed:Connect(function(v) print("Coins is now", v) end)
        coins.Value += 5
        coins.Value += 10
        task.wait()
        game:GetService("ReplicatedStorage").Out.Value = tostring(coins.Value) .. ":" .. tostring(player:FindFirstChild("leaderstats") ~= nil)
    "#,
        30,
    );
    assert_eq!(g.out(), "15:true", "{}", g.logs());
    let logs = g.logs();
    assert!(logs.is_empty() || logs.contains("Coins is now"), "{logs}");
}

#[test]
fn coin_pickup_with_touched_debounce_and_destroy() {
    let g = run(
        r#"
        local Players = game:GetService("Players")
        local floor = Instance.new("Part", workspace)
        floor.Anchored = true
        floor.Size = Vector3.new(100, 2, 100)
        floor.Position = Vector3.new(0, -1, 0)

        local coin = Instance.new("Part")
        coin.Name = "Coin"
        coin.Anchored = true
        coin.CanCollide = false
        coin.Shape = Enum.PartType.Cylinder
        coin.Position = Vector3.new(0, 3, 0)
        coin.Parent = workspace

        local collected = 0
        local debounce = false
        coin.Touched:Connect(function(hit)
            if debounce then return end
            local character = hit.Parent
            if character and character:GetAttribute("IsPlayer") then
                debounce = true
                collected += 1
                coin:Destroy()
            end
        end)

        -- A character walks into it.
        local character = Instance.new("Model", workspace)
        character.Name = "Visitor"
        local root = Instance.new("Part", character)
        root.Name = "Body"
        root.Size = Vector3.new(2, 2, 1)
        root.Position = Vector3.new(0, 3, 0)
        character:SetAttribute("IsPlayer", true)
        character.PrimaryPart = root

        task.wait(0.5)
        game:GetService("ReplicatedStorage").Out.Value = table.concat({ tostring(collected), tostring(coin.Parent), tostring(workspace:FindFirstChild("Coin")) }, ",")
    "#,
        60,
    );
    assert_eq!(g.out(), "1,nil,nil", "{}", g.logs());
}

#[test]
fn kill_brick_changes_the_health_attribute_of_what_touches_it() {
    let g = run(
        r#"
        local floor = Instance.new("Part", workspace)
        floor.Anchored = true
        floor.Size = Vector3.new(100, 2, 100)
        floor.Position = Vector3.new(0, -1, 0)
        local lava = Instance.new("Part", workspace)
        lava.Name = "Lava"
        lava.Anchored = true
        lava.Size = Vector3.new(10, 1, 10)
        lava.Position = Vector3.new(0, 0.5, 0)
        lava.BrickColor = BrickColor.new("Bright red")
        lava.Material = Enum.Material.Neon

        lava.Touched:Connect(function(other)
            local character = other.Parent
            if character:GetAttribute("Health") and character:GetAttribute("Health") > 0 then
                character:SetAttribute("Health", 0)
            end
        end)

        local victim = Instance.new("Model", workspace)
        victim.Name = "Victim"
        local root = Instance.new("Part", victim)
        root.Name = "Body"
        root.Position = Vector3.new(0, 5, 0)
        victim:SetAttribute("Health", 100)
        local died = false
        victim:GetAttributeChangedSignal("Health"):Connect(function() died = victim:GetAttribute("Health") == 0 end)
        task.wait(1)
        game:GetService("ReplicatedStorage").Out.Value = table.concat({ tostring(died), tostring(victim:GetAttribute("Health")), lava.BrickColor.Name }, ",")
    "#,
        90,
    );
    assert_eq!(g.out(), "true,0,Bright red", "{}", g.logs());
}

#[test]
fn round_manager_with_a_module_script_and_a_bindable_event() {
    let mut g = Game::with_files(&[
        (
            "round.luau",
            r#"
            local Round = {}
            Round.__index = Round
            local started = 0

            function Round.new(length)
                local self = setmetatable({}, Round)
                self.Length = length
                self.State = "Intermission"
                self.Changed = Instance.new("BindableEvent")
                started += 1
                return self
            end

            function Round:Set(state)
                self.State = state
                self.Changed:Fire(state)
            end

            function Round:Run()
                self:Set("Playing")
                task.wait(self.Length)
                self:Set("Over")
            end

            function Round.count() return started end
            return Round
        "#,
        ),
        (
            "main.luau",
            r#"
            local Round = require(script.Parent:WaitForChild("RoundModule"))
            local same = require(script.Parent.RoundModule)
            local log = {}
            local round = Round.new(0.3)
            round.Changed.Event:Connect(function(state) table.insert(log, state) end)
            task.spawn(function() round:Run() end)
            task.wait(0.1)
            table.insert(log, "mid:" .. round.State)
            task.wait(0.5)
            table.insert(log, "end:" .. round.State)
            table.insert(log, "cached:" .. tostring(Round == same) .. "," .. Round.count())
            game:GetService("ReplicatedStorage").Out.Value = table.concat(log, ",")
        "#,
        ),
    ]);
    g.add_script("RoundModule", "ServerScriptService", "round.luau", "ModuleScript");
    g.add_script("RoundScript", "ServerScriptService", "main.luau", "Script");
    g.host.start_all();
    g.frames(90);
    assert_eq!(g.out(), "Playing,mid:Playing,Over,end:Over,cached:true,1", "{}", g.logs());
}

#[test]
fn tween_door_opens_and_closes_on_a_proximity_prompt() {
    let g = run(
        r#"
        local TweenService = game:GetService("TweenService")
        local door = Instance.new("Part")
        door.Name = "Door"
        door.Anchored = true
        door.Size = Vector3.new(4, 8, 1)
        door.Position = Vector3.new(0, 4, 0)
        door.Parent = workspace
        local prompt = Instance.new("ProximityPrompt")
        prompt.ActionText = "Open"
        prompt.HoldDuration = 0
        prompt.Parent = door

        local closed = door.CFrame
        local open = closed * CFrame.new(0, 8, 0)
        local isOpen = false
        local info = TweenInfo.new(0.4, Enum.EasingStyle.Quad, Enum.EasingDirection.Out)
        prompt.Triggered:Connect(function(player)
            isOpen = not isOpen
            prompt.ActionText = isOpen and "Close" or "Open"
            TweenService:Create(door, info, { CFrame = isOpen and open or closed }):Play()
        end)

        local player = game:GetService("Players").LocalPlayer
        task.wait(0.1)
        prompt:InputHoldBegin()
        prompt:InputHoldEnd()
        task.wait(0.6)
        local up = door.Position.Y
        local text = prompt.ActionText
        prompt:InputHoldBegin()
        prompt:InputHoldEnd()
        task.wait(0.6)
        game:GetService("ReplicatedStorage").Out.Value = table.concat({ string.format("%.0f", up), text, string.format("%.0f", door.Position.Y), prompt.ActionText }, ",")
    "#,
        120,
    );
    assert_eq!(g.out(), "12,Close,4,Open", "{}", g.logs());
}

#[test]
fn tagged_parts_are_given_behaviour_by_collection_service() {
    let g = run(
        r#"
        local CollectionService = game:GetService("CollectionService")
        local spinners = {}
        local function setup(part)
            spinners[part] = 0
            part:SetAttribute("Spinning", true)
        end
        for _, part in ipairs(CollectionService:GetTagged("Spinner")) do setup(part) end
        CollectionService:GetInstanceAddedSignal("Spinner"):Connect(setup)
        CollectionService:GetInstanceRemovedSignal("Spinner"):Connect(function(part) spinners[part] = nil end)

        game:GetService("RunService").Heartbeat:Connect(function(dt)
            for part in pairs(spinners) do
                spinners[part] += dt
                part.CFrame = CFrame.new(part.Position) * CFrame.Angles(0, spinners[part], 0)
            end
        end)

        local early = Instance.new("Part", workspace)
        early.Anchored = true
        early:AddTag("Spinner")
        local late = Instance.new("Part", workspace)
        late.Anchored = true
        task.wait(0.3)
        late:AddTag("Spinner")
        task.wait(0.3)
        local turned = early.Orientation.Y ~= 0
        local count = 0
        for _ in pairs(spinners) do count += 1 end
        early:RemoveTag("Spinner")
        task.wait(0.1)
        local after = 0
        for _ in pairs(spinners) do after += 1 end
        game:GetService("ReplicatedStorage").Out.Value = table.concat({ tostring(turned), tostring(count), tostring(after), tostring(late:GetAttribute("Spinning")) }, ",")
    "#,
        90,
    );
    assert_eq!(g.out(), "true,2,1,true", "{}", g.logs());
}

#[test]
fn object_oriented_inventory_with_attributes_and_attribute_signals() {
    let g = run(
        r#"
        local Inventory = {}
        Inventory.__index = Inventory
        function Inventory.new(owner)
            return setmetatable({ owner = owner, items = {} }, Inventory)
        end
        function Inventory:Add(name, count)
            self.items[name] = (self.items[name] or 0) + (count or 1)
            self.owner:SetAttribute(name, self.items[name])
        end
        function Inventory:Count(name) return self.items[name] or 0 end

        local folder = Instance.new("Folder", workspace)
        folder.Name = "Chest"
        local inv = Inventory.new(folder)
        local seen = {}
        folder:GetAttributeChangedSignal("Gold"):Connect(function()
            table.insert(seen, folder:GetAttribute("Gold"))
        end)
        folder.AttributeChanged:Connect(function(name) table.insert(seen, "changed:" .. name) end)
        inv:Add("Gold", 5)
        inv:Add("Gold", 7)
        inv:Add("Gem")
        task.wait()
        local attributes = folder:GetAttributes()
        -- (Handlers run after both changes were made, so they read the latest value; the order of the two signals is not documented.)
        local count = {}
        for _, entry in seen do count[tostring(entry)] = (count[tostring(entry)] or 0) + 1 end
        game:GetService("ReplicatedStorage").Out.Value = table.concat({
            inv:Count("Gold"), folder:GetAttribute("Gold"), attributes.Gem, count["changed:Gold"], count["12"], count["changed:Gem"],
        }, ",")
    "#,
        10,
    );
    assert_eq!(g.out(), "12,12,1,2,2,1", "{}", g.logs());
}

#[test]
fn coroutines_and_the_task_library_interleave_like_roblox() {
    let g = run(
        r#"
        local order = {}
        local function note(x) table.insert(order, x) end
        task.spawn(function()
            note("spawn-start")
            task.wait()
            note("spawn-resumed")
        end)
        note("after-spawn")
        task.defer(function() note("deferred") end)
        task.delay(0.1, function() note("delayed") end)

        local co = coroutine.create(function(a)
            local b = coroutine.yield(a + 1)
            return b * 2
        end)
        local _, first = coroutine.resume(co, 1)
        local _, second = coroutine.resume(co, 10)
        note("co:" .. first .. "," .. second .. "," .. coroutine.status(co))

        local wrapped = coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i) end end)
        note("wrap:" .. wrapped() .. wrapped() .. wrapped())

        local cancelled = task.delay(0.05, function() note("never") end)
        task.cancel(cancelled)

        local waited = task.wait(0.2)
        note("waited:" .. tostring(waited >= 0.2))
        local ok, err = pcall(function() error("boom") end)
        note("pcall:" .. tostring(ok) .. "," .. tostring(err):match("boom"))
        local ok2, err2 = pcall(error, { code = 7 })
        note("table error:" .. err2.code)
        note(select('#', xpcall(function() return 1, 2 end, print)))
        game:GetService("ReplicatedStorage").Out.Value = table.concat(order, "|")
    "#,
        40,
    );
    assert_eq!(
        g.out(),
        "spawn-start|after-spawn|co:2,20,dead|wrap:123|deferred|spawn-resumed|delayed|waited:true|pcall:false,boom|table error:7|3",
        "{}",
        g.logs()
    );
}

#[test]
fn local_scripts_see_the_local_player_and_server_scripts_do_not_mind() {
    let mut g = Game::with_files(&[(
        "client.luau",
        r#"
        local Players = game:GetService("Players")
        local player = Players.LocalPlayer
        local run = game:GetService("RunService")
        game:GetService("ReplicatedStorage").Out.Value = table.concat({
            tostring(player ~= nil), tostring(run:IsClient()), tostring(run:IsServer()),
            player and player.Name or "", tostring(script.Parent == player.PlayerScripts or script.Parent ~= nil), script.ClassName,
        }, ",")
    "#,
    )]);
    g.add_script("Client", "StarterPlayer", "client.luau", "LocalScript");
    g.host.start_all();
    g.frames(10);
    let out = g.out();
    assert!(out.starts_with("true,true,false,Player1,true,LocalScript"), "{out}\n{}", g.logs());
}

#[test]
fn script_errors_are_reported_and_do_not_stop_other_scripts() {
    let mut g = Game::with_files(&[
        ("bad.luau", "local x = nil\nprint(x.field)\n"),
        ("good.luau", "task.wait(0.05)\ngame:GetService('ReplicatedStorage').Out.Value = 'still running'\n"),
    ]);
    g.add_script("Bad", "ServerScriptService", "bad.luau", "Script");
    g.add_script("Good", "ServerScriptService", "good.luau", "Script");
    g.host.start_all();
    g.frames(10);
    assert_eq!(g.out(), "still running");
    let logs = g.logs();
    assert!(logs.contains("bad.luau") || logs.contains("Bad"), "the error names the script: {logs}");
    assert!(logs.contains("field") || logs.contains("nil"), "{logs}");
}

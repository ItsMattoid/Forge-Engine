use crate::scripting::testkit::{checks, checks_for, Game};

#[test]
fn get_service_returns_the_same_object_and_refuses_unknown_names() {
    checks(
        r#"
        for _, name in { "Workspace", "Players", "Lighting", "ReplicatedStorage", "ServerStorage", "ServerScriptService", "StarterGui", "StarterPlayer", "RunService", "UserInputService", "TweenService", "HttpService", "Debris", "CollectionService", "SoundService", "ContextActionService", "DataStoreService", "MessagingService", "PhysicsService", "MarketplaceService", "TeleportService", "TextService" } do
            local ok, service = pcall(function() return game:GetService(name) end)
            check("GetService " .. name, ok and service ~= nil and service.ClassName == name, service)
            if ok then
                equal(name .. " is one object", game:GetService(name), service)
                equal(name .. " is a child of game", service.Parent, game)
            end
        end
        throws("unknown service", function() return game:GetService("NoSuchService") end, "NoSuchService")
        equal("FindService finds one", game:FindService("Workspace"), workspace)
        equal("FindService on a missing one", game:FindService("NoSuchService"), nil)
        equal("game.Workspace", game.Workspace, workspace)
        equal("game.Players", game.Players, game:GetService("Players"))
        equal("game is a DataModel", game.ClassName, "DataModel")
        equal("typeof(game)", typeof(game), "Instance")
        check("PlaceId exists", typeof(game.PlaceId) == "number" and typeof(game.GameId) == "number")
        check("workspace.Gravity", near(workspace.Gravity, 196.2, 1e-3))
    "#,
    );
}

#[test]
fn http_service_encodes_json_and_makes_guids_but_cannot_make_requests() {
    checks(
        r#"
        local http = game:GetService("HttpService")
        local text = http:JSONEncode({ a = 1, b = { true, "x", 2.5 }, c = "q\"uote" })
        local back = http:JSONDecode(text)
        check("round trip", back.a == 1 and back.b[1] == true and back.b[2] == "x" and back.b[3] == 2.5 and back.c == "q\"uote", text)
        equal("array", http:JSONEncode({ 1, 2, 3 }), "[1,2,3]")
        equal("string", http:JSONEncode("hi"), "\"hi\"")
        equal("decode number", http:JSONDecode("12"), 12)
        equal("decode null in an array", #http:JSONDecode("[1,null,3]"), 3)
        equal("decode nested", http:JSONDecode('{"x":{"y":[1,2]}}').x.y[2], 2)
        throws("bad JSON", function() http:JSONDecode("{nope") end)
        throws("a function is not JSON", function() http:JSONEncode({ f = function() end }) end)
        local guid = http:GenerateGUID(true)
        check("GUID shape", guid:match("^{%x%x%x%x%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%-%x%x%x%x%x%x%x%x%x%x%x%x}$") ~= nil, guid)
        check("GUIDs differ", http:GenerateGUID() ~= http:GenerateGUID())
        check("without braces", http:GenerateGUID(false):sub(1, 1) ~= "{")
        equal("UrlEncode", http:UrlEncode("a b&c"), "a%20b%26c")
        throws("GetAsync", function() http:GetAsync("http://example.com") end, "not supported")
        throws("PostAsync", function() http:PostAsync("http://example.com", "x") end, "not supported")
        throws("RequestAsync", function() http:RequestAsync({ Url = "http://example.com" }) end, "not supported")
    "#,
    );
}

#[test]
fn data_stores_keep_copies_in_memory() {
    checks_for(
        r#"
        local service = game:GetService("DataStoreService")
        local store = service:GetDataStore("Coins")
        equal("empty key", store:GetAsync("k"), nil)
        local data = { coins = 5, items = { "sword", "shield" } }
        store:SetAsync("k", data)
        data.coins = 99
        local read = store:GetAsync("k")
        equal("what was stored is a copy", read.coins, 5)
        equal("nested values", read.items[2], "shield")
        read.coins = 1
        equal("and so is what is read", store:GetAsync("k").coins, 5)
        equal("same name, same store", service:GetDataStore("Coins"):GetAsync("k").coins, 5)
        equal("another name, another store", service:GetDataStore("Other"):GetAsync("k"), nil)
        equal("another scope, another store", service:GetDataStore("Coins", "scope"):GetAsync("k"), nil)

        equal("IncrementAsync", store:IncrementAsync("n", 3), 3)
        equal("IncrementAsync again", store:IncrementAsync("n"), 4)
        local new = store:UpdateAsync("n", function(old) return old * 10 end)
        equal("UpdateAsync returns the new value", new, 40)
        equal("and stores it", store:GetAsync("n"), 40)
        store:UpdateAsync("n", function() return nil end)
        equal("returning nil cancels the update", store:GetAsync("n"), 40)
        equal("RemoveAsync returns the old value", store:RemoveAsync("n"), 40)
        equal("and removes it", store:GetAsync("n"), nil)

        throws("empty key", function() store:GetAsync("") end, "Key name")
        throws("long key", function() store:GetAsync(string.rep("k", 51)) end, "50")
        throws("a function cannot be stored", function() store:SetAsync("f", { f = function() end }) end)

        local ordered = service:GetOrderedDataStore("Scores")
        ordered:SetAsync("a", 5)
        ordered:SetAsync("b", 9)
        ordered:SetAsync("c", 1)
        throws("ordered stores hold integers", function() ordered:SetAsync("d", "x") end)
        local page = ordered:GetSortedAsync(false, 10):GetCurrentPage()
        equal("largest first", page[1].key, "b")
        equal("the value", page[1].value, 9)
        equal("smallest last", page[#page].key, "c")
    "#,
        6,
    );
}

#[test]
fn context_actions_run_for_the_input_they_are_bound_to() {
    use bevy::input::keyboard::KeyCode;
    let mut g = Game::new(
        r#"
        local cas = game:GetService("ContextActionService")
        local log = {}
        cas:BindAction("Jump", function(name, state, input)
            table.insert(log, name .. ":" .. state.Name .. ":" .. input.KeyCode.Name)
            return Enum.ContextActionResult.Sink
        end, false, Enum.KeyCode.F)
        check_bound = cas:GetBoundActionInfo("Jump") ~= nil
        game:GetService("ReplicatedStorage").Out.Value = ""
        task.wait(0.3)
        cas:UnbindAction("Jump")
        table.insert(log, "bound:" .. tostring(cas:GetAllBoundActionInfo().Jump ~= nil))
        game:GetService("ReplicatedStorage").Out.Value = table.concat(log, "|")
    "#,
    );
    g.press_key(KeyCode::KeyF);
    g.frames(2);
    g.release_key(KeyCode::KeyF);
    g.frames(20);
    assert_eq!(g.out(), "Jump:Begin:F|Jump:End:F|bound:false", "{}", g.logs());
}

#[test]
fn messaging_service_delivers_to_subscribers_in_this_game() {
    checks_for(
        r#"
        local messaging = game:GetService("MessagingService")
        local received = {}
        local connection = messaging:SubscribeAsync("topic", function(message)
            table.insert(received, message.Data)
        end)
        messaging:PublishAsync("topic", "hello")
        messaging:PublishAsync("other", "ignored")
        task.wait(0.1)
        equal("subscriber got it", received[1], "hello")
        equal("only its topic", #received, 1)
        connection:Disconnect()
        messaging:PublishAsync("topic", "again")
        task.wait(0.1)
        equal("disconnected", #received, 1)
    "#,
        20,
    );
}

#[test]
fn tweens_interpolate_properties_with_roblox_easing() {
    let mut g = Game::new(
        r#"
        local TweenService = game:GetService("TweenService")
        local log = {}
        local part = Instance.new("Part", workspace)
        part.Anchored = true
        part.Position = Vector3.zero
        local info = TweenInfo.new(1, Enum.EasingStyle.Linear, Enum.EasingDirection.In)
        check_info = info
        table.insert(log, "info:" .. info.Time .. "," .. info.EasingStyle.Name .. "," .. info.RepeatCount .. "," .. tostring(info.Reverses) .. "," .. info.DelayTime)
        local tween = TweenService:Create(part, info, { Position = Vector3.new(10, 0, 0), Transparency = 1 })
        table.insert(log, "state:" .. tween.PlaybackState.Name)
        tween.Completed:Connect(function(state) table.insert(log, "completed:" .. state.Name) end)
        tween:Play()
        table.insert(log, "playing:" .. tween.PlaybackState.Name)
        task.wait(0.5)
        table.insert(log, "mid:" .. tostring(math.abs(part.Position.X - 5) < 1) .. "," .. tostring(math.abs(part.Transparency - 0.5) < 0.1))
        task.wait(0.7)
        table.insert(log, "end:" .. part.Position.X .. "," .. part.Transparency .. "," .. tween.PlaybackState.Name)

        -- A tween that reverses runs there and back again: 0.5 + 0.5 seconds.
        local other = Instance.new("Part", workspace)
        other.Anchored = true
        local longer = TweenService:Create(other, TweenInfo.new(0.5, Enum.EasingStyle.Linear, Enum.EasingDirection.In, 0, true), { Position = Vector3.new(0, 8, 0) })
        longer:Play()
        task.wait(0.5)
        table.insert(log, "turn:" .. string.format("%.0f", other.Position.Y))
        task.wait(0.6)
        table.insert(log, "back:" .. string.format("%.0f", other.Position.Y))

        -- Cancel leaves the property where it is.
        local third = Instance.new("Part", workspace)
        third.Anchored = true
        local cancelled = TweenService:Create(third, TweenInfo.new(1), { Position = Vector3.new(0, 0, 10) })
        local result
        cancelled.Completed:Connect(function(state) result = state.Name end)
        cancelled:Play()
        task.wait(0.2)
        cancelled:Cancel()
        task.wait(0.1)
        table.insert(log, "cancel:" .. tostring(result) .. "," .. cancelled.PlaybackState.Name .. "," .. tostring(third.Position.Z > 0 and third.Position.Z < 10))

        -- GetValue and the curves
        table.insert(log, "value:" .. string.format("%.2f,%.2f,%.2f", TweenService:GetValue(0.5, Enum.EasingStyle.Quad, Enum.EasingDirection.In), TweenService:GetValue(0.5, Enum.EasingStyle.Quad, Enum.EasingDirection.Out), TweenService:GetValue(0.25, Enum.EasingStyle.Quad, Enum.EasingDirection.InOut)))
        local ok = pcall(function() TweenService:Create(part, info, { NoSuchProperty = 1 }) end)
        table.insert(log, "bad:" .. tostring(not ok))
        game:GetService("ReplicatedStorage").Out.Value = table.concat(log, "|")
    "#,
    );
    g.frames(240);
    assert_eq!(
        g.out(),
        "info:1,Linear,0,false,0|state:Begin|playing:Playing|mid:true,true|completed:Completed|end:10,1,Completed|turn:8|back:0|cancel:Cancelled,Cancelled,true|value:0.25,0.75,0.12|bad:true",
        "{}",
        g.logs()
    );
}

#[test]
fn run_service_binds_steps_and_reports_where_a_script_runs() {
    checks_for(
        r#"
        local run = game:GetService("RunService")
        equal("IsServer", run:IsServer(), true)
        equal("IsClient", run:IsClient(), false)
        equal("IsRunning", run:IsRunning(), true)
        equal("IsStudio", run:IsStudio(), false)
        local beats, steps, renders = 0, 0, 0
        local heartbeat = run.Heartbeat:Connect(function(dt) beats += 1; check("dt", typeof(dt) == "number" and dt > 0) end)
        run.Stepped:Connect(function(t, dt) steps += 1 end)
        run:BindToRenderStep("Test", Enum.RenderPriority.Camera.Value, function() renders += 1 end)
        task.wait(0.1)
        check("Heartbeat fires every frame", beats >= 4, beats)
        check("Stepped fires every frame", steps >= 4, steps)
        check("render steps fire", renders >= 4, renders)
        run:UnbindFromRenderStep("Test")
        local seen = renders
        task.wait(0.1)
        equal("unbound", renders, seen)
        heartbeat:Disconnect()
        local after = beats
        task.wait(0.1)
        equal("disconnected", beats, after)
    "#,
        30,
    );
}

#[test]
fn collection_service_tags_instances_and_announces_them() {
    checks_for(
        r#"
        local tags = game:GetService("CollectionService")
        local added, removed = {}, {}
        local connection = tags:GetInstanceAddedSignal("Hazard"):Connect(function(i) table.insert(added, i.Name) end)
        tags:GetInstanceRemovedSignal("Hazard"):Connect(function(i) table.insert(removed, i.Name) end)
        local a = Instance.new("Part", workspace)
        a.Name = "A"
        local b = Instance.new("Part", workspace)
        b.Name = "B"
        tags:AddTag(a, "Hazard")
        a:AddTag("Hazard") -- again: nothing new
        b:AddTag("Hazard")
        b:AddTag("Other")
        task.wait()
        equal("added signal", table.concat(added, ","), "A,B")
        equal("HasTag", tags:HasTag(a, "Hazard"), true)
        equal("Instance:HasTag", b:HasTag("Other"), true)
        equal("GetTagged", #tags:GetTagged("Hazard"), 2)
        local names = a:GetTags()
        equal("GetTags", #names, 1)
        equal("GetTags on B", #tags:GetTags(b), 2)
        tags:RemoveTag(a, "Hazard")
        task.wait()
        equal("removed signal", table.concat(removed, ","), "A")
        b:Destroy()
        task.wait()
        equal("destroyed instances lose their tags", #tags:GetTagged("Hazard"), 0)
        equal("and announce it", table.concat(removed, ","), "A,B")
        -- Tags set before a connection are reported by GetTagged, and a new part added to the tag later is seen once.
        connection:Disconnect()
        a:AddTag("Hazard")
        task.wait()
        equal("disconnected", #added, 2)
    "#,
        10,
    );
}

#[test]
fn debris_removes_things_later() {
    checks_for(
        r#"
        local debris = game:GetService("Debris")
        local part = Instance.new("Part", workspace)
        debris:AddItem(part, 0.1)
        equal("still there at first", part.Parent, workspace)
        task.wait(0.3)
        equal("removed after its time", part.Parent, nil)
        local kept = Instance.new("Part", workspace)
        debris:AddItem(kept, 10)
        task.wait(0.1)
        equal("not before", kept.Parent, workspace)
    "#,
        30,
    );
}

#[test]
fn services_that_need_the_internet_say_so_clearly() {
    checks(
        r#"
        local function unsupported(name, f)
            local ok, err = pcall(f)
            check(name .. " errors", not ok)
            check(name .. " says why", tostring(err):find("not supported by Forge", 1, true) ~= nil, err)
        end
        local http = game:GetService("HttpService")
        unsupported("GetAsync", function() http:GetAsync("http://example.com") end)
        local teleport = game:GetService("TeleportService")
        unsupported("Teleport", function() teleport:Teleport(1) end)
        local marketplace = game:GetService("MarketplaceService")
        unsupported("GetProductInfo", function() marketplace:GetProductInfo(1) end)
        unsupported("PromptPurchase", function() marketplace:PromptGamePassPurchase(game:GetService("Players").LocalPlayer, 1) end)
        -- A call that does not exist at all is an ordinary Luau error, naming the method.
        throws("a typo", function() workspace:NoSuchMethod() end, "NoSuchMethod")
        throws("a typo on a property", function() return workspace.NoSuchProperty end, "NoSuchProperty")
    "#,
    );
}

#[test]
fn legacy_ray_functions_and_raycast_results() {
    checks_for(
        r#"
        local floor = Instance.new("Part", workspace)
        floor.Anchored = true
        floor.Size = Vector3.new(50, 2, 50)
        floor.Position = Vector3.new(0, -1, 0)
        floor.Name = "Floor"
        local decoy = Instance.new("Part", workspace)
        decoy.Anchored = true
        decoy.Name = "Decoy"
        decoy.Position = Vector3.new(0, 5, 0)
        task.wait(0.1)

        local result = workspace:Raycast(Vector3.new(0, 20, 0), Vector3.new(0, -40, 0))
        check("a ray hits", result ~= nil)
        equal("RaycastResult type", typeof(result), "RaycastResult")
        equal("Instance", result.Instance, decoy)
        check("Position", near(result.Position.Y, 5.5, 1e-3), result.Position)
        check("Normal", (result.Normal - Vector3.new(0, 1, 0)).Magnitude < 1e-3)
        check("Distance", near(result.Distance, 14.5, 1e-3), result.Distance)
        equal("Material", result.Material, Enum.Material.Plastic)

        local params = RaycastParams.new()
        params.FilterType = Enum.RaycastFilterType.Exclude
        params.FilterDescendantsInstances = { decoy }
        local through = workspace:Raycast(Vector3.new(0, 20, 0), Vector3.new(0, -40, 0), params)
        equal("Exclude skips the decoy", through.Instance, floor)
        params.FilterType = Enum.RaycastFilterType.Include
        params.FilterDescendantsInstances = { decoy }
        equal("Include only sees the decoy", workspace:Raycast(Vector3.new(0, 20, 0), Vector3.new(0, -40, 0), params).Instance, decoy)
        equal("a miss is nil", workspace:Raycast(Vector3.new(0, 20, 0), Vector3.new(0, 40, 0)), nil)

        -- The old API: rays in, (part, position, normal, material) out.
        local part, position, normal = workspace:FindPartOnRay(Ray.new(Vector3.new(0, 20, 0), Vector3.new(0, -40, 0)))
        equal("FindPartOnRay", part, decoy)
        check("FindPartOnRay position", near(position.Y, 5.5, 1e-3))
        check("FindPartOnRay normal", normal.Y > 0.9)
        equal("FindPartOnRayWithIgnoreList", (workspace:FindPartOnRayWithIgnoreList(Ray.new(Vector3.new(0, 20, 0), Vector3.new(0, -40, 0)), { decoy })), floor)

        -- Region queries
        local near_parts = workspace:GetPartBoundsInBox(CFrame.new(0, 5, 0), Vector3.new(2, 2, 2))
        equal("GetPartBoundsInBox", near_parts[1], decoy)
        local radius = workspace:GetPartBoundsInRadius(Vector3.new(0, 5, 0), 1)
        equal("GetPartBoundsInRadius", #radius, 1)
        equal("GetPartsInPart", workspace:GetPartsInPart(decoy)[1], nil)
    "#,
        10,
    );
}

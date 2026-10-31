use crate::scripting::testkit::{checks, checks_for};

#[test]
fn instance_new_builds_objects_of_the_documented_classes() {
    checks(
        r#"
        local part = Instance.new("Part")
        equal("ClassName", part.ClassName, "Part")
        equal("Name defaults to the class", part.Name, "Part")
        equal("no parent", part.Parent, nil)
        equal("typeof", typeof(part), "Instance")
        equal("tostring is the name", tostring(part), "Part")
        local inside = Instance.new("Part", workspace)
        equal("second argument is the parent", inside.Parent, workspace)
        throws("unknown class", function() return Instance.new("NoSuchClass") end, "NoSuchClass")
        throws("a class that scripts cannot make", function() return Instance.new("Workspace") end, "Workspace")
        throws("Tween is not created by hand", function() return Instance.new("Tween") end)

        -- The class hierarchy.
        check("Part IsA", part:IsA("Part") and part:IsA("BasePart") and part:IsA("PVInstance") and part:IsA("Instance"))
        check("Part is not a Model", not part:IsA("Model") and not part:IsA("Folder"))
        local model = Instance.new("Model")
        check("Model IsA", model:IsA("Model") and model:IsA("PVInstance") and model:IsA("Instance") and not model:IsA("BasePart"))
        check("Folder", Instance.new("Folder"):IsA("Instance") and not Instance.new("Folder"):IsA("PVInstance"))
        local script = Instance.new("Script")
        check("Script", script:IsA("BaseScript") and script:IsA("LuaSourceContainer") and not script:IsA("ModuleScript"))
        check("ModuleScript", Instance.new("ModuleScript"):IsA("LuaSourceContainer"))
        check("LocalScript", Instance.new("LocalScript"):IsA("BaseScript"))
        check("lights", Instance.new("PointLight"):IsA("Light") and Instance.new("SpotLight"):IsA("Light"))
        check("values", Instance.new("IntValue"):IsA("ValueBase") and Instance.new("Color3Value"):IsA("ValueBase") and not Instance.new("Folder"):IsA("ValueBase"))
        check("services", workspace:IsA("Workspace") and workspace:IsA("Instance") and game:IsA("DataModel"))
        check("unknown class names are false, not errors", part:IsA("NoSuchClass") == false)
        check("Wedge", Instance.new("WedgePart"):IsA("BasePart"))
        for _, class in { "Part", "WedgePart", "CornerWedgePart", "SpawnLocation", "MeshPart", "Model", "Folder", "Configuration", "Actor", "Attachment", "Decal", "Texture", "Sound", "SoundGroup", "ParticleEmitter", "Beam", "Trail", "Fire", "Smoke", "Sparkles", "Explosion", "Highlight", "ClickDetector", "ProximityPrompt", "Animation", "Animator", "BindableEvent", "BindableFunction", "RemoteEvent", "RemoteFunction", "UnreliableRemoteEvent", "BoolValue", "IntValue", "NumberValue", "StringValue", "ObjectValue", "Color3Value", "Vector3Value", "CFrameValue", "BrickColorValue", "PointLight", "SpotLight", "SurfaceLight", "Camera", "Sky", "Atmosphere", "BloomEffect", "BlurEffect", "ColorCorrectionEffect", "SunRaysEffect", "DepthOfFieldEffect", "Clouds", "Weld", "WeldConstraint", "Motor6D", "NoCollisionConstraint", "HingeConstraint", "BallSocketConstraint", "RopeConstraint", "RodConstraint", "SpringConstraint", "PrismaticConstraint", "BodyVelocity", "BodyForce", "BodyThrust", "BodyPosition", "BodyGyro", "BodyAngularVelocity", "VectorForce", "Torque", "LinearVelocity", "AlignPosition", "AlignOrientation", "Script", "LocalScript", "ModuleScript" } do
            local ok, made = pcall(Instance.new, class)
            check("Instance.new(" .. class .. ")", ok and made.ClassName == class, made)
        end
    "#,
    );
}

#[test]
fn the_tree_can_be_navigated_the_way_roblox_documents() {
    checks(
        r#"
        local root = Instance.new("Folder")
        root.Name = "Root"
        local a = Instance.new("Model", root); a.Name = "A"
        local b = Instance.new("Part", root); b.Name = "B"
        local deep = Instance.new("Part", a); deep.Name = "Deep"
        local marker = Instance.new("Part", deep); marker.Name = "A"
        workspace.Name = "Workspace"
        root.Parent = workspace

        local children = root:GetChildren()
        equal("GetChildren count", #children, 2)
        equal("GetChildren keeps order", children[1], a)
        local all = root:GetDescendants()
        equal("GetDescendants count", #all, 4)
        check("a parent comes before its children", table.find(all, a) < table.find(all, deep) and table.find(all, deep) < table.find(all, marker))

        equal("FindFirstChild", root:FindFirstChild("B"), b)
        equal("FindFirstChild is not recursive", root:FindFirstChild("Deep"), nil)
        equal("FindFirstChild recursive", root:FindFirstChild("Deep", true), deep)
        equal("nearest match first", root:FindFirstChild("A"), a)
        equal("FindFirstChildOfClass", root:FindFirstChildOfClass("Part"), b)
        equal("FindFirstChildOfClass is exact", root:FindFirstChildOfClass("BasePart"), nil)
        equal("FindFirstChildWhichIsA", root:FindFirstChildWhichIsA("BasePart"), b)
        equal("FindFirstChildWhichIsA recursive", a:FindFirstChildWhichIsA("BasePart", true), deep)
        equal("FindFirstAncestor", deep:FindFirstAncestor("Root"), root)
        equal("FindFirstAncestor skips itself", marker:FindFirstAncestor("A"), a)
        equal("FindFirstAncestorOfClass", deep:FindFirstAncestorOfClass("Model"), a)
        equal("FindFirstAncestorWhichIsA", marker:FindFirstAncestorWhichIsA("PVInstance"), deep)
        check("IsDescendantOf", marker:IsDescendantOf(root) and marker:IsDescendantOf(workspace) and not root:IsDescendantOf(marker))
        check("IsAncestorOf", root:IsAncestorOf(marker) and not marker:IsAncestorOf(root))
        check("a part is not its own descendant", not root:IsDescendantOf(root))
        equal("GetFullName", deep:GetFullName(), "Workspace.Root.A.Deep")
        equal("GetFullName of a service", workspace:GetFullName(), "Workspace")
        equal("child by name", root.B, b)
        equal("child through a chain", root.A.Deep, deep)
        throws("missing child", function() return root.Nothing end, "Nothing is not a valid member of Folder")
        throws("missing property write", function() root.Nothing = 1 end, "Nothing")
        equal("game.Workspace", game.Workspace, workspace)
        equal("GetService", game:GetService("Workspace"), workspace)
        equal("FindService", game:FindService("Workspace"), workspace)
        equal("FindService unknown", game:FindService("NoSuchService"), nil)
        throws("GetService unknown", function() return game:GetService("NoSuchService") end, "NoSuchService")
        check("IsLoaded", game:IsLoaded())

        -- Reparenting.
        deep.Parent = b
        equal("moved", deep.Parent, b)
        equal("no longer a child", a:FindFirstChild("Deep"), nil)
        throws("a cycle", function() root.Parent = marker end)
        throws("a part cannot be its own parent", function() b.Parent = b end)
        throws("a service cannot be moved", function() workspace.Parent = root end)
        b.Parent = nil
        equal("nil parent", b.Parent, nil)
        check("a removed part keeps its children", deep.Parent == b)

        -- Table keys and identity.
        local seen = {}
        seen[a] = "a"
        equal("an Instance is a table key", seen[root:FindFirstChild("A")], "a")
        equal("the same Instance is the same value", root:FindFirstChild("A") == a, true)
        root:Destroy()
    "#,
    );
}

#[test]
fn clone_and_destroy_follow_roblox() {
    checks(
        r#"
        local model = Instance.new("Model")
        model.Name = "Original"
        local part = Instance.new("Part", model)
        part.Name = "Child"
        part.Size = Vector3.new(7, 8, 9)
        part.Color = Color3.new(1, 0, 0)
        part:SetAttribute("Tag", "x")
        local value = Instance.new("ObjectValue", model)
        value.Value = part
        model.PrimaryPart = part

        local copy = model:Clone()
        check("a copy is a new object", copy ~= model and copy.ClassName == "Model")
        equal("a copy has no parent", copy.Parent, nil)
        equal("name", copy.Name, "Original")
        local copied = copy:FindFirstChild("Child")
        check("deep", copied ~= nil and copied ~= part)
        equal("properties", copied.Size, Vector3.new(7, 8, 9))
        equal("colour", copied.Color, Color3.new(1, 0, 0))
        equal("attributes", copied:GetAttribute("Tag"), "x")
        equal("references inside the copy point inside the copy", copy:FindFirstChildOfClass("ObjectValue").Value, copied)
        equal("PrimaryPart too", copy.PrimaryPart, copied)
        copied.Size = Vector3.one
        equal("the original is untouched", part.Size, Vector3.new(7, 8, 9))

        -- Archivable
        local keep = Instance.new("Part")
        equal("Archivable default", keep.Archivable, true)
        keep.Archivable = false
        equal("a non-archivable object has no clone", keep:Clone(), nil)
        local holder = Instance.new("Folder")
        local inner = Instance.new("Part", holder)
        inner.Archivable = false
        local plain = Instance.new("Part", holder)
        local held = holder:Clone()
        equal("non-archivable descendants are left out", #held:GetChildren(), 1)
        equal("fromExisting copies", Instance.fromExisting(plain).ClassName, "Part")

        -- Destroy
        local folder = Instance.new("Folder", workspace)
        local child = Instance.new("Part", folder)
        folder:Destroy()
        equal("a destroyed object has no parent", folder.Parent, nil)
        equal("so do its children", child.Parent == nil or child.Parent == folder, true)
        throws("a destroyed object cannot be parented", function() folder.Parent = workspace end)
        local again = Instance.new("Part", workspace)
        again:Destroy()
        again:Destroy() -- (a second Destroy is harmless)

        -- ClearAllChildren
        local bin = Instance.new("Folder", workspace)
        for _ = 1, 5 do Instance.new("Part", bin) end
        bin:ClearAllChildren()
        equal("ClearAllChildren", #bin:GetChildren(), 0)
        bin:Destroy()

        -- Remove (the old name) keeps the object.
        local piece = Instance.new("Part", workspace)
        piece:Remove()
        equal("Remove", piece.Parent, nil)
        check("a removed object is still usable", piece.Size.X == 4)
    "#,
    );
}

#[test]
fn legacy_lowercase_methods_still_work() {
    checks(
        r#"
        local folder = Instance.new("Folder", workspace)
        local part = Instance.new("Part", folder)
        part.Name = "P"
        equal("children()", #folder:children(), 1)
        equal("getChildren()", #folder:getChildren(), 1)
        equal("findFirstChild", folder:findFirstChild("P"), part)
        check("isA", part:isA("BasePart"))
        equal("GetChildren is the modern name", #folder:GetChildren(), 1)
        folder:Destroy()
    "#,
    );
}

#[test]
fn attributes_hold_roblox_values_and_announce_changes() {
    checks_for(
        r#"
        local part = Instance.new("Part")
        part:SetAttribute("Number", 5)
        part:SetAttribute("String", "hi")
        part:SetAttribute("Bool", true)
        part:SetAttribute("Vector", Vector3.new(1, 2, 3))
        part:SetAttribute("Vector2", Vector2.new(4, 5))
        part:SetAttribute("Color", Color3.new(1, 0, 0))
        part:SetAttribute("CFrame", CFrame.new(1, 2, 3))
        part:SetAttribute("UDim2", UDim2.new(1, 2, 3, 4))
        part:SetAttribute("Brick", BrickColor.new("Bright red"))
        part:SetAttribute("Range", NumberRange.new(1, 2))
        part:SetAttribute("Sequence", NumberSequence.new(1, 0))
        equal("number", part:GetAttribute("Number"), 5)
        equal("string", part:GetAttribute("String"), "hi")
        equal("bool", part:GetAttribute("Bool"), true)
        equal("Vector3", part:GetAttribute("Vector"), Vector3.new(1, 2, 3))
        equal("Vector2", part:GetAttribute("Vector2"), Vector2.new(4, 5))
        equal("Color3", part:GetAttribute("Color"), Color3.new(1, 0, 0))
        equal("CFrame", part:GetAttribute("CFrame"), CFrame.new(1, 2, 3))
        equal("UDim2", part:GetAttribute("UDim2"), UDim2.new(1, 2, 3, 4))
        equal("BrickColor", part:GetAttribute("Brick"), BrickColor.new("Bright red"))
        equal("NumberRange", part:GetAttribute("Range"), NumberRange.new(1, 2))
        equal("NumberSequence", typeof(part:GetAttribute("Sequence")), "NumberSequence")
        equal("missing", part:GetAttribute("Nothing"), nil)
        local all = part:GetAttributes()
        check("GetAttributes", all.Number == 5 and all.String == "hi" and all.Bool == true)
        part:SetAttribute("Number", nil)
        equal("nil removes", part:GetAttribute("Number"), nil)
        equal("removed from GetAttributes", part:GetAttributes().Number, nil)

        throws("a table is not an attribute", function() part:SetAttribute("T", {}) end)
        throws("a function is not an attribute", function() part:SetAttribute("F", print) end)
        throws("an Instance is not an attribute", function() part:SetAttribute("I", workspace) end)
        throws("names use letters, digits and underscores", function() part:SetAttribute("a b", 1) end)
        throws("names starting with RBX are reserved", function() part:SetAttribute("RBXThing", 1) end)
        throws("names cannot be empty", function() part:SetAttribute("", 1) end)
        throws("names stop at 100 characters", function() part:SetAttribute(string.rep("a", 101), 1) end)

        -- Signals
        local log = {}
        part.AttributeChanged:Connect(function(name) table.insert(log, "any:" .. name) end)
        part:GetAttributeChangedSignal("Level"):Connect(function() table.insert(log, "Level changed") end)
        part:SetAttribute("Level", 1)
        part:SetAttribute("Level", 1) -- not a change
        part:SetAttribute("Level", 2)
        part:SetAttribute("Other", true)
        part:SetAttribute("Level", nil)
        task.wait()
        equal("signals", table.concat(log, ","), "any:Level,Level changed,any:Level,Level changed,any:Other,any:Level,Level changed")

        -- Cloning keeps them.
        part:SetAttribute("Kept", 9)
        equal("clone", part:Clone():GetAttribute("Kept"), 9)
    "#,
        6,
    );
}

#[test]
fn tree_events_fire_as_documented() {
    checks_for(
        r#"
        local log = {}
        local function note(text) table.insert(log, text) end
        local folder = Instance.new("Folder", workspace)
        folder.Name = "F"
        folder.ChildAdded:Connect(function(child) note("added " .. child.Name) end)
        folder.ChildRemoved:Connect(function(child) note("removed " .. child.Name) end)
        folder.DescendantAdded:Connect(function(d) note("descendant+ " .. d.Name) end)
        folder.DescendantRemoving:Connect(function(d) note("descendant- " .. d.Name) end)

        local model = Instance.new("Model")
        model.Name = "M"
        local inner = Instance.new("Part", model)
        inner.Name = "Inner"
        inner.AncestryChanged:Connect(function(child, parent) note("ancestry " .. child.Name .. " -> " .. (parent and parent.Name or "nil")) end)
        model.Parent = folder
        task.wait()
        model.Parent = nil
        task.wait()
        -- (Roblox does not document the order of different signals for one move, so the events are compared as a set.)
        table.sort(log)
        local want = { "added M", "descendant+ M", "descendant+ Inner", "ancestry M -> F", "removed M", "descendant- M", "descendant- Inner", "ancestry M -> nil" }
        table.sort(want)
        equal("events", table.concat(log, "|"), table.concat(want, "|"))

        -- Changed, property signals, Destroying
        log = {}
        local elsewhere = Instance.new("Folder", workspace)
        local part = Instance.new("Part", workspace)
        part.Changed:Connect(function(name) note("changed " .. name) end)
        part:GetPropertyChangedSignal("Size"):Connect(function() note("size " .. tostring(part.Size)) end)
        part:GetPropertyChangedSignal("Parent"):Connect(function() note("parent") end)
        part.Anchored = true
        part.Size = Vector3.new(1, 2, 3)
        part.Size = Vector3.new(1, 2, 3)
        part.Name = "Renamed"
        part.Parent = elsewhere
        task.wait()
        equal("changes", table.concat(log, "|"), "changed Anchored|changed Size|size 1, 2, 3|changed Name|changed Parent|parent")
        part.Destroying:Connect(function() note("destroying") end)
        part:Destroy()
        task.wait()
        equal("Destroying", log[#log], "destroying")

        -- Value objects pass the new value to Changed.
        log = {}
        local number = Instance.new("NumberValue")
        number.Changed:Connect(function(v) note("n=" .. v) end)
        number.Value = 5
        number.Value = 5
        number.Value = 6
        local int = Instance.new("IntValue")
        int.Changed:Connect(function(v) note("i=" .. v) end)
        int.Value = 7.6
        task.wait()
        equal("value changes", table.concat(log, "|"), "n=5|n=6|i=8")
        equal("IntValue holds whole numbers", int.Value, 8)
    "#,
        8,
    );
}

#[test]
fn wait_for_child_waits_and_times_out() {
    checks_for(
        r#"
        local folder = Instance.new("Folder", workspace)
        equal("an existing child is returned at once", (function() Instance.new("Part", folder).Name = "Now" return folder:WaitForChild("Now").Name end)(), "Now")
        task.delay(0.1, function()
            local late = Instance.new("Part")
            late.Name = "Late"
            late.Parent = folder
        end)
        local before = time()
        local found = folder:WaitForChild("Late")
        equal("waits for a child that arrives", found.Name, "Late")
        check("it did wait", time() - before > 0.05)
        equal("times out with nil", folder:WaitForChild("Never", 0.1), nil)
        -- Several scripts can wait for the same child.
        local results = {}
        for i = 1, 3 do
            task.spawn(function() results[i] = folder:WaitForChild("Shared").Name end)
        end
        task.wait(0.05)
        local shared = Instance.new("Part", folder)
        shared.Name = "Shared"
        task.wait(0.05)
        equal("all waiters resume", table.concat(results, ","), "Shared,Shared,Shared")
        -- WaitForChild finds a child that was there already even if it is deep in the waiter's list of timeouts.
        equal("workspace.Camera", workspace:WaitForChild("Camera").ClassName, "Camera")
    "#,
        60,
    );
}

#[test]
fn collection_service_tags_and_signals() {
    checks_for(
        r#"
        local tags = game:GetService("CollectionService")
        local a, b = Instance.new("Part", workspace), Instance.new("Part", workspace)
        local added, removed = {}, {}
        tags:GetInstanceAddedSignal("Coin"):Connect(function(i) table.insert(added, i) end)
        tags:GetInstanceRemovedSignal("Coin"):Connect(function(i) table.insert(removed, i) end)
        tags:AddTag(a, "Coin")
        tags:AddTag(a, "Coin")
        b:AddTag("Coin")
        b:AddTag("Hazard")
        equal("HasTag", tags:HasTag(a, "Coin"), true)
        equal("instance HasTag", b:HasTag("Hazard"), true)
        equal("GetTagged", #tags:GetTagged("Coin"), 2)
        local names = b:GetTags()
        table.sort(names)
        equal("GetTags", table.concat(names, ","), "Coin,Hazard")
        equal("service GetTags", #tags:GetTags(b), 2)
        local all = tags:GetAllTags()
        table.sort(all)
        equal("GetAllTags", table.concat(all, ","), "Coin,Hazard")
        a:RemoveTag("Coin")
        equal("RemoveTag", tags:HasTag(a, "Coin"), false)
        b:Destroy()
        task.wait()
        equal("added signals", #added, 2)
        equal("removed signals: a tag taken and an object destroyed", #removed, 2)
        equal("a destroyed object leaves GetTagged", #tags:GetTagged("Coin"), 0)
    "#,
        6,
    );
}
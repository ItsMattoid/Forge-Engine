use crate::scripting::testkit::checks;

#[test]
fn vector2_has_the_members_and_arithmetic_roblox_documents() {
    checks(
        r#"
        local a, b = Vector2.new(3, 4), Vector2.new(1, 2)
        equal("add", a + b, Vector2.new(4, 6))
        equal("sub", a - b, Vector2.new(2, 2))
        equal("mul number", a * 2, Vector2.new(6, 8))
        equal("number mul", 2 * a, Vector2.new(6, 8))
        equal("mul vector", a * b, Vector2.new(3, 8))
        equal("div", a / 2, Vector2.new(1.5, 2))
        equal("idiv", Vector2.new(5, 7) // 2, Vector2.new(2, 3))
        equal("unm", -a, Vector2.new(-3, -4))
        equal("X", a.X, 3)
        equal("Magnitude", a.Magnitude, 5)
        check("Unit", near(a.Unit.X, 0.6, 1e-6) and near(a.Unit.Y, 0.8, 1e-6))
        equal("Dot", a:Dot(b), 11)
        equal("Cross", a:Cross(b), 2)
        equal("Lerp", Vector2.zero:Lerp(a, 0.5), Vector2.new(1.5, 2))
        equal("Max", a:Max(b), a)
        equal("Min", a:Min(b), b)
        equal("Floor", Vector2.new(1.7, -1.2):Floor(), Vector2.new(1, -2))
        equal("Ceil", Vector2.new(1.2, -1.7):Ceil(), Vector2.new(2, -1))
        equal("Sign", Vector2.new(-5, 0):Sign(), Vector2.new(-1, 0))
        check("FuzzyEq", a:FuzzyEq(Vector2.new(3.000001, 4)))
        equal("typeof", typeof(a), "Vector2")
        equal("tostring", tostring(Vector2.new(1, 2)), "1, 2")
        equal("constants", Vector2.xAxis + Vector2.yAxis, Vector2.one)
        throws("number plus number is not a vector", function() return Vector2.new(1, 1) + "x" end)
    "#,
    );
}

#[test]
fn vector3_members_including_the_ones_reached_through_a_chain_of_names() {
    checks(
        r#"
        local a = Vector3.new(1, 2, 3)
        equal("typeof", typeof(a), "Vector3")
        equal("X", a.X, 1)
        equal("lowercase", a.y, 2)
        -- The VM resolves `Vector3.one.X` as one chain of names; it must find X all the same.
        equal("one.X", Vector3.one.X, 1)
        equal("zero.Y", Vector3.zero.Y, 0)
        equal("xAxis.X", Vector3.xAxis.X, 1)
        equal("yAxis.Y", Vector3.yAxis.Y, 1)
        equal("add", a + Vector3.one, Vector3.new(2, 3, 4))
        equal("mul number", a * 2, Vector3.new(2, 4, 6))
        equal("mul vector", a * Vector3.new(2, 2, 2), Vector3.new(2, 4, 6))
        equal("div", Vector3.new(2, 4, 6) / 2, a)
        equal("idiv", Vector3.new(5, 5, 5) // 2, Vector3.new(2, 2, 2))
        equal("unm", -a, Vector3.new(-1, -2, -3))
        equal("Magnitude", Vector3.new(3, 4, 0).Magnitude, 5)
        check("Unit", near(Vector3.new(3, 4, 0).Unit.X, 0.6, 1e-6))
        equal("Dot", a:Dot(Vector3.new(1, 0, 0)), 1)
        equal("Cross", Vector3.xAxis:Cross(Vector3.yAxis), Vector3.zAxis)
        equal("Lerp", Vector3.zero:Lerp(Vector3.new(2, 0, 0), 0.5), Vector3.xAxis)
        check("Angle", near(Vector3.xAxis:Angle(Vector3.yAxis), math.pi / 2))
        check("signed Angle", near(Vector3.xAxis:Angle(Vector3.yAxis, Vector3.zAxis), math.pi / 2) and near(Vector3.yAxis:Angle(Vector3.xAxis, Vector3.zAxis), -math.pi / 2))
        equal("Ceil", Vector3.new(1.2, -1.2, 0):Ceil(), Vector3.new(2, -1, 0))
        equal("Floor", Vector3.new(1.2, -1.2, 0):Floor(), Vector3.new(1, -2, 0))
        equal("Sign", Vector3.new(-2, 0, 5):Sign(), Vector3.new(-1, 0, 1))
        equal("Abs", Vector3.new(-2, 0, 5):Abs(), Vector3.new(2, 0, 5))
        equal("Max", Vector3.new(1, 5, 2):Max(Vector3.new(3, 1, 2)), Vector3.new(3, 5, 2))
        equal("Min", Vector3.new(1, 5, 2):Min(Vector3.new(3, 1, 2)), Vector3.new(1, 1, 2))
        check("FuzzyEq", a:FuzzyEq(a + Vector3.new(1e-7, 0, 0)))
        equal("FromAxis", Vector3.FromAxis(Enum.Axis.Y), Vector3.yAxis)
        equal("FromNormalId", Vector3.FromNormalId(Enum.NormalId.Top), Vector3.yAxis)
        equal("FromNormalId front", Vector3.FromNormalId(Enum.NormalId.Front), Vector3.new(0, 0, -1))
        equal("tostring", tostring(a), "1, 2, 3")
        local t = {}
        t[Vector3.new(1, 2, 3)] = true
        check("equal vectors are the same key", t[a] == true)
    "#,
    );
}

#[test]
fn cframe_constructors_members_and_conversions() {
    checks(
        r#"
        local cf = CFrame.new(1, 2, 3)
        equal("typeof", typeof(cf), "CFrame")
        equal("Position", cf.Position, Vector3.new(1, 2, 3))
        equal("X Y Z", cf.X + cf.Y + cf.Z, 6)
        equal("identity", CFrame.identity, CFrame.new())
        equal("look", CFrame.new().LookVector, Vector3.new(0, 0, -1))
        equal("right", CFrame.new().RightVector, Vector3.new(1, 0, 0))
        equal("up", CFrame.new().UpVector, Vector3.new(0, 1, 0))
        equal("XVector", CFrame.new().XVector, Vector3.xAxis)
        equal("ZVector", CFrame.new().ZVector, Vector3.zAxis)

        -- Twelve components: the position, then the rotation matrix row by row.
        local quarter = CFrame.Angles(0, math.pi / 2, 0)
        local x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22 = quarter:GetComponents()
        check("matrix from components", near(r00, 0) and near(r02, 1) and near(r20, -1) and near(r11, 1))
        local rebuilt = CFrame.new(x, y, z, r00, r01, r02, r10, r11, r12, r20, r21, r22)
        check("12-argument constructor", rebuilt:FuzzyEq(quarter))
        check("components alias", select('#', quarter:components()) == 12)
        check("R02 member", near(quarter.R02, 1) and near(quarter.R20, -1))

        -- Quaternion constructor: a quarter turn about Y.
        local s = math.sin(math.pi / 4)
        check("quaternion", CFrame.new(0, 0, 0, 0, s, 0, math.cos(math.pi / 4)):FuzzyEq(quarter))

        -- Euler angles in both orders, and back.
        local angled = CFrame.Angles(0.3, 0.5, 0.7)
        local rx, ry, rz = angled:ToEulerAnglesXYZ()
        check("ToEulerAnglesXYZ", near(rx, 0.3) and near(ry, 0.5) and near(rz, 0.7))
        local oriented = CFrame.fromOrientation(0.3, 0.5, 0.7)
        local ox, oy, oz = oriented:ToOrientation()
        check("ToOrientation", near(ox, 0.3) and near(oy, 0.5) and near(oz, 0.7))
        local yx, yy, yz = oriented:ToEulerAnglesYXZ()
        check("ToEulerAnglesYXZ", near(yx, 0.3) and near(yy, 0.5) and near(yz, 0.7))
        check("fromEulerAnglesXYZ is Angles", CFrame.fromEulerAnglesXYZ(0.3, 0.5, 0.7):FuzzyEq(angled))
        check("fromEulerAnglesYXZ is fromOrientation", CFrame.fromEulerAnglesYXZ(0.3, 0.5, 0.7):FuzzyEq(oriented))

        -- Axis and angle.
        local about = CFrame.fromAxisAngle(Vector3.yAxis, 1.2)
        local axis, angle = about:ToAxisAngle()
        check("ToAxisAngle", near(angle, 1.2) and axis:FuzzyEq(Vector3.yAxis, 1e-4))

        -- fromMatrix keeps the axes it is given.
        local m = CFrame.fromMatrix(Vector3.new(5, 0, 0), Vector3.yAxis, Vector3.zAxis)
        check("fromMatrix", m.RightVector:FuzzyEq(Vector3.yAxis) and m.UpVector:FuzzyEq(Vector3.zAxis) and m.Position == Vector3.new(5, 0, 0))

        -- Looking at things.
        local look = CFrame.lookAt(Vector3.new(0, 0, 10), Vector3.zero)
        check("lookAt", look.LookVector:FuzzyEq(Vector3.new(0, 0, -1)) and look.Position == Vector3.new(0, 0, 10))
        check("new(pos, target)", CFrame.new(Vector3.new(0, 0, 10), Vector3.zero):FuzzyEq(look))
        check("lookAlong", CFrame.lookAlong(Vector3.zero, Vector3.xAxis).LookVector:FuzzyEq(Vector3.xAxis))

        -- Composition.
        local a = CFrame.new(1, 0, 0) * CFrame.Angles(0, math.pi / 2, 0)
        check("inverse", (a * a:Inverse()):FuzzyEq(CFrame.new()))
        equal("mul vector", (CFrame.new(1, 0, 0) * Vector3.new(1, 0, 0)), Vector3.new(2, 0, 0))
        equal("add vector", (CFrame.new(1, 0, 0) + Vector3.new(0, 1, 0)).Position, Vector3.new(1, 1, 0))
        equal("sub vector", (CFrame.new(1, 0, 0) - Vector3.new(1, 0, 0)).Position, Vector3.zero)
        check("ToObjectSpace", CFrame.new(1, 0, 0):ToObjectSpace(CFrame.new(3, 0, 0)):FuzzyEq(CFrame.new(2, 0, 0)))
        check("ToWorldSpace", CFrame.new(1, 0, 0):ToWorldSpace(CFrame.new(3, 0, 0)):FuzzyEq(CFrame.new(4, 0, 0)))
        local p, q = CFrame.new(1, 0, 0):ToWorldSpace(CFrame.new(1, 0, 0), CFrame.new(2, 0, 0))
        check("ToWorldSpace converts several", p.X == 2 and q.X == 3)
        equal("PointToObjectSpace", quarter:PointToObjectSpace(Vector3.new(0, 0, -1)):FuzzyEq(Vector3.new(1, 0, 0)), true)
        equal("PointToWorldSpace", CFrame.new(1, 0, 0):PointToWorldSpace(Vector3.new(1, 0, 0)), Vector3.new(2, 0, 0))
        equal("VectorToWorldSpace ignores position", CFrame.new(9, 9, 9):VectorToWorldSpace(Vector3.xAxis), Vector3.xAxis)
        check("Lerp", CFrame.new():Lerp(CFrame.new(2, 0, 0), 0.5):FuzzyEq(CFrame.new(1, 0, 0)))
        check("Rotation has no position", CFrame.new(1, 2, 3).Rotation.Position == Vector3.zero)
        check("Orthonormalize", angled:Orthonormalize():FuzzyEq(angled))
        equal("eq", CFrame.new(1, 2, 3) == CFrame.new(1, 2, 3), true)
        check("tostring has the twelve numbers", #string.split(tostring(CFrame.new(1, 2, 3)), ", ") == 12)
    "#,
    );
}

#[test]
fn color3_conversions() {
    checks(
        r#"
        equal("typeof", typeof(Color3.new()), "Color3")
        equal("fromRGB", Color3.fromRGB(255, 0, 0), Color3.new(1, 0, 0))
        equal("fromRGB clamps", Color3.fromRGB(300, -5, 0), Color3.new(1, 0, 0))
        equal("fromHex", Color3.fromHex('#FF0000'), Color3.new(1, 0, 0))
        equal("fromHex short", Color3.fromHex("0f0"), Color3.new(0, 1, 0))
        equal("ToHex", Color3.new(1, 0.5, 0):ToHex(), "ff8000")
        throws("bad hex", function() return Color3.fromHex("nope") end)
        local h, s, v = Color3.new(1, 0, 0):ToHSV()
        check("ToHSV red", h == 0 and s == 1 and v == 1)
        local blue = Color3.new(0, 0, 1)
        local bh, bs, bv = blue:ToHSV()
        check("HSV round trip", Color3.fromHSV(bh, bs, bv) == blue)
        check("Lerp", Color3.new(0, 0, 0):Lerp(Color3.new(1, 1, 1), 0.5) == Color3.new(0.5, 0.5, 0.5))
        check("R G B", near(Color3.new(0.1, 0.2, 0.3).G, 0.2, 1e-6))
    "#,
    );
}

#[test]
fn brick_colors_snap_to_the_palette_and_follow_the_part() {
    checks(
        r#"
        local red = BrickColor.new("Bright red")
        equal("typeof", typeof(red), "BrickColor")
        equal("Name", red.Name, "Bright red")
        equal("Number", red.Number, 21)
        equal("by number", BrickColor.new(21), red)
        equal("Red()", BrickColor.Red(), red)
        equal("Color", red.Color, Color3.fromRGB(196, 40, 28))
        equal("tostring", tostring(red), "Bright red")
        equal("unknown names are the default grey", BrickColor.new("no such colour").Name, "Medium stone grey")
        equal("from a Color3", BrickColor.new(Color3.new(1, 0, 0)).Name, "Really red")
        equal("from r g b", BrickColor.new(1, 0, 0).Name, "Really red")
        check("random", typeof(BrickColor.random()) == "BrickColor")
        check("palette", typeof(BrickColor.palette(0)) == "BrickColor")
        equal("White", BrickColor.White().Name, "White")

        local part = Instance.new("Part")
        equal("a new part is Medium stone grey", part.BrickColor.Name, "Medium stone grey")
        part.BrickColor = BrickColor.new("Bright blue")
        equal("setting BrickColor sets Color", part.Color, Color3.fromRGB(13, 105, 172))
        part.Color = Color3.fromRGB(196, 40, 28)
        equal("setting Color sets BrickColor", part.BrickColor, red)
        local value = Instance.new("BrickColorValue")
        value.Value = red
        equal("BrickColorValue", value.Value, red)
        throws("a BrickColor property refuses other values", function() part.BrickColor = Color3.new() end)
    "#,
    );
}

#[test]
fn the_data_types_roblox_scripts_build() {
    checks(
        r#"
        -- UDim / UDim2
        local u = UDim2.new(0.5, 10, 0.25, 20)
        equal("UDim2 typeof", typeof(u), "UDim2")
        check("UDim2 members", u.X.Scale == 0.5 and u.X.Offset == 10 and u.Y.Scale == 0.25 and u.Width.Offset == 10 and u.Height.Offset == 20)
        equal("fromScale", UDim2.fromScale(1, 1), UDim2.new(1, 0, 1, 0))
        equal("fromOffset", UDim2.fromOffset(5, 6), UDim2.new(0, 5, 0, 6))
        equal("UDim2 add", u + UDim2.fromOffset(1, 1), UDim2.new(0.5, 11, 0.25, 21))
        equal("UDim2 Lerp", UDim2.new():Lerp(UDim2.fromOffset(10, 10), 0.5), UDim2.fromOffset(5, 5))
        equal("from UDims", UDim2.new(UDim.new(1, 2), UDim.new(3, 4)), UDim2.new(1, 2, 3, 4))
        equal("UDim typeof", typeof(UDim.new(1, 2)), "UDim")

        -- Rect, Ray, Region3
        local r = Rect.new(0, 0, 10, 20)
        check("Rect", typeof(r) == "Rect" and r.Width == 10 and r.Height == 20 and r.Min == Vector2.zero and r.Max == Vector2.new(10, 20))
        equal("Rect from vectors", Rect.new(Vector2.zero, Vector2.new(10, 20)), r)
        local ray = Ray.new(Vector3.zero, Vector3.new(0, 0, -10))
        check("Ray", typeof(ray) == "Ray" and ray.Origin == Vector3.zero and ray.Direction == Vector3.new(0, 0, -10))
        equal("Ray.Unit", ray.Unit.Direction, Vector3.new(0, 0, -1))
        equal("Ray:ClosestPoint", ray:ClosestPoint(Vector3.new(3, 0, -4)), Vector3.new(0, 0, -4))
        equal("Ray:Distance", ray:Distance(Vector3.new(3, 0, -4)), 3)
        local region = Region3.new(Vector3.new(-1, -2, -3), Vector3.new(1, 2, 3))
        check("Region3", typeof(region) == "Region3" and region.Size == Vector3.new(2, 4, 6) and region.CFrame.Position == Vector3.zero)

        -- Ranges and sequences
        local range = NumberRange.new(2, 5)
        check("NumberRange", typeof(range) == "NumberRange" and range.Min == 2 and range.Max == 5)
        equal("NumberRange from one", NumberRange.new(3).Max, 3)
        throws("NumberRange order", function() return NumberRange.new(5, 2) end)
        local seq = NumberSequence.new({ NumberSequenceKeypoint.new(0, 1), NumberSequenceKeypoint.new(0.5, 0, 0.25), NumberSequenceKeypoint.new(1, 1) })
        check("NumberSequence", typeof(seq) == "NumberSequence" and #seq.Keypoints == 3 and seq.Keypoints[2].Envelope == 0.25 and seq.Keypoints[2].Time == 0.5)
        check("NumberSequence from two numbers", #NumberSequence.new(0, 1).Keypoints == 2 and #NumberSequence.new(5).Keypoints == 2)
        throws("sequence must start at 0", function() return NumberSequence.new({ NumberSequenceKeypoint.new(0.5, 0), NumberSequenceKeypoint.new(1, 0) }) end)
        local colors = ColorSequence.new(Color3.new(1, 0, 0), Color3.new(0, 0, 1))
        check("ColorSequence", typeof(colors) == "ColorSequence" and colors.Keypoints[1].Value == Color3.new(1, 0, 0) and colors.Keypoints[2].Time == 1)
        check("ColorSequenceKeypoint", typeof(ColorSequenceKeypoint.new(0.5, Color3.new())) == "ColorSequenceKeypoint")

        -- Physical properties, axes, faces
        local pp = PhysicalProperties.new(2, 0.3, 0.5)
        check("PhysicalProperties", typeof(pp) == "PhysicalProperties" and pp.Density == 2 and pp.Friction == 0.3 and pp.Elasticity == 0.5 and pp.FrictionWeight == 1)
        check("PhysicalProperties from a material", PhysicalProperties.new(Enum.Material.Metal).Density > PhysicalProperties.new(Enum.Material.Wood).Density)
        local axes = Axes.new(Enum.Axis.X, Enum.Axis.Z)
        check("Axes", typeof(axes) == "Axes" and axes.X and axes.Z and not axes.Y)
        local faces = Faces.new(Enum.NormalId.Top, Enum.NormalId.Front)
        check("Faces", typeof(faces) == "Faces" and faces.Top and faces.Front and not faces.Back)

        -- Small integer vectors
        local v16 = Vector3int16.new(1, 2, 3)
        check("Vector3int16", typeof(v16) == "Vector3int16" and v16.Z == 3 and (v16 + v16).X == 2 and v16 == Vector3int16.new(1, 2, 3))
        check("Vector2int16", typeof(Vector2int16.new(1, 2)) == "Vector2int16" and Vector2int16.new(1, 2).Y == 2)

        -- TweenInfo, Font, DateTime
        local info = TweenInfo.new(2, Enum.EasingStyle.Linear, Enum.EasingDirection.In, 3, true, 0.5)
        check("TweenInfo", typeof(info) == "TweenInfo" and info.Time == 2 and info.EasingStyle == Enum.EasingStyle.Linear and info.RepeatCount == 3 and info.Reverses and info.DelayTime == 0.5)
        local plain = TweenInfo.new()
        check("TweenInfo defaults", plain.Time == 1 and plain.EasingStyle == Enum.EasingStyle.Quad and plain.EasingDirection == Enum.EasingDirection.Out and plain.RepeatCount == 0 and not plain.Reverses)
        local font = Font.fromEnum(Enum.Font.Arial)
        check("Font", typeof(font) == "Font" and font.Weight == Enum.FontWeight.Regular and not font.Bold)
        check("Bold font", Font.new("rbxasset://fonts/families/x.json", Enum.FontWeight.Bold).Bold)
        local d = DateTime.fromUnixTimestamp(86400 * 365)
        check("DateTime", typeof(d) == "DateTime" and d.UnixTimestamp == 86400 * 365)
        local parts = d:ToUniversalTime()
        check("DateTime parts", parts.Year == 1971 and parts.Month == 1 and parts.Day == 1 and parts.Hour == 0)
        equal("ToIsoDate", d:ToIsoDate(), "1971-01-01T00:00:00Z")
        equal("format", d:FormatUniversalTime("YYYY-MM-DD HH:mm:ss", "en-us"), "1971-01-01 00:00:00")
        equal("from parts", DateTime.fromUniversalTime(1971, 1, 1, 0, 0, 0).UnixTimestamp, d.UnixTimestamp)
        check("now", DateTime.now().UnixTimestamp > 1.6e9)
    "#,
    );
}

#[test]
fn random_is_seeded_and_covers_its_range() {
    checks(
        r#"
        local a, b = Random.new(1234), Random.new(1234)
        equal("typeof", typeof(a), "Random")
        for i = 1, 20 do
            equal("same seed " .. i, a:NextInteger(1, 1000), b:NextInteger(1, 1000))
        end
        local r = Random.new(7)
        local low, high = math.huge, -math.huge
        for _ = 1, 500 do
            local n = r:NextInteger(5, 8)
            low, high = math.min(low, n), math.max(high, n)
            local f = r:NextNumber()
            check("NextNumber in [0, 1)", f >= 0 and f < 1)
            local g = r:NextNumber(10, 20)
            check("NextNumber range", g >= 10 and g < 20)
        end
        check("NextInteger covers 5..8", low == 5 and high == 8)
        check("NextUnitVector", near(r:NextUnitVector().Magnitude, 1, 1e-4))
        local list = { 1, 2, 3, 4, 5, 6 }
        r:Shuffle(list)
        local sum = 0
        for _, n in list do sum += n end
        equal("Shuffle keeps the values", sum, 21)
        local c = a:Clone()
        equal("Clone continues the same sequence", c:NextInteger(1, 1e6), a:NextInteger(1, 1e6))
        throws("empty range", function() return r:NextInteger(5, 1) end)
    "#,
    );
}

#[test]
fn enums_are_objects_with_names_values_and_lookups() {
    checks(
        r#"
        equal("typeof Enum", typeof(Enum), "Enums")
        equal("typeof an enum", typeof(Enum.Material), "Enum")
        equal("typeof an item", typeof(Enum.Material.Plastic), "EnumItem")
        equal("tostring item", tostring(Enum.Material.Plastic), "Enum.Material.Plastic")
        equal("tostring enum", tostring(Enum.Material), "Enum.Material")
        equal("Name", Enum.Material.Plastic.Name, "Plastic")
        equal("Roblox's number for Plastic", Enum.Material.Plastic.Value, 256)
        equal("Neon", Enum.Material.Neon.Value, 288)
        equal("KeyCode.A", Enum.KeyCode.A.Value, 97)
        equal("KeyCode.Space", Enum.KeyCode.Space.Value, 32)
        equal("EnumType", Enum.Material.Plastic.EnumType, Enum.Material)
        equal("index by name", Enum.Material["Glass"], Enum.Material.Glass)
        equal("FromName", Enum.Material:FromName("Wood"), Enum.Material.Wood)
        equal("FromName unknown", Enum.Material:FromName("Unobtainium"), nil)
        equal("FromValue", Enum.Material:FromValue(256), Enum.Material.Plastic)
        equal("FromValue unknown", Enum.Material:FromValue(-5), nil)
        local items = Enum.NormalId:GetEnumItems()
        equal("GetEnumItems", #items, 6)
        check("GetEnumItems are EnumItems", typeof(items[1]) == "EnumItem")
        check("GetEnums lists the enums", #Enum:GetEnums() > 40)
        check("IsA", Enum.Material.Plastic:IsA("Material") and Enum.Material.Plastic:IsA("EnumItem") and not Enum.Material.Plastic:IsA("Font"))
        equal("an old name is the item it became", Enum.RaycastFilterType.Blacklist, Enum.RaycastFilterType.Exclude)
        equal("Whitelist", Enum.RaycastFilterType.Whitelist, Enum.RaycastFilterType.Include)
        equal("an alias is not listed twice", #Enum.RaycastFilterType:GetEnumItems(), 2)
        throws("unknown item", function() return Enum.Material.Unobtainium end, "Unobtainium")
        throws("unknown enum", function() return Enum.NoSuchEnum end, "NoSuchEnum")
        for _, name in { "PartType", "SurfaceType", "Font", "CameraType", "EasingStyle", "UserInputType", "KeyCode", "RenderPriority", "RunContext", "PlaybackState", "ParticleOrientation", "Technology", "AnimationPriority", "Limb", "MessageType", "ActuatorType", "CoreGuiType", "Axis" } do
            check("Enum." .. name .. " exists", Enum[name] ~= nil)
        end
        -- An item can be assigned to a property of its enum, by item or by name.
        local part = Instance.new("Part")
        part.Material = Enum.Material.Metal
        equal("assigned", part.Material, Enum.Material.Metal)
        throws("wrong enum", function() part.Material = Enum.PartType.Ball end)
        equal("Shape default", part.Shape, Enum.PartType.Block)
    "#,
    );
}

#[test]
fn typeof_names_every_roblox_type() {
    checks(
        r#"
        local function is(value, name) return typeof(value) == name end
        check("nil", is(nil, "nil"))
        check("boolean", is(true, "boolean"))
        check("number", is(1, "number"))
        check("string", is("", "string"))
        check("table", is({}, "table"))
        check("function", is(print, "function"))
        check("thread", is(coroutine.create(print), "thread"))
        check("Instance", is(workspace, "Instance"))
        check("Vector3", is(Vector3.new(), "Vector3"))
        check("Vector2", is(Vector2.new(), "Vector2"))
        check("CFrame", is(CFrame.new(), "CFrame"))
        check("Color3", is(Color3.new(), "Color3"))
        check("signal", is(workspace.ChildAdded, "RBXScriptSignal"))
        check("connection", is(workspace.ChildAdded:Connect(function() end), "RBXScriptConnection"))
        check("RaycastParams", is(RaycastParams.new(), "RaycastParams"))
        check("OverlapParams", is(OverlapParams.new(), "OverlapParams"))
        check("a table with a __type", is(setmetatable({}, { __type = "Thing" }), "Thing"))
        check("type() still says what Lua says", type(Vector3.new()) == "vector" and type(workspace) == "userdata")
    "#,
    );
}
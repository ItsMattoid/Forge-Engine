-- The Roblox value types that are only data: UDim, UDim2, Rect, Ray, Region3, NumberRange, NumberSequence,
-- ColorSequence, PhysicalProperties, BrickColor, Axes, Faces, Font, DateTime, Vector2int16/3int16, TweenInfo and
-- the query parameters. (Vector2, Vector3, CFrame, Color3 and Random are in Rust: lua_types.)
--
-- Each type is a table with a metatable whose `__type` is its name, which is what `typeof` reports. A type that
-- can be the value of a property or attribute also has a codec -- how to turn it into a list of numbers and back --
-- returned at the end for the host (core::DATA_TYPES lists them).

local G = _G

-- The prelude's `typeof` (it names tables by their metatable's `__type`). A local, so that the compiler's fast path for
-- Luau's own `typeof` cannot stand in for it.
local typeof = G.typeof

local function class(name)
	local mt = { __type = name }
	mt.__index = mt
	return mt
end

local function read_only(mt, name)
	mt.__newindex = function(_, key)
		error("Unable to assign property " .. tostring(key) .. ". Property is read only", 2)
	end
end

local function trunc(x)
	return x >= 0 and math.floor(x) or math.ceil(x)
end

local function expect(value, typename, what)
	if typeof(value) ~= typename then
		error("expected " .. what .. " to be a " .. typename .. ", got " .. typeof(value), 3)
	end
end

local codecs = {}

-- ---------------------------------------------------------------- UDim / UDim2
local UDim = class("UDim")
function UDim.new(scale, offset)
	return setmetatable({ Scale = scale or 0, Offset = offset or 0 }, UDim)
end
UDim.__add = function(a, b)
	return UDim.new(a.Scale + b.Scale, a.Offset + b.Offset)
end
UDim.__sub = function(a, b)
	return UDim.new(a.Scale - b.Scale, a.Offset - b.Offset)
end
UDim.__eq = function(a, b)
	return a.Scale == b.Scale and a.Offset == b.Offset
end
UDim.__tostring = function(u)
	return u.Scale .. ", " .. u.Offset
end
read_only(UDim)
codecs.UDim = {
	encode = function(v)
		return { v.Scale, v.Offset }
	end,
	decode = function(n)
		return UDim.new(n[1], n[2])
	end,
}

local UDim2 = class("UDim2")
function UDim2.new(a, b, c, d)
	if type(a) == "table" then
		return setmetatable({ X = a, Y = b, Width = a, Height = b }, UDim2)
	end
	local x, y = UDim.new(a, b), UDim.new(c, d)
	return setmetatable({ X = x, Y = y, Width = x, Height = y }, UDim2)
end
function UDim2.fromScale(x, y)
	return UDim2.new(x, 0, y, 0)
end
function UDim2.fromOffset(x, y)
	return UDim2.new(0, x, 0, y)
end
function UDim2:Lerp(goal, alpha)
	local function mix(a, b)
		return a + (b - a) * alpha
	end
	return UDim2.new(mix(self.X.Scale, goal.X.Scale), mix(self.X.Offset, goal.X.Offset), mix(self.Y.Scale, goal.Y.Scale), mix(self.Y.Offset, goal.Y.Offset))
end
UDim2.__add = function(a, b)
	return UDim2.new(a.X + b.X, a.Y + b.Y)
end
UDim2.__sub = function(a, b)
	return UDim2.new(a.X - b.X, a.Y - b.Y)
end
UDim2.__eq = function(a, b)
	return a.X == b.X and a.Y == b.Y
end
UDim2.__tostring = function(u)
	return "{" .. tostring(u.X) .. "}, {" .. tostring(u.Y) .. "}"
end
read_only(UDim2)
codecs.UDim2 = {
	encode = function(v)
		return { v.X.Scale, v.X.Offset, v.Y.Scale, v.Y.Offset }
	end,
	decode = function(n)
		return UDim2.new(n[1], n[2], n[3], n[4])
	end,
}

-- ---------------------------------------------------------------- Rect / Ray / Region3
local Rect = class("Rect")
function Rect.new(a, b, c, d)
	local min, max
	if type(a) == "number" then
		min, max = Vector2.new(a, b), Vector2.new(c, d)
	else
		min, max = a, b
	end
	return setmetatable({ Min = min, Max = max, Width = max.X - min.X, Height = max.Y - min.Y }, Rect)
end
Rect.__eq = function(a, b)
	return a.Min == b.Min and a.Max == b.Max
end
Rect.__tostring = function(r)
	return r.Min.X .. ", " .. r.Min.Y .. ", " .. r.Max.X .. ", " .. r.Max.Y
end
read_only(Rect)
codecs.Rect = {
	encode = function(v)
		return { v.Min.X, v.Min.Y, v.Max.X, v.Max.Y }
	end,
	decode = function(n)
		return Rect.new(n[1], n[2], n[3], n[4])
	end,
}

local Ray = class("Ray")
local ray_fields = Ray.__index
function Ray.new(origin, direction)
	return setmetatable({ Origin = origin, Direction = direction }, Ray)
end
Ray.__index = function(ray, key)
	if key == "Unit" then
		return Ray.new(ray.Origin, ray.Direction.Unit)
	end
	return ray_fields[key]
end
function Ray:ClosestPoint(point)
	local d = self.Direction
	local len2 = d:Dot(d)
	if len2 == 0 then
		return self.Origin
	end
	local t = math.clamp((point - self.Origin):Dot(d) / len2, 0, 1)
	return self.Origin + d * t
end
function Ray:Distance(point)
	return (self:ClosestPoint(point) - point).Magnitude
end
Ray.__eq = function(a, b)
	return a.Origin == b.Origin and a.Direction == b.Direction
end
Ray.__tostring = function(r)
	return "{" .. tostring(r.Origin) .. "}, {" .. tostring(r.Direction) .. "}"
end

local Region3 = class("Region3")
function Region3.new(min, max)
	local size = max - min
	return setmetatable({ CFrame = CFrame.new((min + max) / 2), Size = size }, Region3)
end
function Region3:ExpandToGrid(resolution)
	local half = self.Size / 2
	local center = self.CFrame.Position
	local lo, hi = center - half, center + half
	local function snap(v, f)
		return Vector3.new(f(v.X / resolution) * resolution, f(v.Y / resolution) * resolution, f(v.Z / resolution) * resolution)
	end
	return Region3.new(snap(lo, math.floor), snap(hi, math.ceil))
end
Region3.__tostring = function(r)
	return tostring(r.CFrame) .. "; " .. tostring(r.Size)
end

local Vector3int16 = class("Vector3int16")
function Vector3int16.new(x, y, z)
	return setmetatable({ X = trunc(x or 0), Y = trunc(y or 0), Z = trunc(z or 0) }, Vector3int16)
end
Vector3int16.__add = function(a, b)
	return Vector3int16.new(a.X + b.X, a.Y + b.Y, a.Z + b.Z)
end
Vector3int16.__sub = function(a, b)
	return Vector3int16.new(a.X - b.X, a.Y - b.Y, a.Z - b.Z)
end
Vector3int16.__mul = function(a, b)
	if type(a) == "number" then
		a, b = b, a
	end
	if type(b) == "number" then
		return Vector3int16.new(a.X * b, a.Y * b, a.Z * b)
	end
	return Vector3int16.new(a.X * b.X, a.Y * b.Y, a.Z * b.Z)
end
Vector3int16.__div = function(a, b)
	if type(b) == "number" then
		return Vector3int16.new(a.X / b, a.Y / b, a.Z / b)
	end
	return Vector3int16.new(a.X / b.X, a.Y / b.Y, a.Z / b.Z)
end
Vector3int16.__unm = function(a)
	return Vector3int16.new(-a.X, -a.Y, -a.Z)
end
Vector3int16.__eq = function(a, b)
	return a.X == b.X and a.Y == b.Y and a.Z == b.Z
end
Vector3int16.__tostring = function(v)
	return v.X .. ", " .. v.Y .. ", " .. v.Z
end
read_only(Vector3int16)
codecs.Vector3int16 = {
	encode = function(v)
		return { v.X, v.Y, v.Z }
	end,
	decode = function(n)
		return Vector3int16.new(n[1], n[2], n[3])
	end,
}

local Vector2int16 = class("Vector2int16")
function Vector2int16.new(x, y)
	return setmetatable({ X = trunc(x or 0), Y = trunc(y or 0) }, Vector2int16)
end
Vector2int16.__add = function(a, b)
	return Vector2int16.new(a.X + b.X, a.Y + b.Y)
end
Vector2int16.__sub = function(a, b)
	return Vector2int16.new(a.X - b.X, a.Y - b.Y)
end
Vector2int16.__mul = function(a, b)
	if type(a) == "number" then
		a, b = b, a
	end
	if type(b) == "number" then
		return Vector2int16.new(a.X * b, a.Y * b)
	end
	return Vector2int16.new(a.X * b.X, a.Y * b.Y)
end
Vector2int16.__div = function(a, b)
	if type(b) == "number" then
		return Vector2int16.new(a.X / b, a.Y / b)
	end
	return Vector2int16.new(a.X / b.X, a.Y / b.Y)
end
Vector2int16.__unm = function(a)
	return Vector2int16.new(-a.X, -a.Y)
end
Vector2int16.__eq = function(a, b)
	return a.X == b.X and a.Y == b.Y
end
Vector2int16.__tostring = function(v)
	return v.X .. ", " .. v.Y
end
read_only(Vector2int16)
codecs.Vector2int16 = {
	encode = function(v)
		return { v.X, v.Y }
	end,
	decode = function(n)
		return Vector2int16.new(n[1], n[2])
	end,
}

-- ---------------------------------------------------------------- NumberRange and the sequences
local NumberRange = class("NumberRange")
function NumberRange.new(min, max)
	max = max or min
	if min > max then
		error("NumberRange: min must be less than or equal to max", 2)
	end
	return setmetatable({ Min = min, Max = max }, NumberRange)
end
NumberRange.__eq = function(a, b)
	return a.Min == b.Min and a.Max == b.Max
end
NumberRange.__tostring = function(r)
	return r.Min .. " " .. r.Max
end
read_only(NumberRange)
codecs.NumberRange = {
	encode = function(v)
		return { v.Min, v.Max }
	end,
	decode = function(n)
		return NumberRange.new(n[1], n[2])
	end,
}

local NumberSequenceKeypoint = class("NumberSequenceKeypoint")
function NumberSequenceKeypoint.new(time, value, envelope)
	if time < 0 or time > 1 then
		error("NumberSequenceKeypoint time must be in the range [0, 1]", 2)
	end
	return setmetatable({ Time = time, Value = value, Envelope = envelope or 0 }, NumberSequenceKeypoint)
end
NumberSequenceKeypoint.__eq = function(a, b)
	return a.Time == b.Time and a.Value == b.Value and a.Envelope == b.Envelope
end
NumberSequenceKeypoint.__tostring = function(k)
	return k.Time .. ", " .. k.Value .. ", " .. k.Envelope
end
read_only(NumberSequenceKeypoint)

local NumberSequence = class("NumberSequence")
local function check_sequence(keypoints, name)
	if #keypoints < 2 then
		error(name .. " must have at least 2 keypoints", 3)
	end
	if keypoints[1].Time ~= 0 then
		error(name .. " must start at time=0", 3)
	end
	if keypoints[#keypoints].Time ~= 1 then
		error(name .. " must end at time=1", 3)
	end
	for i = 2, #keypoints do
		if keypoints[i].Time < keypoints[i - 1].Time then
			error(name .. " time must be in ascending order", 3)
		end
	end
end
function NumberSequence.new(a, b)
	local keypoints
	if type(a) == "table" then
		keypoints = table.clone(a)
	elseif b ~= nil then
		keypoints = { NumberSequenceKeypoint.new(0, a), NumberSequenceKeypoint.new(1, b) }
	else
		keypoints = { NumberSequenceKeypoint.new(0, a), NumberSequenceKeypoint.new(1, a) }
	end
	check_sequence(keypoints, "NumberSequence")
	return setmetatable({ Keypoints = keypoints }, NumberSequence)
end
NumberSequence.__eq = function(a, b)
	if #a.Keypoints ~= #b.Keypoints then
		return false
	end
	for i, k in ipairs(a.Keypoints) do
		if k ~= b.Keypoints[i] then
			return false
		end
	end
	return true
end
NumberSequence.__tostring = function(s)
	local parts = {}
	for i, k in ipairs(s.Keypoints) do
		parts[i] = tostring(k)
	end
	return table.concat(parts, " ")
end
read_only(NumberSequence)
codecs.NumberSequence = {
	encode = function(v)
		local out = {}
		for _, k in ipairs(v.Keypoints) do
			table.insert(out, k.Time)
			table.insert(out, k.Value)
			table.insert(out, k.Envelope)
		end
		return out
	end,
	decode = function(n)
		local keypoints = {}
		for i = 1, #n, 3 do
			table.insert(keypoints, NumberSequenceKeypoint.new(n[i], n[i + 1], n[i + 2]))
		end
		return NumberSequence.new(keypoints)
	end,
}

local ColorSequenceKeypoint = class("ColorSequenceKeypoint")
function ColorSequenceKeypoint.new(time, color)
	if time < 0 or time > 1 then
		error("ColorSequenceKeypoint time must be in the range [0, 1]", 2)
	end
	return setmetatable({ Time = time, Value = color }, ColorSequenceKeypoint)
end
ColorSequenceKeypoint.__eq = function(a, b)
	return a.Time == b.Time and a.Value == b.Value
end
ColorSequenceKeypoint.__tostring = function(k)
	return k.Time .. ", " .. tostring(k.Value)
end
read_only(ColorSequenceKeypoint)

local ColorSequence = class("ColorSequence")
function ColorSequence.new(a, b)
	local keypoints
	if type(a) == "table" then
		keypoints = table.clone(a)
	elseif b ~= nil then
		keypoints = { ColorSequenceKeypoint.new(0, a), ColorSequenceKeypoint.new(1, b) }
	else
		keypoints = { ColorSequenceKeypoint.new(0, a), ColorSequenceKeypoint.new(1, a) }
	end
	check_sequence(keypoints, "ColorSequence")
	return setmetatable({ Keypoints = keypoints }, ColorSequence)
end
ColorSequence.__eq = function(a, b)
	if #a.Keypoints ~= #b.Keypoints then
		return false
	end
	for i, k in ipairs(a.Keypoints) do
		if k ~= b.Keypoints[i] then
			return false
		end
	end
	return true
end
ColorSequence.__tostring = function(s)
	local parts = {}
	for i, k in ipairs(s.Keypoints) do
		parts[i] = tostring(k)
	end
	return table.concat(parts, " ")
end
read_only(ColorSequence)
codecs.ColorSequence = {
	encode = function(v)
		local out = {}
		for _, k in ipairs(v.Keypoints) do
			table.insert(out, k.Time)
			table.insert(out, k.Value.R)
			table.insert(out, k.Value.G)
			table.insert(out, k.Value.B)
		end
		return out
	end,
	decode = function(n)
		local keypoints = {}
		for i = 1, #n, 4 do
			table.insert(keypoints, ColorSequenceKeypoint.new(n[i], Color3.new(n[i + 1], n[i + 2], n[i + 3])))
		end
		return ColorSequence.new(keypoints)
	end,
}

-- ---------------------------------------------------------------- PhysicalProperties
local PhysicalProperties = class("PhysicalProperties")
-- density, friction, elasticity of the materials whose values differ much from a plastic part's
local material_physics = {
	Plastic = { 0.7, 0.3, 0.5 },
	SmoothPlastic = { 0.7, 0.2, 0.5 },
	Neon = { 0.7, 0.3, 0.2 },
	Wood = { 0.35, 0.48, 0.2 },
	WoodPlanks = { 0.35, 0.48, 0.2 },
	Marble = { 2.56, 0.2, 0.17 },
	Slate = { 2.69, 0.4, 0.2 },
	Concrete = { 2.4, 0.7, 0.2 },
	Granite = { 2.69, 0.4, 0.2 },
	Brick = { 1.92, 0.8, 0.15 },
	Cobblestone = { 2.4, 0.5, 0.17 },
	Rock = { 2.69, 0.5, 0.17 },
	Metal = { 7.85, 0.4, 0.25 },
	CorrodedMetal = { 7.85, 0.7, 0.2 },
	DiamondPlate = { 7.85, 0.35, 0.25 },
	Foil = { 2.7, 0.4, 0.25 },
	Grass = { 0.9, 0.4, 0.1 },
	Sand = { 1.6, 0.5, 0.05 },
	Fabric = { 0.7, 0.35, 0.05 },
	Snow = { 0.1, 0.3, 0.03 },
	Mud = { 0.9, 0.3, 0.07 },
	Ground = { 0.9, 0.45, 0.1 },
	Asphalt = { 2.36, 0.8, 0.2 },
	Ice = { 0.92, 0.02, 0.15 },
	Glass = { 2.4, 0.25, 0.2 },
}
function PhysicalProperties.new(density, friction, elasticity, frictionWeight, elasticityWeight)
	if typeof(density) == "EnumItem" then
		local p = material_physics[density.Name] or material_physics.Plastic
		density, friction, elasticity = p[1], p[2], p[3]
	end
	return setmetatable({
		Density = density,
		Friction = friction,
		Elasticity = elasticity,
		FrictionWeight = frictionWeight or 1,
		ElasticityWeight = elasticityWeight or 1,
	}, PhysicalProperties)
end
PhysicalProperties.__eq = function(a, b)
	return a.Density == b.Density and a.Friction == b.Friction and a.Elasticity == b.Elasticity and a.FrictionWeight == b.FrictionWeight and a.ElasticityWeight == b.ElasticityWeight
end
PhysicalProperties.__tostring = function(p)
	return p.Density .. ", " .. p.Friction .. ", " .. p.Elasticity .. ", " .. p.FrictionWeight .. ", " .. p.ElasticityWeight
end
read_only(PhysicalProperties)
codecs.PhysicalProperties = {
	-- nil (the part uses its material's physical properties) is an empty list.
	encode = function(v)
		if v == nil then
			return {}
		end
		return { v.Density, v.Friction, v.Elasticity, v.FrictionWeight, v.ElasticityWeight }
	end,
	decode = function(n)
		if #n == 0 then
			return nil
		end
		return PhysicalProperties.new(n[1], n[2], n[3], n[4], n[5])
	end,
	nullable = true,
}

-- ---------------------------------------------------------------- BrickColor
local BrickColor = class("BrickColor")
local brick_by_number, brick_by_name, brick_list = {}, {}, {}
for _, entry in ipairs(__brickcolors) do
	local color = Color3.fromRGB(entry[3], entry[4], entry[5])
	local brick = setmetatable({
		Name = entry[2],
		Number = entry[1],
		Color = color,
		r = color.R,
		g = color.G,
		b = color.B,
		R = color.R,
		G = color.G,
		B = color.B,
	}, BrickColor)
	brick_by_number[entry[1]] = brick
	if not brick_by_name[string.lower(entry[2])] then
		brick_by_name[string.lower(entry[2])] = brick
	end
	table.insert(brick_list, brick)
end
__brickcolors = nil
local default_brick = brick_by_number[194]

local function nearest_brick(r, g, b)
	local best, best_distance = default_brick, math.huge
	for _, brick in ipairs(brick_list) do
		local dr, dg, db = brick.r - r, brick.g - g, brick.b - b
		local distance = dr * dr + dg * dg + db * db
		if distance < best_distance then
			best, best_distance = brick, distance
		end
	end
	return best
end

function BrickColor.new(a, g, b)
	if type(a) == "string" then
		return brick_by_name[string.lower(a)] or default_brick
	elseif type(a) == "number" and g == nil then
		return brick_by_number[a] or default_brick
	elseif type(a) == "number" then
		return nearest_brick(a, g, b)
	elseif typeof(a) == "Color3" then
		return nearest_brick(a.R, a.G, a.B)
	end
	return default_brick
end
function BrickColor.palette(index)
	return brick_list[(index % #brick_list) + 1]
end
function BrickColor.random()
	return brick_list[math.random(1, #brick_list)]
end
BrickColor.Random = BrickColor.random
for name, brick in pairs({ Red = 21, Blue = 23, Yellow = 24, Green = 28, Black = 26, White = 1, Gray = 194, DarkGray = 199 }) do
	BrickColor[name] = function()
		return brick_by_number[brick]
	end
end
BrickColor.__eq = function(a, b)
	return a.Number == b.Number
end
BrickColor.__tostring = function(c)
	return c.Name
end
read_only(BrickColor)
codecs.BrickColor = {
	encode = function(v)
		return { v.Number }
	end,
	decode = function(n)
		return brick_by_number[n[1]] or default_brick
	end,
}

-- ---------------------------------------------------------------- Axes / Faces
local Axes = class("Axes")
function Axes.new(...)
	local axes = setmetatable({ X = false, Y = false, Z = false, Top = false, Bottom = false, Left = false, Right = false, Back = false, Front = false }, Axes)
	for _, item in { ... } do
		local name = item.Name
		if item.EnumType == Enum.Axis then
			axes[name] = true
		else
			axes[name] = true
			if name == "Top" or name == "Bottom" then
				axes.Y = true
			elseif name == "Left" or name == "Right" then
				axes.X = true
			else
				axes.Z = true
			end
		end
	end
	return axes
end

local Faces = class("Faces")
function Faces.new(...)
	local faces = setmetatable({ Top = false, Bottom = false, Left = false, Right = false, Back = false, Front = false }, Faces)
	for _, item in { ... } do
		faces[item.Name] = true
	end
	return faces
end

-- ---------------------------------------------------------------- Font
local Font = class("Font")
function Font.new(family, weight, style)
	weight = weight or Enum.FontWeight.Regular
	style = style or Enum.FontStyle.Normal
	return setmetatable({ Family = family, Weight = weight, Style = style, Bold = weight.Value >= Enum.FontWeight.Bold.Value }, Font)
end
function Font.fromEnum(font)
	return Font.new("rbxasset://fonts/families/" .. font.Name .. ".json")
end
function Font.fromName(name, weight, style)
	return Font.new("rbxasset://fonts/families/" .. name .. ".json", weight, style)
end
function Font.fromId(id, weight, style)
	return Font.new("rbxassetid://" .. tostring(id), weight, style)
end
Font.__eq = function(a, b)
	return a.Family == b.Family and a.Weight == b.Weight and a.Style == b.Style
end
Font.__tostring = function(f)
	return "Font { Family = " .. f.Family .. ", Weight = " .. tostring(f.Weight) .. ", Style = " .. tostring(f.Style) .. " }"
end

-- ---------------------------------------------------------------- DateTime
local DateTime = class("DateTime")
local month_names = { "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December" }
local day_names = { "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday" }

local function make_datetime(millis)
	return setmetatable({ UnixTimestampMillis = millis, UnixTimestamp = millis // 1000 }, DateTime)
end

-- Days since 1970-01-01 of a civil date (the proleptic Gregorian calendar), and the reverse.
local function days_from_civil(y, m, d)
	y -= m <= 2 and 1 or 0
	local era = (y >= 0 and y or y - 399) // 400
	local yoe = y - era * 400
	local doy = (153 * (m + (m > 2 and -3 or 9)) + 2) // 5 + d - 1
	local doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
	return era * 146097 + doe - 719468
end

local function civil_from_days(z)
	z += 719468
	local era = (z >= 0 and z or z - 146096) // 146097
	local doe = z - era * 146097
	local yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
	local y = yoe + era * 400
	local doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
	local mp = (5 * doy + 2) // 153
	local d = doy - (153 * mp + 2) // 5 + 1
	local m = mp + (mp < 10 and 3 or -9)
	return y + (m <= 2 and 1 or 0), m, d
end

function DateTime.now()
	local seconds = os.time()
	return make_datetime(seconds * 1000 + math.floor((os.clock() % 1) * 1000))
end
function DateTime.fromUnixTimestamp(seconds)
	return make_datetime(math.floor(seconds * 1000))
end
function DateTime.fromUnixTimestampMillis(millis)
	return make_datetime(math.floor(millis))
end
function DateTime.fromUniversalTime(year, month, day, hour, minute, second, millisecond)
	local days = days_from_civil(year, month or 1, day or 1)
	local seconds = days * 86400 + (hour or 0) * 3600 + (minute or 0) * 60 + (second or 0)
	return make_datetime(seconds * 1000 + (millisecond or 0))
end
DateTime.fromLocalTime = DateTime.fromUniversalTime
function DateTime.fromIsoDate(text)
	local y, mo, d, h, mi, s = string.match(text, "^(%d+)-(%d+)-(%d+)T(%d+):(%d+):(%d+)")
	if not y then
		return nil
	end
	return DateTime.fromUniversalTime(tonumber(y), tonumber(mo), tonumber(d), tonumber(h), tonumber(mi), tonumber(s), 0)
end

function DateTime:ToUniversalTime()
	local seconds = self.UnixTimestamp
	local days = seconds // 86400
	local rest = seconds - days * 86400
	local year, month, day = civil_from_days(days)
	return {
		Year = year,
		Month = month,
		Day = day,
		Hour = rest // 3600,
		Minute = (rest % 3600) // 60,
		Second = rest % 60,
		Millisecond = self.UnixTimestampMillis % 1000,
	}
end
-- (Times are kept in UTC: there is no time zone to convert to.)
DateTime.ToLocalTime = DateTime.ToUniversalTime

function DateTime:ToIsoDate()
	local t = self:ToUniversalTime()
	return string.format("%04d-%02d-%02dT%02d:%02d:%02dZ", t.Year, t.Month, t.Day, t.Hour, t.Minute, t.Second)
end

function DateTime:FormatUniversalTime(format)
	local t = self:ToUniversalTime()
	local weekday = ((self.UnixTimestamp // 86400) + 4) % 7 + 1
	local hour12 = t.Hour % 12 == 0 and 12 or t.Hour % 12
	local tokens = {
		YYYY = string.format("%04d", t.Year),
		YY = string.format("%02d", t.Year % 100),
		MMMM = month_names[t.Month],
		MMM = string.sub(month_names[t.Month], 1, 3),
		MM = string.format("%02d", t.Month),
		M = tostring(t.Month),
		dddd = day_names[weekday],
		ddd = string.sub(day_names[weekday], 1, 3),
		DD = string.format("%02d", t.Day),
		D = tostring(t.Day),
		HH = string.format("%02d", t.Hour),
		H = tostring(t.Hour),
		hh = string.format("%02d", hour12),
		h = tostring(hour12),
		mm = string.format("%02d", t.Minute),
		m = tostring(t.Minute),
		ss = string.format("%02d", t.Second),
		s = tostring(t.Second),
		SSS = string.format("%03d", t.Millisecond),
		A = t.Hour < 12 and "AM" or "PM",
		a = t.Hour < 12 and "am" or "pm",
	}
	-- Longest token first, so "MMMM" is not read as "MM" "MM".
	local order = { "YYYY", "MMMM", "dddd", "MMM", "ddd", "SSS", "YY", "MM", "DD", "HH", "hh", "mm", "ss", "M", "D", "H", "h", "m", "s", "A", "a" }
	local out, i = {}, 1
	while i <= #format do
		local matched = false
		for _, token in ipairs(order) do
			if string.sub(format, i, i + #token - 1) == token then
				table.insert(out, tokens[token])
				i += #token
				matched = true
				break
			end
		end
		if not matched then
			table.insert(out, string.sub(format, i, i))
			i += 1
		end
	end
	return table.concat(out)
end
DateTime.FormatLocalTime = DateTime.FormatUniversalTime

DateTime.__eq = function(a, b)
	return a.UnixTimestampMillis == b.UnixTimestampMillis
end
DateTime.__lt = function(a, b)
	return a.UnixTimestampMillis < b.UnixTimestampMillis
end
DateTime.__le = function(a, b)
	return a.UnixTimestampMillis <= b.UnixTimestampMillis
end
DateTime.__tostring = function(d)
	return d:ToIsoDate()
end

-- ---------------------------------------------------------------- TweenInfo
local TweenInfo = class("TweenInfo")
function TweenInfo.new(time, easingStyle, easingDirection, repeatCount, reverses, delayTime)
	return setmetatable({
		Time = time or 1,
		EasingStyle = easingStyle or Enum.EasingStyle.Quad,
		EasingDirection = easingDirection or Enum.EasingDirection.Out,
		RepeatCount = repeatCount or 0,
		Reverses = reverses or false,
		DelayTime = delayTime or 0,
	}, TweenInfo)
end
TweenInfo.__tostring = function(t)
	return string.format("%s, %s, %s, %s, %s, %s", tostring(t.Time), tostring(t.EasingStyle), tostring(t.EasingDirection), tostring(t.RepeatCount), tostring(t.Reverses), tostring(t.DelayTime))
end
read_only(TweenInfo)

-- ---------------------------------------------------------------- query parameters
-- RaycastParams.new() / OverlapParams.new(): plain tables the host reads the fields of (Workspace queries).
--   FilterDescendantsInstances: parts (and everything inside them) the filter applies to
--   FilterType: Exclude (the listed parts are ignored, the default) or Include (only the listed parts count)
--   CollisionGroup: the query behaves as a part of this group, and only sees what the group collides with
--   RespectCanCollide: ignore parts that do not collide (CanCollide off)
--   MaxParts (OverlapParams): 0 = no limit
local function filter_methods(mt)
	function mt:AddToFilter(instances)
		local list = self.FilterDescendantsInstances
		local added = typeof(instances) == "Instance" and { instances } or instances
		-- (The list is replaced, not appended to: a script may be holding the old one.)
		local merged = table.clone(list)
		for _, instance in ipairs(added) do
			table.insert(merged, instance)
		end
		self.FilterDescendantsInstances = merged
	end
end

local RaycastParams = class("RaycastParams")
filter_methods(RaycastParams)
function RaycastParams.new()
	return setmetatable({
		FilterDescendantsInstances = {},
		FilterType = Enum.RaycastFilterType.Exclude,
		IgnoreWater = false,
		CollisionGroup = "Default",
		RespectCanCollide = false,
		BruteForceAllSlow = false,
	}, RaycastParams)
end

local OverlapParams = class("OverlapParams")
filter_methods(OverlapParams)
function OverlapParams.new()
	return setmetatable({
		FilterDescendantsInstances = {},
		FilterType = Enum.RaycastFilterType.Exclude,
		CollisionGroup = "Default",
		RespectCanCollide = false,
		MaxParts = 0,
	}, OverlapParams)
end

-- ---------------------------------------------------------------- Vector3 constructors that need enums
function Vector3.FromAxis(axis)
	return axis == Enum.Axis.X and Vector3.xAxis or axis == Enum.Axis.Y and Vector3.yAxis or Vector3.zAxis
end
local normals = {
	Right = Vector3.new(1, 0, 0),
	Top = Vector3.new(0, 1, 0),
	Back = Vector3.new(0, 0, 1),
	Left = Vector3.new(-1, 0, 0),
	Bottom = Vector3.new(0, -1, 0),
	Front = Vector3.new(0, 0, -1),
}
function Vector3.FromNormalId(normal)
	return normals[normal.Name]
end

-- ---------------------------------------------------------------- result types
-- Tables the host builds that Roblox gives a type of their own (typeof reports it); the fields are plain.
local result_types = {
	RaycastResult = { __type = "RaycastResult" },
}
for _, mt in pairs(result_types) do
	mt.__index = mt
end

-- ---------------------------------------------------------------- exports
for name, value in pairs({
	UDim = UDim,
	UDim2 = UDim2,
	Rect = Rect,
	Ray = Ray,
	Region3 = Region3,
	Vector3int16 = Vector3int16,
	Vector2int16 = Vector2int16,
	NumberRange = NumberRange,
	NumberSequenceKeypoint = NumberSequenceKeypoint,
	NumberSequence = NumberSequence,
	ColorSequenceKeypoint = ColorSequenceKeypoint,
	ColorSequence = ColorSequence,
	PhysicalProperties = PhysicalProperties,
	BrickColor = BrickColor,
	Axes = Axes,
	Faces = Faces,
	Font = Font,
	DateTime = DateTime,
	TweenInfo = TweenInfo,
	RaycastParams = RaycastParams,
	OverlapParams = OverlapParams,
}) do
	G[name] = value
end

return {
	-- value -> list of numbers, for the type named `tag` (an error if the value is not one)
	encode = function(tag, value)
		local codec = codecs[tag]
		if value == nil and not codec.nullable then
			error("expected a " .. tag .. ", got nil", 0)
		end
		if value ~= nil and typeof(value) ~= tag then
			error("expected a " .. tag .. ", got " .. typeof(value), 0)
		end
		return codec.encode(value)
	end,
	-- list of numbers -> value
	decode = function(tag, numbers)
		return codecs[tag].decode(numbers)
	end,
	-- Gives a table built by the host the metatable of one of the result types.
	set_type = function(tag, fields)
		return setmetatable(fields, result_types[tag])
	end,
	-- Which of the codec types a value is (nil when it is none of them).
	tag_of = function(value)
		local t = typeof(value)
		return codecs[t] and t or nil
	end,
	brick_by_number = function(n)
		return brick_by_number[n]
	end,
}

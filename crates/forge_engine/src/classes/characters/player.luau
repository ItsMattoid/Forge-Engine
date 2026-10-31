-- Player:GetMouse() -> a Mouse: where the cursor is (X, Y), what it points at (Hit, Target, UnitRay, Origin) and
-- the button, move and wheel events. It is the older way to read the mouse; UserInputService does the same
-- through events. (A Mouse is not an Instance here; it answers IsA and ClassName like one.)
local mice = setmetatable({}, { __mode = "k" })

local function camera()
	return workspace.CurrentCamera
end

local function make_mouse(player)
	local uis = game:GetService("UserInputService")
	local signals = {}
	for _, name in { "Button1Down", "Button1Up", "Button2Down", "Button2Up", "Move", "WheelForward", "WheelBackward", "Idle", "KeyDown", "KeyUp" } do
		signals[name] = Signal.new(false)
	end

	local function cursor_ray()
		local at = uis:GetMouseLocation()
		return camera():ViewportPointToRay(at.X, at.Y)
	end

	local function cast()
		local ray = cursor_ray()
		local filter = RaycastParams.new()
		local ignored = mice[player] and mice[player].TargetFilter
		if ignored then
			filter.FilterDescendantsInstances = { ignored }
		end
		return ray, workspace:Raycast(ray.Origin, ray.Direction * 1000, filter)
	end

	local members = {
		ClassName = "PlayerMouse",
		Name = "Mouse",
		Icon = "",
		TargetFilter = nil,
		IsA = function(_, name)
			return name == "PlayerMouse" or name == "Mouse" or name == "Instance"
		end,
	}
	local mouse = setmetatable({}, {
		__type = "Instance",
		__tostring = function()
			return "Mouse"
		end,
		__index = function(_, key)
			if signals[key] then
				return signals[key]
			elseif key == "X" then
				return uis:GetMouseLocation().X
			elseif key == "Y" then
				return uis:GetMouseLocation().Y
			elseif key == "ViewSizeX" then
				return camera().ViewportSize.X
			elseif key == "ViewSizeY" then
				return camera().ViewportSize.Y
			elseif key == "UnitRay" then
				return cursor_ray()
			elseif key == "Origin" then
				local ray = cursor_ray()
				return CFrame.lookAt(ray.Origin, ray.Origin + ray.Direction)
			elseif key == "Hit" then
				local ray, result = cast()
				return CFrame.new(result and result.Position or ray.Origin + ray.Direction * 1000)
			elseif key == "Target" then
				local _, result = cast()
				return result and result.Instance or nil
			elseif key == "TargetSurface" then
				return Enum.NormalId.Top
			end
			return members[key]
		end,
		__newindex = function(_, key, value)
			members[key] = value
		end,
	})

	uis.InputBegan:Connect(function(input)
		if input.UserInputType == Enum.UserInputType.MouseButton1 then
			signals.Button1Down:Fire()
		elseif input.UserInputType == Enum.UserInputType.MouseButton2 then
			signals.Button2Down:Fire()
		elseif input.UserInputType == Enum.UserInputType.Keyboard then
			signals.KeyDown:Fire(string.lower(input.KeyCode.Name))
		end
	end)
	uis.InputEnded:Connect(function(input)
		if input.UserInputType == Enum.UserInputType.MouseButton1 then
			signals.Button1Up:Fire()
		elseif input.UserInputType == Enum.UserInputType.MouseButton2 then
			signals.Button2Up:Fire()
		elseif input.UserInputType == Enum.UserInputType.Keyboard then
			signals.KeyUp:Fire(string.lower(input.KeyCode.Name))
		end
	end)
	uis.InputChanged:Connect(function(input)
		if input.UserInputType == Enum.UserInputType.MouseMovement then
			signals.Move:Fire()
		elseif input.UserInputType == Enum.UserInputType.MouseWheel then
			if input.Delta.Z > 0 then
				signals.WheelForward:Fire()
			else
				signals.WheelBackward:Fire()
			end
		end
	end)
	return mouse
end

methods.GetMouse = function(self)
	local mouse = mice[self]
	if not mouse then
		mouse = make_mouse(self)
		mice[self] = mouse
	end
	return mouse
end

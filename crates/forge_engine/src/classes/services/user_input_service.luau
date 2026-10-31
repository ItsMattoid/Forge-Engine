local function input_object(key)
	return setmetatable({
		KeyCode = key,
		UserInputType = Enum.UserInputType.Keyboard,
		UserInputState = Enum.UserInputState.Begin,
		Delta = Vector3.zero,
		Position = Vector3.zero,
	}, { __type = "InputObject" })
end

-- The keys held right now, as InputObjects.
methods.GetKeysPressed = function(self)
	local held = {}
	for _, key in ipairs(Enum.KeyCode:GetEnumItems()) do
		if key ~= Enum.KeyCode.Unknown and self:IsKeyDown(key) then
			table.insert(held, input_object(key))
		end
	end
	return held
end

methods.GetMouseButtonsPressed = function(self)
	local held = {}
	for _, button in { Enum.UserInputType.MouseButton1, Enum.UserInputType.MouseButton2, Enum.UserInputType.MouseButton3 } do
		if self:IsMouseButtonPressed(button) then
			table.insert(held, setmetatable({ KeyCode = Enum.KeyCode.Unknown, UserInputType = button, UserInputState = Enum.UserInputState.Begin, Delta = Vector3.zero, Position = Vector3.zero }, { __type = "InputObject" }))
		end
	end
	return held
end

methods.GetLastInputType = function()
	return Enum.UserInputType.Keyboard
end
methods.GetConnectedGamepads = function()
	return {}
end
methods.GetGamepadConnected = function()
	return false
end
methods.GetFocusedTextBox = function()
	return nil
end
methods.GetPlatform = function()
	return Enum.Platform.Windows
end

-- ---------------------------------------------------------------- what clicks and key presses trigger
-- ClickDetector and ProximityPrompt are objects in the world that react to the player's input; the service that hears the
-- input starts them (once, when the game starts - see ScriptHost::notify_started).

local function character_root(player)
	local character = player and player.Character
	return character and (character.PrimaryPart or character:FindFirstChildWhichIsA("BasePart", true))
end

-- Where an object is, for the distance to a player: a part's position, an attachment's, a model's pivot.
local function position_of(instance)
	if instance:IsA("BasePart") then
		return instance.Position
	elseif instance:IsA("Attachment") then
		return instance.WorldPosition
	elseif instance:IsA("Model") then
		return instance:GetPivot().Position
	end
	return nil
end

local function distance_to_player(player, instance)
	local root, at = character_root(player), position_of(instance)
	if not root or not at then
		return math.huge
	end
	return (root.Position - at).Magnitude
end

-- The ClickDetector of the object under the cursor: one in the part, or in a model around it.
local function detector_at_cursor(self)
	local camera = workspace.CurrentCamera
	if not camera then
		return nil
	end
	local at = self:GetMouseLocation()
	local ray = camera:ViewportPointToRay(at.X, at.Y)
	local ok, hit = pcall(function()
		return workspace:Raycast(ray.Origin, ray.Direction * 1000)
	end)
	local part = ok and hit and hit.Instance
	local current = part
	while current and current ~= workspace do
		local detector = current:FindFirstChildOfClass("ClickDetector")
		if detector then
			return detector, part
		end
		current = current.Parent
	end
	return nil
end

local pressing = {} -- prompt -> true while the keyboard holds it

local function prompts_for(key)
	local found = {}
	for _, descendant in ipairs(workspace:GetDescendants()) do
		if descendant.ClassName == "ProximityPrompt" and descendant.Enabled and descendant.KeyboardKeyCode == key then
			table.insert(found, descendant)
		end
	end
	return found
end

methods.__start = function(self)
	local players = game:GetService("Players")
	local prompt_service = game:GetService("ProximityPromptService")

	self.InputBegan:Connect(function(input)
		local player = players.LocalPlayer
		local kind = input.UserInputType
		if kind == Enum.UserInputType.MouseButton1 or kind == Enum.UserInputType.MouseButton2 then
			local detector, part = detector_at_cursor(self)
			if detector and distance_to_player(player, part) <= detector.MaxActivationDistance then
				Signal.Fire(kind == Enum.UserInputType.MouseButton1 and detector.MouseClick or detector.RightMouseClick, player)
			end
		elseif kind == Enum.UserInputType.Keyboard and prompt_service.Enabled then
			for _, prompt in ipairs(prompts_for(input.KeyCode)) do
				if not pressing[prompt] and prompt.Parent and distance_to_player(player, prompt.Parent) <= prompt.MaxActivationDistance then
					pressing[prompt] = true
					prompt:InputHoldBegin()
				end
			end
		end
	end)

	self.InputEnded:Connect(function(input)
		if input.UserInputType ~= Enum.UserInputType.Keyboard then
			return
		end
		for prompt in pairs(table.clone(pressing)) do
			if prompt.Parent == nil or prompt.KeyboardKeyCode == input.KeyCode then
				pressing[prompt] = nil
				prompt:InputHoldEnd()
			end
		end
	end)
end
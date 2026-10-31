-- Where the calling script runs. In Roblox a Script runs on the server and a LocalScript on a client; here both run
-- in one game, so each answers for its own side (a Script with RunContext set to Client counts as a client script).
-- Code with no script of its own - a ModuleScript - is both. `getfenv(2)` is the environment of the caller, which holds
-- its `script`.
methods.IsServer = function()
	local script = getfenv(2).script
	if script == nil then
		return true
	end
	local class = script.ClassName
	if class == "LocalScript" then
		return false
	elseif class == "Script" then
		return script.RunContext ~= Enum.RunContext.Client
	end
	return true
end

methods.IsClient = function()
	local script = getfenv(2).script
	if script == nil then
		return true
	end
	local class = script.ClassName
	if class == "LocalScript" then
		return true
	elseif class == "Script" then
		return script.RunContext == Enum.RunContext.Client
	end
	return true
end

methods.IsStudio = function()
	return false
end
methods.IsRunning = function()
	return true
end
methods.IsRunMode = function()
	return false
end
methods.IsEdit = function()
	return false
end

-- BindToRenderStep(name, priority, fn): calls fn(deltaTime) every frame, the smaller priorities first.
local steps = {}
local sorted = {}
local connection

local function resort()
	sorted = {}
	for _, step in pairs(steps) do
		table.insert(sorted, step)
	end
	table.sort(sorted, function(a, b)
		if a.priority ~= b.priority then
			return a.priority < b.priority
		end
		return a.order < b.order
	end)
end

local counter = 0

methods.BindToRenderStep = function(self, name, priority, fn)
	if type(name) ~= "string" or type(fn) ~= "function" then
		error("BindToRenderStep(name: string, priority: number, function)", 2)
	end
	if steps[name] then
		error("Name '" .. name .. "' is already bound to RenderStep", 2)
	end
	counter += 1
	steps[name] = { priority = priority, fn = fn, order = counter }
	resort()
	if not connection then
		connection = self.RenderStepped:Connect(function(dt)
			for _, step in ipairs(sorted) do
				local ok, err = pcall(step.fn, dt)
				if not ok then
					warn(err)
				end
			end
		end)
	end
end

methods.UnbindFromRenderStep = function(_, name)
	steps[name] = nil
	resort()
end
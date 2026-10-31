-- Bound actions, kept in the order they are asked in. The service listens to UserInputService once, when the first
-- action is bound.
local actions = {} -- name -> action
local order = {} -- the actions, first to ask first
local counter = 0
local listening = false

local function resort()
	table.sort(order, function(a, b)
		if a.priority ~= b.priority then
			return a.priority > b.priority
		end
		return a.stack > b.stack
	end)
end

local function wants(action, input)
	for _, spec in ipairs(action.inputs) do
		if spec == input.KeyCode or spec == input.UserInputType then
			return true
		end
	end
	return false
end

local function dispatch(input, state)
	-- (A copy: a handler may bind or unbind actions.)
	for _, action in ipairs(table.clone(order)) do
		if actions[action.name] == action and wants(action, input) then
			local result = action.handler(action.name, state, input)
			if result ~= Enum.ContextActionResult.Pass then
				return
			end
		end
	end
end

local function listen()
	if listening then
		return
	end
	listening = true
	local uis = game:GetService("UserInputService")
	uis.InputBegan:Connect(function(input)
		dispatch(input, Enum.UserInputState.Begin)
	end)
	uis.InputEnded:Connect(function(input)
		dispatch(input, Enum.UserInputState.End)
	end)
	uis.InputChanged:Connect(function(input)
		dispatch(input, Enum.UserInputState.Change)
	end)
end

local function info(action)
	return { stackOrder = action.stack, priority = action.priority, createTouchButton = action.button, inputTypes = table.clone(action.inputs) }
end

local function bind(self, name, handler, button, priority, ...)
	if type(handler) ~= "function" then
		error("BindAction: the handler must be a function", 3)
	end
	counter += 1
	local action = { name = name, handler = handler, button = button and true or false, priority = priority, stack = counter, inputs = { ... } }
	local replaced = actions[name]
	if replaced then
		table.remove(order, table.find(order, replaced))
	end
	actions[name] = action
	table.insert(order, action)
	resort()
	listen()
	Signal.Fire(self.BoundActionAdded, name, action.button, info(action))
end

methods.BindAction = function(self, name, handler, button, ...)
	bind(self, name, handler, button, Enum.ContextActionPriority.Medium.Value, ...)
end

methods.BindActionAtPriority = function(self, name, handler, button, priority, ...)
	bind(self, name, handler, button, priority, ...)
end

methods.UnbindAction = function(self, name)
	local action = actions[name]
	if not action then
		return
	end
	actions[name] = nil
	table.remove(order, table.find(order, action))
	Signal.Fire(self.BoundActionRemoved, name, info(action))
end

methods.UnbindAllActions = function(self)
	for name in pairs(table.clone(actions)) do
		methods.UnbindAction(self, name)
	end
end

methods.GetAllBoundActionInfo = function()
	local all = {}
	for name, action in pairs(actions) do
		all[name] = info(action)
	end
	return all
end

methods.GetBoundActionInfo = function(_, name)
	local action = actions[name]
	return action and info(action) or {}
end

-- The buttons, titles and icons are for touch screens, which Forge does not draw.
for _, name in { "SetTitle", "SetImage", "SetDescription", "SetPosition", "BindActivate", "UnbindActivate", "FireActionButtonFoundSignal" } do
	methods[name] = function() end
end

methods.GetCurrentLocalToolIcon = function()
	return nil
end
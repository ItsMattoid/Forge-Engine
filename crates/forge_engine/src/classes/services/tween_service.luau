-- The engine of every Tween. A tween's state lives here (in `tweens`, by the Tween instance); the Tween class (animation/
-- tween.luau) only calls play / pause / cancel.

-- ---------------------------------------------------------------- easing
-- Each style is given as its "In" curve on 0..1; Out and InOut are made from it.
local function bounce_out(t)
	local n1, d1 = 7.5625, 2.75
	if t < 1 / d1 then
		return n1 * t * t
	elseif t < 2 / d1 then
		t -= 1.5 / d1
		return n1 * t * t + 0.75
	elseif t < 2.5 / d1 then
		t -= 2.25 / d1
		return n1 * t * t + 0.9375
	end
	t -= 2.625 / d1
	return n1 * t * t + 0.984375
end

local ease_in = {
	Linear = function(t)
		return t
	end,
	Sine = function(t)
		return 1 - math.cos(t * math.pi / 2)
	end,
	Quad = function(t)
		return t * t
	end,
	Cubic = function(t)
		return t * t * t
	end,
	Quart = function(t)
		return t ^ 4
	end,
	Quint = function(t)
		return t ^ 5
	end,
	Exponential = function(t)
		return t == 0 and 0 or 2 ^ (10 * t - 10)
	end,
	Circular = function(t)
		return 1 - math.sqrt(1 - t * t)
	end,
	Back = function(t)
		local c1 = 1.70158
		return (c1 + 1) * t * t * t - c1 * t * t
	end,
	Bounce = function(t)
		return 1 - bounce_out(1 - t)
	end,
	Elastic = function(t)
		if t == 0 or t == 1 then
			return t
		end
		return -(2 ^ (10 * t - 10)) * math.sin((t * 10 - 10.75) * (2 * math.pi / 3))
	end,
}

local function ease(style, direction, t)
	local curve = ease_in[style] or ease_in.Quad
	if direction == "In" then
		return curve(t)
	elseif direction == "Out" then
		return 1 - curve(1 - t)
	elseif t < 0.5 then
		return curve(t * 2) / 2
	end
	return 1 - curve((1 - t) * 2) / 2
end

methods.GetValue = function(_, alpha, style, direction)
	return ease(style.Name, direction.Name, math.clamp(alpha, 0, 1))
end

-- ---------------------------------------------------------------- blending
-- The value a fraction `t` of the way from `a` to `b`: numbers and native vectors arithmetically, the types that have
-- a Lerp method with it, and anything else (a boolean, an enum ...) by jumping to the goal when the tween ends.
local function blend(a, b, t)
	local kind = typeof(a)
	if kind == "number" then
		return a + (b - a) * t
	elseif kind == "Vector3" then
		return a + (b - a) * t
	elseif kind == "Vector2" or kind == "Color3" or kind == "CFrame" or kind == "UDim2" then
		return a:Lerp(b, t)
	end
	return t >= 1 and b or a
end

-- ---------------------------------------------------------------- tweens
local tweens = setmetatable({}, { __mode = "k" }) -- Tween -> state
local active = {} -- Tween -> true, while it plays (a strong reference: a playing tween is not collected)
local connection

local function finish(tween, state, playback)
	active[tween] = nil
	state.playback = playback
	tween.PlaybackState = Enum.PlaybackState[playback]
	Signal.Fire(tween.Completed, Enum.PlaybackState[playback])
	if next(active) == nil and connection then
		connection:Disconnect()
		connection = nil
	end
end

-- Writes the blended values; false if the instance can no longer take them (it was destroyed, say).
local function apply(state, alpha)
	local inst = state.inst
	local ok = pcall(function()
		for property, goal in pairs(state.goals) do
			inst[property] = blend(state.from[property], goal, alpha)
		end
	end)
	return ok
end

local function step(frame)
	for tween in pairs(table.clone(active)) do
		local state = tweens[tween]
		if state and state.playback ~= "Paused" then
			local dt = frame
			if state.delay > 0 then
				state.delay -= dt
				if state.delay <= 0 then
					dt = -state.delay -- the time left over after the delay
					state.delay = 0
					state.playback = "Playing"
					tween.PlaybackState = Enum.PlaybackState.Playing
				else
					dt = 0
				end
			end
			if state.playback == "Playing" then
				local info = state.info
				state.elapsed += dt
				local duration = math.max(info.Time, 1e-4)
				local alpha = math.min(state.elapsed / duration, 1)
				local eased = ease(info.EasingStyle.Name, info.EasingDirection.Name, alpha)
				if state.reversing then
					eased = 1 - eased
				end
				if not apply(state, eased) then
					finish(tween, state, "Cancelled")
				elseif alpha >= 1 then
					if info.Reverses and not state.reversing then
						state.reversing = true
						state.elapsed = state.elapsed - duration
					elseif info.RepeatCount < 0 or state.repeated < info.RepeatCount then
						state.repeated += 1
						state.reversing = false
						state.elapsed = state.elapsed - duration
					else
						apply(state, state.reversing and 0 or 1)
						finish(tween, state, "Completed")
					end
				end
			end
		end
	end
end

local function start(tween, state)
	state.from = {}
	for property in pairs(state.goals) do
		state.from[property] = state.inst[property]
	end
	state.elapsed = 0
	state.repeated = 0
	state.reversing = false
	state.delay = state.info.DelayTime
	state.playback = state.delay > 0 and "Delayed" or "Playing"
	tween.PlaybackState = Enum.PlaybackState[state.playback]
	active[tween] = true
	if not connection then
		connection = game:GetService("RunService").Heartbeat:Connect(step)
	end
end

define("__tween", {
	play = function(tween)
		local state = tweens[tween]
		if state.playback == "Paused" then
			state.playback = state.delay > 0 and "Delayed" or "Playing"
			tween.PlaybackState = Enum.PlaybackState[state.playback]
		elseif not active[tween] then
			start(tween, state)
		end
	end,
	pause = function(tween)
		local state = tweens[tween]
		if active[tween] and state.playback ~= "Paused" then
			state.playback = "Paused"
			tween.PlaybackState = Enum.PlaybackState.Paused
		end
	end,
	cancel = function(tween)
		local state = tweens[tween]
		if active[tween] then
			finish(tween, state, "Cancelled")
		end
	end,
	register = function(tween, state)
		tweens[tween] = state
	end,
})

methods.Create = function(_, inst, info, goals)
	if typeof(inst) ~= "Instance" then
		error("TweenService:Create expects an Instance as its first argument", 2)
	end
	if typeof(info) ~= "TweenInfo" then
		error("TweenService:Create expects a TweenInfo as its second argument", 2)
	end
	if type(goals) ~= "table" then
		error("TweenService:Create expects a table of goals as its third argument", 2)
	end
	local checked = {}
	for property, goal in pairs(goals) do
		local ok, current = pcall(function()
			return inst[property]
		end)
		if not ok then
			error(tostring(property) .. " is not a valid property of " .. inst.ClassName, 2)
		end
		if typeof(current) ~= typeof(goal) then
			error("Property '" .. tostring(property) .. "' cannot be tweened to a " .. typeof(goal) .. ": it holds a " .. typeof(current), 2)
		end
		checked[property] = goal
	end
	local tween = __spawn("Tween")
	tween.Instance = inst
	__tween.register(tween, { inst = inst, info = info, goals = checked, playback = "Begin" })
	return tween
end